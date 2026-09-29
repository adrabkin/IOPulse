//! Statistics aggregation
//!
//! This module provides functionality for aggregating statistics from multiple workers.
//! The aggregator merges per-worker statistics into a single aggregate view while
//! optionally preserving per-worker details for analysis.
//!
//! # Features
//!
//! - **Aggregate statistics**: Merge all workers into single view
//! - **Per-worker statistics**: Preserve individual worker stats
//! - **Hierarchical structure**: Aggregate → per-worker → per-thread
//! - **Histogram merging**: Correctly merge latency histograms
//! - **Percentile calculation**: Calculate aggregate percentiles
//!
//! # Example
//!
//! ```
//! use iopulse::stats::{WorkerStats, aggregator::StatisticsAggregator};
//! use iopulse::engine::OperationType;
//! use std::time::Duration;
//!
//! // Create some worker statistics
//! let mut worker1 = WorkerStats::new();
//! worker1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
//!
//! let mut worker2 = WorkerStats::new();
//! worker2.record_io(OperationType::Write, 8192, Duration::from_micros(150));
//!
//! // Aggregate them
//! let mut aggregator = StatisticsAggregator::new();
//! aggregator.add_worker(0, worker1);
//! aggregator.add_worker(1, worker2);
//!
//! // Get aggregate statistics
//! let aggregate = aggregator.aggregate();
//! assert_eq!(aggregate.total_ops(), 2);
//! assert_eq!(aggregate.total_bytes(), 12288);
//! ```

use crate::stats::WorkerStats;
use std::collections::HashMap;

/// Statistics aggregator for multiple workers
///
/// Collects statistics from multiple workers and provides both aggregate and
/// per-worker views. The aggregator maintains the original per-worker statistics
/// for detailed analysis while computing aggregate metrics.
///
/// # Usage
///
/// 1. Create aggregator with `new()`
/// 2. Add worker statistics with `add_worker()`
/// 3. Get aggregate view with `aggregate()`
/// 4. Get per-worker view with `per_worker()`
#[derive(Debug)]
pub struct StatisticsAggregator {
    /// Per-worker statistics (worker_id → stats)
    workers: HashMap<usize, WorkerStats>,
    
    /// Cached aggregate statistics (computed on demand)
    aggregate_cache: Option<WorkerStats>,
    
    /// Whether aggregate cache is valid
    cache_valid: bool,
}

impl StatisticsAggregator {
    /// Create a new statistics aggregator
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            aggregate_cache: None,
            cache_valid: false,
        }
    }
    
    /// Add statistics from a worker
    ///
    /// # Arguments
    ///
    /// * `worker_id` - ID of the worker
    /// * `stats` - Statistics from the worker
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::stats::{WorkerStats, aggregator::StatisticsAggregator};
    ///
    /// let mut aggregator = StatisticsAggregator::new();
    /// let stats = WorkerStats::new();
    /// aggregator.add_worker(0, stats);
    /// ```
    pub fn add_worker(&mut self, worker_id: usize, stats: WorkerStats) {
        self.workers.insert(worker_id, stats);
        self.cache_valid = false; // Invalidate cache
    }
    
    /// Get the number of workers
    pub fn num_workers(&self) -> usize {
        self.workers.len()
    }
    
    /// Get aggregate statistics across all workers
    ///
    /// Merges all worker statistics into a single aggregate view. The result
    /// is cached for efficiency - subsequent calls return the cached value
    /// unless new workers are added.
    ///
    /// # Returns
    ///
    /// Aggregate statistics across all workers.
    ///
    /// # Example
    ///
    /// ```
    /// use iopulse::stats::{WorkerStats, aggregator::StatisticsAggregator};
    /// use iopulse::engine::OperationType;
    /// use std::time::Duration;
    ///
    /// let mut aggregator = StatisticsAggregator::new();
    ///
    /// let mut worker1 = WorkerStats::new();
    /// worker1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
    /// aggregator.add_worker(0, worker1);
    ///
    /// let mut worker2 = WorkerStats::new();
    /// worker2.record_io(OperationType::Write, 8192, Duration::from_micros(150));
    /// aggregator.add_worker(1, worker2);
    ///
    /// let aggregate = aggregator.aggregate();
    /// assert_eq!(aggregate.total_ops(), 2);
    /// ```
    pub fn aggregate(&mut self) -> &WorkerStats {
        if !self.cache_valid {
            self.compute_aggregate();
        }
        
        self.aggregate_cache.as_ref().unwrap()
    }
    
    /// Compute aggregate statistics
    fn compute_aggregate(&mut self) {
        let mut aggregate = WorkerStats::new();
        
        for stats in self.workers.values() {
            aggregate.merge(stats).expect("Failed to merge worker statistics");
        }
        
        self.aggregate_cache = Some(aggregate);
        self.cache_valid = true;
    }
    
    /// Get statistics for a specific worker
    ///
    /// # Arguments
    ///
    /// * `worker_id` - ID of the worker
    ///
    /// # Returns
    ///
    /// Statistics for the specified worker, or None if the worker doesn't exist.
    pub fn worker_stats(&self, worker_id: usize) -> Option<&WorkerStats> {
        self.workers.get(&worker_id)
    }
    
    /// Get all per-worker statistics
    ///
    /// Returns a reference to the map of worker ID to statistics.
    ///
    /// # Returns
    ///
    /// Map of worker_id → WorkerStats
    pub fn per_worker(&self) -> &HashMap<usize, WorkerStats> {
        &self.workers
    }
    
    /// Get sorted list of worker IDs
    ///
    /// Returns worker IDs in ascending order for consistent iteration.
    pub fn worker_ids(&self) -> Vec<usize> {
        let mut ids: Vec<usize> = self.workers.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
    
    /// Clear all statistics
    ///
    /// Removes all worker statistics and resets the aggregator to empty state.
    pub fn clear(&mut self) {
        self.workers.clear();
        self.aggregate_cache = None;
        self.cache_valid = false;
    }
}

impl Default for StatisticsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::OperationType;
    use std::time::Duration;
    
    #[test]
    fn test_aggregator_new() {
        let aggregator = StatisticsAggregator::new();
        assert_eq!(aggregator.num_workers(), 0);
    }
    
    #[test]
    fn test_add_worker() {
        let mut aggregator = StatisticsAggregator::new();
        let stats = WorkerStats::new();
        
        aggregator.add_worker(0, stats);
        assert_eq!(aggregator.num_workers(), 1);
    }
    
    #[test]
    fn test_add_multiple_workers() {
        let mut aggregator = StatisticsAggregator::new();
        
        aggregator.add_worker(0, WorkerStats::new());
        aggregator.add_worker(1, WorkerStats::new());
        aggregator.add_worker(2, WorkerStats::new());
        
        assert_eq!(aggregator.num_workers(), 3);
    }
    
    #[test]
    fn test_aggregate_empty() {
        let mut aggregator = StatisticsAggregator::new();
        let aggregate = aggregator.aggregate();
        
        assert_eq!(aggregate.total_ops(), 0);
        assert_eq!(aggregate.total_bytes(), 0);
    }
    
    #[test]
    fn test_aggregate_single_worker() {
        let mut aggregator = StatisticsAggregator::new();
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        stats.record_io(OperationType::Write, 8192, Duration::from_micros(150));
        
        aggregator.add_worker(0, stats);
        
        let aggregate = aggregator.aggregate();
        assert_eq!(aggregate.read_ops(), 1);
        assert_eq!(aggregate.write_ops(), 1);
        assert_eq!(aggregate.total_bytes(), 12288);
    }
    
    #[test]
    fn test_aggregate_multiple_workers() {
        let mut aggregator = StatisticsAggregator::new();
        
        let mut worker1 = WorkerStats::new();
        worker1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        worker1.record_io(OperationType::Write, 8192, Duration::from_micros(150));
        
        let mut worker2 = WorkerStats::new();
        worker2.record_io(OperationType::Read, 2048, Duration::from_micros(80));
        worker2.record_io(OperationType::Write, 4096, Duration::from_micros(120));
        
        let mut worker3 = WorkerStats::new();
        worker3.record_io(OperationType::Read, 8192, Duration::from_micros(200));
        
        aggregator.add_worker(0, worker1);
        aggregator.add_worker(1, worker2);
        aggregator.add_worker(2, worker3);
        
        let aggregate = aggregator.aggregate();
        assert_eq!(aggregate.read_ops(), 3);
        assert_eq!(aggregate.write_ops(), 2);
        assert_eq!(aggregate.read_bytes(), 14336); // 4096 + 2048 + 8192
        assert_eq!(aggregate.write_bytes(), 12288); // 8192 + 4096
        assert_eq!(aggregate.total_ops(), 5);
        assert_eq!(aggregate.total_bytes(), 26624);
    }
    
    #[test]
    fn test_aggregate_with_errors() {
        let mut aggregator = StatisticsAggregator::new();
        
        let mut worker1 = WorkerStats::new();
        worker1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        worker1.record_error();
        
        let mut worker2 = WorkerStats::new();
        worker2.record_io(OperationType::Write, 8192, Duration::from_micros(150));
        worker2.record_error();
        worker2.record_error();
        
        aggregator.add_worker(0, worker1);
        aggregator.add_worker(1, worker2);
        
        let aggregate = aggregator.aggregate();
        assert_eq!(aggregate.errors(), 3);
    }
    
    #[test]
    fn test_worker_stats() {
        let mut aggregator = StatisticsAggregator::new();
        
        let mut stats = WorkerStats::new();
        stats.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        
        aggregator.add_worker(5, stats);
        
        let worker_stats = aggregator.worker_stats(5);
        assert!(worker_stats.is_some());
        assert_eq!(worker_stats.unwrap().read_ops(), 1);
        
        let missing = aggregator.worker_stats(99);
        assert!(missing.is_none());
    }
    
    #[test]
    fn test_worker_ids() {
        let mut aggregator = StatisticsAggregator::new();
        
        aggregator.add_worker(2, WorkerStats::new());
        aggregator.add_worker(0, WorkerStats::new());
        aggregator.add_worker(1, WorkerStats::new());
        
        let ids = aggregator.worker_ids();
        assert_eq!(ids, vec![0, 1, 2]); // Should be sorted
    }
    
    #[test]
    fn test_per_worker() {
        let mut aggregator = StatisticsAggregator::new();
        
        aggregator.add_worker(0, WorkerStats::new());
        aggregator.add_worker(1, WorkerStats::new());
        
        let per_worker = aggregator.per_worker();
        assert_eq!(per_worker.len(), 2);
        assert!(per_worker.contains_key(&0));
        assert!(per_worker.contains_key(&1));
    }
    
    #[test]
    fn test_clear() {
        let mut aggregator = StatisticsAggregator::new();
        
        aggregator.add_worker(0, WorkerStats::new());
        aggregator.add_worker(1, WorkerStats::new());
        assert_eq!(aggregator.num_workers(), 2);
        
        aggregator.clear();
        assert_eq!(aggregator.num_workers(), 0);
    }
    
    #[test]
    fn test_cache_invalidation() {
        let mut aggregator = StatisticsAggregator::new();
        
        let mut worker1 = WorkerStats::new();
        worker1.record_io(OperationType::Read, 4096, Duration::from_micros(100));
        aggregator.add_worker(0, worker1);
        
        // First call computes aggregate
        let aggregate1 = aggregator.aggregate();
        assert_eq!(aggregate1.read_ops(), 1);
        
        // Second call uses cache
        let aggregate2 = aggregator.aggregate();
        assert_eq!(aggregate2.read_ops(), 1);
        
        // Adding new worker invalidates cache
        let mut worker2 = WorkerStats::new();
        worker2.record_io(OperationType::Read, 2048, Duration::from_micros(80));
        aggregator.add_worker(1, worker2);
        
        // Next call recomputes aggregate
        let aggregate3 = aggregator.aggregate();
        assert_eq!(aggregate3.read_ops(), 2);
    }
}

