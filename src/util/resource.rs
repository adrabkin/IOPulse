//! Resource utilization tracking
//!
//! This module provides CPU and memory utilization tracking for the IOPulse process.
//! It reads from /proc/self/stat and /proc/self/status on Linux to get resource usage.

use std::fs;
use std::time::Instant;

/// Resource utilization snapshot
#[derive(Debug, Clone, Copy)]
pub struct ResourceSnapshot {
    /// CPU time in user mode (microseconds)
    pub cpu_user_us: u64,
    /// CPU time in kernel mode (microseconds)
    pub cpu_system_us: u64,
    /// Wall-clock time when snapshot was taken
    pub timestamp: Instant,
    /// Resident Set Size (RSS) in bytes
    pub memory_rss_bytes: u64,
    /// Virtual Memory Size (VmSize) in bytes
    pub memory_vm_bytes: u64,
}

/// Resource utilization statistics
#[derive(Debug, Clone, Copy)]
pub struct ResourceStats {
    /// CPU utilization percentage (0.0 - 100.0 * num_cores)
    pub cpu_percent: f64,
    /// Average memory usage in bytes
    pub memory_bytes: u64,
    /// Peak memory usage in bytes
    pub peak_memory_bytes: u64,
}

impl ResourceSnapshot {
    /// Take a snapshot of current resource utilization
    ///
    /// Reads from /proc/self/stat for CPU time and /proc/self/status for memory.
    /// Returns None if unable to read proc files (e.g., on non-Linux systems).
    pub fn take() -> Option<Self> {
        let cpu = Self::read_cpu_time()?;
        let memory = Self::read_memory()?;
        
        Some(Self {
            cpu_user_us: cpu.0,
            cpu_system_us: cpu.1,
            timestamp: Instant::now(),
            memory_rss_bytes: memory.0,
            memory_vm_bytes: memory.1,
        })
    }
    
    /// Get the number of CPU cores on the system
    ///
    /// Reads from /proc/cpuinfo or uses num_cpus crate as fallback.
    /// Returns None if unable to determine.
    pub fn num_cpus() -> Option<usize> {
        // Try reading from /proc/cpuinfo
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            let count = cpuinfo.lines()
                .filter(|line| line.starts_with("processor"))
                .count();
            if count > 0 {
                return Some(count);
            }
        }
        
        // Fallback: use num_cpus crate (works cross-platform)
        Some(num_cpus::get())
    }
    
    /// Read CPU time from /proc/self/stat
    ///
    /// Returns (user_time_us, system_time_us) or None on error.
    fn read_cpu_time() -> Option<(u64, u64)> {
        let stat = fs::read_to_string("/proc/self/stat").ok()?;
        
        // /proc/self/stat format:
        // pid (comm) state ppid pgrp session tty_nr tpgid flags minflt cminflt majflt cmajflt utime stime ...
        // We want fields 14 (utime) and 15 (stime), which are in clock ticks
        
        let fields: Vec<&str> = stat.split_whitespace().collect();
        if fields.len() < 15 {
            return None;
        }
        
        // Fields 14 and 15 are utime and stime in clock ticks
        let utime_ticks: u64 = fields[13].parse().ok()?;
        let stime_ticks: u64 = fields[14].parse().ok()?;
        
        // Convert clock ticks to microseconds
        // Clock ticks per second is typically 100 (USER_HZ)
        let ticks_per_sec = 100;
        let utime_us = (utime_ticks * 1_000_000) / ticks_per_sec;
        let stime_us = (stime_ticks * 1_000_000) / ticks_per_sec;
        
        Some((utime_us, stime_us))
    }
    
    /// Read memory usage from /proc/self/status
    ///
    /// Returns (rss_bytes, vm_bytes) or None on error.
    fn read_memory() -> Option<(u64, u64)> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        
        let mut rss_kb = None;
        let mut vm_kb = None;
        
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                // VmRSS:     12345 kB
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    rss_kb = parts[1].parse::<u64>().ok();
                }
            } else if line.starts_with("VmSize:") {
                // VmSize:    12345 kB
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    vm_kb = parts[1].parse::<u64>().ok();
                }
            }
            
            if rss_kb.is_some() && vm_kb.is_some() {
                break;
            }
        }
        
        let rss_bytes = rss_kb? * 1024;
        let vm_bytes = vm_kb? * 1024;
        
        Some((rss_bytes, vm_bytes))
    }
    
    /// Calculate CPU utilization between two snapshots
    ///
    /// Returns CPU percentage (0.0 - 100.0 * num_cores).
    /// For example, 150.0 means 1.5 cores worth of CPU time.
    pub fn cpu_percent_since(&self, earlier: &ResourceSnapshot) -> f64 {
        let wall_time_us = self.timestamp.duration_since(earlier.timestamp).as_micros() as u64;
        if wall_time_us == 0 {
            return 0.0;
        }
        
        let cpu_time_us = (self.cpu_user_us + self.cpu_system_us)
            .saturating_sub(earlier.cpu_user_us + earlier.cpu_system_us);
        
        (cpu_time_us as f64 / wall_time_us as f64) * 100.0
    }
}

/// Resource tracker that samples resource utilization over time
#[derive(Debug, Clone)]
pub struct ResourceTracker {
    start_snapshot: Option<ResourceSnapshot>,
    samples: Vec<ResourceSnapshot>,
    peak_memory_bytes: u64,
    // Synthetic stats for distributed mode reconstruction
    synthetic_stats: Option<ResourceStats>,
}

impl ResourceTracker {
    /// Create a new resource tracker
    pub fn new() -> Self {
        Self {
            start_snapshot: None,
            samples: Vec::new(),
            peak_memory_bytes: 0,
            synthetic_stats: None,
        }
    }
    
    /// Start tracking (take initial snapshot)
    pub fn start(&mut self) {
        if let Some(snapshot) = ResourceSnapshot::take() {
            self.peak_memory_bytes = snapshot.memory_rss_bytes;
            self.start_snapshot = Some(snapshot);
        }
    }
    
    /// Sample current resource utilization
    pub fn sample(&mut self) {
        if let Some(snapshot) = ResourceSnapshot::take() {
            self.peak_memory_bytes = self.peak_memory_bytes.max(snapshot.memory_rss_bytes);
            self.samples.push(snapshot);
        }
    }
    
    /// Set synthetic stats (for distributed mode reconstruction)
    ///
    /// This allows setting resource stats from network-received data
    /// without having actual ResourceSnapshot samples.
    pub fn set_synthetic_stats(&mut self, cpu_percent: f64, memory_bytes: u64, peak_memory_bytes: u64) {
        self.synthetic_stats = Some(ResourceStats {
            cpu_percent,
            memory_bytes,
            peak_memory_bytes,
        });
    }
    
    /// Get resource statistics
    ///
    /// Returns None if no samples were taken or tracking is not supported.
    pub fn stats(&self) -> Option<ResourceStats> {
        // If synthetic stats are set, return those (for distributed mode)
        if let Some(synthetic) = self.synthetic_stats {
            return Some(synthetic);
        }
        
        let start = self.start_snapshot.as_ref()?;
        
        // Take a final snapshot if we don't have any samples yet
        let final_snapshot = if self.samples.is_empty() {
            ResourceSnapshot::take()
        } else {
            None
        };
        
        // Use either samples or final snapshot
        if let Some(final_snap) = final_snapshot {
            // No samples during test, but we can calculate from start to now
            let cpu_percent = final_snap.cpu_percent_since(start);
            return Some(ResourceStats {
                cpu_percent,
                memory_bytes: final_snap.memory_rss_bytes,
                peak_memory_bytes: self.peak_memory_bytes.max(final_snap.memory_rss_bytes),
            });
        }
        
        if self.samples.is_empty() {
            // No samples and couldn't take final snapshot, just use start
            return Some(ResourceStats {
                cpu_percent: 0.0,
                memory_bytes: start.memory_rss_bytes,
                peak_memory_bytes: self.peak_memory_bytes,
            });
        }
        
        // Calculate CPU percentage from start to last sample
        let last = self.samples.last()?;
        let cpu_percent = last.cpu_percent_since(start);
        
        // Calculate average memory usage
        let total_memory: u64 = self.samples.iter()
            .map(|s| s.memory_rss_bytes)
            .sum();
        let avg_memory = total_memory / self.samples.len() as u64;
        
        Some(ResourceStats {
            cpu_percent,
            memory_bytes: avg_memory,
            peak_memory_bytes: self.peak_memory_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_resource_snapshot() {
        // This test only works on Linux
        if let Some(snapshot) = ResourceSnapshot::take() {
            assert!(snapshot.cpu_user_us > 0 || snapshot.cpu_system_us > 0);
            assert!(snapshot.memory_rss_bytes > 0);
            assert!(snapshot.memory_vm_bytes > 0);
            assert!(snapshot.memory_vm_bytes >= snapshot.memory_rss_bytes);
        }
    }
    
    #[test]
    fn test_cpu_percent() {
        // This test only works on Linux
        if let Some(start) = ResourceSnapshot::take() {
            // Do some CPU work
            let mut sum = 0u64;
            for i in 0..1_000_000 {
                sum = sum.wrapping_add(i);
            }
            
            thread::sleep(Duration::from_millis(10));
            
            if let Some(end) = ResourceSnapshot::take() {
                let cpu_percent = end.cpu_percent_since(&start);
                // Should have used some CPU
                assert!(cpu_percent >= 0.0);
                // Shouldn't exceed 100% per core * num_cores (reasonable upper bound)
                assert!(cpu_percent <= 10000.0);
                
                // Prevent optimization
                assert!(sum > 0);
            }
        }
    }
    
    #[test]
    fn test_resource_tracker() {
        let mut tracker = ResourceTracker::new();
        tracker.start();
        
        // Do some work and sample
        thread::sleep(Duration::from_millis(10));
        tracker.sample();
        
        thread::sleep(Duration::from_millis(10));
        tracker.sample();
        
        if let Some(stats) = tracker.stats() {
            assert!(stats.cpu_percent >= 0.0);
            assert!(stats.memory_bytes > 0);
            assert!(stats.peak_memory_bytes >= stats.memory_bytes);
        }
    }
}
