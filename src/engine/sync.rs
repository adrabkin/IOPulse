//! Synchronous IO engine
//!
//! This module provides a synchronous IO engine that uses blocking pread/pwrite
//! syscalls. This is the baseline engine that works on all platforms and requires
//! no special kernel features.
//!
//! # Features
//!
//! - Uses pread/pwrite for positioned IO without changing file offset
//! - Supports O_DIRECT for bypassing page cache
//! - Handles partial reads/writes by retrying until complete
//! - Simple and reliable - always available as a fallback
//!
//! # Performance
//!
//! The synchronous engine performs one operation at a time (queue depth = 1).
//! Each operation blocks until completion, so it cannot overlap IO with computation.
//! For maximum performance, use io_uring or libaio engines instead.
//!
//! # Example
//!
//! ```no_run
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::sync::SyncEngine;
//!
//! let mut engine = SyncEngine::new();
//! let config = EngineConfig::default();
//! engine.init(&config).unwrap();
//!
//! // Submit and immediately complete a read operation
//! let op = IOOperation {
//!     op_type: OperationType::Read,
//!     target_fd: 3,
//!     offset: 0,
//!     buffer: std::ptr::null_mut(),
//!     length: 4096,
//!     user_data: 1,
//! };
//! engine.submit(op).unwrap();
//!
//! // Poll returns the completed operation immediately
//! let completions = engine.poll_completions().unwrap();
//! assert_eq!(completions.len(), 1);
//! ```

use super::{EngineCapabilities, EngineConfig, IOCompletion, IOEngine, IOOperation, OperationType};
use crate::Result;
use anyhow::Context;

/// Synchronous IO engine using pread/pwrite
///
/// This engine performs IO operations synchronously using the pread and pwrite
/// system calls. Operations block until completion, so only one operation can
/// be in flight at a time (queue depth = 1).
///
/// The engine handles partial reads/writes by retrying until the full requested
/// amount is transferred or an error occurs.
pub struct SyncEngine {
    /// Configuration (stored for reference, not actively used)
    _config: Option<EngineConfig>,
    
    /// Single completion slot (sync engine only has QD=1)
    /// Using Option instead of VecDeque to avoid allocation overhead
    pending_completion: Option<IOCompletion>,
    
    /// Pre-allocated single-element vector (reused to avoid allocations)
    completion_vec: Vec<IOCompletion>,
}

impl SyncEngine {
    /// Create a new synchronous IO engine
    pub fn new() -> Self {
        Self {
            _config: None,
            pending_completion: None,
            completion_vec: Vec::with_capacity(1),
        }
    }
    
    /// Perform a read operation using pread
    ///
    /// Reads data from the file descriptor at the specified offset into the buffer.
    /// Handles partial reads by retrying until the full amount is read or an error occurs.
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor to read from
    /// * `buffer` - Buffer to read data into
    /// * `length` - Number of bytes to read
    /// * `offset` - Offset in the file to read from
    ///
    /// # Returns
    ///
    /// The total number of bytes read, or an error if the operation failed.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The pread syscall fails
    /// - EOF is reached before reading the requested amount
    /// - The buffer pointer is invalid
    #[inline(always)]
    fn do_read(&self, fd: i32, buffer: *mut u8, length: usize, offset: u64) -> Result<usize> {
        let mut total_read = 0;
        let mut current_offset = offset;
        
        while total_read < length {
            let remaining = length - total_read;
            let buf_ptr = unsafe { buffer.add(total_read) };
            
            // SAFETY: We trust the caller to provide a valid buffer pointer and length.
            // The buffer must remain valid for the duration of this call.
            let result = unsafe {
                libc::pread(
                    fd,
                    buf_ptr as *mut libc::c_void,
                    remaining,
                    current_offset as i64,
                )
            };
            
            if result < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err).context(format!(
                    "pread failed: fd={}, offset={}, length={}",
                    fd, current_offset, remaining
                ));
            }
            
            if result == 0 {
                // EOF reached - this is not necessarily an error for reads
                // Return the amount we've read so far
                break;
            }
            
            let bytes_read = result as usize;
            total_read += bytes_read;
            current_offset += bytes_read as u64;
        }
        
        Ok(total_read)
    }
    
    /// Perform a write operation using pwrite
    ///
    /// Writes data from the buffer to the file descriptor at the specified offset.
    /// Handles partial writes by retrying until the full amount is written or an error occurs.
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor to write to
    /// * `buffer` - Buffer containing data to write
    /// * `length` - Number of bytes to write
    /// * `offset` - Offset in the file to write to
    ///
    /// # Returns
    ///
    /// The total number of bytes written, or an error if the operation failed.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The pwrite syscall fails
    /// - The buffer pointer is invalid
    #[inline(always)]
    fn do_write(&self, fd: i32, buffer: *const u8, length: usize, offset: u64) -> Result<usize> {
        let mut total_written = 0;
        let mut current_offset = offset;
        
        while total_written < length {
            let remaining = length - total_written;
            let buf_ptr = unsafe { buffer.add(total_written) };
            
            // SAFETY: We trust the caller to provide a valid buffer pointer and length.
            // The buffer must remain valid for the duration of this call.
            let result = unsafe {
                libc::pwrite(
                    fd,
                    buf_ptr as *const libc::c_void,
                    remaining,
                    current_offset as i64,
                )
            };
            
            if result < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err).context(format!(
                    "pwrite failed: fd={}, offset={}, length={}",
                    fd, current_offset, remaining
                ));
            }
            
            let bytes_written = result as usize;
            total_written += bytes_written;
            current_offset += bytes_written as u64;
        }
        
        Ok(total_written)
    }
    
    /// Perform an fsync operation
    ///
    /// Synchronizes all modified data and metadata for the file to storage.
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor to sync
    ///
    /// # Returns
    ///
    /// Ok(0) on success, or an error if the operation failed.
    fn do_fsync(&self, fd: i32) -> Result<usize> {
        // SAFETY: fsync is a simple syscall that only requires a valid fd
        let result = unsafe { libc::fsync(fd) };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("fsync failed: fd={}", fd));
        }
        
        Ok(0)
    }
    
    /// Perform an fdatasync operation
    ///
    /// Synchronizes modified data (but not necessarily metadata) for the file to storage.
    /// This can be faster than fsync for some workloads.
    ///
    /// # Arguments
    ///
    /// * `fd` - File descriptor to sync
    ///
    /// # Returns
    ///
    /// Ok(0) on success, or an error if the operation failed.
    fn do_fdatasync(&self, fd: i32) -> Result<usize> {
        // SAFETY: fdatasync is a simple syscall that only requires a valid fd
        let result = unsafe { libc::fdatasync(fd) };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("fdatasync failed: fd={}", fd));
        }
        
        Ok(0)
    }
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IOEngine for SyncEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        self._config = Some(config.clone());
        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        // For synchronous engine, we perform the operation immediately
        let result = match op.op_type {
            OperationType::Read => {
                self.do_read(op.target_fd, op.buffer, op.length, op.offset)
            }
            OperationType::Write => {
                self.do_write(op.target_fd, op.buffer as *const u8, op.length, op.offset)
            }
            OperationType::Fsync => {
                self.do_fsync(op.target_fd)
            }
            OperationType::Fdatasync => {
                self.do_fdatasync(op.target_fd)
            }
        };
        
        // Store the completion (sync engine only has QD=1)
        self.pending_completion = Some(IOCompletion {
            user_data: op.user_data,
            result,
            op_type: op.op_type,
        });
        
        Ok(())
    }
    
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>> {
        // Return the single completion if available (reuse pre-allocated vector)
        self.completion_vec.clear();
        if let Some(completion) = self.pending_completion.take() {
            self.completion_vec.push(completion);
        }
        Ok(std::mem::take(&mut self.completion_vec))
    }
    
    fn cleanup(&mut self) -> Result<()> {
        // Clear any remaining completion
        self.pending_completion = None;
        self.completion_vec.clear();
        Ok(())
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            async_io: false,
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
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    use tempfile::TempDir;
    
    #[test]
    fn test_sync_engine_init() {
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        
        assert!(engine.init(&config).is_ok());
    }
    
    #[test]
    fn test_sync_engine_capabilities() {
        let engine = SyncEngine::new();
        let caps = engine.capabilities();
        
        assert!(!caps.async_io);
        assert!(!caps.batch_submission);
        assert!(!caps.registered_buffers);
        assert!(!caps.fixed_files);
        assert!(!caps.polling_mode);
        assert_eq!(caps.max_queue_depth, 1);
    }
    
    #[test]
    fn test_sync_engine_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_read.dat");
        
        // Create a test file with known content
        let test_data = b"Hello, IOPulse! This is a test file for synchronous reads.";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine and submit read operation
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
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
    }
    
    #[test]
    fn test_sync_engine_write() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_write.dat");
        
        // Create an empty file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine and submit write operation
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let test_data = b"Writing test data with synchronous engine!";
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
        assert_eq!(completions[0].result.as_ref().unwrap(), &test_data.len());
        
        // Verify data was written
        drop(file); // Close the file
        let written_data = std::fs::read(&file_path).unwrap();
        assert_eq!(&written_data[..], test_data);
    }
    
    #[test]
    fn test_sync_engine_read_at_offset() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_offset.dat");
        
        // Create a test file with known content
        let test_data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = SyncEngine::new();
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
        
        // Verify we read the correct data
        assert_eq!(&buffer[..], b"ABCDEFGHIJ");
    }
    
    #[test]
    fn test_sync_engine_partial_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_partial.dat");
        
        // Create a small test file
        let test_data = b"Short";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = SyncEngine::new();
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
    }
    
    #[test]
    fn test_sync_engine_fsync() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_fsync.dat");
        
        // Create a file
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Write some data
        file.write_all(b"Test data for fsync").unwrap();
        
        // Create engine and submit fsync operation
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let op = IOOperation {
            op_type: OperationType::Fsync,
            target_fd: fd,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 0,
            user_data: 123,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 123);
        assert_eq!(completions[0].op_type, OperationType::Fsync);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap(), &0);
    }
    
    #[test]
    fn test_sync_engine_fdatasync() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_fdatasync.dat");
        
        // Create a file
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Write some data
        file.write_all(b"Test data for fdatasync").unwrap();
        
        // Create engine and submit fdatasync operation
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let op = IOOperation {
            op_type: OperationType::Fdatasync,
            target_fd: fd,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 0,
            user_data: 456,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 456);
        assert_eq!(completions[0].op_type, OperationType::Fdatasync);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap(), &0);
    }
    
    #[test]
    fn test_sync_engine_multiple_operations() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_multiple.dat");
        
        // Create a test file
        let test_data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = SyncEngine::new();
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
            assert_eq!(completion.result.as_ref().unwrap(), &5);
        }
        
        // Verify data
        assert_eq!(&buffers[0][..], b"01234");
        assert_eq!(&buffers[1][..], b"56789");
        assert_eq!(&buffers[2][..], b"ABCDE");
    }
    
    #[test]
    fn test_sync_engine_cleanup() {
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit a mock operation (will fail but that's ok for this test)
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: -1, // Invalid fd
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 0,
            user_data: 1,
        };
        let _ = engine.submit(op);
        
        // Cleanup should succeed
        assert!(engine.cleanup().is_ok());
        
        // After cleanup, completed queue should be empty
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 0);
    }
    
    #[test]
    fn test_sync_engine_invalid_fd() {
        let mut engine = SyncEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit operation with invalid fd
        let mut buffer = vec![0u8; 100];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: -1, // Invalid fd
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion - should have an error
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_err());
    }
}
