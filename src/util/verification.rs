//! Data verification utilities
//!
//! This module provides utilities for verifying data integrity during IO operations.
//! It supports various verification patterns and can detect data corruption or loss.

/// Verification pattern for data integrity checking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationPattern {
    /// All zeros
    Zeros,
    /// All ones (0xFF)
    Ones,
    /// Random data with a specific seed
    Random(u64),
    /// Sequential bytes (0x00, 0x01, 0x02, ..., 0xFF, 0x00, ...)
    Sequential,
}

/// Verification result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// Data matches expected pattern
    Success,
    /// Data does not match expected pattern
    Failure {
        /// Offset of first mismatch
        offset: usize,
        /// Expected value
        expected: u8,
        /// Actual value
        actual: u8,
    },
}

/// Verify that a buffer contains the expected pattern
///
/// # Arguments
///
/// * `buffer` - The buffer to verify
/// * `pattern` - The expected pattern
/// * `offset` - Starting offset in the file (for sequential pattern)
///
/// # Returns
///
/// `VerificationResult::Success` if the pattern matches, or
/// `VerificationResult::Failure` with details if verification fails.
pub fn verify_buffer(
    buffer: &[u8],
    pattern: VerificationPattern,
    offset: u64,
) -> VerificationResult {
    match pattern {
        VerificationPattern::Zeros => verify_zeros(buffer),
        VerificationPattern::Ones => verify_ones(buffer),
        VerificationPattern::Random(seed) => verify_random(buffer, seed),
        VerificationPattern::Sequential => verify_sequential(buffer, offset),
    }
}

/// Fill a buffer with a specific pattern
///
/// # Arguments
///
/// * `buffer` - The buffer to fill
/// * `pattern` - The pattern to use
/// * `offset` - Starting offset in the file (for sequential pattern)
pub fn fill_buffer(buffer: &mut [u8], pattern: VerificationPattern, offset: u64) {
    match pattern {
        VerificationPattern::Zeros => {
            buffer.fill(0);
        }
        VerificationPattern::Ones => {
            buffer.fill(0xFF);
        }
        VerificationPattern::Random(seed) => {
            fill_random(buffer, seed);
        }
        VerificationPattern::Sequential => {
            fill_sequential(buffer, offset);
        }
    }
}

fn verify_zeros(buffer: &[u8]) -> VerificationResult {
    for (i, &byte) in buffer.iter().enumerate() {
        if byte != 0 {
            return VerificationResult::Failure {
                offset: i,
                expected: 0,
                actual: byte,
            };
        }
    }
    VerificationResult::Success
}

fn verify_ones(buffer: &[u8]) -> VerificationResult {
    for (i, &byte) in buffer.iter().enumerate() {
        if byte != 0xFF {
            return VerificationResult::Failure {
                offset: i,
                expected: 0xFF,
                actual: byte,
            };
        }
    }
    VerificationResult::Success
}

fn verify_random(buffer: &[u8], seed: u64) -> VerificationResult {
    let mut state = seed;
    for (i, &byte) in buffer.iter().enumerate() {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        let expected = (state >> 16) as u8;
        if byte != expected {
            return VerificationResult::Failure {
                offset: i,
                expected,
                actual: byte,
            };
        }
    }
    VerificationResult::Success
}

fn verify_sequential(buffer: &[u8], file_offset: u64) -> VerificationResult {
    for (i, &byte) in buffer.iter().enumerate() {
        let expected = ((file_offset + i as u64) % 256) as u8;
        if byte != expected {
            return VerificationResult::Failure {
                offset: i,
                expected,
                actual: byte,
            };
        }
    }
    VerificationResult::Success
}

fn fill_random(buffer: &mut [u8], seed: u64) {
    // Use simple LCG (same as verify_random) for deterministic verification
    // This ensures write and read produce the same sequence
    let mut state = seed;
    for byte in buffer.iter_mut() {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *byte = (state >> 16) as u8;
    }
}

fn fill_sequential(buffer: &mut [u8], file_offset: u64) {
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = ((file_offset + i as u64) % 256) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_zeros() {
        let mut buffer = vec![0u8; 1024];
        fill_buffer(&mut buffer, VerificationPattern::Zeros, 0);
        assert_eq!(
            verify_buffer(&buffer, VerificationPattern::Zeros, 0),
            VerificationResult::Success
        );

        buffer[100] = 1;
        match verify_buffer(&buffer, VerificationPattern::Zeros, 0) {
            VerificationResult::Failure { offset, expected, actual } => {
                assert_eq!(offset, 100);
                assert_eq!(expected, 0);
                assert_eq!(actual, 1);
            }
            _ => panic!("Expected failure"),
        }
    }

    #[test]
    fn test_verify_ones() {
        let mut buffer = vec![0u8; 1024];
        fill_buffer(&mut buffer, VerificationPattern::Ones, 0);
        assert_eq!(
            verify_buffer(&buffer, VerificationPattern::Ones, 0),
            VerificationResult::Success
        );
    }

    #[test]
    fn test_verify_random() {
        let mut buffer = vec![0u8; 1024];
        let seed = 12345u64;
        fill_buffer(&mut buffer, VerificationPattern::Random(seed), 0);
        assert_eq!(
            verify_buffer(&buffer, VerificationPattern::Random(seed), 0),
            VerificationResult::Success
        );

        // Different seed should fail
        match verify_buffer(&buffer, VerificationPattern::Random(54321), 0) {
            VerificationResult::Failure { .. } => {}
            _ => panic!("Expected failure with different seed"),
        }
    }

    #[test]
    fn test_verify_sequential() {
        let mut buffer = vec![0u8; 512];
        fill_buffer(&mut buffer, VerificationPattern::Sequential, 0);
        assert_eq!(
            verify_buffer(&buffer, VerificationPattern::Sequential, 0),
            VerificationResult::Success
        );

        // Verify at different offset
        fill_buffer(&mut buffer, VerificationPattern::Sequential, 1000);
        assert_eq!(
            verify_buffer(&buffer, VerificationPattern::Sequential, 1000),
            VerificationResult::Success
        );
    }

    #[test]
    fn test_sequential_wraps() {
        let mut buffer = vec![0u8; 300];
        fill_buffer(&mut buffer, VerificationPattern::Sequential, 200);
        
        // Should wrap around at 256
        assert_eq!(buffer[0], 200);
        assert_eq!(buffer[55], 255);
        assert_eq!(buffer[56], 0);
        assert_eq!(buffer[57], 1);
    }
}
