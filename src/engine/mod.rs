//! IO engine abstraction
//!
//! This module defines the core abstraction for IO engines in IOPulse. An IO engine
//! is responsible for submitting IO operations to the operating system and retrieving
//! completions. Different engines use different kernel interfaces (io_uring, libaio,
//! synchronous syscalls, mmap) to achieve varying levels of performance and features.
//!
//! # Architecture
//!
//! The `IOEngine` trait provides a uniform interface that all engines must implement.
//! This allows the worker threads to be agnostic to the underlying IO mechanism,
//! enabling runtime selection of the most appropriate engine for the workload.
//!
//! # Engine Types
//!
//! - **Synchronous**: Uses blocking pread/pwrite syscalls (baseline, always available)
//! - **io_uring**: Modern Linux async IO interface (Linux 5.1+, highest performance)
//! - **libaio**: Linux native async IO (widely available, good performance)
//! - **mmap**: Memory-mapped IO using mmap/memcpy (useful for specific workloads)
//!
//! # Example
//!
//! ```no_run
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::sync::SyncEngine;
//!
//! let mut engine = SyncEngine::new();
//! let config = EngineConfig {
//!     queue_depth: 32,
//!     use_registered_buffers: false,
//!     use_fixed_files: false,
//!     polling_mode: false,
//! };
//!
//! engine.init(&config).expect("Failed to initialize engine");
//!
//! // Submit operations and poll for completions
//! // ... (see IOEngine trait documentation for details)
//!
//! engine.cleanup().expect("Failed to cleanup engine");
//! ```

use crate::Result;
use std::os::unix::io::RawFd;

/// IO engine trait for all backends
///
/// This trait defines the interface that all IO engines must implement. Engines are
/// responsible for submitting IO operations to the kernel and retrieving completions.
///
/// # Lifecycle
///
/// 1. Create engine instance (via `new()` on concrete type)
/// 2. Call `init()` with configuration
/// 3. Submit operations via `submit()` and poll completions via `poll_completions()`
/// 4. Call `cleanup()` when done
///
/// # Thread Safety
///
/// Engines must be `Send` to allow transfer between threads, but are not required to
/// be `Sync`. Each worker thread owns its own engine instance.
///
/// # Error Handling
///
/// All methods return `Result<T>` to allow proper error propagation. Engines should
/// return descriptive errors that include context about what operation failed.
pub trait IOEngine: Send {
    /// Initialize the engine with the given configuration
    ///
    /// This method is called once after engine creation and before any IO operations.
    /// Engines should allocate resources, set up kernel structures (e.g., io_uring
    /// rings, libaio contexts), and prepare for IO submission.
    ///
    /// # Arguments
    ///
    /// * `config` - Engine configuration including queue depth and optimization flags
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails (e.g., insufficient resources,
    /// unsupported kernel version, invalid configuration).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use iopulse::engine::{IOEngine, EngineConfig};
    /// # use iopulse::engine::sync::SyncEngine;
    /// let mut engine = SyncEngine::new();
    /// let config = EngineConfig {
    ///     queue_depth: 32,
    ///     use_registered_buffers: false,
    ///     use_fixed_files: false,
    ///     polling_mode: false,
    /// };
    /// engine.init(&config)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    fn init(&mut self, config: &EngineConfig) -> Result<()>;
    
    /// Submit an IO operation to the engine
    ///
    /// For asynchronous engines (io_uring, libaio), this queues the operation but
    /// may not immediately submit it to the kernel. For synchronous engines, this
    /// performs the operation immediately and blocks until completion.
    ///
    /// # Arguments
    ///
    /// * `op` - The IO operation to submit (see `IOOperation` for details)
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails. For async engines, this typically means
    /// the submission queue is full. For sync engines, this means the syscall failed.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    /// - The buffer pointer in `op` is valid and properly aligned
    /// - The buffer remains valid until the operation completes
    /// - The file descriptor is valid and open
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use iopulse::engine::{IOEngine, IOOperation, OperationType};
    /// # use iopulse::engine::sync::SyncEngine;
    /// # let mut engine = SyncEngine::new();
    /// # let fd = 3;
    /// # let mut buffer = vec![0u8; 4096];
    /// let op = IOOperation {
    ///     op_type: OperationType::Read,
    ///     target_fd: fd,
    ///     offset: 0,
    ///     buffer: buffer.as_mut_ptr(),
    ///     length: 4096,
    ///     user_data: 1,
    /// };
    /// engine.submit(op)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    fn submit(&mut self, op: IOOperation) -> Result<()>;
    
    /// Poll for completed IO operations
    ///
    /// This method retrieves completed operations from the engine. For asynchronous
    /// engines, this may also trigger submission of queued operations to the kernel.
    ///
    /// # Returns
    ///
    /// A vector of completed operations. May be empty if no operations have completed.
    /// For synchronous engines, this typically returns the operation that was just
    /// submitted via `submit()`.
    ///
    /// # Errors
    ///
    /// Returns an error if polling fails (e.g., kernel error, invalid state).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use iopulse::engine::IOEngine;
    /// # use iopulse::engine::sync::SyncEngine;
    /// # let mut engine = SyncEngine::new();
    /// let completions = engine.poll_completions()?;
    /// for completion in completions {
    ///     match completion.result {
    ///         Ok(bytes) => println!("Completed {} bytes", bytes),
    ///         Err(e) => eprintln!("IO error: {}", e),
    ///     }
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>>;
    
    /// Cleanup and release engine resources
    ///
    /// This method is called when the engine is no longer needed. Engines should
    /// release all resources, close kernel structures, and ensure all pending
    /// operations are completed or cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error if cleanup fails. Note that even if cleanup fails, the
    /// engine should be considered unusable and should not be used again.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use iopulse::engine::IOEngine;
    /// # use iopulse::engine::sync::SyncEngine;
    /// # let mut engine = SyncEngine::new();
    /// engine.cleanup()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    fn cleanup(&mut self) -> Result<()>;
    
    /// Get engine-specific capabilities
    ///
    /// Returns a description of what features this engine supports. This allows
    /// workers to adapt their behavior based on engine capabilities.
    ///
    /// # Returns
    ///
    /// An `EngineCapabilities` struct describing supported features.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use iopulse::engine::IOEngine;
    /// # use iopulse::engine::sync::SyncEngine;
    /// # let engine = SyncEngine::new();
    /// let caps = engine.capabilities();
    /// if caps.async_io {
    ///     println!("Engine supports asynchronous IO");
    /// }
    /// if caps.batch_submission {
    ///     println!("Engine supports batch submission");
    /// }
    /// ```
    fn capabilities(&self) -> EngineCapabilities;
}

/// Engine configuration
///
/// Configuration parameters for initializing an IO engine. Different engines may
/// use different subsets of these parameters based on their capabilities.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum number of outstanding IO operations (queue depth)
    ///
    /// For async engines (io_uring, libaio), this determines the size of the
    /// submission and completion queues. For sync engines, this is typically ignored.
    ///
    /// Typical values: 1-1024, with 32-128 being common for most workloads.
    pub queue_depth: usize,
    
    /// Use registered buffers optimization (io_uring only)
    ///
    /// When enabled, buffers are pre-registered with the kernel to avoid repeated
    /// virtual-to-physical address translation. This can improve performance but
    /// requires buffers to be registered before use.
    pub use_registered_buffers: bool,
    
    /// Use fixed files optimization (io_uring only)
    ///
    /// When enabled, file descriptors are pre-registered with the kernel to avoid
    /// repeated file table lookups. This can improve performance for workloads that
    /// repeatedly access the same files.
    pub use_fixed_files: bool,
    
    /// Use polling mode instead of interrupts (io_uring only)
    ///
    /// When enabled, the kernel polls for completions instead of using interrupts.
    /// This can reduce latency for high-IOPS workloads but increases CPU usage.
    pub polling_mode: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            queue_depth: 32,
            use_registered_buffers: false,
            use_fixed_files: false,
            polling_mode: false,
        }
    }
}

/// IO operation descriptor
///
/// Describes a single IO operation to be submitted to an engine. The operation
/// includes the type (read/write/fsync), target file descriptor, offset, buffer,
/// and a user-provided data field for tracking.
///
/// # Safety
///
/// The buffer pointer must be valid and properly aligned for the duration of the
/// operation. For O_DIRECT operations, buffers must be aligned to the device's
/// block size (typically 512 or 4096 bytes).
#[derive(Debug)]
pub struct IOOperation {
    /// Type of operation (read, write, fsync, etc.)
    pub op_type: OperationType,
    
    /// File descriptor of the target file or device
    pub target_fd: RawFd,
    
    /// Byte offset within the file/device where the operation should occur
    ///
    /// For read/write operations, this is the starting offset. For fsync, this
    /// field is ignored.
    pub offset: u64,
    
    /// Pointer to the buffer for read/write operations
    ///
    /// For reads, data will be written to this buffer. For writes, data will be
    /// read from this buffer. For fsync, this field is ignored.
    ///
    /// # Safety
    ///
    /// The buffer must remain valid until the operation completes. The caller is
    /// responsible for ensuring proper alignment (especially for O_DIRECT).
    pub buffer: *mut u8,
    
    /// Length of the operation in bytes
    ///
    /// For read/write, this is the number of bytes to transfer. For fsync, this
    /// field is ignored.
    pub length: usize,
    
    /// User-provided data for tracking this operation
    ///
    /// This value is returned in the corresponding `IOCompletion` and can be used
    /// to correlate completions with submissions. Common uses include storing an
    /// index into a buffer pool or a pointer to operation metadata.
    pub user_data: u64,
}

// Safety: IOOperation contains a raw pointer but is only used within a single thread
// and the pointer lifetime is managed by the caller
unsafe impl Send for IOOperation {}

/// Operation type
///
/// Specifies the type of IO operation to perform. Different engines may support
/// different subsets of these operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
    /// Read data from the target into the buffer
    ///
    /// Reads `length` bytes from the target at `offset` into the buffer pointed
    /// to by `buffer`. The operation completes when all bytes are read or an
    /// error occurs.
    Read,
    
    /// Write data from the buffer to the target
    ///
    /// Writes `length` bytes from the buffer pointed to by `buffer` to the target
    /// at `offset`. The operation completes when all bytes are written or an
    /// error occurs.
    Write,
    
    /// Synchronize file data to storage
    ///
    /// Ensures that all modified data for the file descriptor is written to the
    /// underlying storage device. This operation does not use the buffer, offset,
    /// or length fields.
    Fsync,
    
    /// Synchronize file data (but not metadata) to storage
    ///
    /// Similar to Fsync but only synchronizes file data, not metadata (e.g., file
    /// timestamps). This can be faster than Fsync for some workloads.
    Fdatasync,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationType::Read => write!(f, "read"),
            OperationType::Write => write!(f, "write"),
            OperationType::Fsync => write!(f, "fsync"),
            OperationType::Fdatasync => write!(f, "fdatasync"),
        }
    }
}

/// Completed IO operation
///
/// Represents the result of an IO operation that has completed. Contains the
/// user data from the original operation, the result (success with byte count
/// or error), and the operation type.
#[derive(Debug)]
pub struct IOCompletion {
    /// User data from the original IOOperation
    ///
    /// This value matches the `user_data` field from the `IOOperation` that was
    /// submitted, allowing the caller to correlate completions with submissions.
    pub user_data: u64,
    
    /// Result of the operation
    ///
    /// On success, contains the number of bytes transferred (for read/write) or
    /// 0 (for fsync). On failure, contains the error that occurred.
    ///
    /// Note: For partial reads/writes, the byte count may be less than the
    /// requested length. The caller should check for this and resubmit if needed.
    pub result: Result<usize>,
    
    /// Type of operation that completed
    ///
    /// This matches the `op_type` field from the original `IOOperation`.
    pub op_type: OperationType,
}

/// Engine capabilities
///
/// Describes the features and optimizations supported by an IO engine. This allows
/// workers to adapt their behavior based on what the engine can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCapabilities {
    /// Engine supports asynchronous IO
    ///
    /// If true, the engine can have multiple operations in flight simultaneously.
    /// If false, operations are performed synchronously (one at a time).
    pub async_io: bool,
    
    /// Engine supports batch submission
    ///
    /// If true, multiple operations can be submitted in a single syscall, reducing
    /// overhead. If false, each operation requires a separate syscall.
    pub batch_submission: bool,
    
    /// Engine supports registered buffers
    ///
    /// If true, buffers can be pre-registered with the kernel to avoid repeated
    /// virtual-to-physical address translation. This is an io_uring-specific feature.
    pub registered_buffers: bool,
    
    /// Engine supports fixed files
    ///
    /// If true, file descriptors can be pre-registered with the kernel to avoid
    /// repeated file table lookups. This is an io_uring-specific feature.
    pub fixed_files: bool,
    
    /// Engine supports polling mode
    ///
    /// If true, the engine can poll for completions instead of using interrupts,
    /// which can reduce latency at the cost of increased CPU usage.
    pub polling_mode: bool,
    
    /// Maximum queue depth supported by the engine
    ///
    /// The maximum number of operations that can be outstanding simultaneously.
    /// For synchronous engines, this is typically 1.
    pub max_queue_depth: usize,
}

impl Default for EngineCapabilities {
    fn default() -> Self {
        Self {
            async_io: false,
            batch_submission: false,
            registered_buffers: false,
            fixed_files: false,
            polling_mode: false,
            max_queue_depth: 1,
        }
    }
}

pub mod sync;
pub mod mock;

#[cfg(feature = "io_uring")]
pub mod io_uring;

#[cfg(target_os = "linux")]
pub mod libaio;

pub mod mmap;
