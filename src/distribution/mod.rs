//! Random distribution implementations
//!
//! This module provides various statistical distributions for generating random
//! block numbers in IO operations. Different distributions simulate different access
//! patterns and are useful for testing various scenarios.
//!
//! # Distributions
//!
//! - **Uniform**: Equal probability for all blocks (default random)
//! - **Zipf**: Power law distribution (hot/cold data)
//! - **Pareto**: 80/20 rule (Pareto principle)
//! - **Gaussian**: Normal distribution (locality of reference)
//!
//! # Block-Based Design
//!
//! Distributions generate block numbers (0, 1, 2, ..., N-1) rather than byte offsets.
//! This ensures offsets are naturally aligned to block size, which is required for
//! O_DIRECT and provides optimal performance.
//!
//! The worker converts block numbers to byte offsets: `offset = block_num * block_size`
//!
//! # Example
//!
//! ```
//! use iopulse::distribution::{Distribution, uniform::UniformDistribution};
//!
//! let mut dist = UniformDistribution::new();
//! let block_num = dist.next_block(1024); // Random block in range [0, 1024)
//! assert!(block_num < 1024);
//!
//! // Worker converts to byte offset:
//! let block_size = 4096;
//! let offset = block_num * block_size;  // Naturally aligned to 4K
//! ```

/// Distribution trait for block number generation
///
/// This trait defines the interface for all random distributions used to generate
/// block numbers for IO operations. Each distribution implements a different
/// statistical pattern.
///
/// # Block-Based Design
///
/// Distributions work in blocks, not bytes. This ensures:
/// - Offsets are naturally aligned to block size (required for O_DIRECT)
/// - No runtime alignment overhead
/// - Clearer semantics (storage works in blocks, not bytes)
/// - Better performance (1 multiplication vs 2 divisions + 1 multiplication)
///
/// # Thread Safety
///
/// Distributions must be `Send` to allow transfer between threads. Each worker
/// thread typically owns its own distribution instance to avoid contention.
///
/// # Implementation Notes
///
/// - Distributions should be fast (called for every IO operation)
/// - Use efficient PRNGs (xoshiro, PCG, etc.)
/// - Consider pre-computed lookup tables for complex distributions
/// - Ensure thread-safety (no shared mutable state)
pub trait Distribution: Send {
    /// Generate next block number within range
    ///
    /// Returns a random block number in the range [0, num_blocks). The distribution
    /// of returned values depends on the specific distribution implementation.
    ///
    /// The worker converts this to a byte offset: `offset = block_num * block_size`
    ///
    /// # Arguments
    ///
    /// * `num_blocks` - Number of blocks in the file/device
    ///
    /// # Returns
    ///
    /// A block number in the range [0, num_blocks).
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::distribution::{Distribution, uniform::UniformDistribution};
    ///
    /// let mut dist = UniformDistribution::new();
    /// let block_num = dist.next_block(1000);
    /// assert!(block_num < 1000);
    ///
    /// // Convert to byte offset (worker does this):
    /// let block_size = 4096;
    /// let offset = block_num * block_size;  // Naturally aligned
    /// ```
    fn next_block(&mut self, num_blocks: u64) -> u64;
}

pub mod uniform;
pub mod zipf;
pub mod pareto;
pub mod gaussian;
pub mod sequential;
