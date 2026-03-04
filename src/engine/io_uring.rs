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
use std::os::unix::io::RawFd;

/// Maximum number of files that can be pre-registered with the fixed files feature.
const MAX_REGISTERED_FILES: u32 = 1024;

/// SQPOLL idle timeout in milliseconds before the kernel polling thread sleeps.
///
/// While the thread is awake it polls the submission queue without any syscalls.
/// After this many milliseconds of idleness it sleeps, and the next wakeup costs
/// one extra syscall.
const SQPOLL_IDLE_MS: u32 = 2000;

/// io_uring IO engine
///
/// This engine uses Linux's io_uring interface for high-performance asynchronous IO.
/// It supports batch submission, batch completion polling, and three major
/// kernel-side optimizations:
///
/// - **SQPOLL** (`polling_mode`): A dedicated kernel thread polls the submission
///   queue, eliminating the `io_uring_enter` syscall on every submit.
/// - **Fixed files** (`use_fixed_files`): File descriptors are pre-registered with
///   the kernel, replacing per-operation fd table lookups with a direct array index.
/// - **Registered buffers** (`use_registered_buffers`): Buffer memory regions are
///   pinned and registered with the kernel, eliminating repeated virtual-to-physical
///   address translation per IO.
pub struct IoUringEngine {
    /// The io_uring instance
    ring: Option<IoUring>,

    /// Configuration
    config: Option<EngineConfig>,

    /// Map of user_data to operation type for completion tracking
    pending_ops: HashMap<u64, OperationType>,

    // --- Fixed files state ---
    /// Map from RawFd to its slot index in the registered file table
    registered_files: HashMap<RawFd, u32>,
    /// Next available slot in the registered file table
    next_file_slot: u32,

    // --- Registered buffers state ---
    /// Map from buffer pointer (as usize) to its registered buffer index
    registered_buffers: HashMap<usize, u16>,
    /// All currently registered iovecs, in index order
    registered_bufs_iovecs: Vec<libc::iovec>,
    /// Buffers seen in submit() but not yet registered: (ptr, len)
    ///
    /// Deferred until after `poll_completions()` drains all in-flight ops,
    /// because `register_buffers` requires no ops in-flight in the kernel.
    pending_buf_registrations: Vec<(usize, usize)>,
}

impl IoUringEngine {
    /// Create a new io_uring engine
    pub fn new() -> Self {
        Self {
            ring: None,
            config: None,
            pending_ops: HashMap::new(),
            registered_files: HashMap::new(),
            next_file_slot: 0,
            registered_buffers: HashMap::new(),
            registered_bufs_iovecs: Vec::new(),
            pending_buf_registrations: Vec::new(),
        }
    }

    /// Flush deferred buffer registrations.
    ///
    /// Must only be called when `pending_ops` is empty (no ops in-flight in the
    /// kernel), because `io_uring_register(IORING_REGISTER_BUFFERS)` requires
    /// quiescence.
    fn flush_buffer_registrations(&mut self) -> Result<()> {
        if self.pending_buf_registrations.is_empty() {
            return Ok(());
        }

        // Move pending into the persistent iovec vec, skipping duplicates.
        let pending: Vec<(usize, usize)> = self.pending_buf_registrations.drain(..).collect();
        for (ptr, len) in pending {
            if !self.registered_buffers.contains_key(&ptr) {
                let index = self.registered_bufs_iovecs.len() as u16;
                self.registered_bufs_iovecs.push(libc::iovec {
                    iov_base: ptr as *mut libc::c_void,
                    iov_len: len,
                });
                self.registered_buffers.insert(ptr, index);
            }
        }

        if self.registered_bufs_iovecs.is_empty() {
            return Ok(());
        }

        // Unregister any previous registration, then register the full updated set.
        // SAFETY: The iovecs point to caller-managed memory that lives at least as
        // long as the engine.  We verify no ops are in-flight before calling this
        // (pending_ops empty).
        {
            let ring = self.ring.as_ref().unwrap();
            let _ = ring.submitter().unregister_buffers(); // ignore ENXIO on first call
            unsafe {
                ring.submitter()
                    .register_buffers(&self.registered_bufs_iovecs)
                    .context("Failed to register buffers with io_uring")?;
            }
        }

        Ok(())
    }
}

impl Default for IoUringEngine {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: IoUringEngine is used from a single thread at a time (each worker
// owns its own engine instance).  The raw pointers stored in registered_bufs_iovecs
// point to caller-managed buffers; the caller is responsible for ensuring those
// buffers outlive the engine and are not aliased.
unsafe impl Send for IoUringEngine {}

impl IOEngine for IoUringEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        // Create io_uring, optionally with SQPOLL for kernel-side SQ polling.
        //
        // SQPOLL spins a dedicated kernel thread that polls the submission queue,
        // eliminating the io_uring_enter syscall for every submit.  The thread
        // sleeps after SQPOLL_IDLE_MS ms of inactivity and is woken on the next
        // submission (one extra syscall).
        //
        // Note: on kernels < 5.11, SQPOLL requires CAP_SYS_ADMIN.
        let ring = if config.polling_mode {
            IoUring::builder()
                .setup_sqpoll(SQPOLL_IDLE_MS)
                .build(config.queue_depth as u32)
                .context("Failed to create io_uring instance with SQPOLL (may require CAP_SYS_ADMIN on kernels < 5.11)")?
        } else {
            IoUring::new(config.queue_depth as u32)
                .context("Failed to create io_uring instance")?
        };

        // Pre-allocate a sparse fixed-file table so that per-submit
        // register_files_update() calls can fill in individual slots without
        // requiring a full-quiescence re-registration.
        if config.use_fixed_files {
            ring.submitter()
                .register_files_sparse(MAX_REGISTERED_FILES)
                .context("Failed to allocate sparse fixed-file table")?;
        }

        self.ring = Some(ring);
        self.config = Some(config.clone());

        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        let use_fixed_files = self.config.as_ref().map(|c| c.use_fixed_files).unwrap_or(false);
        let use_reg_bufs = self.config.as_ref().map(|c| c.use_registered_buffers).unwrap_or(false);

        // --- Fixed files: lazily register each new fd ---
        //
        // register_files_update() fills one slot in the pre-allocated sparse
        // table without requiring quiescence, so it's safe to call here even
        // when other ops are in-flight.
        let fixed_file_slot: Option<u32> = if use_fixed_files {
            if let Some(&slot) = self.registered_files.get(&op.target_fd) {
                Some(slot)
            } else if self.next_file_slot < MAX_REGISTERED_FILES {
                let slot = self.next_file_slot;
                let registered = {
                    let ring = self.ring.as_ref().expect("Engine not initialized");
                    ring.submitter().register_files_update(slot, &[op.target_fd]).is_ok()
                };
                if registered {
                    self.registered_files.insert(op.target_fd, slot);
                    self.next_file_slot += 1;
                    Some(slot)
                } else {
                    None
                }
            } else {
                None // table full, fall back to regular fd
            }
        } else {
            None
        };

        // --- Registered buffers: look up already-registered buffers ---
        //
        // New buffers are queued for registration and will be registered by
        // flush_buffer_registrations() after poll_completions() drains all
        // in-flight ops (register_buffers requires quiescence).
        let buf_index: Option<u16> = if use_reg_bufs {
            match op.op_type {
                OperationType::Read | OperationType::Write => {
                    let ptr = op.buffer as usize;
                    if let Some(&idx) = self.registered_buffers.get(&ptr) {
                        Some(idx)
                    } else {
                        // Defer registration; use plain Read/Write this time.
                        self.pending_buf_registrations.push((ptr, op.length));
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        // Store the operation type for completion tracking
        self.pending_ops.insert(op.user_data, op.op_type);

        // Build the fd target (Fixed slot or plain Fd)
        let ring = self.ring.as_mut().expect("Engine not initialized");

        // Build the appropriate io_uring SQE, using optimized opcodes where
        // possible (ReadFixed/WriteFixed when buffer is registered; Fixed fd
        // when file is registered).
        let entry = match op.op_type {
            OperationType::Read => match (fixed_file_slot, buf_index) {
                (Some(slot), Some(bidx)) => opcode::ReadFixed::new(
                    types::Fixed(slot),
                    op.buffer,
                    op.length as u32,
                    bidx,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (Some(slot), None) => opcode::Read::new(
                    types::Fixed(slot),
                    op.buffer,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (None, Some(bidx)) => opcode::ReadFixed::new(
                    types::Fd(op.target_fd),
                    op.buffer,
                    op.length as u32,
                    bidx,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (None, None) => opcode::Read::new(
                    types::Fd(op.target_fd),
                    op.buffer,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),
            },

            OperationType::Write => match (fixed_file_slot, buf_index) {
                (Some(slot), Some(bidx)) => opcode::WriteFixed::new(
                    types::Fixed(slot),
                    op.buffer as *const u8,
                    op.length as u32,
                    bidx,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (Some(slot), None) => opcode::Write::new(
                    types::Fixed(slot),
                    op.buffer as *const u8,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (None, Some(bidx)) => opcode::WriteFixed::new(
                    types::Fd(op.target_fd),
                    op.buffer as *const u8,
                    op.length as u32,
                    bidx,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),

                (None, None) => opcode::Write::new(
                    types::Fd(op.target_fd),
                    op.buffer as *const u8,
                    op.length as u32,
                )
                .offset(op.offset)
                .build()
                .user_data(op.user_data),
            },

            OperationType::Fsync => {
                if let Some(slot) = fixed_file_slot {
                    opcode::Fsync::new(types::Fixed(slot)).build().user_data(op.user_data)
                } else {
                    opcode::Fsync::new(types::Fd(op.target_fd)).build().user_data(op.user_data)
                }
            }

            OperationType::Fdatasync => {
                if let Some(slot) = fixed_file_slot {
                    opcode::Fsync::new(types::Fixed(slot))
                        .flags(types::FsyncFlags::DATASYNC)
                        .build()
                        .user_data(op.user_data)
                } else {
                    opcode::Fsync::new(types::Fd(op.target_fd))
                        .flags(types::FsyncFlags::DATASYNC)
                        .build()
                        .user_data(op.user_data)
                }
            }
        };

        // Push to submission queue.
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

        // Once all in-flight ops are done, register any buffers that were
        // deferred during submit().  This is the quiescence window required by
        // register_buffers.
        if self.pending_ops.is_empty() {
            let use_reg_bufs = self.config.as_ref().map(|c| c.use_registered_buffers).unwrap_or(false);
            if use_reg_bufs {
                self.flush_buffer_registrations()?;
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

        // Unregister files and buffers before dropping the ring.
        if let Some(ref ring) = self.ring {
            let _ = ring.submitter().unregister_files();
            let _ = ring.submitter().unregister_buffers();
        }

        // Drop the ring (automatic cleanup)
        self.ring = None;
        self.pending_ops.clear();
        self.registered_files.clear();
        self.next_file_slot = 0;
        self.registered_buffers.clear();
        self.registered_bufs_iovecs.clear();
        self.pending_buf_registrations.clear();

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

    // --- Tests for the three major perf features ---

    #[test]
    fn test_io_uring_engine_fixed_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_fixed_files.dat");

        let test_data = b"Fixed files test: pre-registered fd skips table lookup";
        std::fs::write(&file_path, test_data).unwrap();

        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();

        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: true,
            polling_mode: false,
        };
        engine.init(&config).unwrap();

        // Verify capability reflects the config
        assert!(engine.capabilities().fixed_files);

        // Submit multiple reads against the same fd — after the first read the
        // fd is in the registered table and subsequent ops use types::Fixed.
        for i in 0..3u64 {
            let mut buffer = vec![0u8; test_data.len()];
            let op = IOOperation {
                op_type: OperationType::Read,
                target_fd: fd,
                offset: 0,
                buffer: buffer.as_mut_ptr(),
                length: buffer.len(),
                user_data: i,
            };
            engine.submit(op).unwrap();
            let completions = engine.poll_completions().unwrap();
            assert_eq!(completions.len(), 1);
            assert!(completions[0].result.is_ok(), "read {i} failed");
            assert_eq!(&buffer[..], test_data);
        }

        // fd should be registered after the first submit
        assert_eq!(engine.next_file_slot, 1);
        assert!(engine.registered_files.contains_key(&fd));

        engine.cleanup().unwrap();
    }

    #[test]
    fn test_io_uring_engine_registered_buffers() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_reg_bufs.dat");

        let test_data = b"Registered buffers: pinned memory skips virt-to-phys translation";
        std::fs::write(&file_path, test_data).unwrap();

        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();

        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: true,
            use_fixed_files: false,
            polling_mode: false,
        };
        engine.init(&config).unwrap();
        assert!(engine.capabilities().registered_buffers);

        let mut buffer = vec![0u8; test_data.len()];
        let buf_ptr = buffer.as_mut_ptr();

        // First iteration: buffer is new, uses plain Read, deferred for registration.
        let op1 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buf_ptr,
            length: buffer.len(),
            user_data: 1,
        };
        engine.submit(op1).unwrap();
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_ok());
        assert_eq!(&buffer[..], test_data);

        // After poll_completions drains pending_ops, buffer should be registered.
        assert!(engine.registered_buffers.contains_key(&(buf_ptr as usize)));

        // Second iteration: buffer is now registered, uses ReadFixed (hot path).
        buffer.iter_mut().for_each(|b| *b = 0);
        let op2 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buf_ptr,
            length: buffer.len(),
            user_data: 2,
        };
        engine.submit(op2).unwrap();
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_ok());
        assert_eq!(&buffer[..], test_data);

        engine.cleanup().unwrap();
    }

    #[test]
    fn test_io_uring_engine_sqpoll() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_sqpoll.dat");

        let test_data = b"SQPOLL: kernel thread polls SQ, eliminating submit syscalls";
        std::fs::write(&file_path, test_data).unwrap();

        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();

        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: true,
        };

        // SQPOLL requires CAP_SYS_ADMIN on kernels < 5.11.  Skip gracefully if
        // we're running without the required privilege.
        match engine.init(&config) {
            Err(ref e) if e.to_string().contains("Operation not permitted") => {
                eprintln!("Skipping SQPOLL test: requires CAP_SYS_ADMIN on this kernel");
                return;
            }
            Err(e) => panic!("Unexpected io_uring init error: {e}"),
            Ok(()) => {}
        }

        assert!(engine.capabilities().polling_mode);

        let mut buffer = vec![0u8; test_data.len()];
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buffer.as_mut_ptr(),
            length: buffer.len(),
            user_data: 7,
        };

        engine.submit(op).unwrap();
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 7);
        assert!(completions[0].result.is_ok());
        assert_eq!(&buffer[..], test_data);

        engine.cleanup().unwrap();
    }

    #[test]
    fn test_io_uring_engine_all_features() {
        // Smoke-test with all three features enabled simultaneously.
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_all_features.dat");

        let test_data = b"All three io_uring perf features active at once";
        std::fs::write(&file_path, test_data).unwrap();

        let file = File::open(&file_path).unwrap();
        let fd = file.as_raw_fd();

        let mut engine = IoUringEngine::new();
        let config = EngineConfig {
            queue_depth: 32,
            use_registered_buffers: true,
            use_fixed_files: true,
            polling_mode: true,
        };

        match engine.init(&config) {
            Err(ref e) if e.to_string().contains("Operation not permitted") => {
                eprintln!("Skipping all-features test: SQPOLL requires CAP_SYS_ADMIN on this kernel");
                return;
            }
            Err(e) => panic!("Unexpected init error: {e}"),
            Ok(()) => {}
        }

        let caps = engine.capabilities();
        assert!(caps.polling_mode);
        assert!(caps.fixed_files);
        assert!(caps.registered_buffers);

        let mut buffer = vec![0u8; test_data.len()];
        let buf_ptr = buffer.as_mut_ptr();

        // Iteration 1: first time, defers buffer registration, registers fd.
        let op1 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buf_ptr,
            length: buffer.len(),
            user_data: 1,
        };
        engine.submit(op1).unwrap();
        let completions = engine.poll_completions().unwrap();
        assert!(completions[0].result.is_ok());
        assert_eq!(&buffer[..], test_data);

        // Iteration 2: both fd and buffer are registered — hot path.
        buffer.iter_mut().for_each(|b| *b = 0);
        let op2 = IOOperation {
            op_type: OperationType::Read,
            target_fd: fd,
            offset: 0,
            buffer: buf_ptr,
            length: buffer.len(),
            user_data: 2,
        };
        engine.submit(op2).unwrap();
        let completions = engine.poll_completions().unwrap();
        assert!(completions[0].result.is_ok());
        assert_eq!(&buffer[..], test_data);

        engine.cleanup().unwrap();
    }
}
