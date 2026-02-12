//! Buffer management and alignment for high-performance IO
//!
//! This module provides memory-aligned buffers required for O_DIRECT operations
//! and a buffer pool to avoid allocations in the hot path.

use std::alloc::{alloc, dealloc, Layout};
use std::collections::VecDeque;
use std::ptr;

/// Fill pattern for buffer initialization and verification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillPattern {
    /// All zeros
    Zeros,
    /// All ones (0xFF)
    Ones,
    /// Random data with a specific seed
    Random(u64),
    /// Sequential bytes (0x00, 0x01, 0x02, ..., 0xFF, 0x00, ...)
    Sequential,
}

/// Memory-aligned buffer suitable for O_DIRECT operations
///
/// This buffer ensures proper alignment (typically 512 or 4096 bytes)
/// required by O_DIRECT file operations.
pub struct AlignedBuffer {
    ptr: *mut u8,
    size: usize,
    alignment: usize,
    layout: Layout,
}

impl AlignedBuffer {
    /// Create a new aligned buffer with the specified size and alignment
    ///
    /// # Arguments
    /// * `size` - Size of the buffer in bytes
    /// * `alignment` - Alignment requirement (typically 512 or 4096)
    ///
    /// # Panics
    /// Panics if alignment is not a power of 2 or if allocation fails
    pub fn new(size: usize, alignment: usize) -> Self {
        assert!(alignment.is_power_of_two(), "Alignment must be a power of 2");
        assert!(size > 0, "Buffer size must be greater than 0");

        let layout = Layout::from_size_align(size, alignment)
            .expect("Invalid layout parameters");

        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            panic!("Failed to allocate aligned buffer");
        }

        AlignedBuffer {
            ptr,
            size,
            alignment,
            layout,
        }
    }

    /// Get a raw pointer to the buffer
    #[inline(always)]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Get a mutable raw pointer to the buffer
    #[inline(always)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    /// Get the buffer as a slice
    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.size) }
    }

    /// Get the buffer as a mutable slice
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.size) }
    }

    /// Get the size of the buffer in bytes
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the alignment of the buffer
    #[inline(always)]
    pub fn alignment(&self) -> usize {
        self.alignment
    }

    /// Verify that the buffer is properly aligned
    #[inline(always)]
    pub fn is_aligned(&self) -> bool {
        (self.ptr as usize) % self.alignment == 0
    }

    /// Fill the buffer with a specific pattern
    pub fn fill(&mut self, pattern: FillPattern) {
        let slice = self.as_mut_slice();
        
        match pattern {
            FillPattern::Zeros => {
                unsafe { ptr::write_bytes(self.ptr, 0, self.size) };
            }
            FillPattern::Ones => {
                unsafe { ptr::write_bytes(self.ptr, 0xFF, self.size) };
            }
            FillPattern::Random(seed) => {
                // Simple LCG for deterministic random pattern
                let mut state = seed;
                for byte in slice.iter_mut() {
                    state = state.wrapping_mul(1103515245).wrapping_add(12345);
                    *byte = (state >> 16) as u8;
                }
            }
            FillPattern::Sequential => {
                for (i, byte) in slice.iter_mut().enumerate() {
                    *byte = (i % 256) as u8;
                }
            }
        }
    }

    /// Verify that the buffer contains the expected pattern
    ///
    /// Returns `Ok(())` if the pattern matches, or `Err(offset)` with the
    /// first mismatched byte offset if verification fails.
    pub fn verify(&self, pattern: FillPattern) -> Result<(), usize> {
        let slice = self.as_slice();
        
        match pattern {
            FillPattern::Zeros => {
                for (i, &byte) in slice.iter().enumerate() {
                    if byte != 0 {
                        return Err(i);
                    }
                }
            }
            FillPattern::Ones => {
                for (i, &byte) in slice.iter().enumerate() {
                    if byte != 0xFF {
                        return Err(i);
                    }
                }
            }
            FillPattern::Random(seed) => {
                let mut state = seed;
                for (i, &byte) in slice.iter().enumerate() {
                    state = state.wrapping_mul(1103515245).wrapping_add(12345);
                    let expected = (state >> 16) as u8;
                    if byte != expected {
                        return Err(i);
                    }
                }
            }
            FillPattern::Sequential => {
                for (i, &byte) in slice.iter().enumerate() {
                    let expected = (i % 256) as u8;
                    if byte != expected {
                        return Err(i);
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.ptr, self.layout);
        }
    }
}

// AlignedBuffer is Send because it owns its memory
unsafe impl Send for AlignedBuffer {}

/// Pre-allocated pool of aligned buffers for zero-allocation IO operations
///
/// The buffer pool maintains a collection of pre-allocated buffers that can be
/// borrowed and returned without any allocation overhead in the hot path.
pub struct BufferPool {
    buffers: Vec<AlignedBuffer>,
    available: VecDeque<usize>,
    buffer_size: usize,
    alignment: usize,
}

impl BufferPool {
    /// Create a new buffer pool with the specified parameters
    ///
    /// # Arguments
    /// * `num_buffers` - Number of buffers to pre-allocate
    /// * `buffer_size` - Size of each buffer in bytes
    /// * `alignment` - Alignment requirement (typically 512 or 4096)
    pub fn new(num_buffers: usize, buffer_size: usize, alignment: usize) -> Self {
        let mut buffers = Vec::with_capacity(num_buffers);
        let mut available = VecDeque::with_capacity(num_buffers);

        for i in 0..num_buffers {
            buffers.push(AlignedBuffer::new(buffer_size, alignment));
            available.push_back(i);
        }

        BufferPool {
            buffers,
            available,
            buffer_size,
            alignment,
        }
    }
    
    /// Pre-fill all buffers with random data
    ///
    /// This should be called once at initialization to avoid regenerating
    /// random data for every write operation.
    pub fn prefill_random(&mut self) {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        
        for buffer in &mut self.buffers {
            let slice = buffer.as_mut_slice();
            rng.fill_bytes(slice);
        }
    }

    /// Get a buffer from the pool
    ///
    /// Returns `Some(index)` if a buffer is available, or `None` if the pool is empty.
    /// The caller must return the buffer using `return_buffer()` when done.
    #[inline(always)]
    pub fn get(&mut self) -> Option<usize> {
        self.available.pop_front()
    }

    /// Return a buffer to the pool
    ///
    /// # Arguments
    /// * `index` - The buffer index previously obtained from `get()`
    ///
    /// # Panics
    /// Panics if the index is out of bounds
    #[inline(always)]
    pub fn return_buffer(&mut self, index: usize) {
        assert!(index < self.buffers.len(), "Invalid buffer index");
        self.available.push_back(index);
    }

    /// Get a reference to a buffer by index
    ///
    /// # Panics
    /// Panics if the index is out of bounds
    #[inline]
    pub fn get_buffer(&self, index: usize) -> &AlignedBuffer {
        &self.buffers[index]
    }

    /// Get a mutable reference to a buffer by index
    ///
    /// # Panics
    /// Panics if the index is out of bounds
    #[inline(always)]
    pub fn get_buffer_mut(&mut self, index: usize) -> &mut AlignedBuffer {
        &mut self.buffers[index]
    }

    /// Get the number of available buffers
    #[inline]
    pub fn available_count(&self) -> usize {
        self.available.len()
    }

    /// Get the total number of buffers in the pool
    #[inline]
    pub fn total_count(&self) -> usize {
        self.buffers.len()
    }

    /// Get the size of each buffer
    #[inline]
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get the alignment of buffers in the pool
    #[inline]
    pub fn alignment(&self) -> usize {
        self.alignment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aligned_buffer_creation() {
        let buffer = AlignedBuffer::new(4096, 512);
        assert_eq!(buffer.size(), 4096);
        assert_eq!(buffer.alignment(), 512);
        assert!(buffer.is_aligned());
    }

    #[test]
    fn test_aligned_buffer_4k_alignment() {
        let buffer = AlignedBuffer::new(8192, 4096);
        assert_eq!(buffer.size(), 8192);
        assert_eq!(buffer.alignment(), 4096);
        assert!(buffer.is_aligned());
    }

    #[test]
    #[should_panic(expected = "Alignment must be a power of 2")]
    fn test_invalid_alignment() {
        let _ = AlignedBuffer::new(4096, 513);
    }

    #[test]
    fn test_buffer_fill_zeros() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        buffer.fill(FillPattern::Zeros);
        
        for &byte in buffer.as_slice() {
            assert_eq!(byte, 0);
        }
    }

    #[test]
    fn test_buffer_fill_ones() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        buffer.fill(FillPattern::Ones);
        
        for &byte in buffer.as_slice() {
            assert_eq!(byte, 0xFF);
        }
    }

    #[test]
    fn test_buffer_fill_sequential() {
        let mut buffer = AlignedBuffer::new(512, 512);
        buffer.fill(FillPattern::Sequential);
        
        let slice = buffer.as_slice();
        for i in 0..256 {
            assert_eq!(slice[i], i as u8);
        }
        // Wraps around after 256
        for i in 256..512 {
            assert_eq!(slice[i], (i % 256) as u8);
        }
    }

    #[test]
    fn test_buffer_fill_random() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        let seed = 12345u64;
        buffer.fill(FillPattern::Random(seed));
        
        // Verify the pattern is deterministic
        let mut buffer2 = AlignedBuffer::new(1024, 512);
        buffer2.fill(FillPattern::Random(seed));
        
        assert_eq!(buffer.as_slice(), buffer2.as_slice());
    }

    #[test]
    fn test_buffer_verify_zeros() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        buffer.fill(FillPattern::Zeros);
        assert!(buffer.verify(FillPattern::Zeros).is_ok());
    }

    #[test]
    fn test_buffer_verify_ones() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        buffer.fill(FillPattern::Ones);
        assert!(buffer.verify(FillPattern::Ones).is_ok());
    }

    #[test]
    fn test_buffer_verify_sequential() {
        let mut buffer = AlignedBuffer::new(512, 512);
        buffer.fill(FillPattern::Sequential);
        assert!(buffer.verify(FillPattern::Sequential).is_ok());
    }

    #[test]
    fn test_buffer_verify_random() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        let seed = 54321u64;
        buffer.fill(FillPattern::Random(seed));
        assert!(buffer.verify(FillPattern::Random(seed)).is_ok());
    }

    #[test]
    fn test_buffer_verify_mismatch() {
        let mut buffer = AlignedBuffer::new(1024, 512);
        buffer.fill(FillPattern::Zeros);
        
        // Corrupt one byte
        buffer.as_mut_slice()[100] = 0xFF;
        
        match buffer.verify(FillPattern::Zeros) {
            Err(offset) => assert_eq!(offset, 100),
            Ok(_) => panic!("Expected verification to fail"),
        }
    }

    #[test]
    fn test_buffer_pool_creation() {
        let pool = BufferPool::new(10, 4096, 512);
        assert_eq!(pool.total_count(), 10);
        assert_eq!(pool.available_count(), 10);
        assert_eq!(pool.buffer_size(), 4096);
        assert_eq!(pool.alignment(), 512);
    }

    #[test]
    fn test_buffer_pool_get_return() {
        let mut pool = BufferPool::new(5, 4096, 512);
        
        // Get all buffers
        let mut indices = Vec::new();
        for _ in 0..5 {
            indices.push(pool.get().expect("Should have buffer available"));
        }
        
        assert_eq!(pool.available_count(), 0);
        assert!(pool.get().is_none());
        
        // Return all buffers
        for index in indices {
            pool.return_buffer(index);
        }
        
        assert_eq!(pool.available_count(), 5);
    }

    #[test]
    fn test_buffer_pool_access() {
        let mut pool = BufferPool::new(3, 1024, 512);
        
        let index = pool.get().unwrap();
        let buffer = pool.get_buffer_mut(index);
        buffer.fill(FillPattern::Ones);
        
        let buffer_ref = pool.get_buffer(index);
        assert!(buffer_ref.verify(FillPattern::Ones).is_ok());
        
        pool.return_buffer(index);
    }

    #[test]
    fn test_buffer_pool_all_aligned() {
        let pool = BufferPool::new(10, 4096, 4096);
        
        for i in 0..pool.total_count() {
            let buffer = pool.get_buffer(i);
            assert!(buffer.is_aligned());
        }
    }
}
