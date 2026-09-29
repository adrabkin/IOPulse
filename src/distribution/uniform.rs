//! Uniform random distribution
//!
//! This module provides a uniform random distribution where all blocks have
//! equal probability. This is the default and simplest distribution.
//!
//! # Performance
//!
//! Uses the xoshiro256++ PRNG which is very fast and has good statistical
//! properties. This is important since next_block() is called for every IO.
//!
//! # Example
//!
//! ```
//! use iopulse::distribution::{Distribution, uniform::UniformDistribution};
//!
//! let mut dist = UniformDistribution::new();
//!
//! // Generate 10 random block numbers
//! for _ in 0..10 {
//!     let block_num = dist.next_block(1024);
//!     assert!(block_num < 1024);
//! }
//!
//! // Worker converts to byte offset:
//! let block_size = 4096;
//! let offset = block_num * block_size;  // Naturally aligned to 4K
//! ```

use super::Distribution;
use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Uniform random distribution
///
/// Generates block numbers with equal probability across the entire range.
/// Uses xoshiro256++ PRNG for fast, high-quality random numbers.
pub struct UniformDistribution {
    rng: Xoshiro256PlusPlus,
}

impl UniformDistribution {
    /// Create a new uniform distribution with random seed
    pub fn new() -> Self {
        Self {
            rng: Xoshiro256PlusPlus::from_entropy(),
        }
    }
    
    /// Create a new uniform distribution with specific seed
    ///
    /// Useful for reproducible tests.
    pub fn with_seed(seed: u64) -> Self {
        Self {
            rng: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }
}

impl Default for UniformDistribution {
    fn default() -> Self {
        Self::new()
    }
}

impl Distribution for UniformDistribution {
    #[inline(always)]
    fn next_block(&mut self, num_blocks: u64) -> u64 {
        if num_blocks == 0 {
            return 0;
        }
        self.rng.gen_range(0..num_blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_uniform_distribution_basic() {
        let mut dist = UniformDistribution::new();
        
        for _ in 0..100 {
            let block_num = dist.next_block(1000);
            assert!(block_num < 1000);
        }
    }
    
    #[test]
    fn test_uniform_distribution_zero_max() {
        let mut dist = UniformDistribution::new();
        let block_num = dist.next_block(0);
        assert_eq!(block_num, 0);
    }
    
    #[test]
    fn test_uniform_distribution_seeded() {
        let mut dist1 = UniformDistribution::with_seed(12345);
        let mut dist2 = UniformDistribution::with_seed(12345);
        
        // Same seed should produce same sequence
        for _ in 0..10 {
            let block1 = dist1.next_block(1000);
            let block2 = dist2.next_block(1000);
            assert_eq!(block1, block2);
        }
    }
    
    #[test]
    fn test_uniform_distribution_coverage() {
        let mut dist = UniformDistribution::with_seed(42);
        let num_blocks = 100u64;
        let mut buckets = vec![0u32; 10];
        
        // Generate many samples
        for _ in 0..10000 {
            let block_num = dist.next_block(num_blocks);
            let bucket = (block_num * 10 / num_blocks) as usize;
            if bucket < 10 {
                buckets[bucket] += 1;
            }
        }
        
        // Each bucket should have roughly 1000 samples (10000 / 10)
        // Allow 20% deviation for randomness
        for count in buckets {
            assert!(count > 800 && count < 1200, "Bucket count {} outside expected range", count);
        }
    }
    
    #[test]
    fn test_uniform_distribution_large_range() {
        let mut dist = UniformDistribution::new();
        let num_blocks = 1024 * 1024 * 1024u64; // 1 billion blocks
        
        for _ in 0..100 {
            let block_num = dist.next_block(num_blocks);
            assert!(block_num < num_blocks);
        }
    }
}
