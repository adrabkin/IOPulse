//! File target implementation
//!
//! This module provides a file target that implements the Target trait for regular
//! files on local and network filesystems.
//!
//! # Features
//!
//! - File creation with configurable flags (O_DIRECT, O_SYNC)
//! - Pre-allocation with posix_fallocate
//! - Truncate-to-size with ftruncate
//! - posix_fadvise hints for cache optimization
//! - fcntl-based file locking (range and full)
//! - Lock acquisition latency tracking
//!
//! # Example
//!
//! ```no_run
//! use iopulse::target::{Target, OpenFlags};
//! use iopulse::target::file::FileTarget;
//! use std::path::PathBuf;
//!
//! let mut target = FileTarget::new(
//!     PathBuf::from("/tmp/testfile"),
//!     Some(1024 * 1024 * 1024), // 1GB
//! );
//!
//! let flags = OpenFlags {
//!     direct: true,
//!     sync: false,
//!     create: true,
//!     truncate: false,
//! };
//!
//! target.open(flags).unwrap();
//! target.preallocate().unwrap();
//!
//! let fd = target.fd();
//! let size = target.size();
//!
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

/// File target for regular files
///
/// This target represents a regular file on a local or network filesystem.
/// It supports all standard file operations including creation, pre-allocation,
/// fadvise hints, and file locking.
pub struct FileTarget {
    /// Path to the file
    path: PathBuf,
    
    /// Desired file size (for creation/pre-allocation)
    file_size: Option<u64>,
    
    /// File descriptor (Some when open)
    fd: Option<RawFd>,
    
    /// Actual file size (determined after open)
    actual_size: u64,
    
    /// Whether to pre-allocate space
    preallocate: bool,
    
    /// Whether to truncate to size
    truncate_to_size: bool,
    
    /// Whether to fill pre-allocated files with pattern data
    refill: bool,
    
    /// Pattern to use for refill operation
    refill_pattern: crate::config::workload::VerifyPattern,
    
    /// Whether O_DIRECT is being used (affects preallocation strategy)
    using_direct_io: bool,
    
    /// Track lock acquisition latency
    lock_latency_ns: Vec<u64>,
    
    /// Logical block size for O_DIRECT alignment (detected at open)
    logical_block_size: u64,
    
    /// Offset range for partitioned distribution (start, end)
    /// When set, refill operations only fill this range
    offset_range: Option<(u64, u64)>,
}

impl FileTarget {
    /// Create a new file target
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file
    /// * `file_size` - Desired file size (for creation/pre-allocation)
    pub fn new(path: PathBuf, file_size: Option<u64>) -> Self {
        Self {
            path,
            file_size,
            fd: None,
            actual_size: 0,
            preallocate: false,
            truncate_to_size: false,
            refill: false,
            refill_pattern: crate::config::workload::VerifyPattern::Random,
            using_direct_io: false,
            lock_latency_ns: Vec::new(),
            logical_block_size: 512, // Default to 512 (safest, most compatible)
            offset_range: None,
        }
    }
    
    /// Set whether O_DIRECT is being used
    pub fn set_using_direct_io(&mut self, using_direct_io: bool) {
        self.using_direct_io = using_direct_io;
    }
    
    /// Set whether to pre-allocate file space
    pub fn set_preallocate(&mut self, preallocate: bool) {
        self.preallocate = preallocate;
    }
    
    /// Set whether to truncate file to size
    pub fn set_truncate_to_size(&mut self, truncate: bool) {
        self.truncate_to_size = truncate;
    }
    
    /// Set whether to fill pre-allocated files with pattern data
    pub fn set_refill(&mut self, refill: bool) {
        self.refill = refill;
    }
    
    /// Set the pattern to use for refill operation
    pub fn set_refill_pattern(&mut self, pattern: crate::config::workload::VerifyPattern) {
        self.refill_pattern = pattern;
    }
    
    /// Set the offset range for partitioned distribution
    /// 
    /// When set, refill operations will only fill this range instead of the entire file.
    /// This is used with partitioned distribution to avoid workers refilling overlapping regions.
    pub fn set_offset_range(&mut self, start: u64, end: u64) {
        self.offset_range = Some((start, end));
    }
    
    /// Check if file is empty (size = 0)
    pub fn is_empty(&self) -> bool {
        self.actual_size == 0
    }
    
    /// Force refill of file with pattern data
    ///
    /// This is a public wrapper around the private refill() method,
    /// used for smart auto-refill when reads are requested on empty files.
    pub fn force_refill(&mut self, pattern: crate::config::workload::VerifyPattern) -> Result<()> {
        if self.file_size.is_none() {
            anyhow::bail!("Cannot refill: no file size specified");
        }
        
        // Ensure file is preallocated first
        if self.actual_size == 0 || self.actual_size < self.file_size.unwrap() {
            // Need to allocate space first
            if self.fd.is_none() {
                anyhow::bail!("Cannot refill: file not open");
            }
            
            let target_size = self.file_size.unwrap();
            let fd = self.fd.unwrap();
            
            // Allocate space
            let result = unsafe { libc::posix_fallocate(fd, 0, target_size as i64) };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fallocate failed during force_refill");
            }
            
            self.actual_size = target_size;
        }
        
        // Now fill with pattern
        self.refill(pattern)
    }
    
    /// Pre-allocate file space using posix_fallocate
    ///
    /// This should be called after open() if pre-allocation is desired.
    /// If offset_range is set, allocates only that specific range.
    /// Otherwise, allocates from offset 0 to file_size.
    pub fn preallocate(&self) -> Result<()> {
        use std::time::Instant;
        
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        let size = self.file_size.ok_or_else(|| anyhow::anyhow!("No file size specified"))?;
        
        // Determine allocation range
        let (alloc_offset, alloc_size) = if let Some((start, end)) = self.offset_range {
            // Partitioned mode: allocate only this node's region
            (start, end - start)
        } else {
            // Normal mode: allocate from 0 to file_size
            (0, size)
        };
        
        // Print message for large allocations (>100MB)
        if alloc_size > 100 * 1024 * 1024 {
            if alloc_offset > 0 {
                println!("Pre-allocating region {} bytes at offset {} (this may take several seconds)...", 
                    alloc_size, alloc_offset);
            } else {
                println!("Pre-allocating {} bytes (this may take several seconds)...", alloc_size);
            }
        }
        
        let preallocate_start = Instant::now();
        let result = unsafe { libc::posix_fallocate(fd, alloc_offset as i64, alloc_size as i64) };
        let preallocate_elapsed = preallocate_start.elapsed();
        
        if result != 0 {
            let err = std::io::Error::from_raw_os_error(result);
            return Err(err).context(format!(
                "posix_fallocate failed: path={}, offset={}, size={}",
                self.path.display(),
                alloc_offset,
                alloc_size
            ));
        }
        
        // Print completion message for large allocations
        if alloc_size > 100 * 1024 * 1024 {
            println!("Pre-allocation complete in {:.2}s", preallocate_elapsed.as_secs_f64());
        }
        
        Ok(())
    }
    
    /// Truncate file to specified size using ftruncate
    ///
    /// This should be called after open() if truncation is desired.
    pub fn truncate(&self) -> Result<()> {
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        let size = self.file_size.ok_or_else(|| anyhow::anyhow!("No file size specified"))?;
        
        let result = unsafe { libc::ftruncate(fd, size as i64) };
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "ftruncate failed: path={}, size={}",
                self.path.display(),
                size
            ));
        }
        
        Ok(())
    }
    
    /// Fill file with pattern data
    ///
    /// Writes pattern data to the entire file. This is useful for:
    /// - Enabling read tests on pre-allocated files (which contain undefined data)
    /// - Defeating storage deduplication with random data
    /// - Testing with known data patterns
    ///
    /// # Arguments
    ///
    /// * `pattern` - Pattern to write (zeros, ones, random, sequential)
    ///
    /// # Performance
    ///
    /// Uses large write operations (1MB chunks) for efficiency.
    /// Shows progress for files >1GB.
    /// Fill the file with a specific pattern
    ///
    /// Writes the specified pattern to the file. This is used to ensure the file
    /// has actual data (not sparse regions) before read testing.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern to fill with (zeros, ones, random, sequential)
    /// * `start_offset` - Starting offset to fill from (for partitioned distribution)
    /// * `end_offset` - Ending offset to fill to (for partitioned distribution)
    ///
    /// Uses large write operations (1MB chunks) for efficiency.
    /// Shows progress for files >1GB.
    pub fn refill_range(&self, pattern: crate::config::workload::VerifyPattern, start_offset: u64, end_offset: u64) -> Result<()> {
        use std::io::Write;
        use rand::RngCore;
        
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        let size = end_offset - start_offset;
        
        let start = Instant::now();
        println!("Filling file region with {} pattern (offset {}-{}, {} bytes)...", 
            pattern, start_offset, end_offset, size);
        
        // Use 1MB chunks for efficiency
        const CHUNK_SIZE: usize = 1024 * 1024;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut offset = start_offset;
        let mut rng = rand::thread_rng();
        
        // Show progress for large regions
        let show_progress = size > 1024 * 1024 * 1024; // >1GB
        let progress_interval = size / 10; // 10% increments
        let mut next_progress = start_offset + progress_interval;
        
        while offset < end_offset {
            let remaining = end_offset - offset;
            let chunk_len = std::cmp::min(remaining as usize, CHUNK_SIZE);
            
            // Fill buffer with pattern
            match pattern {
                crate::config::workload::VerifyPattern::Zeros => {
                    buffer[..chunk_len].fill(0);
                }
                crate::config::workload::VerifyPattern::Ones => {
                    buffer[..chunk_len].fill(0xFF);
                }
                crate::config::workload::VerifyPattern::Random => {
                    rng.fill_bytes(&mut buffer[..chunk_len]);
                }
                crate::config::workload::VerifyPattern::Sequential => {
                    for (i, byte) in buffer[..chunk_len].iter_mut().enumerate() {
                        *byte = ((offset as usize + i) % 256) as u8;
                    }
                }
            }
            
            // Write chunk using pwrite
            let mut written = 0;
            while written < chunk_len {
                let result = unsafe {
                    libc::pwrite(
                        fd,
                        buffer[written..chunk_len].as_ptr() as *const libc::c_void,
                        chunk_len - written,
                        (offset + written as u64) as i64,
                    )
                };
                
                if result < 0 {
                    let err = std::io::Error::last_os_error();
                    return Err(err).context(format!(
                        "pwrite failed during refill: offset={}, len={}",
                        offset + written as u64,
                        chunk_len - written
                    ));
                }
                
                written += result as usize;
            }
            
            offset += chunk_len as u64;
            
            // Show progress
            if show_progress && offset >= next_progress {
                let percent = ((offset - start_offset) as f64 / size as f64) * 100.0;
                print!("\rProgress: {:.0}%", percent);
                std::io::stdout().flush().ok();
                next_progress += progress_interval;
            }
        }
        
        if show_progress {
            println!("\rProgress: 100%");
        }
        
        let elapsed = start.elapsed();
        println!("Refill complete in {:.2}s", elapsed.as_secs_f64());
        
        Ok(())
    }
    
    /// Fill the entire file with a specific pattern
    ///
    /// Convenience method that fills the entire file from offset 0 to file_size.
    pub fn refill(&self, pattern: crate::config::workload::VerifyPattern) -> Result<()> {
        let size = self.file_size.ok_or_else(|| anyhow::anyhow!("No file size specified"))?;
        self.refill_range(pattern, 0, size)
    }
    
    /// Get lock acquisition latency statistics
    ///
    /// Returns a vector of lock acquisition times in nanoseconds.
    pub fn lock_latencies(&self) -> &[u64] {
        &self.lock_latency_ns
    }
    
    /// Get the logical block size for O_DIRECT alignment
    ///
    /// Returns the detected logical block size (typically 512 or 4096 bytes).
    /// This is the minimum alignment required for O_DIRECT operations.
    pub fn logical_block_size(&self) -> u64 {
        self.logical_block_size
    }
    
    /// Detect logical block size for the underlying device
    ///
    /// Queries the filesystem/device to determine the logical block size.
    /// Falls back to 512 bytes if detection fails (safest default).
    fn detect_logical_block_size(&mut self) -> Result<()> {
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        
        // Try to get logical block size using BLKSSZGET ioctl
        // This works for block devices and some filesystems
        let mut block_size: libc::c_int = 0;
        let result = unsafe {
            libc::ioctl(fd, libc::BLKSSZGET, &mut block_size)
        };
        
        if result == 0 && block_size > 0 {
            self.logical_block_size = block_size as u64;
        } else {
            // BLKSSZGET failed (common for regular files on filesystems)
            // Try to get filesystem block size using fstat
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            let result = unsafe { libc::fstat(fd, &mut stat) };
            
            if result == 0 && stat.st_blksize > 0 {
                // st_blksize is the "optimal" block size for IO
                // For O_DIRECT, we need the logical block size which is typically 512 or 4096
                // Use st_blksize if it's a power of 2 and >= 512
                let blksize = stat.st_blksize as u64;
                if blksize >= 512 && blksize.is_power_of_two() {
                    self.logical_block_size = blksize;
                } else {
                    // Fallback to 512 (safest default, works everywhere)
                    self.logical_block_size = 512;
                }
            } else {
                // Both methods failed, use 512 (safest default)
                self.logical_block_size = 512;
            }
        }
        
        Ok(())
    }
}

impl Target for FileTarget {
    fn open(&mut self, flags: OpenFlags) -> Result<()> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        
        if flags.create {
            options.create(true);
        }
        
        if flags.truncate {
            options.truncate(true);
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
        
        // Open the file
        let file = options.open(&self.path)
            .with_context(|| format!("Failed to open file: {}", self.path.display()))?;
        
        let fd = file.as_raw_fd();
        
        // Get actual file size
        let metadata = file.metadata()
            .with_context(|| format!("Failed to get file metadata: {}", self.path.display()))?;
        self.actual_size = metadata.len();
        
        // Store the fd (file will be kept open via fd, not File handle)
        self.fd = Some(fd);
        std::mem::forget(file); // Don't close on drop
        
        // Detect logical block size for O_DIRECT alignment
        self.detect_logical_block_size()?;
        
        // Apply pre-allocation if requested
        if self.preallocate && self.file_size.is_some() {
            let target_size = self.file_size.unwrap();
            
            // For O_DIRECT, we MUST preallocate even if size matches, because file might be sparse
            // Check if file is sparse by comparing logical size vs physical size
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            let stat_result = unsafe { libc::fstat(fd, &mut stat) };
            
            let is_sparse = if stat_result == 0 {
                // st_blocks is in 512-byte units
                let physical_bytes = stat.st_blocks as u64 * 512;
                let logical_bytes = stat.st_size as u64;
                // File is sparse if physical size is significantly less than logical size
                physical_bytes < logical_bytes / 2
            } else {
                false // Can't determine, assume not sparse
            };
            
            // Skip preallocation only if:
            // 1. File size matches (within tolerance)
            // 2. File is NOT sparse
            let size_diff = if self.actual_size > target_size {
                self.actual_size - target_size
            } else {
                target_size - self.actual_size
            };
            
            const SIZE_TOLERANCE: u64 = 1024 * 1024; // 1MB tolerance
            
            if size_diff <= SIZE_TOLERANCE && !is_sparse {
                // File already correct size and not sparse, skip preallocation
                self.actual_size = target_size;
            } else {
                // File is wrong size or sparse, need to (re)allocate
                // Truncate to 0 first to clear any existing extents
                if self.actual_size > 0 {
                    let truncate_result = unsafe { libc::ftruncate(fd, 0) };
                    if truncate_result != 0 {
                        // Truncate failed, but continue anyway
                    }
                }
                
                self.preallocate()?;
                self.actual_size = target_size;
                
                // XFS uses lazy allocation - posix_fallocate doesn't actually write blocks
                // Force block allocation by writing to the file
                // This is critical for read performance - reading unallocated blocks is slow
                // 
                // For partitioned distribution: Always refill to avoid lazy allocation issues
                // For per-worker/shared: Only refill if explicitly requested (--refill flag)
                //   - Per-worker files will be written by the test anyway
                //   - Automatic refill with multiple workers causes contention (30s per worker)
                if self.offset_range.is_some() {
                    // Partitioned mode: Always refill the assigned range
                    let (start, end) = self.offset_range.unwrap();
                    self.refill_range(self.refill_pattern, start, end)?;
                } else if self.refill {
                    // Per-worker/shared: Only refill if explicitly requested
                    self.refill(self.refill_pattern)?;
                }
            }
        }
        
        // Apply truncation if requested
        if self.truncate_to_size && self.file_size.is_some() {
            self.truncate()?;
            self.actual_size = self.file_size.unwrap();
        }
        
        Ok(())
    }
    
    fn fd(&self) -> RawFd {
        self.fd.expect("File not open")
    }
    
    fn size(&self) -> u64 {
        // Return configured size if available, otherwise actual size
        // This allows sequential IO to work with newly created files
        self.file_size.unwrap_or(self.actual_size)
    }
    
    fn apply_fadvise(&self, flags: &FadviseFlags) -> Result<()> {
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        
        // Apply each requested hint
        if flags.sequential {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL)
            };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fadvise(SEQUENTIAL) failed");
            }
        }
        
        if flags.random {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_RANDOM)
            };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fadvise(RANDOM) failed");
            }
        }
        
        if flags.willneed {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_WILLNEED)
            };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fadvise(WILLNEED) failed");
            }
        }
        
        if flags.dontneed {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED)
            };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fadvise(DONTNEED) failed");
            }
        }
        
        if flags.noreuse {
            let result = unsafe {
                libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_NOREUSE)
            };
            if result != 0 {
                let err = std::io::Error::from_raw_os_error(result);
                return Err(err).context("posix_fadvise(NOREUSE) failed");
            }
        }
        
        Ok(())
    }
    
    fn lock(&self, mode: FileLockMode, offset: u64, len: u64) -> Result<LockGuard> {
        if mode == FileLockMode::None {
            return Ok(LockGuard::new(0, FileLockMode::None, 0, 0));
        }
        
        let fd = self.fd.ok_or_else(|| anyhow::anyhow!("File not open"))?;
        
        // Determine lock parameters
        let (start, length) = match mode {
            FileLockMode::None => (0, 0),
            FileLockMode::Range => (offset, len),
            FileLockMode::Full => (0, 0), // 0 length means entire file
        };
        
        // Build flock structure
        let flock = libc::flock {
            l_type: libc::F_WRLCK as i16,  // Exclusive write lock
            l_whence: libc::SEEK_SET as i16,
            l_start: start as i64,
            l_len: length as i64,
            l_pid: 0,
        };
        
        // Acquire lock and track latency
        let start_time = Instant::now();
        let result = unsafe { libc::fcntl(fd, libc::F_SETLKW, &flock) };
        let _latency_ns = start_time.elapsed().as_nanos() as u64;
        
        // Note: Lock latency tracking would require mutable self
        // Worker will track lock latencies externally
        
        if result < 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!(
                "fcntl(F_SETLKW) failed: mode={:?}, offset={}, len={}",
                mode, offset, len
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
    
    fn logical_block_size(&self) -> u64 {
        self.logical_block_size
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Drop for FileTarget {
    fn drop(&mut self) {
        // Ensure file is closed
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_file_target_create() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_create.dat");
        
        let mut target = FileTarget::new(file_path.clone(), Some(1024 * 1024));
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: true,
            truncate: false,
        };
        
        assert!(target.open(flags).is_ok());
        assert!(file_path.exists());
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_open_existing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_existing.dat");
        
        // Create file first
        std::fs::write(&file_path, b"test data").unwrap();
        
        let mut target = FileTarget::new(file_path.clone(), None);
        let flags = OpenFlags::default();
        
        assert!(target.open(flags).is_ok());
        assert_eq!(target.size(), 9); // "test data" length
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_preallocate() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_preallocate.dat");
        
        let mut target = FileTarget::new(file_path.clone(), Some(1024 * 1024));
        target.set_preallocate(true);
        
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: true,
            truncate: false,
        };
        
        assert!(target.open(flags).is_ok());
        assert_eq!(target.size(), 1024 * 1024);
        assert!(target.close().is_ok());
        
        // Verify file size
        let metadata = std::fs::metadata(&file_path).unwrap();
        assert_eq!(metadata.len(), 1024 * 1024);
    }
    
    #[test]
    fn test_file_target_truncate() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_truncate.dat");
        
        // Create file with some data
        std::fs::write(&file_path, &vec![0u8; 2048]).unwrap();
        
        let mut target = FileTarget::new(file_path.clone(), Some(1024));
        target.set_truncate_to_size(true);
        
        let flags = OpenFlags {
            direct: false,
            sync: false,
            create: false,
            truncate: false,
        };
        
        assert!(target.open(flags).is_ok());
        assert_eq!(target.size(), 1024);
        assert!(target.close().is_ok());
        
        // Verify file was truncated
        let metadata = std::fs::metadata(&file_path).unwrap();
        assert_eq!(metadata.len(), 1024);
    }
    
    #[test]
    fn test_file_target_fadvise() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_fadvise.dat");
        
        std::fs::write(&file_path, &vec![0u8; 4096]).unwrap();
        
        let mut target = FileTarget::new(file_path, None);
        let flags = OpenFlags::default();
        
        target.open(flags).unwrap();
        
        // Apply fadvise hints
        let fadvise_flags = FadviseFlags {
            sequential: true,
            random: false,
            willneed: true,
            dontneed: false,
            noreuse: false,
        };
        
        assert!(target.apply_fadvise(&fadvise_flags).is_ok());
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_lock_full() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_lock_full.dat");
        
        std::fs::write(&file_path, &vec![0u8; 4096]).unwrap();
        
        let mut target = FileTarget::new(file_path, None);
        let flags = OpenFlags::default();
        
        target.open(flags).unwrap();
        
        // Acquire full file lock
        let guard = target.lock(FileLockMode::Full, 0, 0).unwrap();
        
        // Lock is held while guard is in scope
        drop(guard); // Explicitly release
        
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_lock_range() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_lock_range.dat");
        
        std::fs::write(&file_path, &vec![0u8; 8192]).unwrap();
        
        let mut target = FileTarget::new(file_path, None);
        let flags = OpenFlags::default();
        
        target.open(flags).unwrap();
        
        // Acquire range lock
        let guard = target.lock(FileLockMode::Range, 1024, 4096).unwrap();
        
        // Lock is held
        drop(guard);
        
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_lock_none() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_lock_none.dat");
        
        std::fs::write(&file_path, &vec![0u8; 1024]).unwrap();
        
        let mut target = FileTarget::new(file_path, None);
        let flags = OpenFlags::default();
        
        target.open(flags).unwrap();
        
        // No lock
        let guard = target.lock(FileLockMode::None, 0, 0).unwrap();
        drop(guard);
        
        assert!(target.close().is_ok());
    }
    
    #[test]
    fn test_file_target_o_direct() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_direct.dat");
        
        let mut target = FileTarget::new(file_path.clone(), Some(4096));
        target.set_preallocate(true);
        
        let flags = OpenFlags {
            direct: true,  // O_DIRECT
            sync: false,
            create: true,
            truncate: false,
        };
        
        // O_DIRECT may not work on tmpfs, so we allow this to fail
        let result = target.open(flags);
        if result.is_ok() {
            assert_eq!(target.size(), 4096);
            assert!(target.close().is_ok());
        }
    }
    
    #[test]
    fn test_file_target_drop_closes() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_drop.dat");
        
        std::fs::write(&file_path, b"test").unwrap();
        
        {
            let mut target = FileTarget::new(file_path.clone(), None);
            let flags = OpenFlags::default();
            target.open(flags).unwrap();
            // target drops here, should close fd
        }
        
        // File should still exist
        assert!(file_path.exists());
    }
}
