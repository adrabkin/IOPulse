# Duration Modes Explained

**Date:** January 19, 2026  
**Purpose:** Explain the difference between duration-based and run-until-complete modes

---

## Two Completion Modes

IOPulse supports two ways to determine when a test completes:

### 1. Duration Mode (`--duration <seconds>`)

**Behavior:** Run for a fixed amount of time, then stop.

**Example:**
```bash
./iopulse test.dat --file-size 1G --duration 1s --write-percent 100 --random --direct
```

**What happens:**
- Test runs for exactly 1 second
- Performs as many IOs as possible in that time
- Stops when time expires (regardless of bytes transferred)
- Reports IOPS and throughput

**Use case:** Quick performance measurement

**Results:**
- IOPulse: 1,099 ops in 1.029s = 1,070 IOPS

### 2. Run-Until-Complete Mode (`--duration 0s`)

**Behavior:** Run until the full file size has been transferred, then stop.

**Example:**
```bash
./iopulse test.dat --file-size 1G --duration 0s --write-percent 100 --random --direct
```

**What happens:**
- Test runs until `total_bytes_transferred >= file_size`
- For 1GB file with 4K blocks = 262,144 operations
- Stops when 1GB has been transferred
- Reports total time and IOPS

**Use case:** Measure time to complete a specific workload

**Results:**
- IOPulse: 262,144 ops in 3m53s (233s) = 1,125 IOPS average

---

## Key Differences

### Duration Mode
- **Fixed time**, variable operations
- Quick measurement (1-10 seconds typical)
- Good for comparing IOPS across configurations
- Doesn't complete the full file

### Run-Until-Complete Mode
- **Fixed operations** (file size / block size), variable time
- Long measurement (minutes for large files with O_DIRECT)
- Good for measuring sustained performance
- Completes the full file

---

## Why Run-Until-Complete Takes So Long

With O_DIRECT and 1GB file:

**Math:**
- File size: 1,073,741,824 bytes (1GB)
- Block size: 4,096 bytes (4K)
- Operations needed: 262,144 (1GB / 4K)
- IOPS with O_DIRECT: ~1,100 IOPS
- **Time needed: 262,144 / 1,100 = 238 seconds (~4 minutes)**

**Why O_DIRECT is slow:**
- Bypasses page cache (no RAM buffering)
- Every IO goes to physical storage
- Measures true storage performance
- This is CORRECT behavior

**Comparison:**
- Buffered IO: ~250K IOPS (page cache) = 1 second for 1GB
- O_DIRECT: ~1K IOPS (real storage) = 4 minutes for 1GB
- **O_DIRECT is 250x slower** (measuring storage, not RAM)

---

## When to Use Each Mode

### Use Duration Mode When:
- Quick performance check
- Comparing configurations
- Testing different block sizes
- Measuring peak IOPS
- Time is limited

### Use Run-Until-Complete When:
- Measuring sustained performance
- Completing a specific workload
- Testing file creation time
- Verifying data integrity (write then read entire file)
- Benchmarking real-world scenarios

---

## Important Notes

### Random IO with Run-Until-Complete

With random IO, you may write to the same blocks multiple times (rewrites):
- Total bytes transferred: 1GB
- Unique blocks written: May be less than 1GB
- File coverage: Depends on distribution

**Example:**
- 262,144 operations × 4K = 1GB transferred
- But with uniform random, ~63% unique blocks
- Actual file coverage: ~630MB (rest are rewrites)

This is expected and correct for random IO.

### Sequential IO with Run-Until-Complete

With sequential IO, you write each block exactly once:
- Total bytes transferred: 1GB
- Unique blocks written: 1GB
- File coverage: 100%

**Example:**
- 262,144 operations × 4K = 1GB transferred
- All blocks written exactly once
- File is completely filled

---

---

## Recommendations

### For Quick Tests
```bash
# Use duration mode (1-10 seconds)
./iopulse test.dat --file-size 1G --duration 1s --write-percent 100 --random --direct
```

### For Sustained Performance
```bash
# Use run-until-complete mode
./iopulse test.dat --file-size 1G --duration 0s --write-percent 100 --random --direct
```

### For Realistic Workloads
```bash
# Use duration mode with realistic parameters
./iopulse test.dat --file-size 1G --duration 60s \
  --read-percent 70 --write-percent 30 --random \
  --distribution zipf --zipf-theta 1.2 --direct
```

---

## Summary

- **Duration mode:** Fixed time, quick measurement
- **Run-until-complete:** Fixed operations, sustained measurement
- **O_DIRECT is slow** (4 minutes for 1GB) - this is correct, measuring real storage

---

**Key Takeaway:** Don't confuse "slow" with "broken". O_DIRECT is supposed to be slow - it's measuring real storage performance, not page cache.

