# IOPulse Performance Analysis

**Date:** 2026-03-03
**Scope:** Full codebase review across all modules for performance improvement opportunities
**Method:** Parallel analysis of 4 module groups with line-level code inspection

---

## Executive Summary

IOPulse is a well-architected IO profiling tool with several excellent design decisions (cache-line aligned stats, zero hot-path allocations, pre-allocated buffers). However, there are actionable improvements ranging from critical algorithmic fixes to minor optimizations.

**Critical issues (3):** libaio submit batching broken, O(n) completion matching in worker, io_uring missing key optimizations
**High priority (6):** Histogram cloning overhead, Zipf/Pareto scaling bias, unnecessary queue depth sampling, mmap missing MAP_POPULATE, adaptive queue depth, CDF caching
**Medium priority (8):** Various smaller optimizations detailed below

---

## 1. IO Engines (`src/engine/`)

### CRITICAL: libaio Submit Not Batched
**File:** `libaio.rs:253`
```
io_submit(ctx, 1, &mut iocb_ptr)  // Always submits 1 at a time
```
The entire purpose of libaio is batched async submission, but `io_submit()` is called with count=1 per operation. Should accumulate multiple iocbs and submit as a batch (10-32 at a time). This defeats the async advantage over sync IO.

### CRITICAL: io_uring Missing Key Optimizations
**File:** `io_uring.rs:108-110` (TODO comments)
- **IORING_SETUP_SQPOLL** not implemented — kernel polling thread would eliminate submit syscalls entirely
- **Registered buffers** not implemented — would avoid virtual-to-physical translation per IO
- **Fixed files** not implemented — would avoid file descriptor table lookup per IO

These are the three biggest io_uring performance features and all are marked TODO.

### HIGH: io_uring Completion Retry Loop
**File:** `io_uring.rs:212-240`
Inefficient retry loop when not all completions arrive at once. May cause multiple `submit_and_wait` syscalls. Should call `submit_and_wait` once with a higher `min_nr` parameter.

### HIGH: mmap Missing MAP_POPULATE
**File:** `mmap.rs:128`
Uses `MAP_SHARED` without `MAP_POPULATE`. First access to each page triggers a page fault. Adding conditional `MAP_POPULATE` would pre-fault pages and eliminate first-access latency spikes.

### MEDIUM: libaio O(n) iocb Lookup
**File:** `libaio.rs:314-318`
Linear search through iocb array to find index by data value on every completion. Should use HashMap keyed by `event.obj` pointer for O(1) lookup.

### MEDIUM: mmap Always Uses MS_SYNC
**File:** `mmap.rs:217`
`msync` always uses `MS_SYNC` (blocking). Should support `MS_ASYNC` for non-blocking flush when durability isn't critical.

### LOW: Sync Engine No O_DIRECT Validation
**File:** `sync.rs`
No buffer alignment validation for O_DIRECT. Caller is trusted but silent misalignment would cause IO errors.

---

## 2. Worker & Coordinator (`src/worker/`, `src/coordinator/`)

### CRITICAL: O(n) Completion Matching
**File:** `worker/mod.rs:1496-1500`
```rust
let op_idx = in_flight_ops.iter()
    .position(|op| op.buf_idx == completion.user_data as usize)?;
let in_flight_op = in_flight_ops.remove(op_idx);  // O(n) removal
```
Linear search + Vec removal in the hottest path. With QD=256 at 100K+ IOPS, this is millions of linear scans per second. **Fix:** Use HashMap<user_data, index> with `swap_remove()` for O(1) lookup and removal.

**Estimated impact:** 20-30% latency reduction at high queue depths.

### HIGH: Unnecessary Queue Depth Sampling
**File:** `worker/mod.rs:502`
`sample_queue_depth()` called on every submit operation (millions/sec). Should only be sampled during periodic live stats updates.

**Estimated impact:** 5-10% CPU reduction.

### HIGH: Histogram Cloning Overhead
**File:** `worker/mod.rs:580-604`
Clones ~60KB of histogram data per worker every 1K ops for shared live stats. With 100 workers at 1K Hz updates = ~6 GB/sec memory bandwidth just for stats sharing. **Fix:** Use incremental delta snapshots or copy-on-write.

### MEDIUM: Unused InFlightOp Fields
**File:** `worker/mod.rs:90-91`
`offset` and `length` fields stored per in-flight op but never used in completion processing. Wastes 16 bytes per in-flight operation.

### MEDIUM: No Adaptive Queue Depth
Fixed queue depth with no backpressure mechanism. No monitoring of engine queue availability. Could benefit from dynamic adjustment based on completion rates.

### MEDIUM: Linear Block Size Pattern Selection
**File:** `worker/mod.rs:1562-1570`
Linear scan through weighted patterns on every IO. Pre-computing cumulative weights into an array or using alias method would be faster.

### LOW: Missing Branch Prediction Hints
No `#[cold]` annotations on error paths. No unlikely/likely hints for rare conditions in the main loop.

---

## 3. Statistics & Output (`src/stats/`, `src/output/`)

**This module is exceptionally well designed.** Key strengths:

- **Cache-line alignment:** `AlignedCounter` with 64-byte alignment prevents false sharing (verified by tests)
- **Zero hot-path allocations:** All structures pre-allocated in `WorkerStats::new()`
- **Fixed-size histograms:** 112 buckets, 928 bytes, O(1) update (~20 CPU instructions)
- **Relaxed atomics only:** No unnecessary synchronization barriers
- **Deferred formatting:** JSON/CSV output happens after test completion, not during
- **Linear aggregation:** O(n workers × 112 buckets), cached to prevent recomputation

### Per-IO Recording Cost
~20 CPU instructions, 0 allocations. This is near-optimal.

### No Issues Found
The stats module is production-quality with no performance concerns.

---

## 4. Target & Distribution (`src/target/`, `src/distribution/`)

### HIGH: Zipf/Pareto Modulo Bias
**File:** `zipf.rs:140-142`, `pareto.rs:146`
```rust
let block_num = ((rank as u64) * num_blocks) / (self.cdf.len() as u64);
```
Integer division scaling from CDF index to block number creates modulo bias when `cdf.len()` and `num_blocks` aren't evenly divisible. Some blocks get slightly more traffic than the distribution intends. **Fix:** Use proper inverse transform sampling or rejection sampling.

### HIGH: CDF Memory and Recomputation
**File:** `zipf.rs:103-109`, `pareto.rs:105-111`
- 8MB per distribution instance for 1M-entry CDF
- CDF recomputed for each different file size (no caching across files)
- **Fix:** Cache CDFs keyed by (distribution_params, num_blocks)

### MEDIUM: Gaussian Boundary Clamping Bias
**File:** `gaussian.rs:151`
```rust
let clamped = value.max(0.0).min(num_blocks_f64 - 1.0);
```
Values outside [0, N) are clamped, concentrating probability mass at boundaries. **Fix:** Use rejection sampling — regenerate values that fall outside range.

### MEDIUM: Tree Traversal Inefficiency
**File:** `layout.rs:323`
`exists()` stat call for every subdirectory during remainder file distribution. With depth=4, width=10 trees: 10,000 stat() calls. **Fix:** Cache directory list during `generate()`.

### MEDIUM: fadvise Always Full-File
**File:** `file.rs:581, 591, etc.`
All `posix_fadvise` calls use range (0, 0) meaning entire file. No partial range support for targeted readahead.

### LOW: Random Refill Pattern Overhead
**File:** `file.rs:307`
Uses `rand::thread_rng()` per fill call during preallocation. Should cache the RNG instance.

### LOW: Text-Based Dataset Markers
**File:** `dataset_marker.rs`
Text format with string parsing for marker files. Binary format would be faster to read/write.

---

## Priority Matrix

| # | Issue | Module | Severity | Effort | Impact |
|---|-------|--------|----------|--------|--------|
| 1 | libaio submit not batched | engine | Critical | Medium | High — fixes fundamental async IO bug |
| 2 | O(n) completion matching | worker | Critical | Low | High — 20-30% latency at high QD |
| 3 | io_uring SQPOLL/registered bufs | engine | Critical | High | High — eliminates syscall overhead |
| 4 | Histogram cloning for live stats | worker | High | Medium | Medium — 6GB/s bandwidth at scale |
| 5 | Zipf/Pareto scaling bias | distribution | High | Medium | Medium — correctness issue |
| 6 | Queue depth sampling frequency | worker | High | Low | Medium — 5-10% CPU reduction |
| 7 | MAP_POPULATE for mmap | engine | High | Low | Medium — eliminates page fault spikes |
| 8 | CDF caching across files | distribution | High | Medium | Medium — memory and CPU |
| 9 | Adaptive queue depth | worker | Medium | High | Medium — better backpressure |
| 10 | libaio iocb lookup O(n) | engine | Medium | Low | Low-Medium |
| 11 | Gaussian clamping bias | distribution | Medium | Low | Low — correctness |
| 12 | Tree traversal stat() calls | target | Medium | Low | Low — setup only |
| 13 | Block size pattern selection | worker | Medium | Low | Low — easy win |
| 14 | mmap MS_ASYNC support | engine | Medium | Low | Low |
| 15 | fadvise partial ranges | target | Medium | Medium | Low |
| 16 | InFlightOp unused fields | worker | Low | Low | Minimal |
| 17 | Branch prediction hints | worker | Low | Low | Minimal |
| 18 | Binary dataset markers | target | Low | Medium | Minimal |

---

## Quick Wins (Low effort, immediate value)

1. **Fix completion matching** — Replace linear search with HashMap (~30 lines changed)
2. **Reduce queue depth sampling** — Move from per-submit to periodic (~5 lines)
3. **Add MAP_POPULATE** — Single flag addition to mmap call (~1 line)
4. **Remove unused InFlightOp fields** — Delete 2 struct fields (~2 lines)
5. **Cache RNG for refill** — Store thread_rng in struct (~3 lines)

## Architectural Wins (Higher effort, significant value)

1. **Implement io_uring SQPOLL + registered buffers** — Eliminates submit syscalls
2. **Batch libaio submissions** — Accumulate and submit 10-32 iocbs per syscall
3. **Incremental stats snapshots** — Avoid full histogram cloning
4. **Proper inverse CDF sampling** — Fix distribution accuracy for Zipf/Pareto
