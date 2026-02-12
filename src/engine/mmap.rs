//! Memory-mapped IO engine
//!
//! This module provides an IO engine that uses memory-mapped I/O (mmap) for file access.
//! Instead of using read/write syscalls, the file is mapped into the process's address
//! space and accessed via memcpy operations.
//!
//! # Features
//!
//! - Memory-mapped file access via mmap
//! - Read operations via memcpy from mapped region
//! - Write operations via memcpy to mapped region
//! - madvise hints for access pattern optimization
//! - msync for write persistence
//! - Automatic munmap cleanup
//!
//! # Performance
//!
//! mmap can be very efficient for certain workloads:
//! - Eliminates syscall overhead for small I/Os
//! - Leverages page cache effectively
//! - Good for random access patterns
//! - Excellent for read-heavy workloads
//!
//! However, it may not be ideal for:
//! - Very large files (address space limitations)
//! - Write-heavy workloads (page faults)
//! - O_DIRECT scenarios (mmap bypasses O_DIRECT)
//!
//! # Requirements
//!
//! - POSIX-compliant system with mmap support
//! - Sufficient virtual address space
//!
//! # Example
//!
//! ```no_run
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::mmap::MmapEngine;
//!
//! let mut engine = MmapEngine::new();
//! let config = EngineConfig::default();
//! engine.init(&config).unwrap();
//!
//! // Operations use memcpy instead of syscalls
//! // ... (see IOEngine trait documentation)
//!
//! engine.cleanup().unwrap();
//! ```

use super::{EngineCapabilities, EngineConfig, IOCompletion, IOEngine, IOOperation, OperationType};
use crate::Result;
use anyhow::Context;
use std::collections::{HashMap, VecDeque};
use std::os::unix::io::RawFd;
use std::ptr;

/// Memory mapping information for a file descriptor
struct MmapInfo {
    /// Mapped address
    addr: *mut u8,
    /// Mapped size
    size: usize,
}

// Safety: MmapInfo owns the mapped memory region and is only used within
// a single thread. The pointer is valid for the lifetime of the MmapInfo.
unsafe impl Send for MmapInfo {}

/// Memory-mapped IO engine
///
/// This engine uses mmap to map files into memory and performs I/O via memcpy.
/// It's efficient for certain workloads but has different characteristics than
/// traditional I/O engines.
pub struct MmapEngine {
    /// Configuration
    config: Option<EngineConfig>,
    
    /// Map of file descriptors to their mmap info
    mappings: HashMap<RawFd, MmapInfo>,
    
    /// Queue of completed operations
    ///
    /// Since mmap operations complete immediately (memcpy is synchronous),
    /// we queue completions here and return them from poll_completions().
    completed: VecDeque<IOCompletion>,
}

impl MmapEngine {
    /// Create a new mmap engine
    pub fn new() -> Self {
        Self {
            config: None,
            mappings: HashMap::new(),
            completed: VecDeque::new(),
        }
    }
    
    /// Get or create a memory mapping for a file descriptor
    ///
    /// This method maps the file into memory if it hasn't been mapped yet.
    /// Subsequent operations on the same fd will reuse the existing mapping.
    fn get_or_create_mapping(&mut self, fd: RawFd, _need_write: bool) -> Result<&mut MmapInfo> {
        if !self.mappings.contains_key(&fd) {
            // Get file size
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            let result = unsafe { libc::fstat(fd, &mut stat) };
            if result < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err).context(format!("fstat failed for fd={}", fd));
            }
            
            let file_size = stat.st_size as usize;
            if file_size == 0 {
                anyhow::bail!("Cannot mmap file with size 0 (fd={})", fd);
            }
            
            // Always use PROT_READ | PROT_WRITE for mixed workloads
            // If we create a read-only mapping and later need to write, we'd segfault
            // This is safer and supports all workload types
            let prot = libc::PROT_READ | libc::PROT_WRITE;
            
            // Create memory mapping
            let addr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    file_size,
                    prot,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            
            if addr == libc::MAP_FAILED {
                let err = std::io::Error::last_os_error();
                return Err(err).context(format!(
                    "mmap failed: fd={}, size={}",
                    fd, file_size
                ));
            }
            
            let info = MmapInfo {
                addr: addr as *mut u8,
                size: file_size,
            };
            
            self.mappings.insert(fd, info);
        }
        
        Ok(self.mappings.get_mut(&fd).unwrap())
    }
    
    /// Perform a read operation via memcpy from mapped region
    fn do_read(&mut self, fd: RawFd, buffer: *mut u8, length: usize, offset: u64) -> Result<usize> {
        let mapping = self.get_or_create_mapping(fd, false)?;  // Read-only mapping
        
        // Validate offset and length
        let offset_usize = offset as usize;
        if offset_usize >= mapping.size {
            // Reading past end of file returns 0 (EOF)
            return Ok(0);
        }
        
        // Calculate actual bytes to read (may be less than requested at EOF)
        let available = mapping.size - offset_usize;
        let to_read = length.min(available);
        
        // Copy from mapped region to buffer
        unsafe {
            let src = mapping.addr.add(offset_usize);
            ptr::copy_nonoverlapping(src, buffer, to_read);
        }
        
        Ok(to_read)
    }
    
    /// Perform a write operation via memcpy to mapped region
    fn do_write(&mut self, fd: RawFd, buffer: *const u8, length: usize, offset: u64) -> Result<usize> {
        let mapping = self.get_or_create_mapping(fd, true)?;  // Read-write mapping
        
        // Validate offset and length
        let offset_usize = offset as usize;
        if offset_usize >= mapping.size {
            anyhow::bail!(
                "Write offset {} exceeds file size {} (fd={})",
                offset,
                mapping.size,
                fd
            );
        }
        
        // Calculate actual bytes to write (may be less than requested at EOF)
        let available = mapping.size - offset_usize;
        let to_write = length.min(available);
        
        // Copy from buffer to mapped region
        unsafe {
            let dst = mapping.addr.add(offset_usize);
            ptr::copy_nonoverlapping(buffer, dst, to_write);
        }
        
        Ok(to_write)
    }
    
    /// Perform an msync operation to flush writes to disk
    fn do_msync(&mut self, fd: RawFd, sync_data_only: bool) -> Result<usize> {
        let mapping = self.mappings.get(&fd)
            .ok_or_else(|| anyhow::anyhow!("File not mapped: fd={}", fd))?;
        
        let flags = if sync_data_only {
            libc::MS_SYNC  // Synchronous sync (data only for fdatasync)
        } else {
            libc::MS_SYNC  // Synchronous sync (data + metadata for fsync)
        };
        
        let result = unsafe {
            libc::msync(mapping.addr as *mut libc::c_void, mapping.size, flags)
        };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("msync failed: fd={}", fd));
        }
        
        Ok(0)
    }
    
    /// Apply madvise hints to a mapped region
    pub fn apply_madvise(&mut self, fd: RawFd, flags: &crate::config::workload::MadviseFlags) -> Result<()> {
        let mapping = self.mappings.get(&fd)
            .ok_or_else(|| anyhow::anyhow!("File not mapped: fd={}", fd))?;
        
        // Apply each requested hint
        if flags.sequential {
            unsafe {
                libc::madvise(
                    mapping.addr as *mut libc::c_void,
                    mapping.size,
                    libc::MADV_SEQUENTIAL,
                );
            }
        }
        
        if flags.random {
            unsafe {
                libc::madvise(
                    mapping.addr as *mut libc::c_void,
                    mapping.size,
                    libc::MADV_RANDOM,
                );
            }
        }
        
        if flags.willneed {
            unsafe {
                libc::madvise(
                    mapping.addr as *mut libc::c_void,
                    mapping.size,
                    libc::MADV_WILLNEED,
                );
            }
        }
        
        if flags.dontneed {
            unsafe {
                libc::madvise(
                    mapping.addr as *mut libc::c_void,
                    mapping.size,
                    libc::MADV_DONTNEED,
                );
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            if flags.hugepage {
                unsafe {
                    libc::madvise(
                        mapping.addr as *mut libc::c_void,
                        mapping.size,
                        libc::MADV_HUGEPAGE,
                    );
                }
            }
            
            if flags.nohugepage {
                unsafe {
                    libc::madvise(
                        mapping.addr as *mut libc::c_void,
                        mapping.size,
                        libc::MADV_NOHUGEPAGE,
                    );
                }
            }
        }
        
        Ok(())
    }
}

impl Default for MmapEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IOEngine for MmapEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        self.config = Some(config.clone());
        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        // For mmap engine, operations complete immediately via memcpy
        let result = match op.op_type {
            OperationType::Read => {
                self.do_read(op.target_fd, op.buffer, op.length, op.offset)
            }
            OperationType::Write => {
                self.do_write(op.target_fd, op.buffer as *const u8, op.length, op.offset)
            }
            OperationType::Fsync => {
                self.do_msync(op.target_fd, false)
            }
            OperationType::Fdatasync => {
                self.do_msync(op.target_fd, true)
            }
        };
        
        // Queue the completion
        let completion = IOCompletion {
            user_data: op.user_data,
            result,
            op_type: op.op_type,
        };
        
        self.completed.push_back(completion);
        Ok(())
    }
    
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>> {
        // Return all completed operations and clear the queue
        let completions: Vec<IOCompletion> = self.completed.drain(..).collect();
        Ok(completions)
    }
    
    fn cleanup(&mut self) -> Result<()> {
        // Unmap all mapped regions
        for (fd, mapping) in self.mappings.drain() {
            let result = unsafe {
                libc::munmap(mapping.addr as *mut libc::c_void, mapping.size)
            };
            
            if result < 0 {
                let err = std::io::Error::last_os_error();
                eprintln!("Warning: munmap failed for fd={}: {}", fd, err);
                // Continue cleanup even if munmap fails
            }
        }
        
        self.completed.clear();
        Ok(())
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            async_io: false,  // mmap operations are synchronous (memcpy)
            batch_submission: false,
            registered_buffers: false,
            fixed_files: false,
            polling_mode: false,
            max_queue_depth: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::unix::io::AsRawFd;
    use tempfile::TempDir;
    
    #[test]
    fn test_mmap_engine_init() {
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        
        assert!(engine.init(&config).is_ok());
    }
    
    #[test]
    fn test_mmap_engine_capabilities() {
        let engine = MmapEngine::new();
        let caps = engine.capabilities();
        
        assert!(!caps.async_io);  // mmap is synchronous
        assert!(!caps.batch_submission);
        assert!(!caps.registered_buffers);
        assert!(!caps.fixed_files);
        assert!(!caps.polling_mode);
        assert_eq!(caps.max_queue_depth, 1);
    }
    
    #[test]
    fn test_mmap_engine_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_read.dat");
        
        // Create a test file with known content
        let test_data = b"Hello from mmap! Memory-mapped IO is efficient.";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit read operation
        let mut buffer = vec![0u8; test_data.len()];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 42,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 42);
        assert_eq!(completions[0].op_type, OperationType::Read);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap(), &test_data.len());
        
        // Verify data
        assert_eq!(&buffer[..], test_data);
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_mmap_engine_write() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_write.dat");
        
        // Create a file with initial content
        let initial_data = vec![0u8; 1024];
        std::fs::write(&file_path, &initial_data).unwrap();
        
        // Open the file for read/write
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit write operation
        let test_data = b"Writing with mmap engine!";
        let op = IOOperation {
            op_type: OperationType::Write,
            target_fd: fd,
            offset: 0,
            buffer: test_data.as_ptr() as *mut u8,
            length: test_data.len(),
            user_data: 99,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 99);
        assert_eq!(completions[0].op_type, OperationType::Write);
        assert!(completions[0].result.is_ok());
        
        // Sync to disk
        let sync_op = IOOperation {
            op_type: OperationType::Fsync,
            target_fd: fd,
            offset: 0,
            buffer: ptr::null_mut(),
            length: 0,
            user_data: 100,
        };
        engine.submit(sync_op).unwrap();
        engine.poll_completions().unwrap();
        
        engine.cleanup().unwrap();
        drop(file);
        
        // Verify data was written
        let written_data = std::fs::read(&file_path).unwrap();
        assert_eq!(&written_data[..test_data.len()], test_data);
    }
    
    #[test]
    fn test_mmap_engine_read_at_offset() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_offset.dat");
        
        // Create a test file
        let test_data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Read from offset 10
        let mut buffer = vec![0u8; 10];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 10,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_ok());
        
        // Verify data
        assert_eq!(&buffer[..], b"ABCDEFGHIJ");
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_mmap_engine_partial_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_partial.dat");
        
        // Create a small file
        let test_data = b"Short";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Try to read more than available
        let mut buffer = vec![0u8; 100];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_ok());
        
        // Should only read what's available
        let bytes_read = completions[0].result.as_ref().unwrap();
        assert_eq!(*bytes_read, test_data.len());
        assert_eq!(&buffer[..*bytes_read], test_data);
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_mmap_engine_multiple_operations() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_multiple.dat");
        
        // Create a test file
        let test_data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit multiple read operations
        let mut buffers = vec![vec![0u8; 5]; 3];
        for (i, buffer) in buffers.iter_mut().enumerate() {
            let op = IOOperation {
                op_type: OperationType::Read,
                target_fd: fd,
                offset: (i * 5) as u64,
                buffer: buffer.as_mut_ptr(),
                length: buffer.len(),
                user_data: i as u64,
            };
            engine.submit(op).unwrap();
        }
        
        // Poll for completions
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 3);
        
        // Verify all operations completed successfully
        for (i, completion) in completions.iter().enumerate() {
            assert_eq!(completion.user_data, i as u64);
            assert!(completion.result.is_ok());
        }
        
        // Verify data
        assert_eq!(&buffers[0][..], b"01234");
        assert_eq!(&buffers[1][..], b"56789");
        assert_eq!(&buffers[2][..], b"ABCDE");
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_mmap_engine_msync() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_msync.dat");
        
        // Create a file
        let initial_data = vec![0u8; 1024];
        std::fs::write(&file_path, &initial_data).unwrap();
        
        // Open the file for read/write
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Write some data
        let test_data = b"Test data for msync";
        let write_op = IOOperation {
            op_type: OperationType::Write,
            target_fd: fd,
            offset: 0,
            buffer: test_data.as_ptr() as *mut u8,
            length: test_data.len(),
            user_data: 1,
        };
        engine.submit(write_op).unwrap();
        engine.poll_completions().unwrap();
        
        // Sync to disk
        let sync_op = IOOperation {
            op_type: OperationType::Fsync,
            target_fd: fd,
            offset: 0,
            buffer: ptr::null_mut(),
            length: 0,
            user_data: 2,
        };
        engine.submit(sync_op).unwrap();
        
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].op_type, OperationType::Fsync);
        assert!(completions[0].result.is_ok());
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_mmap_engine_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_cleanup.dat");
        
        // Create a file
        std::fs::write(&file_path, &vec![0u8; 1024]).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine and perform an operation (creates mapping)
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let mut buffer = vec![0u8; 100];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        engine.submit(op).unwrap();
        engine.poll_completions().unwrap();
        
        // Cleanup should unmap the region
        assert!(engine.cleanup().is_ok());
        assert!(engine.mappings.is_empty());
    }
    
    #[test]
    fn test_mmap_engine_read_past_eof() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_eof.dat");
        
        // Create a small file
        let test_data = b"Small";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = MmapEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Try to read past EOF
        let mut buffer = vec![0u8; 10];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 100, // Way past EOF
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        
        engine.submit(op).unwrap();
        
        // Should return 0 bytes (EOF)
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap(), &0);
        
        engine.cleanup().unwrap();
    }
}
