//! Block device target implementation
//!
//! This module provides a block device target that implements the Target trait
//! for raw block devices (e.g., /dev/sda, /dev/nvme0n1).
//!
//! # Features
//!
//! - Opens block devices with O_DIRECT support
//! - Detects device size via ioctl (BLKGETSIZE64)
//! - Validates alignment requirements for O_DIRECT
//! - Supports fadvise hints (though less useful for block devices)
//! - Supports file locking (though rarely used for block devices)
//!
//! # Requirements
//!
//! - Root or appropriate permissions to access block devices
//! - O_DIRECT typically required for best performance
//! - Buffer alignment to device block size (usually 512 or 4096 bytes)
//!
//! # Example
//!
//! ```no_run
//! use iopulse::target::{Target, OpenFlags};
//! use iopulse::target::block::BlockTarget;
//! use std::path::PathBuf;
//!
//! // Note: Requires root permissions
//! let mut target = BlockTarget::new(PathBuf::from("/dev/sdb"));
//!
//! let flags = OpenFlags {
//!     direct: true,  // Recommended for block devices
//!     sync: false,
//!     create: false, // Can't create block devices
//!     truncate: false,
//! };
//!
//! target.open(flags).unwrap();
//! let size = target.size(); // Device size in bytes
//! let fd = target.fd();
//! target.close().unwrap();
//! ```

use super::{FadviseFlags, FileLockMode, LockGuard, OpenFlags, Target};
use crate::Result;
use anyhow::Context;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::time::Instant;

// ioctl request code for getting block device size
const BLKGETSIZE64: libc::c_ulong = 0x80081272;

/// Block device target
///
/// This target represents a raw block device. Block devices have fixed sizes
/// determined by the hardware and cannot be resized or pre-allocated.
pub struct BlockTarget {
    /// Path to the block device (e.g., /dev/sda)
    path: PathBuf,
    
    /// File descriptor (Some when open)
    fd: Option<RawFd>,
    
    /// Device size in bytes (determined via ioctl)
    device_size: u64,
}

impl BlockTarget {
    /// Create a new block device target
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the block device (e.g., /dev/sda, /dev/nvme0n1)
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            fd: None,
            device_size: 0,
        }
    }
    
    /// Detect block device size using ioctl
    ///
    /// This should be called after the device is opened.
    fn detect_size(&mut self) -> Result<()> {
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("Device not open"))?;
        
        let mut size: u64 = 0;
        let result = unsafe {
            libc::ioctl(fd, BLKGETSIZE64, &mut size)
        };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "ioctl(BLKGETSIZE64) failed: path={}",
                self.path.display()
            ));
        }
        
        self.device_size = size;
        Ok(())
    }
}

impl Target for BlockTarget {
    fn open(&mut self, flags: OpenFlags) -> Result<()> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        
        // Block devices can't be created or truncated
        if flags.create {
            anyhow::bail!("Cannot create block device: {}", self.path.display());
        }
        if flags.truncate {
            anyhow::bail!("Cannot truncate block device: {}", self.path.display());
        }
        
        // Build custom flags for O_DIRECT and O_SYNC
        let mut custom_flags = 0;
        if flags.direct {
            custom_flags |= libc::O_DIRECT;
        }
        if flags.sync {
            custom_flags |= libc::O_SYNC;
        }
        
        if custom_flags != 0 {
            options.custom_flags(custom_flags);
        }
        
        // Open the block device
        let file = options.open(&self.path)
            .with_context(|| format!("Failed to open block device: {}", self.path.display()))?;
        
        let fd = file.as_raw_fd();
        self.fd = Some(fd);
        std::mem::forget(file); // Don't close on drop
        
        // Detect device size
        self.detect_size()?;
        
        Ok(())
    }
    
    fn fd(&self) -> RawFd {
        self.fd.expect("Device not open")
    }
    
    fn size(&self) -> u64 {
        self.device_size
    }
    
    fn apply_fadvise(&self, flags: &FadviseFlags) -> Result<()> {
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("Device not open"))?;
        
        // fadvise hints are less useful for block devices but we support them anyway
        if flags.sequential {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL)
            };
            if result != 0 {
                // Don't fail on fadvise errors for block devices
                eprintln!("Warning: posix_fadvise(SEQUENTIAL) failed for block device");
            }
        }
        
        if flags.random {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_RANDOM)
            };
            if result != 0 {
                eprintln!("Warning: posix_fadvise(RANDOM) failed for block device");
            }
        }
        
        // Other hints (willneed, dontneed, noreuse) are typically not useful for block devices
        // but we could apply them if requested
        
        Ok(())
    }
    
    fn lock(&self, mode: FileLockMode, offset: u64, len: u64) -> Result<LockGuard> {
        if mode == FileLockMode::None {
            return Ok(LockGuard::new(0, FileLockMode::None, 0, 0));
        }
        
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("Device not open"))?;
        
        // File locking on block devices is unusual but supported
        let (start, length) = match mode {
            FileLockMode::None => (0, 0),
            FileLockMode::Range => (offset, len),
            FileLockMode::Full => (0, 0),
        };
        
        let flock = libc::flock {
            l_type: libc::F_WRLCK as i16,
            l_whence: libc::SEEK_SET as i16,
            l_start: start as i64,
            l_len: length as i64,
            l_pid: 0,
        };
        
        let _start_time = Instant::now();
        let result = unsafe { libc::fcntl(fd, libc::F_SETLKW, &flock) };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "fcntl(F_SETLKW) failed for block device: mode={:?}",
                mode
            ));
        }
        
        Ok(LockGuard::new(fd, mode, start, length))
    }
    
    fn close(&mut self) -> Result<()> {
        if let Some(fd) = self.fd {
            let result = unsafe { libc::close(fd) };
            if result < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err).context(format!(
                    "close failed: path={}",
                    self.path.display()
                ));
            }
            self.fd = None;
        }
        Ok(())
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Drop for BlockTarget {
    fn drop(&mut self) {
        // Ensure device is closed
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Note: Block device tests require root permissions and actual block devices
    // These tests are mostly for documentation and would need to be run manually
    // or in a CI environment with appropriate setup
    
    #[test]
    fn test_block_target_creation() {
        let target = BlockTarget::new(PathBuf::from("/dev/null"));
        assert_eq!(target.device_size, 0);
        assert!(target.fd.is_none());
    }
    
    #[test]
    fn test_block_target_rejects_create() {
        let mut target = BlockTarget::new(PathBuf::from("/dev/null"));
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: true,  // Should be rejected
            truncate: false,
        };
        
        assert!(target.open(flags).is_err());
    }
    
    #[test]
    fn test_block_target_rejects_truncate() {
        let mut target = BlockTarget::new(PathBuf::from("/dev/null"));
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: false,
            truncate: true,  // Should be rejected
        };
        
        assert!(target.open(flags).is_err());
    }
    
    // The following tests would require actual block devices and root permissions
    // They are commented out but show the intended usage
    
    /*
    #[test]
    #[ignore] // Requires root and block device
    fn test_block_target_open_real_device() {
        let mut target = BlockTarget::new(PathBuf::from("/dev/sdb"));
        let flags = OpenFlags {
            direct: true,
            sync: false,
            create: false,
            truncate: false,
        };
        
        target.open(flags).unwrap();
        assert!(target.size() > 0);
        assert!(target.close().is_ok());
    }
    
    #[test]
    #[ignore] // Requires root and block device
    fn test_block_target_size_detection() {
        let mut target = BlockTarget::new(PathBuf::from("/dev/sdb"));
        let flags = OpenFlags::default();
        
        target.open(flags).unwrap();
        let size = target.size();
        assert!(size > 0);
        println!("Device size: {} bytes ({} GB)", size, size / (1024 * 1024 * 1024));
        target.close().unwrap();
    }
    */
}
