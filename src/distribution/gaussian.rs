//! Gaussian (normal) distribution implementation
//!
//! This module provides a Gaussian/normal distribution for simulating locality
//! of reference where accesses cluster around a center point.
//!
//! # Characteristics
//!
//! - Bell curve centered at a configurable point
//! - Standard deviation controls spread
//! - Simulates spatial locality
//! - Good for testing cache locality
//!
//! # Parameters
//!
//! - **stddev**: Standard deviation (spread of distribution)
//! - **center**: Center point as fraction of range (0.0-1.0)
//!
//! # Use Cases
//!
//! - Simulating locality of reference
//! - Testing cache locality
//! - Workload with hot region
//! - Sequential-ish access with variation
//!
//! # Performance
//!
//! Uses Box-Muller transform for generating normal random variables (O(1)).
//!
//! # Example
//!
//! ```
//! use iopulse::distribution::{Distribution, gaussian::GaussianDistribution};
//!
//! // Center at 50%, stddev = 0.1 (10% of range)
//! let mut dist = GaussianDistribution::new(0.1, 0.5);
//! let block_num = dist.next_block(1024 * 1024);
//! // Offset will cluster around 512KB (50% of 1MB)
//! ```

use super::Distribution;
use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::f64::consts::PI;

/// Gaussian distribution for locality of reference
///
/// Generates offsets following a normal distribution centered at a configurable
/// point. The standard deviation controls how spread out the accesses are.
pub struct GaussianDistribution {
    /// Standard deviation (spread)
    stddev: f64,
    
    /// Center point as fraction of range (0.0-1.0)
    center: f64,
    
    /// Random number generator
    rng: Xoshiro256PlusPlus,
    
    /// Cached spare value from Box-Muller transform
    spare: Option<f64>,
}

impl GaussianDistribution {
    /// Create a new Gaussian distribution
    ///
    /// # Arguments
    ///
    /// * `stddev` - Standard deviation (must be > 0)
    /// * `center` - Center point as fraction (0.0-1.0)
    ///   - 0.0: Center at start
    ///   - 0.5: Center at middle (default)
    ///   - 1.0: Center at end
    ///
    /// # Panics
    ///
    /// Panics if stddev <= 0 or center outside [0.0, 1.0].
    pub fn new(stddev: f64, center: f64) -> Self {
        assert!(stddev > 0.0, "Standard deviation must be positive");
        assert!(center >= 0.0 && center <= 1.0, "Center must be in range [0.0, 1.0]");
        
        Self {
            stddev,
            center,
            rng: Xoshiro256PlusPlus::from_entropy(),
            spare: None,
        }
    }
    
    /// Create a new Gaussian distribution with specific seed
    ///
    /// Useful for reproducible tests.
    pub fn with_seed(stddev: f64, center: f64, seed: u64) -> Self {
        assert!(stddev > 0.0, "Standard deviation must be positive");
        assert!(center >= 0.0 && center <= 1.0, "Center must be in range [0.0, 1.0]");
        
        Self {
            stddev,
            center,
            rng: Xoshiro256PlusPlus::seed_from_u64(seed),
            spare: None,
        }
    }
    
    /// Generate a standard normal random variable using Box-Muller transform
    ///
    /// This generates two independent normal(0,1) variables from two uniform
    /// random variables. We cache the spare for the next call.
    fn generate_standard_normal(&mut self) -> f64 {
        // Use cached spare if available
        if let Some(spare) = self.spare.take() {
            return spare;
        }
        
        // Box-Muller transform
        let u1: f64 = self.rng.gen();
        let u2: f64 = self.rng.gen();
        
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = 2.0 * PI * u2;
        
        let z0 = r * theta.cos();
        let z1 = r * theta.sin();
        
        // Cache z1 for next call
        self.spare = Some(z1);
        
        z0
    }
}

impl Distribution for GaussianDistribution {
    fn next_block(&mut self, num_blocks: u64) -> u64 {
        if num_blocks == 0 {
            return 0;
        }
        
        if num_blocks == 1 {
            return 0;
        }
        
        // Generate standard normal N(0,1)
        let z = self.generate_standard_normal();
        
        // Transform to N(center, stddev)
        let num_blocks_f64 = num_blocks as f64;
        let center_block = self.center * num_blocks_f64;
        let value = center_block + z * self.stddev * num_blocks_f64;
        
        // Clamp to valid range [0, num_blocks)
        let clamped = value.max(0.0).min(num_blocks_f64 - 1.0);
        clamped as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_gaussian_distribution_basic() {
        let mut dist = GaussianDistribution::new(0.1, 0.5);
        
        for _ in 0..100 {
            let block_num = dist.next_block(1000);
            assert!(block_num < 1000);
        }
    }
    
    #[test]
    fn test_gaussian_distribution_zero_max() {
        let mut dist = GaussianDistribution::new(0.1, 0.5);
        let block_num = dist.next_block(0);
        assert_eq!(block_num, 0);
    }
    
    #[test]
    fn test_gaussian_distribution_seeded() {
        let mut dist1 = GaussianDistribution::with_seed(0.1, 0.5, 12345);
        let mut dist2 = GaussianDistribution::with_seed(0.1, 0.5, 12345);
        
        // Same seed should produce same sequence
        for _ in 0..10 {
            let block1 = dist1.next_block(1000);
            let block2 = dist2.next_block(1000);
            assert_eq!(block1, block2);
        }
    }
    
    #[test]
    fn test_gaussian_distribution_clustering() {
        let mut dist = GaussianDistribution::with_seed(0.1, 0.5, 42);
        let num_blocks = 1000u64;
        let center = (num_blocks / 2) as i64;
        let mut distances: Vec<i64> = Vec::new();
        
        // Generate samples and measure distance from center
        for _ in 0..1000 {
            let block_num = dist.next_block(num_blocks) as i64;
            let distance = (block_num - center).abs();
            distances.push(distance);
        }
        
        // Calculate average distance from center
        let avg_distance = distances.iter().sum::<i64>() as f64 / distances.len() as f64;
        
        // With stddev=0.1 (10% of range), average distance should be small
        // (roughly 0.1 * num_blocks * 0.8 due to normal distribution properties)
        assert!(avg_distance < num_blocks as f64 * 0.15,
            "Gaussian should cluster around center: avg_distance={}, num_blocks={}",
            avg_distance, num_blocks);
    }
    
    #[test]
    fn test_gaussian_distribution_center_positions() {
        // Test different center positions
        for center in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut dist = GaussianDistribution::new(0.1, center);
            
            for _ in 0..50 {
                let block_num = dist.next_block(1000);
                assert!(block_num < 1000);
            }
        }
    }
    
    #[test]
    fn test_gaussian_distribution_stddev_range() {
        // Test different standard deviations
        for stddev in [0.05, 0.1, 0.2, 0.3] {
            let mut dist = GaussianDistribution::new(stddev, 0.5);
            
            for _ in 0..50 {
                let block_num = dist.next_block(1000);
                assert!(block_num < 1000);
            }
        }
    }
    
    #[test]
    #[should_panic(expected = "Standard deviation must be positive")]
    fn test_gaussian_distribution_invalid_stddev() {
        let _ = GaussianDistribution::new(0.0, 0.5);
    }
    
    #[test]
    #[should_panic(expected = "Center must be in range")]
    fn test_gaussian_distribution_invalid_center_high() {
        let _ = GaussianDistribution::new(0.1, 1.5);
    }
    
    #[test]
    #[should_panic(expected = "Center must be in range")]
    fn test_gaussian_distribution_invalid_center_low() {
        let _ = GaussianDistribution::new(0.1, -0.5);
    }
    
    #[test]
    fn test_gaussian_distribution_large_range() {
        let mut dist = GaussianDistribution::new(0.1, 0.5);
        let num_blocks = 1024 * 1024 * 1024u64; // 1 billion blocks
        
        for _ in 0..100 {
            let block_num = dist.next_block(num_blocks);
            assert!(block_num < num_blocks);
        }
    }
}
