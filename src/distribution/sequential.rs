//! Sequential block generation
//!
//! Generates sequential block numbers starting from 0 and incrementing by 1.
//! When the end of the file is reached, wraps back to the beginning.

use crate::distribution::Distribution;

/// Sequential block generator
///
/// Generates block numbers in sequential order: 0, 1, 2, 3, ...
/// Wraps around to 0 when reaching the end of the file.
#[derive(Debug)]
pub struct SequentialDistribution {
    /// Current block number
    current_block: u64,
}

impl SequentialDistribution {
    /// Create a new sequential distribution
    pub fn new() -> Self {
        Self {
            current_block: 0,
        }
    }
}

impl Default for SequentialDistribution {
    fn default() -> Self {
        Self::new()
    }
}

impl Distribution for SequentialDistribution {
    fn next_block(&mut self, num_blocks: u64) -> u64 {
        if num_blocks == 0 {
            return 0; // Handle empty file case
        }
        
        let block = self.current_block;
        
        // Increment for next call
        self.current_block += 1;
        
        // Wrap around if we exceed num_blocks
        if self.current_block >= num_blocks {
            self.current_block = 0;
        }
        
        block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sequential_basic() {
        let mut dist = SequentialDistribution::new();
        
        assert_eq!(dist.next_block(100), 0);
        assert_eq!(dist.next_block(100), 1);
        assert_eq!(dist.next_block(100), 2);
        assert_eq!(dist.next_block(100), 3);
    }
    
    #[test]
    fn test_sequential_wraparound() {
        let mut dist = SequentialDistribution::new();
        let num_blocks = 3;
        
        assert_eq!(dist.next_block(num_blocks), 0);
        assert_eq!(dist.next_block(num_blocks), 1);
        assert_eq!(dist.next_block(num_blocks), 2);
        assert_eq!(dist.next_block(num_blocks), 0);  // Wrapped
        assert_eq!(dist.next_block(num_blocks), 1);  // Wrapped
    }
    
    #[test]
    fn test_sequential_large_range() {
        let mut dist = SequentialDistribution::new();
        let num_blocks = 1000000;
        
        for i in 0..100 {
            assert_eq!(dist.next_block(num_blocks), i);
        }
    }
}
