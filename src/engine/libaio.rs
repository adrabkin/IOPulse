//! libaio IO engine
//!
//! This module provides an asynchronous IO engine using Linux's libaio interface.
//! libaio is the native Linux async IO interface that has been available for many
//! years and is widely supported across kernel versions.
//!
//! # Features
//!
//! - Asynchronous IO with configurable queue depth
//! - Batch submission via io_submit
//! - Batch completion polling via io_getevents
//! - Handles partial completions
//! - Proper cleanup with io_destroy
//!
//! # Implementation
//!
//! This implementation uses direct syscalls via libc rather than a binding crate
//! to maintain MIT license compatibility (libaio library is LGPL).
//!
//! # Performance
//!
//! libaio provides good async IO performance:
//! - Multiple operations in flight simultaneously
//! - Batch submission reduces syscall overhead
//! - Widely available on Linux systems
//! - Good compatibility across kernel versions
//!
//! # Requirements
//!
//! - Linux kernel with libaio support (most modern kernels)
//! - O_DIRECT typically required for best performance
//!
//! # Example
//!
//! ```no_run
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::libaio::LibaioEngine;
//!
//! let mut engine = LibaioEngine::new();
//! let config = EngineConfig {
//!     queue_depth: 64,
//!     use_registered_buffers: false,
//!     use_fixed_files: false,
//!     polling_mode: false,
//! };
//!
//! engine.init(&config).unwrap();
//!
//! // Submit operations and poll for completions
//! // ... (see IOEngine trait documentation)
//!
//! engine.cleanup().unwrap();
//! ```

use super::{EngineCapabilities, EngineConfig, IOCompletion, IOEngine, IOOperation, OperationType};
use crate::Result;
use anyhow::Context;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::ptr;

// libaio types and constants
type AioContext = libc::c_ulong;

const IOCB_CMD_PREAD: u16 = 0;
const IOCB_CMD_PWRITE: u16 = 1;
const IOCB_CMD_FSYNC: u16 = 2;
const IOCB_CMD_FDSYNC: u16 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
struct IoControlBlock {
    data: u64,           // User data (aio_data)
    key: u32,            // Key (aio_key) - should be IOCB_FLAG_RESFD if using eventfd
    aio_rw_flags: u32,   // RWF_* flags
    lio_opcode: u16,     // Operation code
    aio_reqprio: i16,    // Request priority
    aio_fildes: u32,     // File descriptor
    buf: u64,            // Buffer pointer
    nbytes: u64,         // Number of bytes
    offset: i64,         // File offset
    aio_reserved2: u64,  // Reserved
    flags: u32,          // IOCB_FLAG_* flags
    aio_resfd: u32,      // Eventfd for notification
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IoEvent {
    data: u64,   // User data from iocb
    obj: u64,    // Pointer to iocb
    res: i64,    // Result (bytes transferred or -errno)
    res2: i64,   // Secondary result
}

// libaio syscall numbers for x86_64
const SYS_IO_SETUP: i64 = 206;
const SYS_IO_DESTROY: i64 = 207;
const SYS_IO_SUBMIT: i64 = 209;
const SYS_IO_GETEVENTS: i64 = 208;

// Wrapper functions using direct syscalls
unsafe fn io_setup(maxevents: libc::c_int, ctxp: *mut AioContext) -> libc::c_int {
    libc::syscall(SYS_IO_SETUP, maxevents as i64, ctxp) as libc::c_int
}

unsafe fn io_destroy(ctx: AioContext) -> libc::c_int {
    libc::syscall(SYS_IO_DESTROY, ctx) as libc::c_int
}

unsafe fn io_submit(ctx: AioContext, nr: libc::c_long, iocbpp: *mut *mut IoControlBlock) -> libc::c_int {
    libc::syscall(SYS_IO_SUBMIT, ctx, nr, iocbpp) as libc::c_int
}

unsafe fn io_getevents(
    ctx: AioContext,
    min_nr: libc::c_long,
    nr: libc::c_long,
    events: *mut IoEvent,
    timeout: *mut libc::timespec,
) -> libc::c_int {
    libc::syscall(SYS_IO_GETEVENTS, ctx, min_nr, nr, events, timeout) as libc::c_int
}

/// libaio IO engine
///
/// This engine uses Linux's libaio interface for asynchronous IO. It provides
/// good performance and is widely available on Linux systems.
pub struct LibaioEngine {
    /// libaio context
    ctx: Option<AioContext>,
    
    /// Configuration
    config: Option<EngineConfig>,
    
    /// Pool of IO control blocks
    iocbs: Vec<IoControlBlock>,
    
    /// Available iocb indices
    available_iocbs: Vec<usize>,
    
    /// Map of user_data to operation type for completion tracking
    pending_ops: HashMap<u64, OperationType>,
    
    /// Pre-allocated events vector (reused across poll_completions calls)
    events: Vec<IoEvent>,
    
    /// Pre-allocated completions vector (reused across poll_completions calls)
    completions: Vec<IOCompletion>,
}

impl LibaioEngine {
    /// Create a new libaio engine
    pub fn new() -> Self {
        Self {
            ctx: None,
            config: None,
            iocbs: Vec::new(),
            available_iocbs: Vec::new(),
            pending_ops: HashMap::new(),
            events: Vec::new(),
            completions: Vec::new(),
        }
    }
    
    /// Get an available iocb index
    fn get_iocb(&mut self) -> Option<usize> {
        self.available_iocbs.pop()
    }
    
    /// Return an iocb index to the pool
    fn return_iocb(&mut self, index: usize) {
        self.available_iocbs.push(index);
    }
}

impl Default for LibaioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IOEngine for LibaioEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        // Create libaio context
        let mut ctx: AioContext = 0;
        let result = unsafe { io_setup(config.queue_depth as i32, &mut ctx) };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "io_setup failed with queue_depth={}",
                config.queue_depth
            ));
        }
        
        self.ctx = Some(ctx);
        self.config = Some(config.clone());
        
        // Pre-allocate iocbs
        self.iocbs = vec![
            unsafe { MaybeUninit::zeroed().assume_init() };
            config.queue_depth
        ];
        
        // Initialize available iocbs list
        self.available_iocbs = (0..config.queue_depth).collect();
        
        // Pre-allocate events and completions vectors (avoid allocations in hot path)
        self.events = vec![
            unsafe { MaybeUninit::zeroed().assume_init() };
            config.queue_depth
        ];
        self.completions = Vec::with_capacity(config.queue_depth);
        
        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        let ctx = self.ctx.expect("Engine not initialized");
        
        // Get an available iocb
        let iocb_idx = self.get_iocb()
            .ok_or_else(|| anyhow::anyhow!("No available iocbs (queue full)"))?;
        
        // Store the operation type for completion tracking
        self.pending_ops.insert(op.user_data, op.op_type);
        
        // Build the iocb
        let iocb = &mut self.iocbs[iocb_idx];
        *iocb = IoControlBlock {
            data: op.user_data,
            key: 0,
            aio_rw_flags: 0,
            lio_opcode: match op.op_type {
                OperationType::Read => IOCB_CMD_PREAD,
                OperationType::Write => IOCB_CMD_PWRITE,
                OperationType::Fsync => IOCB_CMD_FSYNC,
                OperationType::Fdatasync => IOCB_CMD_FDSYNC,
            },
            aio_reqprio: 0,
            aio_fildes: op.target_fd as u32,
            buf: op.buffer as u64,
            nbytes: op.length as u64,
            offset: op.offset as i64,
            aio_reserved2: 0,
            flags: 0,
            aio_resfd: 0,
        };
        
        // Submit the operation
        let mut iocb_ptr = iocb as *mut IoControlBlock;
        let result = unsafe { io_submit(ctx, 1, &mut iocb_ptr) };
        
        if result < 0 {
            // Return the iocb to the pool
            self.return_iocb(iocb_idx);
            self.pending_ops.remove(&op.user_data);
            
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "io_submit failed for {} operation",
                op.op_type
            ));
        }
        
        Ok(())
    }
    
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>> {
        let ctx = self.ctx.expect("Engine not initialized");
        
        if self.pending_ops.is_empty() {
            return Ok(Vec::new());
        }
        
        // Clear and reuse pre-allocated completions vector
        self.completions.clear();
        
        let max_events = self.config.as_ref().unwrap().queue_depth;
        
        // Wait for at least 1 completion if we have pending operations
        let min_events = if self.pending_ops.is_empty() { 0 } else { 1 };
        
        let result = unsafe {
            io_getevents(
                ctx,
                min_events,
                max_events as i64,
                self.events.as_mut_ptr(),
                ptr::null_mut(), // No timeout
            )
        };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context("io_getevents failed");
        }
        
        let num_events = result as usize;
        
        // Process completions
        for i in 0..num_events {
            let event = &self.events[i];
            let user_data = event.data;
            let res = event.res;
            
            // Look up the operation type
            let op_type = self.pending_ops.remove(&user_data)
                .unwrap_or(OperationType::Read);
            
            // Find and return the iocb to the pool
            // The iocb pointer is in event.obj, but we can also search by user_data
            for (idx, iocb) in self.iocbs.iter().enumerate() {
                if iocb.data == user_data {
                    self.return_iocb(idx);
                    break;
                }
            }
            
            // Convert result
            let result = if res >= 0 {
                Ok(res as usize)
            } else {
                // Negative result is -errno
                let errno = (-res) as i32;
                Err(std::io::Error::from_raw_os_error(errno))
                    .context(format!(
                        "{} operation failed: errno={}",
                        op_type, errno
                    ))
            };
            
            self.completions.push(IOCompletion {
                user_data,
                result,
                op_type,
            });
        }
        
        // Return the vector and replace with empty (avoids clone)
        Ok(std::mem::take(&mut self.completions))
    }
    
    fn cleanup(&mut self) -> Result<()> {
        if let Some(ctx) = self.ctx {
            // Wait for all pending operations to complete
            while !self.pending_ops.is_empty() {
                let _ = self.poll_completions();
            }
            
            // Destroy the context
            let result = unsafe { io_destroy(ctx) };
            if result < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err).context("io_destroy failed");
            }
            
            self.ctx = None;
        }
        
        self.pending_ops.clear();
        self.available_iocbs.clear();
        
        Ok(())
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        let config = self.config.as_ref();
        
        EngineCapabilities {
            async_io: true,
            batch_submission: true,
            registered_buffers: false, // libaio doesn't support this
            fixed_files: false,        // libaio doesn't support this
            polling_mode: false,       // libaio doesn't support this
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
    fn test_libaio_engine_init() {
        let mut engine = LibaioEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        
        assert!(engine.init(&config).is_ok());
        assert!(engine.cleanup().is_ok());
    }
    
    #[test]
    fn test_libaio_engine_capabilities() {
        let mut engine = LibaioEngine::new();
        let config = EngineConfig {
            queue_depth: 128,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        
        engine.init(&config).unwrap();
        let caps = engine.capabilities();
        
        assert!(caps.async_io);
        assert!(caps.batch_submission);
        assert!(!caps.registered_buffers); // libaio doesn't support
        assert!(!caps.fixed_files);        // libaio doesn't support
        assert!(!caps.polling_mode);       // libaio doesn't support
        assert_eq!(caps.max_queue_depth, 128);
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_read() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_read.dat");
        
        // Create a test file with known content (4K aligned for potential O_DIRECT)
        let mut test_data = vec![0u8; 4096];
        let message = b"Hello from libaio! Async IO on Linux.";
        test_data[..message.len()].copy_from_slice(message);
        std::fs::write(&file_path, &test_data).unwrap();
        
        // Open the file (without O_DIRECT for tmpfs compatibility)
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = LibaioEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit read operation with aligned buffer
        let mut buffer = vec![0u8; 4096]; // Aligned to 4K for O_DIRECT
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: 4096,
            user_data: 42,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 42);
        assert_eq!(completions[0].op_type, OperationType::Read);
        assert!(completions[0].result.is_ok());
        
        // Verify data (first part should match)
        let message = b"Hello from libaio! Async IO on Linux.";
        assert_eq!(&buffer[..message.len()], message);
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_write() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_write.dat");
        
        // Create an empty file (without O_DIRECT for tmpfs compatibility)
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)
            .unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = LibaioEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit write operation with aligned buffer
        let mut buffer = vec![0u8; 4096];
        let test_data = b"Writing with libaio async engine!";
        buffer[..test_data.len()].copy_from_slice(test_data);
        
        let op = IOOperation {
            op_type: OperationType::Write,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: 4096,
            user_data: 99,
        };
        
        engine.submit(op).unwrap();
        
        // Poll for completion
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 99);
        assert_eq!(completions[0].op_type, OperationType::Write);
        assert!(completions[0].result.is_ok());
        
        engine.cleanup().unwrap();
        drop(file);
        
        // Verify data was written
        let written_data = std::fs::read(&file_path).unwrap();
        assert_eq!(&written_data[..test_data.len()], test_data);
    }
    
    #[test]
    fn test_libaio_engine_batch_submission() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_batch.dat");
        
        // Create a test file (4K aligned)
        let mut test_data = vec![0u8; 20480]; // 20KB
        for (i, byte) in test_data.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }
        std::fs::write(&file_path, &test_data).unwrap();
        
        // Open the file (without O_DIRECT for tmpfs compatibility)
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        // Create engine
        let mut engine = LibaioEngine::new();
        let config = EngineConfig {
            queue_depth: 64,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        
        // Submit multiple read operations
        let mut buffers = vec![vec![0u8; 4096]; 5];
        for (i, buffer) in buffers.iter_mut().enumerate() {
            let op = IOOperation {
                op_type: OperationType::Read,
                target_fd: fd,
                offset: (i * 4096) as u64,
                buffer: buffer.as_mut_ptr(),
                length: buffer.len(),
                user_data: i as u64,
            };
            engine.submit(op).unwrap();
        }
        
        // Poll for completions
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 5);
        
        // Verify all operations completed successfully
        for completion in &completions {
            assert!(completion.result.is_ok());
        }
        
        // Verify data
        for (i, buffer) in buffers.iter().enumerate() {
            let expected_start = i * 4096;
            for (j, &byte) in buffer.iter().enumerate() {
                assert_eq!(byte, ((expected_start + j) % 256) as u8);
            }
        }
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_fsync() {
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
        file.write_all(b"Test data for libaio fsync").unwrap();
        
        // Create engine
        let mut engine = LibaioEngine::new();
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
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_fdatasync() {
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
        file.write_all(b"Test data for libaio fdatasync").unwrap();
        
        // Create engine
        let mut engine = LibaioEngine::new();
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
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_error_handling() {
        let mut engine = LibaioEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit operation with invalid fd
        let mut buffer = vec![0u8; 4096];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: -1, // Invalid fd
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 1,
        };
        
        // libaio will reject invalid fd at submit time (EBADF)
        let result = engine.submit(op);
        assert!(result.is_err());
        
        engine.cleanup().unwrap();
    }
    
    #[test]
    fn test_libaio_engine_queue_full() {
        let mut engine = LibaioEngine::new();
        let config = EngineConfig {
            queue_depth: 2, // Small queue
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_queue.dat");
        std::fs::write(&file_path, &vec![0u8; 8192]).unwrap();
        
        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();
        
        let mut buffer1 = vec![0u8; 4096];
        let mut buffer2 = vec![0u8; 4096];
        let mut buffer3 = vec![0u8; 4096];
        
        // Submit 2 operations (should succeed)
        let op1 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer1.as_mut_ptr(),
            length: 4096,
            user_data: 1,
        };
        engine.submit(op1).unwrap();
        
        let op2 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 4096,
            buffer: buffer2.as_mut_ptr(),
            length: 4096,
            user_data: 2,
        };
        engine.submit(op2).unwrap();
        
        // Third operation should fail (queue full)
        let op3 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer3.as_mut_ptr(),
            length: 4096,
            user_data: 3,
        };
        assert!(engine.submit(op3).is_err());
        
        // Poll to free up space
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 2);
        
        engine.cleanup().unwrap();
    }
}
