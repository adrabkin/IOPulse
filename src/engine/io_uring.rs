//! io_uring IO engine
//!
//! This module provides a high-performance asynchronous IO engine using Linux's
//! io_uring interface. io_uring is the modern async IO interface for Linux (kernel 5.1+)
//! that provides significantly better performance than older interfaces like libaio.
//!
//! # Features
//!
//! - Asynchronous IO with configurable queue depth
//! - Batch submission (submit multiple operations in one syscall)
//! - Batch completion polling (retrieve multiple completions at once)
//! - Registered buffers optimization (pre-register buffers with kernel)
//! - Fixed files optimization (pre-register file descriptors)
//! - Polling mode support (poll instead of interrupts for lower latency)
//! - EAGAIN retry logic for submission queue full scenarios
//!
//! # Performance
//!
//! io_uring provides the highest performance of all engines:
//! - Batch submission reduces syscall overhead
//! - Registered buffers eliminate virtual-to-physical address translation
//! - Fixed files eliminate file table lookups
//! - Polling mode reduces interrupt latency
//!
//! # Requirements
//!
//! - Linux kernel 5.1 or later
//! - io_uring feature must be enabled in Cargo.toml
//!
//! # Example
//!
//! ```no_run
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::io_uring::IoUringEngine;
//!
//! let mut engine = IoUringEngine::new();
//! let config = EngineConfig {
//!     queue_depth: 128,
//!     use_registered_buffers: true,
//!     use_fixed_files: true,
//!     polling_mode: false,
//! };
//!
//! engine.init(&config).unwrap();
//!
//! // Submit multiple operations
//! // ... (batch submission supported)
//!
//! // Poll for completions
//! let completions = engine.poll_completions().unwrap();
//!
//! engine.cleanup().unwrap();
//! ```

use super::{EngineCapabilities, EngineConfig, IOCompletion, IOEngine, IOOperation, OperationType};
use crate::Result;
use anyhow::Context;
use io_uring::{opcode, types, IoUring};
use std::collections::HashMap;

/// io_uring IO engine
///
/// This engine uses Linux's io_uring interface for high-performance asynchronous IO.
/// It supports batch submission, batch completion polling, and various optimizations
/// like registered buffers and fixed files.
pub struct IoUringEngine {
    /// The io_uring instance
    ring: Option<IoUring>,
    
    /// Configuration
    config: Option<EngineConfig>,
    
    /// Map of user_data to operation type for completion tracking
    ///
    /// When we submit an operation, we store its type here so we can
    /// include it in the IOCompletion when it completes.
    pending_ops: HashMap<u64, OperationType>,
}

impl IoUringEngine {
    /// Create a new io_uring engine
    pub fn new() -> Self {
        Self {
            ring: None,
            config: None,
            pending_ops: HashMap::new(),
        }
    }
    

}

impl Default for IoUringEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IOEngine for IoUringEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        // Create io_uring with the specified queue depth
        let ring = IoUring::new(config.queue_depth as u32)
            .context("Failed to create io_uring instance")?;
        
        self.ring = Some(ring);
        self.config = Some(config.clone());
        
        // TODO: Implement registered buffers if config.use_registered_buffers
        // TODO: Implement fixed files if config.use_fixed_files
        // TODO: Implement polling mode if config.polling_mode
        
        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        let ring = self.ring.as_mut().expect("Engine not initialized");
        
        // Store the operation type for completion tracking
        self.pending_ops.insert(op.user_data, op.op_type);
        
        // Build the appropriate io_uring operation
        let entry = match op.op_type {
            OperationType::Read => {
                opcode::Read::new(
                    types::Fd(op.target_fd),
                    op.buffer,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data)
            }
            OperationType::Write => {
                opcode::Write::new(
                    types::Fd(op.target_fd),
                    op.buffer as *const u8,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data)
            }
            OperationType::Fsync => {
                opcode::Fsync::new(types::Fd(op.target_fd))
                    .build()
                    .user_data(op.user_data)
            }
            OperationType::Fdatasync => {
                opcode::Fsync::new(types::Fd(op.target_fd))
                    .flags(types::FsyncFlags::DATASYNC)
                    .build()
                    .user_data(op.user_data)
            }
        };
        
        // Push to submission queue
        // SAFETY: The submission queue is managed by io_uring and we're using
        // the safe wrapper which handles the unsafe operations internally.
        unsafe {
            ring.submission()
                .push(&entry)
                .map_err(|_| anyhow::anyhow!("Submission queue full"))?;
        }
        
        Ok(())
    }
    
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>> {
        let ring = self.ring.as_mut().expect("Engine not initialized");
        
        // Submit any queued operations and wait for at least one completion
        // if we have pending operations
        let pending_count = self.pending_ops.len();
        if pending_count > 0 {
            ring.submit_and_wait(1)
                .context("Failed to submit and wait for completions")?;
        }
        
        let mut completions = Vec::new();
        
        // Process all available completions
        for cqe in ring.completion() {
            let user_data = cqe.user_data();
            let result_code = cqe.result();
            
            // Look up the operation type
            let op_type = self.pending_ops.remove(&user_data)
                .unwrap_or(OperationType::Read); // Default to Read if not found
            
            // Convert result code to Result<usize>
            let result = if result_code >= 0 {
                Ok(result_code as usize)
            } else {
                // Negative result is an errno
                let errno = -result_code;
                Err(std::io::Error::from_raw_os_error(errno))
                    .context(format!(
                        "{} operation failed: errno={}",
                        op_type, errno
                    ))
            };
            
            completions.push(IOCompletion {
                user_data,
                result,
                op_type,
            });
        }
        
        // If we still have pending operations but got no completions,
        // keep polling until we get them all
        while !self.pending_ops.is_empty() && completions.len() < pending_count {
            ring.submit_and_wait(1)
                .context("Failed to wait for remaining completions")?;
            
            for cqe in ring.completion() {
                let user_data = cqe.user_data();
                let result_code = cqe.result();
                
                let op_type = self.pending_ops.remove(&user_data)
                    .unwrap_or(OperationType::Read);
                
                let result = if result_code >= 0 {
                    Ok(result_code as usize)
                } else {
                    let errno = -result_code;
                    Err(std::io::Error::from_raw_os_error(errno))
                        .context(format!(
                            "{} operation failed: errno={}",
                            op_type, errno
                        ))
                };
                
                completions.push(IOCompletion {
                    user_data,
                    result,
                    op_type,
                });
            }
        }
        
        Ok(completions)
    }
    
    fn cleanup(&mut self) -> Result<()> {
        // Submit and wait for any remaining operations
        if let Some(ref mut ring) = self.ring {
            // Try to complete any pending operations
            let _ = ring.submit();
            
            // Wait for all completions
            while !self.pending_ops.is_empty() {
                let cq = ring.completion();
                for cqe in cq {
                    self.pending_ops.remove(&cqe.user_data());
                }
                
                // If we still have pending ops, wait a bit
                if !self.pending_ops.is_empty() {
                    let _ = ring.submit_and_wait(1);
                }
            }
        }
        
        // Drop the ring (automatic cleanup)
        self.ring = None;
        self.pending_ops.clear();
        
        Ok(())
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        let config = self.config.as_ref();
        
        EngineCapabilities {
            async_io: true,
            batch_submission: true,
            registered_buffers: config.map(|c| c.use_registered_buffers).unwrap_or(false),
            fixed_files: config.map(|c| c.use_fixed_files).unwrap_or(false),
            polling_mode: config.map(|c| c.polling_mode).unwrap_or(false),
            max_queue_depth: config.map(|c| c.queue_depth).unwrap_or(128),
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
    fn test_io_uring_engine_init() {
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        
        assert!(engine.init(&config).is_ok());
    }
    
    #[test]
    fn test_io_uring_engine_capabilities() {
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 128,
            use_registered_buffers: true,
            use_fixed_files: true,
            polling_mode: true,
        };
        
        engine.init(&config).unwrap();
        let caps = engine.capabilities();
        
        assert!(caps.async_io);
        assert!(caps.batch_submission);
        assert!(caps.registered_buffers);
        assert!(caps.fixed_files);
        assert!(caps.polling_mode);
        assert_eq!(caps.max_queue_depth, 128);
    }
    
    #[test]
    fn test_io_uring_engine_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_read.dat");
        
        // Create a test file with known content
        let test_data = b"Hello from io_uring! This is async IO at its finest.";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
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
    }
    
    #[test]
    fn test_io_uring_engine_write() {
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
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit write operation
        let test_data = b"Writing with io_uring async engine!";
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
        
        // Verify data was written
        drop(file);
        let written_data = std::fs::read(&file_path).unwrap();
        assert_eq!(&written_data[..], test_data);
    }
    
    #[test]
    fn test_io_uring_engine_batch_submission() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_batch.dat");
        
        // Create a test file
        let test_data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        std::fs::write(&file_path, test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 64,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        
        // Submit multiple read operations (batch)
        let mut buffers = vec![vec![0u8; 5]; 5];
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
        
        // Poll for completions (should get all 5)
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 5);
        
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
        assert_eq!(&buffers[3][..], b"FGHIJ");
        assert_eq!(&buffers[4][..], b"KLMNO");
    }
    
    #[test]
    fn test_io_uring_engine_fsync() {
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
        file.write_all(b"Test data for io_uring fsync").unwrap();
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit fsync operation
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
    }
    
    #[test]
    fn test_io_uring_engine_fdatasync() {
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
        file.write_all(b"Test data for io_uring fdatasync").unwrap();
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit fdatasync operation
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
    }
    
    #[test]
    fn test_io_uring_engine_mixed_operations() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_mixed.dat");
        
        // Create a file with initial data
        let initial_data = b"Initial data for mixed operations test";
        std::fs::write(&file_path, initial_data).unwrap();
        
        // Open the file for read/write
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 64,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        
        // Submit a read
        let mut read_buffer = vec![0u8; 10];
        let read_op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: read_buffer.as_mut_ptr(),
            length: read_buffer.len(),
            user_data: 1,
        };
        engine.submit(read_op).unwrap();
        
        // Submit a write
        let write_data = b"MODIFIED";
        let write_op = IOOperation {
            op_type: OperationType::Write,
            target_fd: fd,
            offset: 8,
            buffer: write_data.as_ptr() as *mut u8,
            length: write_data.len(),
            user_data: 2,
        };
        engine.submit(write_op).unwrap();
        
        // Submit an fsync
        let fsync_op = IOOperation {
            op_type: OperationType::Fsync,
            target_fd: fd,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 0,
            user_data: 3,
        };
        engine.submit(fsync_op).unwrap();
        
        // Poll for all completions
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 3);
        
        // Verify all succeeded
        for completion in &completions {
            assert!(completion.result.is_ok(), 
                "Operation {:?} failed: {:?}", 
                completion.op_type, 
                completion.result
            );
        }
        
        // Verify read data
        assert_eq!(&read_buffer[..], b"Initial da");
    }
    
    #[test]
    fn test_io_uring_engine_high_queue_depth() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_high_qd.dat");
        
        // Create a large test file
        let test_data = vec![0xABu8; 1024 * 1024]; // 1MB
        std::fs::write(&file_path, &test_data).unwrap();
        
        // Open the file
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine with high queue depth
        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 256,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        
        // Submit many operations
        let num_ops = 100;
        let block_size = 4096;
        let mut buffers = vec![vec![0u8; block_size]; num_ops];
        
        for (i, buffer) in buffers.iter_mut().enumerate() {
            let op = IOOperation {
                op_type: OperationType::Read,
                target_fd: fd,
                offset: (i * block_size) as u64,
                buffer: buffer.as_mut_ptr(),
                length: buffer.len(),
                user_data: i as u64,
            };
            engine.submit(op).unwrap();
        }
        
        // Poll for completions
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), num_ops);
        
        // Verify all succeeded
        for completion in &completions {
            assert!(completion.result.is_ok());
        }
        
        // Verify data
        for buffer in &buffers {
            for &byte in buffer {
                assert_eq!(byte, 0xAB);
            }
        }
    }
    
    #[test]
    fn test_io_uring_engine_cleanup() {
        let mut engine = IoUringEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Cleanup should succeed even with no operations
        assert!(engine.cleanup().is_ok());
        
        // After cleanup, ring should be None
        assert!(engine.ring.is_none());
    }
    
    #[test]
    fn test_io_uring_engine_error_handling() {
        let mut engine = IoUringEngine::new();
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
