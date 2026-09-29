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
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// A shared memory-mapped region for a file.
///
/// Multiple workers accessing the same file share one mmap() call via the
/// global MMAP_REGISTRY, eliminating per-worker page-table overhead and
/// MAP_POPULATE contention that degrades performance at high thread counts.
///
/// munmap is called automatically when the last Arc is dropped.
struct SharedMmapRegion {
    addr: *mut u8,
    size: usize,
}

// Safety: addr points to a MAP_SHARED region backed by a file. The region
// is valid until munmap (called in Drop). Concurrent reads/writes to
// non-overlapping offsets are safe; callers are responsible for
// synchronization at shared offsets (consistent with fio's model).
unsafe impl Send for SharedMmapRegion {}
unsafe impl Sync for SharedMmapRegion {}

impl Drop for SharedMmapRegion {
    fn drop(&mut self) {
        let result = unsafe { libc::munmap(self.addr as *mut libc::c_void, self.size) };
        if result < 0 {
            let err = std::io::Error::last_os_error();
            eprintln!("Warning: munmap failed during shared region drop: {}", err);
        }
    }
}

/// Global registry of shared mmap regions, keyed by inode number.
///
/// Weak references allow regions to be freed automatically when no worker
/// holds them. The registry is populated lazily on first access per file.
static MMAP_REGISTRY: OnceLock<Mutex<HashMap<u64, Weak<SharedMmapRegion>>>> = OnceLock::new();

fn mmap_registry() -> &'static Mutex<HashMap<u64, Weak<SharedMmapRegion>>> {
    MMAP_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Memory-mapped IO engine
///
/// This engine uses mmap to map files into memory and performs I/O via memcpy.
/// It's efficient for certain workloads but has different characteristics than
/// traditional I/O engines.
pub struct MmapEngine {
    /// Configuration
    config: Option<EngineConfig>,

    /// Per-fd mapping to shared regions.
    ///
    /// Holds an Arc to keep the shared region alive for this engine's lifetime.
    /// Different workers may share the same underlying SharedMmapRegion for
    /// the same file (same inode), avoiding redundant mmap() calls.
    mappings: HashMap<RawFd, Arc<SharedMmapRegion>>,

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
    
    /// Get or create a memory mapping for a file descriptor.
    ///
    /// Checks the global MMAP_REGISTRY for an existing live mapping for this
    /// file (identified by inode). If found, reuses it — no mmap() call.
    /// Otherwise creates a new mapping, stores it in the registry, and returns it.
    ///
    /// This sharing eliminates the 128× page-walk overhead from MAP_POPULATE
    /// that occurs when every worker independently maps the same file.
    fn get_or_create_mapping(&mut self, fd: RawFd, _need_write: bool) -> Result<(*mut u8, usize)> {
        if let Some(region) = self.mappings.get(&fd) {
            return Ok((region.addr, region.size));
        }

        // Get file metadata: size for mmap, inode for registry lookup.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut stat) } < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("fstat failed for fd={}", fd));
        }

        let file_size = stat.st_size as usize;
        if file_size == 0 {
            anyhow::bail!("Cannot mmap file with size 0 (fd={})", fd);
        }

        let inode = stat.st_ino;

        // Lock the registry for the duration of lookup + optional mmap creation.
        // This prevents two workers from both calling mmap for the same inode
        // simultaneously, ensuring MAP_POPULATE runs exactly once per file.
        let mut registry = mmap_registry().lock().unwrap();

        let region = if let Some(weak) = registry.get(&inode) {
            if let Some(existing) = weak.upgrade() {
                // Another worker already mapped this file — reuse it.
                existing
            } else {
                // Weak reference is stale (no workers hold it); fall through to create.
                Self::create_new_mapping(fd, inode, file_size, &mut registry)?
            }
        } else {
            Self::create_new_mapping(fd, inode, file_size, &mut registry)?
        };

        let (addr, size) = (region.addr, region.size);
        self.mappings.insert(fd, region);
        Ok((addr, size))
    }

    /// Create a new mmap region, register it, and return the Arc.
    ///
    /// Called while holding the MMAP_REGISTRY lock to prevent races.
    fn create_new_mapping(
        fd: RawFd,
        inode: u64,
        file_size: usize,
        registry: &mut HashMap<u64, Weak<SharedMmapRegion>>,
    ) -> Result<Arc<SharedMmapRegion>> {
        // Always use PROT_READ | PROT_WRITE for mixed workloads.
        let prot = libc::PROT_READ | libc::PROT_WRITE;

        // MAP_POPULATE pre-faults all pages at mmap time, eliminating page
        // fault latency spikes on first access. With shared mappings this
        // cost is paid once regardless of worker count, not N times.
        #[cfg(target_os = "linux")]
        let map_flags = libc::MAP_SHARED | libc::MAP_POPULATE;
        #[cfg(not(target_os = "linux"))]
        let map_flags = libc::MAP_SHARED;

        let addr = unsafe {
            libc::mmap(ptr::null_mut(), file_size, prot, map_flags, fd, 0)
        };

        if addr == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("mmap failed: fd={}, size={}", fd, file_size));
        }

        let region = Arc::new(SharedMmapRegion {
            addr: addr as *mut u8,
            size: file_size,
        });

        registry.insert(inode, Arc::downgrade(&region));
        Ok(region)
    }
    
    /// Perform a read operation via memcpy from mapped region
    fn do_read(&mut self, fd: RawFd, buffer: *mut u8, length: usize, offset: u64) -> Result<usize> {
        let (addr, size) = self.get_or_create_mapping(fd, false)?;

        let offset_usize = offset as usize;
        if offset_usize >= size {
            // Reading past end of file returns 0 (EOF)
            return Ok(0);
        }

        let available = size - offset_usize;
        let to_read = length.min(available);

        unsafe {
            ptr::copy_nonoverlapping(addr.add(offset_usize), buffer, to_read);
        }

        Ok(to_read)
    }

    /// Perform a write operation via memcpy to mapped region
    fn do_write(&mut self, fd: RawFd, buffer: *const u8, length: usize, offset: u64) -> Result<usize> {
        let (addr, size) = self.get_or_create_mapping(fd, true)?;

        let offset_usize = offset as usize;
        if offset_usize >= size {
            anyhow::bail!(
                "Write offset {} exceeds file size {} (fd={})",
                offset,
                size,
                fd
            );
        }

        let available = size - offset_usize;
        let to_write = length.min(available);

        unsafe {
            ptr::copy_nonoverlapping(buffer, addr.add(offset_usize), to_write);
        }

        Ok(to_write)
    }

    /// Perform an msync operation to flush writes to disk
    fn do_msync(&mut self, fd: RawFd, _sync_data_only: bool) -> Result<usize> {
        let region = self.mappings.get(&fd)
            .ok_or_else(|| anyhow::anyhow!("File not mapped: fd={}", fd))?;

        let result = unsafe {
            libc::msync(region.addr as *mut libc::c_void, region.size, libc::MS_SYNC)
        };

        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("msync failed: fd={}", fd));
        }

        Ok(0)
    }
    
    /// Apply madvise hints to a mapped region
    pub fn apply_madvise(&mut self, fd: RawFd, flags: &crate::config::workload::MadviseFlags) -> Result<()> {
        let region = self.mappings.get(&fd)
            .ok_or_else(|| anyhow::anyhow!("File not mapped: fd={}", fd))?;
        let (addr, size) = (region.addr as *mut libc::c_void, region.size);

        if flags.sequential {
            unsafe { libc::madvise(addr, size, libc::MADV_SEQUENTIAL); }
        }
        if flags.random {
            unsafe { libc::madvise(addr, size, libc::MADV_RANDOM); }
        }
        if flags.willneed {
            unsafe { libc::madvise(addr, size, libc::MADV_WILLNEED); }
        }
        if flags.dontneed {
            unsafe { libc::madvise(addr, size, libc::MADV_DONTNEED); }
        }

        #[cfg(target_os = "linux")]
        {
            if flags.hugepage {
                unsafe { libc::madvise(addr, size, libc::MADV_HUGEPAGE); }
            }
            if flags.nohugepage {
                unsafe { libc::madvise(addr, size, libc::MADV_NOHUGEPAGE); }
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
        // Release per-engine Arc references to shared mapping regions.
        // munmap is called automatically by SharedMmapRegion::drop() when
        // the last Arc is released (i.e., when all workers have cleaned up).
        self.mappings.clear();
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
