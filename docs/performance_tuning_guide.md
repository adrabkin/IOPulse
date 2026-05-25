# IOPulse Performance Tuning Guide

**Date:** January 22, 2026  
**Purpose:** Document proven configurations for maximum performance based on extensive testing

---

## Overview

This guide documents performance optimizations discovered during IOPulse development and testing. All recommendations are based on measured results on real hardware.

**Test System:**
- 96 CPUs (48 physical cores, 2 threads per core)
- 2 NUMA nodes (2 sockets, 24 cores each)
- Amazon EC2 instance

---

## Key Findings

### 1. Engine Selection

**For Maximum IOPS (Buffered I/O):**
```bash
# mmap engine: 1.26M - 2.04M IOPS
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --engine mmap --threads 4
```

**For O_DIRECT (True Storage Performance):**
```bash
# io_uring engine: 5.96K IOPS with O_DIRECT
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4 --direct
```

**For Reliability:**
```bash
# sync engine: Always works, good baseline
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --engine sync --threads 4
```

**Performance Ranking:**
1. **mmap:** 1.26M IOPS (buffered, fastest)
2. **io_uring:** 363K IOPS (buffered), 5.96K IOPS (O_DIRECT)
3. **libaio:** 301K IOPS (buffered)
4. **sync:** 267K IOPS (buffered, baseline)

---

### 2. NUMA Affinity (CRITICAL for CPU-Intensive Workloads)

**Massive Performance Gains with NUMA:**

**1M Block Size + NUMA Node 0:**
```bash
# Baseline: 6.13K IOPS
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 32 --block-size 1M --engine mmap

# With NUMA: 9.50K IOPS (+55% improvement!)
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 32 --block-size 1M --engine mmap --numa-zones 0
```

**10M Block Size + NUMA Both Nodes:**
```bash
# Baseline: 682 IOPS
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 32 --block-size 10M --engine mmap

# With NUMA: 1.50K IOPS (+120% improvement! 2.2x faster!)
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 32 --block-size 10M --engine mmap --numa-zones 0,1
```

**Overprovisioned (128 threads on 96 CPUs) + NUMA:**
```bash
# Baseline: 4.50K IOPS
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 128 --block-size 1M --engine mmap

# With NUMA: 6.78K IOPS (+51% improvement!)
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 128 --block-size 1M --engine mmap --numa-zones 0,1
```

**When to Use NUMA Affinity:**
- ✅ Large block sizes (1M+)
- ✅ mmap engine (CPU-intensive memcpy)
- ✅ High thread counts (32+)
- ✅ Overprovisioned scenarios (threads > CPUs)
- ❌ Small block sizes (4K) - minimal benefit
- ❌ I/O-bound workloads - minimal benefit

---

### 3. CPU Affinity

**Moderate Performance Gains:**

**1M Block Size + CPU Pinning:**
```bash
# Baseline: 6.13K IOPS
# With CPU pinning: 7.21K IOPS (+18% improvement)
iopulse test.dat --file-size 5G --duration 10s --write-percent 100 --random \
  --threads 32 --block-size 1M --engine mmap --cpu-cores 0-31
```

**When to Use CPU Affinity:**
- ✅ Moderate thread counts (8-32)
- ✅ CPU-bound workloads
- ✅ When you want predictable performance
- ❌ Low thread counts (1-4) - minimal benefit
- ❌ I/O-bound workloads - minimal benefit

---

### 4. Think Time (Simulating Application Processing)

**Realistic Application Simulation:**

**OLTP Database (50µs query processing):**
```bash
# Simulates database query processing between I/Os
iopulse test.dat --file-size 10G --duration 60s --read-percent 70 --write-percent 30 \
  --random --distribution zipf --zipf-theta 1.0 \
  --block-size 8k --threads 16 --queue-depth 32 \
  --think-time 50us --think-mode sleep
```

**Expected:** IOPS reduced to realistic levels (~10-20K IOPS per thread)

**Adaptive Think Time (Scales with I/O Latency):**
```bash
# Think time adapts to actual I/O performance
iopulse test.dat --file-size 10G --duration 60s --write-percent 100 --random \
  --threads 8 --think-adaptive-percent 50
```

**Effect:** Adds 50% of I/O latency as think time (simulates processing)

---

### 5. Data Verification

**Ensuring Data Integrity:**

**Write with Verification Pattern:**
```bash
# Write deterministic data
iopulse test.dat --file-size 1G --duration 0s --write-percent 100 --random \
  --verify --verify-pattern zeros
```

**Read and Verify:**
```bash
# Verify data integrity
iopulse test.dat --file-size 1G --duration 0s --read-percent 100 --random \
  --verify --verify-pattern zeros
```

**Expected:** "Verification: Operations: X, Failures: 0, Success: 100.00%"

**Supported Patterns:**
- `zeros` - All zeros (fast, simple)
- `ones` - All 0xFF (fast, simple)
- `sequential` - Sequential bytes (0x00, 0x01, ..., 0xFF, 0x00, ...)
- `random` - Deterministic random (offset-based seed)

**Important:** Use `--duration 0s` (run-until-complete) to ensure all blocks are written before verifying.

---

### 6. File Distribution Modes

**For Maximum Aggregate Bandwidth:**
```bash
# Per-worker files: 1.43M IOPS (4 workers)
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --threads 4 --file-distribution per-worker
```

**For Shared File Testing:**
```bash
# Shared file: 288K IOPS (4 workers)
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --threads 4 --file-distribution shared
```

**For Partitioned Access:**
```bash
# Partitioned: 361K IOPS (4 workers, no overlap)
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --threads 4 --file-distribution partitioned
```

---

### 7. O_DIRECT vs Buffered I/O

**O_DIRECT (True Storage Performance):**
```bash
# Bypasses page cache, measures real storage
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4 --direct
```

**Result:** 4.00K IOPS (proves real I/O)

**Buffered I/O (Maximum IOPS):**
```bash
# Uses page cache, maximum IOPS
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4
```

**Result:** 363K IOPS (90x faster than O_DIRECT)

**Recommendation:** Always test BOTH modes. O_DIRECT proves I/O is real.

---

## Real-World Workload Examples

### Example 1: OLTP Database (MySQL/PostgreSQL)

**Characteristics:**
- Random access with hot data (Zipf distribution)
- Read-heavy (70/30 read/write)
- 8K block size (database page)
- Moderate concurrency (16-32 threads)
- Query processing time (50µs think time)

**Optimized Configuration:**
```bash
iopulse /data/db_test.dat \
  --file-size 100G \
  --duration 300s \
  --read-percent 70 --write-percent 30 \
  --random \
  --distribution zipf --zipf-theta 1.0 \
  --block-size 8k \
  --threads 32 \
  --queue-depth 32 \
  --engine io_uring \
  --think-time 50us --think-mode sleep \
  --numa-zones 0,1 \
  --direct
```

**Expected Performance:**
- 20-30K IOPS (realistic for OLTP)
- 25-35% coverage (hot data)
- 70/30 read/write ratio

---

### Example 2: High-Throughput Streaming

**Characteristics:**
- Sequential or random access
- Large block sizes (1M-10M)
- High thread count
- Maximum throughput

**Optimized Configuration:**
```bash
iopulse /data/stream_test.dat \
  --file-size 100G \
  --duration 60s \
  --write-percent 100 \
  --random \
  --block-size 1M \
  --threads 128 \
  --engine mmap \
  --numa-zones 0,1
```

**Expected Performance:**
- 6-7K IOPS
- 6-7 GB/s throughput
- **+51% improvement with NUMA affinity**

---

### Example 3: Cache/CDN Simulation

**Characteristics:**
- Random access with popular content (Zipf)
- Read-heavy (95/5)
- Medium block sizes (64K)
- High concurrency

**Optimized Configuration:**
```bash
iopulse /data/cache_test.dat \
  --file-size 50G \
  --duration 300s \
  --read-percent 95 --write-percent 5 \
  --random \
  --distribution zipf --zipf-theta 1.2 \
  --block-size 64k \
  --threads 64 \
  --queue-depth 128 \
  --engine io_uring \
  --numa-zones 0,1
```

**Expected Performance:**
- High IOPS (100K+)
- 15-25% coverage (popular content)
- 95/5 read/write ratio

---

### Example 4: Data Integrity Testing

**Characteristics:**
- Write then verify
- Deterministic patterns
- Complete coverage

**Optimized Configuration:**
```bash
# Step 1: Write with verification pattern
iopulse /data/integrity_test.dat \
  --file-size 10G \
  --duration 0s \
  --write-percent 100 \
  --random \
  --verify --verify-pattern random

# Step 2: Read and verify
iopulse /data/integrity_test.dat \
  --file-size 10G \
  --duration 0s \
  --read-percent 100 \
  --random \
  --verify --verify-pattern random
```

**Expected:** 100.00% verification success, 0 failures

---

## Performance Optimization Checklist

### For I/O-Bound Workloads (Small Blocks, O_DIRECT):
- ✅ Use io_uring or libaio engine
- ✅ Set queue depth 32-128
- ✅ Use O_DIRECT flag
- ✅ Multiple threads (4-16)
- ❌ NUMA affinity not critical
- ❌ Large block sizes not needed

### For CPU-Bound Workloads (Large Blocks, mmap):
- ✅ Use mmap engine
- ✅ Large block sizes (1M-10M)
- ✅ **NUMA affinity CRITICAL** (50-120% improvement!)
- ✅ High thread count (32-128)
- ✅ Spread across NUMA nodes (--numa-zones 0,1)
- ❌ O_DIRECT not compatible with mmap

### For Mixed Workloads:
- ✅ Use io_uring engine (best all-around)
- ✅ Set appropriate read/write ratio
- ✅ Pre-fill files (auto-refill handles this)
- ✅ Use realistic distributions (zipf, pareto)
- ✅ Add think time for realism

### For Maximum Reliability:
- ✅ Use sync engine (always works)
- ✅ Test with O_DIRECT (proves real I/O)
- ✅ Verify file contents (not just IOPS)
- ✅ Use data verification (--verify)

---

## Common Mistakes to Avoid

### Mistake 1: Not Testing with O_DIRECT
**Problem:** Buffered I/O can give misleading results (page cache effects)
**Solution:** Always test BOTH buffered and O_DIRECT
**Impact:** O_DIRECT proves I/O is real (cannot be faked)

### Mistake 2: Ignoring NUMA on Multi-Socket Systems
**Problem:** Missing 50-120% performance improvement
**Solution:** Use `--numa-zones 0,1` for CPU-intensive workloads
**Impact:** Massive performance gains for large blocks + high threads

### Mistake 3: Using mmap on Empty Files
**Problem:** mmap cannot map empty files (POSIX limitation)
**Solution:** IOPulse auto-fills empty files automatically
**Impact:** Seamless user experience (handled automatically)

### Mistake 4: Wrong Block Size for Workload
**Problem:** 4K blocks for streaming, 10M blocks for OLTP
**Solution:** Match block size to real-world use case
- OLTP: 4K-16K
- Streaming: 1M-10M
- Object storage: 64K-1M

### Mistake 5: Not Using Distributions
**Problem:** Uniform random doesn't match real workloads
**Solution:** Use zipf (hot/cold) or pareto (80/20) for realism
**Impact:** More realistic testing, better insights

---

## Performance Baselines (Reference)

**Test System: 96 CPUs, 2 NUMA nodes, EBS storage**

| Configuration | IOPS | Throughput | Notes |
|---------------|------|------------|-------|
| sync, 4K, 4 threads | 267K | 1.04 GB/s | Baseline |
| io_uring, 4K, 4 threads, QD=32 | 363K | 1.42 GB/s | +36% vs sync |
| mmap, 4K, 4 threads | 1.26M | 4.93 GB/s | +372% vs sync |
| io_uring, 4K, 4 threads, QD=32, O_DIRECT | 5.96K | 23.3 MB/s | Real storage |
| mmap, 1M, 32 threads | 6.13K | 5.99 GB/s | CPU-bound |
| mmap, 1M, 32 threads, NUMA node 0 | 9.50K | 9.28 GB/s | +55% with NUMA |
| mmap, 10M, 32 threads, NUMA both | 1.50K | 14.67 GB/s | +120% with NUMA |
| mmap, 1M, 128 threads, NUMA both | 6.78K | 6.62 GB/s | +51% with NUMA |

---

## Recommendations by Use Case

### Use Case 1: Storage Benchmarking
**Goal:** Measure true storage performance

**Configuration:**
- Engine: io_uring or libaio
- O_DIRECT: Yes
- Queue depth: 32-128
- Block size: 4K-64K
- Threads: 4-16
- NUMA: Optional

**Why:** O_DIRECT bypasses cache, measures real storage

---

### Use Case 2: Application Simulation
**Goal:** Simulate real application behavior

**Configuration:**
- Engine: io_uring
- Distribution: zipf or pareto (hot/cold data)
- Think time: 10-100µs (application processing)
- Mixed read/write: Match application ratio
- Block size: Match application
- NUMA: Yes for high threads

**Why:** Realistic workload patterns, think time simulates processing

---

### Use Case 3: Maximum Performance Testing
**Goal:** Push system to limits

**Configuration:**
- Engine: mmap
- Block size: 1M-10M
- Threads: 128+ (overprovision)
- NUMA: **CRITICAL** (--numa-zones 0,1)
- Buffered I/O (no O_DIRECT)

**Why:** mmap + large blocks + NUMA = maximum throughput

---

### Use Case 4: Data Integrity Validation
**Goal:** Verify storage doesn't corrupt data

**Configuration:**
- Verification: Yes (--verify)
- Pattern: zeros, ones, or sequential
- Duration: 0s (run-until-complete)
- Random access: Yes
- Threads: 1-4 (simplicity)

**Why:** Complete coverage, deterministic patterns, verifiable

---

## Advanced Optimizations

### Optimization 1: Per-Worker Files for Aggregate Bandwidth
```bash
# 4 workers, each with own file: 1.43M IOPS aggregate
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --threads 4 --file-distribution per-worker
```

**Use when:** Testing aggregate bandwidth across multiple files/disks

---

### Optimization 2: Partitioned Distribution for MPI-IO Simulation
```bash
# Each worker gets exclusive region: 361K IOPS
iopulse test.dat --file-size 1G --duration 10s --write-percent 100 --random \
  --threads 4 --file-distribution partitioned
```

**Use when:** Simulating parallel I/O with no overlap (HPC workloads)

---

### Optimization 3: Write Patterns for Deduplication Testing
```bash
# Random data defeats deduplication
iopulse test.dat --file-size 10G --duration 60s --write-percent 100 --random \
  --write-pattern random

# Zeros/ones for dedup-friendly testing
iopulse test.dat --file-size 10G --duration 60s --write-percent 100 --random \
  --write-pattern zeros
```

**Use when:** Testing storage with deduplication/compression

---

## Summary

**Key Takeaways:**
1. **NUMA affinity is CRITICAL** for CPU-intensive workloads (+50-120% improvement)
2. **mmap engine is fastest** for buffered I/O (1.26M IOPS)
3. **O_DIRECT is essential** for validating real storage performance
4. **Think time adds realism** for application simulation
5. **Distributions matter** - use zipf/pareto for hot/cold patterns
6. **Verification works** - 100% success rate with proper configuration

**Performance Hierarchy:**
1. mmap + large blocks + NUMA affinity = **Maximum throughput** (14.67 GB/s)
2. io_uring + O_DIRECT + high QD = **Real storage performance** (5.96K IOPS)
3. sync engine = **Reliable baseline** (267K IOPS)

---

**Last Updated:** January 22, 2026  
**Based on:** Extensive testing during Tasks 24a-24j  
**Test System:** 96 CPUs, 2 NUMA nodes, Amazon EC2
