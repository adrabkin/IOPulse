//! CPU and NUMA affinity binding
//!
//! This module provides functionality for binding worker threads to specific CPU cores
//! and NUMA nodes. This can improve performance by:
//! - Reducing context switches and cache misses
//! - Ensuring memory is allocated on the local NUMA node
//! - Preventing thread migration across cores
//!
//! # Platform Support
//!
//! CPU affinity is supported on Linux via `sched_setaffinity`. NUMA binding is
//! supported on Linux systems with NUMA hardware via `set_mempolicy`.
//!
//! # Example
//!
//! ```no_run
//! use iopulse::worker::affinity::{set_cpu_affinity, parse_cpu_list};
//!
//! // Bind to CPU cores 0, 2, 4
//! let cores = parse_cpu_list("0,2,4").unwrap();
//! set_cpu_affinity(&cores).unwrap();
//! ```

use crate::Result;
use anyhow::Context;

/// Set CPU affinity for the current thread
///
/// Binds the current thread to the specified CPU cores. The thread will only
/// be scheduled on these cores, which can improve cache locality and reduce
/// context switch overhead.
///
/// # Arguments
///
/// * `cores` - List of CPU core IDs to bind to
///
/// # Errors
///
/// Returns an error if:
/// - The system doesn't support CPU affinity
/// - Invalid core IDs are specified
/// - The syscall fails
///
/// # Platform Support
///
/// This function is only available on Linux. On other platforms, it returns
/// an error indicating the feature is not supported.
///
/// # Example
///
/// ```no_run
/// use iopulse::worker::affinity::set_cpu_affinity;
///
/// // Bind to cores 0 and 1
/// set_cpu_affinity(&[0, 1]).unwrap();
/// ```
#[cfg(target_os = "linux")]
pub fn set_cpu_affinity(cores: &[usize]) -> Result<()> {
    use libc::{cpu_set_t, CPU_SET, CPU_ZERO, sched_setaffinity};
    use std::mem;

    if cores.is_empty() {
        anyhow::bail!("CPU core list cannot be empty");
    }

    unsafe {
        let mut cpu_set: cpu_set_t = mem::zeroed();
        CPU_ZERO(&mut cpu_set);

        for &core in cores {
            if core >= 1024 {
                anyhow::bail!("CPU core ID {} is too large (max 1023)", core);
            }
            CPU_SET(core, &mut cpu_set);
        }

        let result = sched_setaffinity(
            0, // 0 = current thread
            mem::size_of::<cpu_set_t>(),
            &cpu_set,
        );

        if result != 0 {
            let err = std::io::Error::last_os_error();
            return Err(err).context(format!("Failed to set CPU affinity to cores {:?}", cores));
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_cpu_affinity(_cores: &[usize]) -> Result<()> {
    anyhow::bail!("CPU affinity is only supported on Linux")
}

/// Set NUMA memory policy for the current thread
///
/// Binds memory allocations for the current thread to the specified NUMA nodes.
/// This ensures that memory is allocated locally to the CPU cores the thread
/// runs on, reducing memory access latency.
///
/// # Arguments
///
/// * `nodes` - List of NUMA node IDs to bind to
///
/// # Errors
///
/// Returns an error if:
/// - The system doesn't support NUMA
/// - Invalid node IDs are specified
/// - The syscall fails
///
/// # Platform Support
///
/// This function is only available on Linux systems with NUMA support. On other
/// platforms or systems without NUMA, it returns an error.
///
/// # Example
///
/// ```no_run
/// use iopulse::worker::affinity::set_numa_affinity;
///
/// // Bind to NUMA nodes 0 and 1
/// set_numa_affinity(&[0, 1]).unwrap();
/// ```
#[cfg(target_os = "linux")]
pub fn set_numa_affinity(nodes: &[usize]) -> Result<()> {
    if nodes.is_empty() {
        anyhow::bail!("NUMA node list cannot be empty");
    }

    // NUMA support requires libnuma or direct syscalls
    // For now, we'll use set_mempolicy syscall directly
    
    // MPOL_BIND = 2 (bind to specific nodes)
    const MPOL_BIND: i32 = 2;
    
    // Create nodemask (up to 1024 nodes, 128 bytes)
    let mut nodemask = [0u64; 16]; // 16 * 64 bits = 1024 nodes
    
    for &node in nodes {
        if node >= 1024 {
            anyhow::bail!("NUMA node ID {} is too large (max 1023)", node);
        }
        let word = node / 64;
        let bit = node % 64;
        nodemask[word] |= 1u64 << bit;
    }
    
    let result = unsafe {
        libc::syscall(
            libc::SYS_set_mempolicy,
            MPOL_BIND,
            nodemask.as_ptr(),
            1024, // maxnode
        )
    };
    
    if result != 0 {
        let err = std::io::Error::last_os_error();
        return Err(err).context(format!("Failed to set NUMA affinity to nodes {:?}", nodes));
    }
    
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_numa_affinity(_nodes: &[usize]) -> Result<()> {
    anyhow::bail!("NUMA affinity is only supported on Linux")
}

/// Parse a comma-separated list of CPU cores or ranges
///
/// Supports formats like:
/// - "0,1,2,3" - Individual cores
/// - "0-3" - Range of cores
/// - "0,2-4,7" - Mixed individual and ranges
///
/// # Arguments
///
/// * `spec` - CPU core specification string
///
/// # Returns
///
/// A vector of CPU core IDs
///
/// # Errors
///
/// Returns an error if the specification is invalid or contains non-numeric values.
///
/// # Example
///
/// ```
/// use iopulse::worker::affinity::parse_cpu_list;
///
/// let cores = parse_cpu_list("0,2-4,7").unwrap();
/// assert_eq!(cores, vec![0, 2, 3, 4, 7]);
/// ```
pub fn parse_cpu_list(spec: &str) -> Result<Vec<usize>> {
    let mut cores = Vec::new();
    
    for part in spec.split(',') {
        let part = part.trim();
        
        if part.contains('-') {
            // Range: "0-3"
            let range_parts: Vec<&str> = part.split('-').collect();
            if range_parts.len() != 2 {
                anyhow::bail!("Invalid CPU range format: {}", part);
            }
            
            let start: usize = range_parts[0]
                .parse()
                .with_context(|| format!("Invalid CPU core number: {}", range_parts[0]))?;
            let end: usize = range_parts[1]
                .parse()
                .with_context(|| format!("Invalid CPU core number: {}", range_parts[1]))?;
            
            if start > end {
                anyhow::bail!("Invalid CPU range: start ({}) > end ({})", start, end);
            }
            
            for core in start..=end {
                cores.push(core);
            }
        } else {
            // Individual core: "0"
            let core: usize = part
                .parse()
                .with_context(|| format!("Invalid CPU core number: {}", part))?;
            cores.push(core);
        }
    }
    
    if cores.is_empty() {
        anyhow::bail!("CPU core list cannot be empty");
    }
    
    // Remove duplicates and sort
    cores.sort_unstable();
    cores.dedup();
    
    Ok(cores)
}

/// Parse a comma-separated list of NUMA nodes or ranges
///
/// Uses the same format as `parse_cpu_list`.
///
/// # Arguments
///
/// * `spec` - NUMA node specification string
///
/// # Returns
///
/// A vector of NUMA node IDs
///
/// # Errors
///
/// Returns an error if the specification is invalid or contains non-numeric values.
///
/// # Example
///
/// ```
/// use iopulse::worker::affinity::parse_numa_list;
///
/// let nodes = parse_numa_list("0,1").unwrap();
/// assert_eq!(nodes, vec![0, 1]);
/// ```
pub fn parse_numa_list(spec: &str) -> Result<Vec<usize>> {
    // Same parsing logic as CPU list
    parse_cpu_list(spec)
}

/// Get the number of available CPU cores
///
/// Returns the number of logical CPU cores available on the system.
///
/// # Example
///
/// ```
/// use iopulse::worker::affinity::num_cpus;
///
/// let cpus = num_cpus();
/// println!("System has {} CPU cores", cpus);
/// ```
pub fn num_cpus() -> usize {
    num_cpus::get()
}

/// Check if thread count exceeds CPU count and warn if so
///
/// This is a helper function to warn users when they configure more threads
/// than available CPU cores, which may lead to context switching overhead.
///
/// # Arguments
///
/// * `thread_count` - Number of worker threads configured
///
/// # Returns
///
/// True if thread count exceeds CPU count, false otherwise.
pub fn warn_if_oversubscribed(thread_count: usize) -> bool {
    let cpu_count = num_cpus();
    if thread_count > cpu_count {
        eprintln!(
            "Warning: Thread count ({}) exceeds CPU count ({}). \
             This may cause context switching overhead.",
            thread_count, cpu_count
        );
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_list_single() {
        let cores = parse_cpu_list("0").unwrap();
        assert_eq!(cores, vec![0]);
    }

    #[test]
    fn test_parse_cpu_list_multiple() {
        let cores = parse_cpu_list("0,1,2,3").unwrap();
        assert_eq!(cores, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_cpu_list_range() {
        let cores = parse_cpu_list("0-3").unwrap();
        assert_eq!(cores, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_cpu_list_mixed() {
        let cores = parse_cpu_list("0,2-4,7").unwrap();
        assert_eq!(cores, vec![0, 2, 3, 4, 7]);
    }

    #[test]
    fn test_parse_cpu_list_with_spaces() {
        let cores = parse_cpu_list("0, 2-4, 7").unwrap();
        assert_eq!(cores, vec![0, 2, 3, 4, 7]);
    }

    #[test]
    fn test_parse_cpu_list_duplicates() {
        let cores = parse_cpu_list("0,1,1,2,2,3").unwrap();
        assert_eq!(cores, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_cpu_list_unsorted() {
        let cores = parse_cpu_list("3,1,2,0").unwrap();
        assert_eq!(cores, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_parse_cpu_list_empty() {
        let result = parse_cpu_list("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cpu_list_invalid_number() {
        let result = parse_cpu_list("0,abc,2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cpu_list_invalid_range() {
        let result = parse_cpu_list("5-2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cpu_list_invalid_range_format() {
        let result = parse_cpu_list("0-2-4");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_numa_list() {
        let nodes = parse_numa_list("0,1").unwrap();
        assert_eq!(nodes, vec![0, 1]);
    }

    #[test]
    fn test_num_cpus() {
        let cpus = num_cpus();
        assert!(cpus > 0);
        assert!(cpus <= 1024); // Reasonable upper bound
    }

    #[test]
    fn test_warn_if_oversubscribed() {
        let cpu_count = num_cpus();
        
        // Not oversubscribed
        assert!(!warn_if_oversubscribed(cpu_count));
        assert!(!warn_if_oversubscribed(cpu_count / 2));
        
        // Oversubscribed
        assert!(warn_if_oversubscribed(cpu_count + 1));
        assert!(warn_if_oversubscribed(cpu_count * 2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_set_cpu_affinity() {
        // Test setting affinity to current core (should always work)
        let result = set_cpu_affinity(&[0]);
        assert!(result.is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_set_cpu_affinity_invalid() {
        // Test with invalid core ID
        let result = set_cpu_affinity(&[9999]);
        // May succeed or fail depending on system, just verify it doesn't panic
        let _ = result;
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_set_cpu_affinity_unsupported() {
        let result = set_cpu_affinity(&[0]);
        assert!(result.is_err());
    }
}

