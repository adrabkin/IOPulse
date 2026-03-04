//! Target abstraction
//!
//! This module defines the abstraction for IO targets in IOPulse. A target represents
//! something that can receive IO operations - typically a file, block device, or directory.
//!
//! # Architecture
//!
//! The `Target` trait provides a uniform interface for all target types. This allows
//! workers to be agnostic to whether they're operating on files, block devices, or
//! other target types.
//!
//! # Target Types
//!
//! - **File**: Regular files on local or network filesystems
//! - **Block Device**: Raw block devices (TODO)
//! - **Directory Tree**: Directory structures for metadata testing (TODO)
//!
//! # Example
//!
//! ```no_run
//! use iopulse::target::{Target, OpenFlags};
//! // FileTarget will be implemented in Task 12
//! // use iopulse::target::file::FileTarget;
//! # struct FileTarget;
//! # impl FileTarget {
//! #     fn new(path: std::path::PathBuf, size: Option<u64>) -> Self { Self }
//! # }
//! # impl iopulse::target::Target for FileTarget {
//! #     fn open(&mut self, _flags: OpenFlags) -> iopulse::Result<()> { Ok(()) }
//! #     fn fd(&self) -> std::os::unix::io::RawFd { 3 }
//! #     fn size(&self) -> u64 { 1024 * 1024 }
//! #     fn apply_fadvise(&self, _flags: &iopulse::target::FadviseFlags) -> iopulse::Result<()> { Ok(()) }
//! #     fn lock(&self, _mode: iopulse::target::FileLockMode, _offset: u64, _len: u64) -> iopulse::Result<iopulse::target::LockGuard> {
//! #         Ok(iopulse::target::LockGuard::new(3, iopulse::target::FileLockMode::None, 0, 0))
//! #     }
//! #     fn close(&mut self) -> iopulse::Result<()> { Ok(()) }
//! # }
//! use std::path::PathBuf;
//!
//! let mut target = FileTarget::new(PathBuf::from("/tmp/testfile"), Some(1024 * 1024));
//! let flags = OpenFlags {
//!     direct: false,
//!     sync: false,
//!     create: true,
//!     truncate: false,
//! };
//!
//! target.open(flags).unwrap();
//! let fd = target.fd();
//! let size = target.size();
//! target.close().unwrap();
//! ```

use crate::Result;
use std::os::unix::io::RawFd;

/// Target trait for IO targets
///
/// This trait defines the interface that all target types must implement. Targets
/// represent destinations for IO operations (files, block devices, etc.).
///
/// # Lifecycle
///
/// 1. Create target instance (via `new()` on concrete type)
/// 2. Call `open()` with flags
/// 3. Use `fd()` for IO operations
/// 4. Call `close()` when done
///
/// # Thread Safety
///
/// Targets must be `Send` to allow transfer between threads. Each worker thread
/// typically owns its own set of targets.
pub trait Target: Send {
    /// Open/prepare the target
    ///
    /// This method opens the target and prepares it for IO operations. For files,
    /// this opens the file with the specified flags. For block devices, this opens
    /// the device. For directories, this may create the directory structure.
    ///
    /// # Arguments
    ///
    /// * `flags` - Open flags specifying how to open the target
    ///
    /// # Errors
    ///
    /// Returns an error if the target cannot be opened (e.g., file doesn't exist,
    /// insufficient permissions, invalid flags).
    fn open(&mut self, flags: OpenFlags) -> Result<()>;
    
    /// Get file descriptor for IO operations
    ///
    /// Returns the file descriptor that can be used with IO engines. This method
    /// should only be called after `open()` has succeeded.
    ///
    /// # Panics
    ///
    /// May panic if called before `open()` or after `close()`.
    fn fd(&self) -> RawFd;
    
    /// Get target size in bytes
    ///
    /// Returns the size of the target in bytes. For files, this is the file size.
    /// For block devices, this is the device size. For directories, this may return
    /// the total size of all files in the tree.
    ///
    /// # Returns
    ///
    /// The size in bytes, or 0 if the size cannot be determined.
    fn size(&self) -> u64;
    
    /// Apply fadvise hints to the target
    ///
    /// Provides access pattern hints to the kernel for cache optimization.
    /// This method should be called after `open()` and before IO operations begin.
    ///
    /// # Arguments
    ///
    /// * `flags` - fadvise flags to apply
    ///
    /// # Errors
    ///
    /// Returns an error if fadvise fails. Note that some filesystems may ignore
    /// fadvise hints without returning an error.
    fn apply_fadvise(&self, flags: &FadviseFlags) -> Result<()>;
    
    /// Apply file lock
    ///
    /// Acquires a file lock according to the specified mode. The lock is held
    /// until the returned `LockGuard` is dropped.
    ///
    /// # Arguments
    ///
    /// * `mode` - Lock mode (none, range, or full)
    /// * `offset` - Starting offset for range locks
    /// * `len` - Length for range locks (0 = to EOF)
    ///
    /// # Returns
    ///
    /// A `LockGuard` that releases the lock when dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if lock acquisition fails.
    fn lock(&self, mode: FileLockMode, offset: u64, len: u64) -> Result<LockGuard>;
    
    /// Close the target
    ///
    /// Closes the target and releases any associated resources. After calling
    /// this method, the target should not be used for IO operations.
    ///
    /// # Errors
    ///
    /// Returns an error if closing fails. Note that even if an error is returned,
    /// the target should be considered closed and should not be used again.
    fn close(&mut self) -> Result<()>;
    
    /// Get logical block size for O_DIRECT alignment
    ///
    /// Returns the logical block size of the underlying device/filesystem.
    /// This is the minimum alignment required for O_DIRECT operations.
    ///
    /// # Returns
    ///
    /// The logical block size in bytes (typically 512 or 4096).
    /// Default implementation returns 512 (safest, most compatible).
    fn logical_block_size(&self) -> u64 {
        512 // Safe default for most devices
    }
    
    /// Get mutable reference to concrete type (for downcasting)
    ///
    /// This method allows downcasting from the trait object to the concrete type.
    /// Used for accessing type-specific methods like force_refill on FileTarget.
    ///
    /// # Returns
    ///
    /// A mutable reference to Any that can be downcast to the concrete type.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Open flags for targets
///
/// Specifies how a target should be opened. Different target types may interpret
/// these flags differently.
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    /// Use direct IO (O_DIRECT) - bypass page cache
    pub direct: bool,
    
    /// Use synchronous IO (O_SYNC) - writes are synchronous
    pub sync: bool,
    
    /// Create the target if it doesn't exist
    pub create: bool,
    
    /// Truncate the target to zero size on open
    pub truncate: bool,
}

impl Default for OpenFlags {
    fn default() -> Self {
        Self {
            direct: false,
            sync: false,
            create: false,
            truncate: false,
        }
    }
}

/// fadvise flags for access pattern hints
///
/// These flags provide hints to the kernel about how the file will be accessed,
/// allowing the kernel to optimize caching behavior.
#[derive(Debug, Clone, Default)]
pub struct FadviseFlags {
    /// Sequential access pattern
    pub sequential: bool,
    
    /// Random access pattern
    pub random: bool,
    
    /// Will need this data soon (prefetch)
    pub willneed: bool,
    
    /// Don't need this data (drop from cache)
    pub dontneed: bool,
    
    /// Data will be accessed only once
    pub noreuse: bool,
}

/// File locking mode
///
/// Specifies how file locks should be acquired for IO operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLockMode {
    /// No locking
    None,
    
    /// Lock the specific byte range for each IO operation
    Range,
    
    /// Lock the entire file for each IO operation
    Full,
}

/// RAII guard for file locks
///
/// The lock is automatically released when this guard is dropped.
pub struct LockGuard {
    fd: RawFd,
    lock_type: FileLockMode,
    start: u64,
    len: u64,
}

impl LockGuard {
    /// Create a new lock guard
    ///
    /// # Safety
    ///
    /// The caller must ensure that the file descriptor is valid and that
    /// the lock has been successfully acquired.
    pub fn new(fd: RawFd, lock_type: FileLockMode, start: u64, len: u64) -> Self {
        Self {
            fd,
            lock_type,
            start,
            len,
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Unlock the file
        if self.lock_type != FileLockMode::None {
            let flock = libc::flock {
                l_type: libc::F_UNLCK as i16,
                l_whence: libc::SEEK_SET as i16,
                l_start: self.start as i64,
                l_len: self.len as i64,
                l_pid: 0,
            };
            
            unsafe {
                libc::fcntl(self.fd, libc::F_SETLK, &flock);
            }
            // Ignore errors on unlock - nothing we can do
        }
    }
}

pub mod file;
pub mod block;
pub mod layout;
pub mod layout_manifest;
pub mod dataset_marker;

pub use layout_manifest::LayoutManifest;
pub use dataset_marker::DatasetMarker;

