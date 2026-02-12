//! Mock IO engine for testing
//!
//! This module provides a mock implementation of the IOEngine trait that can be used
//! in tests. The mock engine simulates IO operations without actually performing any
//! system calls, making tests fast and deterministic.
//!
//! # Features
//!
//! - Configurable success/failure behavior
//! - Tracks all submitted operations
//! - Simulates latency with configurable delays
//! - Supports all operation types
//! - Thread-safe operation tracking
//!
//! # Example
//!
//! ```
//! use iopulse::engine::{IOEngine, EngineConfig, IOOperation, OperationType};
//! use iopulse::engine::mock::MockEngine;
//!
//! let mut engine = MockEngine::new();
//! let config = EngineConfig::default();
//! engine.init(&config).unwrap();
//!
//! // Submit a mock operation
//! let op = IOOperation {
//!     op_type: OperationType::Read,
//!     target_fd: 1,
//!     offset: 0,
//!     buffer: std::ptr::null_mut(),
//!     length: 4096,
//!     user_data: 42,
//! };
//! engine.submit(op).unwrap();
//!
//! // Poll for completions
//! let completions = engine.poll_completions().unwrap();
//! assert_eq!(completions.len(), 1);
//! assert_eq!(completions[0].user_data, 42);
//! ```

use super::{EngineCapabilities, EngineConfig, IOCompletion, IOEngine, IOOperation, OperationType};
use crate::Result;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Mock IO engine for testing
///
/// This engine simulates IO operations without performing actual system calls.
/// It can be configured to succeed or fail operations, track submitted operations,
/// and simulate various engine capabilities.
#[derive(Clone)]
pub struct MockEngine {
    /// Configuration used to initialize the engine
    config: Option<EngineConfig>,
    
    /// Queue of pending operations
    pending: Arc<Mutex<VecDeque<IOOperation>>>,
    
    /// Whether operations should succeed or fail
    should_fail: Arc<Mutex<bool>>,
    
    /// Error message to return when operations fail
    error_message: Arc<Mutex<String>>,
    
    /// Number of bytes to return for successful operations
    bytes_per_op: Arc<Mutex<usize>>,
    
    /// Capabilities to report
    capabilities: EngineCapabilities,
    
    /// Track all submitted operations for verification
    submitted_ops: Arc<Mutex<Vec<OperationRecord>>>,
}

/// Record of a submitted operation for testing verification
#[derive(Debug, Clone)]
pub struct OperationRecord {
    pub op_type: OperationType,
    pub target_fd: i32,
    pub offset: u64,
    pub length: usize,
    pub user_data: u64,
}

impl MockEngine {
    /// Create a new mock engine with default settings
    ///
    /// By default, the engine:
    /// - Succeeds all operations
    /// - Returns the requested number of bytes for each operation
    /// - Reports no special capabilities (synchronous, no batch submission, etc.)
    pub fn new() -> Self {
        Self {
            config: None,
            pending: Arc::new(Mutex::new(VecDeque::new())),
            should_fail: Arc::new(Mutex::new(false)),
            error_message: Arc::new(Mutex::new("Mock IO error".to_string())),
            bytes_per_op: Arc::new(Mutex::new(0)), // 0 means use requested length
            capabilities: EngineCapabilities::default(),
            submitted_ops: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    /// Create a new mock engine with custom capabilities
    ///
    /// This allows tests to simulate different engine types (async, batch submission, etc.)
    pub fn with_capabilities(capabilities: EngineCapabilities) -> Self {
        Self {
            capabilities,
            ..Self::new()
        }
    }
    
    /// Configure the engine to fail all operations
    ///
    /// When enabled, all submitted operations will complete with an error.
    pub fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.lock().unwrap() = should_fail;
    }
    
    /// Set the error message returned when operations fail
    pub fn set_error_message(&self, message: String) {
        *self.error_message.lock().unwrap() = message;
    }
    
    /// Set the number of bytes returned for successful operations
    ///
    /// If set to 0 (default), the engine returns the requested length from the
    /// operation. If set to a non-zero value, all operations return that many bytes.
    /// This can be used to simulate partial reads/writes.
    pub fn set_bytes_per_op(&self, bytes: usize) {
        *self.bytes_per_op.lock().unwrap() = bytes;
    }
    
    /// Get the number of operations currently pending
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
    
    /// Get a copy of all submitted operations for verification
    pub fn submitted_operations(&self) -> Vec<OperationRecord> {
        self.submitted_ops.lock().unwrap().clone()
    }
    
    /// Clear the submitted operations history
    pub fn clear_submitted_operations(&self) {
        self.submitted_ops.lock().unwrap().clear();
    }
    
    /// Get the number of submitted operations
    pub fn submitted_count(&self) -> usize {
        self.submitted_ops.lock().unwrap().len()
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl IOEngine for MockEngine {
    fn init(&mut self, config: &EngineConfig) -> Result<()> {
        self.config = Some(config.clone());
        Ok(())
    }
    
    fn submit(&mut self, op: IOOperation) -> Result<()> {
        // Record the operation for verification
        let record = OperationRecord {
            op_type: op.op_type,
            target_fd: op.target_fd,
            offset: op.offset,
            length: op.length,
            user_data: op.user_data,
        };
        self.submitted_ops.lock().unwrap().push(record);
        
        // Queue the operation for completion
        self.pending.lock().unwrap().push_back(op);
        Ok(())
    }
    
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>> {
        let mut completions = Vec::new();
        let mut pending = self.pending.lock().unwrap();
        
        // Process all pending operations
        while let Some(op) = pending.pop_front() {
            let should_fail = *self.should_fail.lock().unwrap();
            let result = if should_fail {
                let error_msg = self.error_message.lock().unwrap().clone();
                Err(anyhow::anyhow!(error_msg))
            } else {
                let bytes = *self.bytes_per_op.lock().unwrap();
                let bytes_transferred = if bytes == 0 { op.length } else { bytes };
                Ok(bytes_transferred)
            };
            
            completions.push(IOCompletion {
                user_data: op.user_data,
                result,
                op_type: op.op_type,
            });
        }
        
        Ok(completions)
    }
    
    fn cleanup(&mut self) -> Result<()> {
        // Clear any pending operations
        self.pending.lock().unwrap().clear();
        Ok(())
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        self.capabilities.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mock_engine_basic() {
        let mut engine = MockEngine::new();
        let config = EngineConfig::default();
        
        engine.init(&config).unwrap();
        
        // Submit a read operation
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: 1,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 4096,
            user_data: 42,
        };
        engine.submit(op).unwrap();
        
        // Poll for completions
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].user_data, 42);
        assert_eq!(completions[0].op_type, OperationType::Read);
        assert!(completions[0].result.is_ok());
        assert_eq!(completions[0].result.as_ref().unwrap(), &4096);
    }
    
    #[test]
    fn test_mock_engine_failure() {
        let mut engine = MockEngine::new();
        engine.set_should_fail(true);
        engine.set_error_message("Test error".to_string());
        
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let op = IOOperation {
            op_type: OperationType::Write,
            target_fd: 2,
            offset: 1024,
            buffer: std::ptr::null_mut(),
            length: 8192,
            user_data: 99,
        };
        engine.submit(op).unwrap();
        
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].result.is_err());
        assert_eq!(
            completions[0].result.as_ref().unwrap_err().to_string(),
            "Test error"
        );
    }
    
    #[test]
    fn test_mock_engine_partial_transfer() {
        let mut engine = MockEngine::new();
        engine.set_bytes_per_op(2048);
        
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        let op = IOOperation {
            op_type: OperationType::Read,
            target_fd: 3,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 4096,
            user_data: 1,
        };
        engine.submit(op).unwrap();
        
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].result.as_ref().unwrap(), &2048);
    }
    
    #[test]
    fn test_mock_engine_multiple_operations() {
        let mut engine = MockEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit multiple operations
        for i in 0..5 {
            let op = IOOperation {
                op_type: if i % 2 == 0 { OperationType::Read } else { OperationType::Write },
                target_fd: 1,
                offset: i * 4096,
                buffer: std::ptr::null_mut(),
                length: 4096,
                user_data: i,
            };
            engine.submit(op).unwrap();
        }
        
        // All operations should complete
        let completions = engine.poll_completions().unwrap();
        assert_eq!(completions.len(), 5);
        
        // Verify user data
        for (i, completion) in completions.iter().enumerate() {
            assert_eq!(completion.user_data, i as u64);
        }
    }
    
    #[test]
    fn test_mock_engine_operation_tracking() {
        let mut engine = MockEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit operations
        let op1 = IOOperation {
            op_type: OperationType::Read,
            target_fd: 1,
            offset: 0,
            buffer: std::ptr::null_mut(),
            length: 4096,
            user_data: 1,
        };
        engine.submit(op1).unwrap();
        
        let op2 = IOOperation {
            op_type: OperationType::Write,
            target_fd: 2,
            offset: 8192,
            buffer: std::ptr::null_mut(),
            length: 16384,
            user_data: 2,
        };
        engine.submit(op2).unwrap();
        
        // Verify tracking
        let submitted = engine.submitted_operations();
        assert_eq!(submitted.len(), 2);
        assert_eq!(submitted[0].op_type, OperationType::Read);
        assert_eq!(submitted[0].offset, 0);
        assert_eq!(submitted[0].length, 4096);
        assert_eq!(submitted[1].op_type, OperationType::Write);
        assert_eq!(submitted[1].offset, 8192);
        assert_eq!(submitted[1].length, 16384);
        
        // Clear and verify
        engine.clear_submitted_operations();
        assert_eq!(engine.submitted_count(), 0);
    }
    
    #[test]
    fn test_mock_engine_capabilities() {
        let caps = EngineCapabilities {
            async_io: true,
            batch_submission: true,
            registered_buffers: true,
            fixed_files: true,
            polling_mode: true,
            max_queue_depth: 256,
        };
        
        let engine = MockEngine::with_capabilities(caps.clone());
        let reported_caps = engine.capabilities();
        
        assert_eq!(reported_caps, caps);
    }
    
    #[test]
    fn test_mock_engine_cleanup() {
        let mut engine = MockEngine::new();
        let config = EngineConfig::default();
        engine.init(&config).unwrap();
        
        // Submit operations
        for i in 0..3 {
            let op = IOOperation {
                op_type: OperationType::Read,
                target_fd: 1,
                offset: i * 4096,
                buffer: std::ptr::null_mut(),
                length: 4096,
                user_data: i,
            };
            engine.submit(op).unwrap();
        }
        
        assert_eq!(engine.pending_count(), 3);
        
        // Cleanup should clear pending operations
        engine.cleanup().unwrap();
        assert_eq!(engine.pending_count(), 0);
    }
}
