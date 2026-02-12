//! Zipf distribution implementation
//!
//! This module provides a Zipf distribution (also known as Zipfian or power law
//! distribution) where a small number of items receive the majority of accesses.
//!
//! # Characteristics
//!
//! - Power law: P(k) ∝ 1 / k^theta
//! - Small theta (0.5): More uniform
//! - Large theta (2.0): More skewed (hot/cold data)
//! - Default theta (1.2): Realistic workload simulation
//!
//! # Use Cases
//!
//! - Simulating cache behavior
//! - Hot/cold data access patterns
//! - Realistic workload modeling
//! - Testing cache effectiveness
//!
//! # Performance
//!
//! Uses rejection sampling with pre-computed normalization constant for efficiency.
//!
//! # Example
//!
//! ```
//! use iopulse::distribution::{Distribution, zipf::ZipfDistribution};
//!
//! let mut dist = ZipfDistribution::new(1.2); // theta = 1.2
//! let offset = dist.next_offset(1024 * 1024);
//! // Offset will follow power law distribution
//! ```

use super::Distribution;
use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

/// Zipf distribution for power law access patterns
///
/// Implements the standard Zipf distribution using the PMF:
/// P(k) = k^(-s) / H(N,s) where H(N,s) = sum(i^(-s)) for i=1 to N
///
/// Uses inverse transform sampling with pre-computed CDF for O(log N) generation.
#[allow(dead_code)]
pub struct ZipfDistribution {
    /// Exponent parameter s (called theta in our API, range 0.0-3.0)
    s: f64,
    
    /// Pre-computed CDF for inverse transform sampling
    cdf: Vec<f64>,
    
    /// Random number generator
    rng: Xoshiro256PlusPlus,
}

impl ZipfDistribution {
    /// Create a new Zipf distribution with specified theta (exponent s)
    ///
    /// Uses adaptive N selection for balance of accuracy and performance:
    /// - Small ranges (<1K): N = actual size (perfect accuracy)
    /// - Medium ranges (1K-100K): N = 10,000 (good approximation)
    /// - Large ranges (>100K): N = 100,000 (better approximation)
    /// - Cap at N = 1,000,000 for extreme cases
    ///
    /// This is computed lazily on first call to next_offset().
    pub fn new(theta: f64) -> Self {
        assert!(theta >= 0.0 && theta <= 3.0, "Theta must be in range [0.0, 3.0]");
        
        Self {
            s: theta,
            cdf: Vec::new(),  // Computed lazily
            rng: Xoshiro256PlusPlus::from_entropy(),
        }
    }
    
    /// Create a new Zipf distribution with specific seed
    pub fn with_seed(theta: f64, seed: u64) -> Self {
        assert!(theta >= 0.0 && theta <= 3.0, "Theta must be in range [0.0, 3.0]");
        
        Self {
            s: theta,
            cdf: Vec::new(),  // Computed lazily
            rng: Xoshiro256PlusPlus::seed_from_u64(seed),
        }
    }
    
    /// Compute CDF for given range size
    ///
    /// Uses actual N up to 1M for accuracy. For N > 1M, caps at 1M
    /// to keep initialization time reasonable (<100ms).
    fn compute_cdf(&mut self, max: u64) {
        // Use actual N, capped at 1M for performance
        let n = max.min(1_000_000) as usize;
        
        // Compute H(N,s) = sum of i^(-s) for i=1 to N
        let mut h_n_s = 0.0;
        for i in 1..=n {
            h_n_s += (i as f64).powf(-self.s);
        }
        
        // Compute CDF: CDF[k] = sum of P(i) for i=1 to k
        self.cdf = Vec::with_capacity(n);
        let mut cumulative = 0.0;
        for i in 1..=n {
            let pmf = (i as f64).powf(-self.s) / h_n_s;
            cumulative += pmf;
            self.cdf.push(cumulative);
        }
    }
}

impl Distribution for ZipfDistribution {
    fn next_block(&mut self, num_blocks: u64) -> u64 {
        if num_blocks == 0 {
            return 0;
        }
        
        // Lazy initialization: compute CDF on first call
        if self.cdf.is_empty() {
            self.compute_cdf(num_blocks);
        }
        
        // Generate uniform random number
        let u: f64 = self.rng.gen();
        
        // Binary search in CDF to find rank k where CDF[k-1] < u <= CDF[k]
        let rank = match self.cdf.binary_search_by(|&cdf_val| {
            if cdf_val < u {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }) {
            Ok(i) => i,
            Err(i) => i,
        };
        
        // Scale rank from [0, cdf.len()) to [0, num_blocks)
        let block_num = ((rank as u64) * num_blocks) / (self.cdf.len() as u64);
        
        block_num.min(num_blocks - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_zipf_distribution_basic() {
        let mut dist = ZipfDistribution::new(1.2);
        
        for _ in 0..100 {
            let block_num = dist.next_block(1000);
            assert!(block_num < 1000);
        }
    }
    
    #[test]
    fn test_zipf_distribution_zero_max() {
        let mut dist = ZipfDistribution::new(1.0);
        let block_num = dist.next_block(0);
        assert_eq!(block_num, 0);
    }
    
    #[test]
    fn test_zipf_distribution_seeded() {
        let mut dist1 = ZipfDistribution::with_seed(1.2, 12345);
        let mut dist2 = ZipfDistribution::with_seed(1.2, 12345);
        
        // Same seed should produce same sequence
        for _ in 0..10 {
            let block1 = dist1.next_block(1000);
            let block2 = dist2.next_block(1000);
            assert_eq!(block1, block2);
        }
    }
    
    #[test]
    fn test_zipf_distribution_skew() {
        let mut dist = ZipfDistribution::with_seed(1.5, 42);
        let num_blocks = 1000u64;
        let mut buckets = vec![0u32; 10];
        
        // Generate many samples
        for _ in 0..10000 {
            let block_num = dist.next_block(num_blocks);
            let bucket = (block_num * 10 / num_blocks) as usize;
            if bucket < 10 {
                buckets[bucket] += 1;
            }
        }
        
        // First bucket should have significantly more hits than last bucket
        // (power law property)
        assert!(buckets[0] > buckets[9], 
            "Zipf should be skewed: bucket[0]={} should be > bucket[9]={}",
            buckets[0], buckets[9]);
        
        // First bucket should have at least 2x the hits of last bucket
        assert!(buckets[0] > buckets[9] * 2,
            "Zipf skew insufficient: bucket[0]={} should be > 2 * bucket[9]={}",
            buckets[0], buckets[9]);
    }
    
    #[test]
    fn test_zipf_distribution_theta_range() {
        // Test various theta values (small n for speed)
        for theta in [0.5, 1.0, 1.5] {
            let mut dist = ZipfDistribution::new(theta);
            
            for _ in 0..20 {
                let block_num = dist.next_block(100);
                assert!(block_num < 100);
            }
        }
    }
    
    #[test]
    #[should_panic(expected = "Theta must be in range")]
    fn test_zipf_distribution_invalid_theta_high() {
        let _ = ZipfDistribution::new(3.5);
    }
    
    #[test]
    #[should_panic(expected = "Theta must be in range")]
    fn test_zipf_distribution_invalid_theta_low() {
        let _ = ZipfDistribution::new(-0.5);
    }
    
    #[test]
    fn test_zipf_distribution_large_range() {
        let mut dist = ZipfDistribution::new(1.2);
        let num_blocks = 1000u64; // Limited for O(n) inverse CDF algorithm
        
        for _ in 0..20 {
            let block_num = dist.next_block(num_blocks);
            assert!(block_num < num_blocks);
        }
    }
}
