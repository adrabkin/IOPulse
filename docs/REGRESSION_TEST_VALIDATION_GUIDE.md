# IOPulse Regression Test Validation Guide

**Purpose:** Ensure tests not only "pass" but actually perform their intended function correctly.

**Date:** January 27, 2026  
**Version:** 1.0

---

## Philosophy: Trust But Verify

A test that completes without errors is NOT the same as a test that works correctly.

**Example of Silent Failure:**
- Test: Read from non-existent file
- Result: 0 throughput, non-zero IOPS, no error
- Status: "PASS" (no error code)
- Reality: **FAILED** (no actual I/O happened)

**This guide ensures we catch silent failures.**

---

## Validation Principles

### 1. Verify Actual I/O Happened
- **Throughput > 0** for all tests (unless duration=0)
- **IOPS > 0** for all tests (unless duration=0)
- **Bytes read/written > 0** for all tests

### 2. Verify Correct Operation Type
- **Write tests:** write_ops > 0, write_bytes > 0
- **Read tests:** read_ops > 0, read_bytes > 0
- **Mixed tests:** Both read and write > 0, ratio matches config

### 3. Verify Performance Is Reasonable
- **Not too slow:** IOPS within ±50% of baseline (accounting for variance)
- **Not too fast:** Suspiciously high IOPS suggests fake I/O (buffered when expecting O_DIRECT)

### 4. Verify No Errors
- **errors = 0** for all tests
- **No "Failed to" messages** in output
- **No panics or crashes**

### 5. Verify Test-Specific Behavior
- **Auto-refill:** Check for "Filling with" message when expected
- **Verification:** Check success rate is 100% for matching patterns
- **Distributions:** Check coverage matches expected range
- **File distribution:** Check correct number of files created

---

## Test-by-Test Validation


### Phase 1: Core Engine Tests (O_DIRECT)

**Purpose:** Verify all I/O engines work correctly with O_DIRECT.

#### Test 1: sync engine - write (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Synchronous I/O engine with O_DIRECT
- Random write workload
- 4 worker threads

**Expected Behavior:**
- File is pre-allocated (1GB)
- Workers perform random writes
- O_DIRECT bypasses page cache
- Each write is synchronous (blocking)

**Validation Checklist:**
- [ ] Test completes without errors
- [ ] write_ops > 0 (should be ~500K-600K ops for 2s)
- [ ] write_bytes > 0 (should be ~2-3 GB for 128K blocks)
- [ ] read_ops = 0 (write-only test)
- [ ] errors = 0
- [ ] IOPS within ±50% of baseline (267K IOPS)
- [ ] Throughput > 0 MB/s
- [ ] Live stats show progress every 1s

**Red Flags:**
- ❌ Throughput = 0 (no actual I/O)
- ❌ IOPS > 10M (suspiciously high, suggests buffered I/O)
- ❌ IOPS < 50K (too slow, suggests throttling or bug)
- ❌ read_ops > 0 (should be write-only)

---

#### Test 2: io_uring engine - write (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Async I/O engine (io_uring) with O_DIRECT
- Queue depth 32 (32 in-flight operations per worker)
- Random write workload

**Expected Behavior:**
- File is pre-allocated (1GB)
- Workers submit I/O operations to io_uring
- Up to 32 operations in-flight per worker (128 total)
- Higher IOPS than sync engine (async advantage)

**Validation Checklist:**
- [ ] Test completes without errors
- [ ] write_ops > 0 (should be ~700K-800K ops)
- [ ] write_bytes > 0
- [ ] read_ops = 0
- [ ] errors = 0
- [ ] IOPS within ±50% of baseline (363K IOPS)
- [ ] IOPS > sync engine (async should be faster)
- [ ] Throughput > 0 MB/s

**Red Flags:**
- ❌ IOPS < sync engine (async should be faster)
- ❌ IOPS = sync engine (queue depth not working)
- ❌ Throughput = 0

---

#### Test 3: libaio engine - write (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine libaio --queue-depth 32 --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Async I/O engine (libaio) with O_DIRECT
- Queue depth 32
- Random write workload

**Expected Behavior:**
- File is pre-allocated (1GB)
- Workers submit I/O operations to libaio
- Similar performance to io_uring (both async)

**Validation Checklist:**
- [ ] Test completes without errors
- [ ] write_ops > 0 (should be ~600K-700K ops)
- [ ] write_bytes > 0
- [ ] read_ops = 0
- [ ] errors = 0
- [ ] IOPS within ±50% of baseline (301K IOPS)
- [ ] IOPS comparable to io_uring (both async)
- [ ] Throughput > 0 MB/s

**Red Flags:**
- ❌ IOPS << io_uring (should be similar)
- ❌ IOPS < sync engine (async should be faster)

---

#### Test 4: mmap engine - write (buffered, auto-fill)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine mmap --threads 4
```

**What It Tests:**
- Memory-mapped I/O engine (buffered, no O_DIRECT)
- Random write workload
- Auto-fill on empty file

**Expected Behavior:**
- File is created and auto-filled (mmap requires existing file)
- Workers use mmap() to map file into memory
- Writes go to page cache (very fast)
- Much higher IOPS than O_DIRECT engines

**Validation Checklist:**
- [ ] Test completes without errors
- [ ] Output shows "Filling with Random pattern" (auto-fill)
- [ ] write_ops > 0 (should be ~2.5M ops)
- [ ] write_bytes > 0
- [ ] read_ops = 0
- [ ] errors = 0
- [ ] IOPS within ±50% of baseline (1.26M IOPS)
- [ ] IOPS >> O_DIRECT engines (buffered is much faster)
- [ ] Throughput > 0 MB/s

**Red Flags:**
- ❌ No "Filling with" message (auto-fill didn't trigger)
- ❌ IOPS < 500K (too slow for buffered I/O)
- ❌ IOPS similar to O_DIRECT (should be much faster)

---

#### Test 5: sync engine - write (buffered)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4
```

**What It Tests:**
- Synchronous I/O engine without O_DIRECT (buffered)
- Random write workload
- Page cache enabled

**Expected Behavior:**
- File is created (sparse or pre-allocated)
- Workers perform random writes to page cache
- Much faster than O_DIRECT (no disk sync)

**Validation Checklist:**
- [ ] Test completes without errors
- [ ] write_ops > 0 (should be ~2M+ ops)
- [ ] write_bytes > 0
- [ ] read_ops = 0
- [ ] errors = 0
- [ ] IOPS >> O_DIRECT sync (buffered is much faster)
- [ ] Throughput > 0 MB/s

**Red Flags:**
- ❌ IOPS similar to O_DIRECT (buffering not working)
- ❌ IOPS < 500K (too slow for buffered)

---


### Phase 2: Workload Type Tests (O_DIRECT)

**Purpose:** Verify read, write, and mixed workloads work correctly.

#### Test 6: 100% write workload (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Pure write workload (no reads)
- O_DIRECT mode

**Expected Behavior:**
- File is pre-allocated
- All operations are writes
- No read operations

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] read_ops = 0 (write-only)
- [ ] read_bytes = 0 (write-only)
- [ ] errors = 0
- [ ] Write percentage = 100.00%

**Red Flags:**
- ❌ read_ops > 0 (should be write-only)
- ❌ Write percentage < 100%

---

#### Test 7: 100% read workload (O_DIRECT, pre-fill)

**Command:**
```bash
# Phase 1: Fill file
iopulse test.dat --file-size 1G --duration 1s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s

# Phase 2: Read test
iopulse test.dat --file-size 1G --duration 2s --read-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Pure read workload (no writes)
- O_DIRECT mode
- File must be pre-filled (Phase 1)

**Expected Behavior:**
- Phase 1: Creates and fills file with data
- Phase 2: Reads from filled file
- All operations are reads
- No write operations in Phase 2

**Validation Checklist:**
- [ ] Phase 1: write_ops > 0, write_bytes > 0
- [ ] Phase 2: read_ops > 0, read_bytes > 0
- [ ] Phase 2: write_ops = 0 (read-only)
- [ ] Phase 2: write_bytes = 0 (read-only)
- [ ] Phase 2: errors = 0
- [ ] Phase 2: Read percentage = 100.00%
- [ ] Phase 2: Throughput > 0 (CRITICAL - proves file was read)

**Red Flags:**
- ❌ Phase 2: Throughput = 0 (file doesn't exist or empty)
- ❌ Phase 2: write_ops > 0 (should be read-only)
- ❌ Phase 2: Read percentage < 100%
- ❌ Phase 2: IOPS = 0 (no I/O happened)

**CRITICAL:** This test caught the "read from non-existent file" bug. Always verify throughput > 0!

---

#### Test 8: Mixed 70/30 read/write (O_DIRECT)

**Command:**
```bash
# Phase 1: Fill file
iopulse test.dat --file-size 1G --duration 1s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s

# Phase 2: Mixed test
iopulse test.dat --file-size 1G --duration 2s --read-percent 70 --write-percent 30 \
  --random --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Mixed workload (70% reads, 30% writes)
- O_DIRECT mode
- Ratio accuracy

**Expected Behavior:**
- Phase 1: Fills file
- Phase 2: Performs mixed operations
- Ratio should be close to 70/30 (within ±5%)

**Validation Checklist:**
- [ ] Phase 2: read_ops > 0
- [ ] Phase 2: write_ops > 0
- [ ] Phase 2: read_bytes > 0
- [ ] Phase 2: write_bytes > 0
- [ ] Phase 2: errors = 0
- [ ] Phase 2: Read percentage = 65-75% (±5% tolerance)
- [ ] Phase 2: Write percentage = 25-35% (±5% tolerance)
- [ ] Phase 2: Throughput > 0

**Red Flags:**
- ❌ Read percentage = 100% or 0% (ratio not working)
- ❌ Write percentage = 100% or 0% (ratio not working)
- ❌ Ratio outside ±10% (poor accuracy)

---

#### Test 9: Mixed 50/50 read/write (O_DIRECT)

**Command:**
```bash
# Phase 1: Fill file
iopulse test.dat --file-size 1G --duration 1s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s

# Phase 2: Mixed test
iopulse test.dat --file-size 1G --duration 2s --read-percent 50 --write-percent 50 \
  --random --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Balanced mixed workload (50/50)
- O_DIRECT mode

**Expected Behavior:**
- Equal reads and writes
- Ratio should be very close to 50/50

**Validation Checklist:**
- [ ] Phase 2: read_ops ≈ write_ops (within ±10%)
- [ ] Phase 2: read_bytes ≈ write_bytes (within ±10%)
- [ ] Phase 2: Read percentage = 45-55%
- [ ] Phase 2: Write percentage = 45-55%
- [ ] Phase 2: errors = 0
- [ ] Phase 2: Throughput > 0

**Red Flags:**
- ❌ Ratio far from 50/50 (e.g., 70/30)
- ❌ One operation type = 0

---


### Phase 3: Access Pattern Tests (O_DIRECT)

**Purpose:** Verify random and sequential access patterns work correctly.

#### Test 10: Random access pattern (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Random access (--random flag)
- Operations target random offsets in file

**Expected Behavior:**
- Workers access random locations in file
- High coverage of file (many different blocks accessed)
- No sequential pattern

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage > 50% (many blocks accessed)

**Red Flags:**
- ❌ Coverage < 10% (not really random)
- ❌ Sequential pattern detected (should be random)

---

#### Test 11: Sequential access pattern (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Sequential access (no --random flag)
- Operations target consecutive offsets

**Expected Behavior:**
- Workers access file sequentially
- Each worker has its own sequential stream
- High coverage (sequential fills file)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage > 80% (sequential covers most of file)

**Red Flags:**
- ❌ Coverage < 50% (not really sequential)
- ❌ Random pattern detected (should be sequential)

---


### Phase 4: Distribution Tests (O_DIRECT)

**Purpose:** Verify access distributions (uniform, zipf, pareto, gaussian) work correctly.

#### Test 12: Uniform distribution (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --distribution uniform --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Uniform distribution (all blocks equally likely)
- Random access with no hot/cold bias

**Expected Behavior:**
- All blocks have equal probability of access
- High coverage (85-95% of blocks accessed)
- No hot spots

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage = 85-95% (uniform spreads across file)
- [ ] No hot spots (all blocks accessed roughly equally)

**Red Flags:**
- ❌ Coverage < 50% (not uniform)
- ❌ Coverage > 99% (suspicious, might be sequential)
- ❌ Hot spots detected (should be uniform)

---

#### Test 13: Zipf distribution (O_DIRECT, theta=1.2)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --distribution zipf --zipf-theta 1.2 --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Zipf distribution (hot/cold data pattern)
- theta=1.2 (realistic workload, ~20% coverage)
- Power law distribution

**Expected Behavior:**
- Some blocks accessed frequently (hot data)
- Most blocks accessed rarely or never (cold data)
- Coverage ~15-25% (theta=1.2 typical)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage = 15-25% (±10% tolerance)
- [ ] Hot spots visible (some blocks accessed much more)

**Red Flags:**
- ❌ Coverage < 5% (too concentrated, bug)
- ❌ Coverage > 50% (not really zipf)
- ❌ Coverage = 85-95% (looks like uniform, not zipf)
- ❌ No hot spots (distribution not working)

**CRITICAL:** This test previously showed 8.8% coverage (should be ~20%). Verify it's now correct!

---

#### Test 14: Pareto distribution (O_DIRECT, h=0.9)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --distribution pareto --pareto-h 0.9 --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Pareto distribution (80/20 rule)
- h=0.9 (classic 80/20: 80% of ops hit 20% of blocks)

**Expected Behavior:**
- 80% of operations hit 20% of blocks (hot data)
- 20% of operations hit 80% of blocks (cold data)
- Coverage ~18-22% (±10% tolerance)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage = 18-22% (80/20 rule)
- [ ] Hot spots visible

**Red Flags:**
- ❌ Coverage < 10% (too concentrated)
- ❌ Coverage > 40% (not really 80/20)
- ❌ Coverage = 11% (previous bug, should be ~20%)

**CRITICAL:** This test previously showed 11% coverage (should be ~20%). Verify it's now correct!

---

#### Test 15: Gaussian distribution (O_DIRECT, stddev=0.1)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --distribution gaussian --gaussian-stddev 0.1 --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Gaussian distribution (bell curve, locality)
- stddev=0.1 (tight locality around center)

**Expected Behavior:**
- Most accesses near file center (50% offset)
- Bell curve distribution
- Coverage ~40-50% (2-sigma range)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Coverage = 40-50% (±10% tolerance)
- [ ] Hot spot near file center

**Red Flags:**
- ❌ Coverage < 20% (too tight)
- ❌ Coverage > 70% (too loose)
- ❌ Hot spot not near center (distribution broken)

---


### Phase 5: Buffered I/O Tests (Coverage Only)

**Purpose:** Verify buffered I/O works (no O_DIRECT).

#### Test 16: Buffered I/O - baseline

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4
```

**What It Tests:**
- Buffered I/O (page cache enabled)
- Baseline performance without O_DIRECT

**Expected Behavior:**
- Much faster than O_DIRECT (no disk sync)
- Writes go to page cache
- Very high IOPS (>1M)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS > 1M (buffered is very fast)
- [ ] IOPS >> O_DIRECT (at least 3-5x faster)

**Red Flags:**
- ❌ IOPS < 500K (too slow for buffered)
- ❌ IOPS similar to O_DIRECT (buffering not working)

---

#### Test 17: Buffered I/O - io_uring

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4
```

**What It Tests:**
- Buffered I/O with async engine
- io_uring with page cache

**Expected Behavior:**
- Very high IOPS (buffered + async)
- Should be fastest configuration

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS > 1M
- [ ] IOPS >= buffered sync (async should be equal or faster)

**Red Flags:**
- ❌ IOPS < buffered sync (async should be faster)
- ❌ IOPS < 500K

---


### Phase 6: Queue Depth Tests (O_DIRECT)

**Purpose:** Verify queue depth parameter works correctly.

#### Test 18: Queue depth 1 (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --queue-depth 1 --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- QD=1 (synchronous, one operation at a time per worker)
- Baseline for queue depth comparison

**Expected Behavior:**
- Each worker has max 1 in-flight operation
- Lower IOPS than higher queue depths
- Synchronous behavior

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS reasonable for QD=1 (~200-300K)

**Red Flags:**
- ❌ IOPS > 500K (QD=1 shouldn't be this fast)
- ❌ IOPS = 0

---

#### Test 19: Queue depth 32 (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- QD=32 (32 in-flight operations per worker)
- Async I/O advantage

**Expected Behavior:**
- Higher IOPS than QD=1 (async parallelism)
- 128 total in-flight operations (32 × 4 workers)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS > QD=1 test (async should be faster)
- [ ] IOPS within ±50% of baseline (363K)

**Red Flags:**
- ❌ IOPS = QD=1 test (queue depth not working)
- ❌ IOPS < QD=1 test (should be faster)

---

#### Test 20: Queue depth 128 (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine io_uring --queue-depth 128 --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- QD=128 (high queue depth)
- Maximum async parallelism

**Expected Behavior:**
- Highest IOPS (512 total in-flight operations)
- Should be faster than QD=32

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS >= QD=32 test (higher QD should be equal or faster)

**Red Flags:**
- ❌ IOPS < QD=32 (should be faster or equal)
- ❌ IOPS = QD=1 (queue depth not working)

---


### Phase 7: File Distribution Tests (O_DIRECT)

**Purpose:** Verify shared, per-worker, and partitioned file distribution modes.

#### Test 21: Shared file distribution (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --file-distribution shared --direct --live-interval 1s
```

**What It Tests:**
- All workers share single file
- Default behavior

**Expected Behavior:**
- Single file created (test.dat)
- All 4 workers access same file
- File locking may be used (if configured)

**Validation Checklist:**
- [ ] Only 1 file created (test.dat)
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] All workers contribute to operations

**Red Flags:**
- ❌ Multiple files created (should be single file)
- ❌ Only 1 worker active (others blocked)

---

#### Test 22: Per-worker file distribution (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --file-distribution per-worker --direct --live-interval 1s
```

**What It Tests:**
- Each worker gets its own file
- No file contention between workers

**Expected Behavior:**
- 4 files created (test.dat.0, test.dat.1, test.dat.2, test.dat.3)
- Each worker accesses only its file
- No locking needed

**Validation Checklist:**
- [ ] Exactly 4 files created
- [ ] Files named: test.dat.0, test.dat.1, test.dat.2, test.dat.3
- [ ] Each file is 1GB
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] All workers contribute equally

**Red Flags:**
- ❌ Wrong number of files (should be 4)
- ❌ Files wrong size
- ❌ Only 1 file created (per-worker not working)

---

#### Test 23: Partitioned file distribution (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --file-distribution partitioned --direct --live-interval 1s
```

**What It Tests:**
- Single file partitioned into regions
- Each worker gets exclusive region (no overlap)
- Worker 0: 0-256MB, Worker 1: 256-512MB, etc.

**Expected Behavior:**
- Single file created (1GB)
- Each worker accesses only its region
- No overlap between workers
- No locking needed

**Validation Checklist:**
- [ ] Only 1 file created (test.dat)
- [ ] File is 1GB
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] All workers contribute equally
- [ ] Each worker accesses only its region (no overlap)

**Red Flags:**
- ❌ Multiple files created (should be single file)
- ❌ Workers accessing overlapping regions
- ❌ Unequal worker distribution

---


### Phase 8: Write Pattern Tests (O_DIRECT)

**Purpose:** Verify different write patterns (random, zeros, ones, sequential).

#### Test 24: Write pattern: random (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --write-pattern random --direct --live-interval 1s
```

**What It Tests:**
- Random data pattern (defeats deduplication)
- Each write contains random bytes

**Expected Behavior:**
- File contains random data (not zeros)
- Storage deduplication defeated
- Normal performance

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] File contains random data (verify with: od -An -N1000 test.dat | sort -u | wc -l should be ~256)

**Red Flags:**
- ❌ File contains all zeros (pattern not working)
- ❌ File contains single byte value (pattern not working)

---

#### Test 25: Write pattern: zeros (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --write-pattern zeros --direct --live-interval 1s
```

**What It Tests:**
- Zeros pattern (all bytes = 0x00)
- Storage deduplication friendly

**Expected Behavior:**
- File contains all zeros
- May trigger storage deduplication
- Normal performance

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] File contains all zeros (verify with: od -An -N1000 test.dat | sort -u should show only "0")

**Red Flags:**
- ❌ File contains non-zero bytes (pattern not working)
- ❌ File contains random data (wrong pattern)

---

#### Test 26: Write pattern: ones (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --write-pattern ones --direct --live-interval 1s
```

**What It Tests:**
- Ones pattern (all bytes = 0xFF)

**Expected Behavior:**
- File contains all 0xFF bytes
- Normal performance

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] File contains all 0xFF (verify with: od -An -tx1 -N1000 test.dat | sort -u should show only "ff")

**Red Flags:**
- ❌ File contains zeros (wrong pattern)
- ❌ File contains random data (wrong pattern)

---

#### Test 27: Write pattern: sequential (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --write-pattern sequential --direct --live-interval 1s
```

**What It Tests:**
- Sequential pattern (byte value = offset % 256)
- Predictable data pattern

**Expected Behavior:**
- File contains sequential pattern
- Byte at offset N = N % 256
- Normal performance

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] File contains sequential pattern (verify with: od -An -tx1 -N256 test.dat should show 00 01 02 ... ff)

**Red Flags:**
- ❌ File contains all zeros (wrong pattern)
- ❌ File contains random data (wrong pattern)
- ❌ Pattern doesn't match offset % 256

---


### Phase 9: Edge Case Tests (O_DIRECT)

**Purpose:** Verify edge cases and boundary conditions.

#### Test 28: Single thread (O_DIRECT, threads=1)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 1 --direct --live-interval 1s
```

**What It Tests:**
- Single worker thread
- No parallelism

**Expected Behavior:**
- Only 1 worker active
- Lower IOPS than multi-threaded (no parallelism)
- Still functional

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS reasonable for single thread (~50-100K)
- [ ] IOPS < multi-threaded tests (no parallelism)

**Red Flags:**
- ❌ IOPS = 0
- ❌ IOPS same as 4-thread test (parallelism not working)

---

#### Test 29: Many threads (O_DIRECT, threads=8)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 8 --direct --live-interval 1s
```

**What It Tests:**
- High thread count (8 workers)
- Parallelism scaling

**Expected Behavior:**
- 8 workers active
- Higher IOPS than 4 threads (more parallelism)
- Should scale reasonably (not perfectly linear)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS > 4-thread test (more parallelism)
- [ ] IOPS < 2× 4-thread test (scaling not perfect)

**Red Flags:**
- ❌ IOPS = 4-thread test (parallelism not working)
- ❌ IOPS < 4-thread test (contention issues)

---

#### Test 30: Small block size (O_DIRECT, 4K)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --block-size 4k --direct --live-interval 1s
```

**What It Tests:**
- Small block size (4KB)
- Higher IOPS, lower throughput

**Expected Behavior:**
- Many small operations
- High IOPS (more operations)
- Lower throughput (smaller blocks)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS > 128K block test (smaller blocks = more ops)
- [ ] Throughput < 128K block test (smaller blocks = less MB/s)
- [ ] Average block size ≈ 4KB

**Red Flags:**
- ❌ IOPS < 128K block test (should be higher)
- ❌ Throughput > 128K block test (should be lower)
- ❌ Average block size != 4KB (wrong block size)

---

#### Test 31: Large block size (O_DIRECT, 1M)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --block-size 1M --direct --live-interval 1s
```

**What It Tests:**
- Large block size (1MB)
- Lower IOPS, higher throughput

**Expected Behavior:**
- Fewer large operations
- Lower IOPS (fewer operations)
- Higher throughput (larger blocks)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS < 4K block test (larger blocks = fewer ops)
- [ ] Throughput > 4K block test (larger blocks = more MB/s)
- [ ] Average block size ≈ 1MB

**Red Flags:**
- ❌ IOPS > 4K block test (should be lower)
- ❌ Throughput < 4K block test (should be higher)
- ❌ Average block size != 1MB (wrong block size)

---


### Phase 10: Auto-Refill Tests (Task 24r, Buffered)

**Purpose:** Verify automatic file filling works correctly for read tests on empty files.

#### Test 32: Auto-fill: read-only on empty file (buffered)

**Command:**
```bash
rm -f test.dat
iopulse test.dat --file-size 100M --read-percent 100 --duration 1s --random
```

**What It Tests:**
- Auto-refill triggers for read-only test on non-existent file
- File is automatically created and filled
- Test proceeds without manual intervention

**Expected Behavior:**
- File doesn't exist initially
- IOPulse detects read-only test on empty file
- Automatically fills file with random data
- Test proceeds with reads

**Validation Checklist:**
- [ ] Output shows "Filling with Random pattern" message
- [ ] File is created (100MB)
- [ ] read_ops > 0
- [ ] read_bytes > 0
- [ ] write_ops = 0 (read-only)
- [ ] errors = 0
- [ ] Throughput > 0 (CRITICAL - proves file was filled and read)

**Red Flags:**
- ❌ No "Filling with" message (auto-fill didn't trigger)
- ❌ Throughput = 0 (file not filled, reads failed silently)
- ❌ read_ops = 0 (no reads happened)
- ❌ File doesn't exist after test (not created)

**CRITICAL:** This test verifies the P0 fix for silent read failures!

---

#### Test 33: Auto-fill: mixed 50/50 on empty file (buffered)

**Command:**
```bash
rm -f test.dat
iopulse test.dat --file-size 100M --read-percent 50 --write-percent 50 --duration 1s --random
```

**What It Tests:**
- Auto-refill triggers for mixed test on empty file
- File is filled before test starts

**Expected Behavior:**
- File doesn't exist initially
- IOPulse detects mixed test needs filled file
- Automatically fills file
- Test proceeds with mixed operations

**Validation Checklist:**
- [ ] Output shows "Filling with Random pattern" message
- [ ] File is created (100MB)
- [ ] read_ops > 0
- [ ] write_ops > 0
- [ ] read_bytes > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Read/write ratio ≈ 50/50

**Red Flags:**
- ❌ No "Filling with" message
- ❌ Throughput = 0
- ❌ read_ops = 0 (reads failed)

---

#### Test 34: Auto-fill: mmap on empty file (buffered)

**Command:**
```bash
rm -f test.dat
iopulse test.dat --file-size 100M --write-percent 100 --duration 1s --random --engine mmap
```

**What It Tests:**
- mmap engine requires existing file
- Auto-fill creates and fills file for mmap

**Expected Behavior:**
- File doesn't exist initially
- IOPulse creates and fills file (mmap requires existing file)
- mmap engine maps file into memory
- Test proceeds

**Validation Checklist:**
- [ ] Output shows "Filling with Random pattern" message
- [ ] File is created (100MB)
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS very high (mmap is fast)

**Red Flags:**
- ❌ No "Filling with" message
- ❌ Test fails with "file not found" (auto-fill didn't work)
- ❌ IOPS < 500K (mmap should be very fast)

---

#### Test 35: Auto-fill: --no-refill flag errors correctly

**Command:**
```bash
rm -f test.dat
# This should FAIL (test passes if command fails)
if iopulse test.dat --file-size 100M --read-percent 100 --duration 1s --random --no-refill 2>&1; then
    false  # Command succeeded, test fails
else
    true   # Command failed, test passes
fi
```

**What It Tests:**
- --no-refill flag prevents auto-fill
- Read-only test on empty file should error
- User explicitly disabled auto-fill

**Expected Behavior:**
- File doesn't exist
- IOPulse detects read-only test on empty file
- --no-refill flag prevents auto-fill
- **Command fails with error** (this is correct behavior)

**Validation Checklist:**
- [ ] Command exits with error code (non-zero)
- [ ] Error message mentions file doesn't exist or needs filling
- [ ] File is NOT created
- [ ] No "Filling with" message (auto-fill disabled)

**Red Flags:**
- ❌ Command succeeds (should fail)
- ❌ File is created (auto-fill triggered despite --no-refill)
- ❌ "Filling with" message appears (flag not respected)

---

#### Test 36: Auto-fill: write-only skips refill

**Command:**
```bash
rm -f test.dat
iopulse test.dat --file-size 100M --write-percent 100 --duration 1s --random 2>&1 | grep -qv 'Filling with'
```

**What It Tests:**
- Write-only tests don't trigger auto-fill
- File will be filled by the test itself
- No unnecessary pre-filling

**Expected Behavior:**
- File doesn't exist initially
- IOPulse creates sparse file (or pre-allocates)
- **No auto-fill** (write test will fill it)
- Test proceeds with writes

**Validation Checklist:**
- [ ] No "Filling with" message in output
- [ ] File is created
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0

**Red Flags:**
- ❌ "Filling with" message appears (unnecessary refill)
- ❌ write_ops = 0 (no writes happened)

---


### Phase 11: Advanced Features (Task 24j, O_DIRECT)

**Purpose:** Verify advanced features (think time, verification, CPU affinity, NUMA).

#### Test 37: Think time: sleep mode (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 100M --duration 1s --write-percent 100 --random \
  --threads 2 --think-time 100us --think-mode sleep --direct --live-interval 1s
```

**What It Tests:**
- Think time between operations (simulates application processing)
- Sleep mode (actual sleep, not busy-wait)
- 100µs delay between operations

**Expected Behavior:**
- Workers sleep 100µs between operations
- Lower IOPS than without think time
- IOPS ≈ 1 / (100µs + operation_time)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS < test without think time (delay reduces IOPS)
- [ ] IOPS ≈ expected based on think time

**Red Flags:**
- ❌ IOPS same as without think time (think time not working)
- ❌ IOPS = 0

---

#### Test 38: Verification: zeros pattern (O_DIRECT, 100% success)

**Command:**
```bash
# Phase 1: Write zeros
iopulse test.dat --file-size 50M --duration 0s --write-percent 100 --random \
  --threads 1 --verify --verify-pattern zeros --direct --live-interval 1s

# Phase 2: Read and verify zeros
iopulse test.dat --file-size 50M --duration 0s --read-percent 100 --random \
  --threads 1 --verify --verify-pattern zeros --direct --live-interval 1s
```

**What It Tests:**
- Data verification feature
- Write zeros, then verify reads return zeros
- Success rate should be 100%

**Expected Behavior:**
- Phase 1: Writes zeros to file
- Phase 2: Reads file and verifies each block contains zeros
- Success rate = 100.00%

**Validation Checklist:**
- [ ] Phase 1: write_ops > 0, errors = 0
- [ ] Phase 2: read_ops > 0, errors = 0
- [ ] Phase 2: Output shows "Success:    100.00%"
- [ ] Phase 2: No verification failures
- [ ] Throughput > 0 in both phases

**Red Flags:**
- ❌ Success rate < 100% (data corruption or verification bug)
- ❌ No "Success:" line in output (verification not running)
- ❌ Throughput = 0 (no I/O happened)

---

#### Test 39: CPU affinity: pin to CPUs 0-3 (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 100M --duration 1s --write-percent 100 --random \
  --threads 4 --cpu-cores 0-3 --direct --live-interval 1s
```

**What It Tests:**
- CPU affinity (pin workers to specific cores)
- 4 workers pinned to CPUs 0-3

**Expected Behavior:**
- Workers are pinned to CPUs 0, 1, 2, 3
- Normal performance (affinity shouldn't hurt)
- Workers don't migrate between cores

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS similar to test without affinity (±20%)
- [ ] No "Failed to set CPU affinity" errors

**Red Flags:**
- ❌ IOPS significantly lower than without affinity (affinity causing issues)
- ❌ Error messages about CPU affinity
- ❌ Test fails to start

---

#### Test 40: NUMA baseline: 128 threads, 1M blocks, mmap, no affinity (buffered)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 5s --write-percent 100 --random \
  --threads 128 --block-size 1M --engine mmap
```

**What It Tests:**
- High thread count (128 workers)
- Large blocks (1MB)
- mmap engine (buffered)
- No NUMA affinity (baseline)

**Expected Behavior:**
- 128 workers active
- Very high IOPS (mmap + buffered)
- Workers may be on different NUMA nodes (no affinity)

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS very high (>1M)
- [ ] All 128 workers contribute

**Red Flags:**
- ❌ IOPS < 500K (too slow for mmap + buffered)
- ❌ Only some workers active (others blocked)

---

#### Test 41: NUMA optimized: 128 threads, 1M blocks, mmap, NUMA both nodes (buffered)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 5s --write-percent 100 --random \
  --threads 128 --block-size 1M --engine mmap --numa-zones 0,1
```

**What It Tests:**
- NUMA affinity (workers distributed across NUMA nodes 0 and 1)
- Should improve performance vs baseline (better memory locality)

**Expected Behavior:**
- Workers pinned to NUMA nodes 0 and 1
- Better performance than baseline (less cross-NUMA traffic)
- Memory allocated on local NUMA node

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] IOPS >= baseline test (NUMA should help or be equal)
- [ ] No NUMA affinity errors

**Red Flags:**
- ❌ IOPS < baseline (NUMA affinity hurting performance)
- ❌ Error messages about NUMA
- ❌ Test fails to start

---

#### Test 42: Smart partitioning: run-until-complete 1G, 4 workers (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 0s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s
```

**What It Tests:**
- Run-until-complete mode (duration=0)
- File is partitioned across workers
- Each worker fills its region completely
- Test stops when all regions filled

**Expected Behavior:**
- File is partitioned into 4 regions (256MB each)
- Each worker fills its region completely
- Test stops when all workers complete
- Total bytes written = 1GB

**Validation Checklist:**
- [ ] write_ops > 0
- [ ] write_bytes ≈ 1GB (±10%)
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Test completes (doesn't run forever)
- [ ] Duration reasonable (not too fast or slow)

**Red Flags:**
- ❌ write_bytes << 1GB (didn't fill entire file)
- ❌ write_bytes >> 1GB (wrote too much)
- ❌ Test runs forever (doesn't stop)
- ❌ Throughput = 0

---


### Phase 12: Layout_Manifest Tests

**Purpose:** Verify layout_manifest feature works correctly (directory trees, file lists).

#### Test 43: Layout: Generate 50 files (depth=2, width=3)

**Command:**
```bash
iopulse layout_dir --dir-depth 2 --dir-width 3 --total-files 50 --file-size 4k \
  --duration 1s --write-percent 100 --random
```

**What It Tests:**
- Directory tree generation
- 50 files distributed across tree
- depth=2 (2 levels of subdirectories)
- width=3 (3 subdirectories per level)

**Expected Behavior:**
- Directory tree created: layout_dir/d0/d0, layout_dir/d0/d1, etc.
- 50 files created across tree
- Each file is 4KB
- Files are written to

**Validation Checklist:**
- [ ] Directory tree created
- [ ] Exactly 50 files created (verify with: find layout_dir -type f | wc -l)
- [ ] Each file is 4KB
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0

**Red Flags:**
- ❌ Wrong number of files (not 50)
- ❌ Files wrong size (not 4KB)
- ❌ No directory tree (flat structure)
- ❌ Throughput = 0

---

#### Test 44: Layout: Export manifest

**Command:**
```bash
iopulse layout_dir --dir-depth 2 --dir-width 3 --total-files 50 --file-size 4k \
  --export-layout-manifest regression.layout_manifest --duration 0
```

**What It Tests:**
- Layout manifest export
- Manifest file creation
- duration=0 (generate layout only, no I/O test)

**Expected Behavior:**
- Directory tree created
- 50 files created
- Manifest file exported (regression.layout_manifest)
- No I/O test runs (duration=0)

**Validation Checklist:**
- [ ] Manifest file created (regression.layout_manifest)
- [ ] Manifest file is valid JSON
- [ ] Manifest contains 50 file entries
- [ ] Each entry has path and size
- [ ] Directory tree created
- [ ] 50 files created

**Red Flags:**
- ❌ Manifest file not created
- ❌ Manifest is invalid JSON
- ❌ Wrong number of files in manifest
- ❌ Missing file paths or sizes

---

#### Test 45: Layout: Import manifest, PARTITIONED, 4 workers (O_DIRECT, live stats)

**Command:**
```bash
iopulse layout_dir --layout-manifest regression.layout_manifest --duration 2s \
  --threads 4 --file-distribution partitioned --write-percent 100 --random \
  --direct --live-interval 1s
```

**What It Tests:**
- Layout manifest import
- PARTITIONED mode (files divided among workers)
- O_DIRECT with layout
- File sizes from manifest (not --file-size flag)

**Expected Behavior:**
- Reads manifest file
- Loads 50 files from manifest
- Partitions files among 4 workers (12-13 files each)
- Each worker accesses only its files
- Uses file sizes from manifest (4KB)

**Validation Checklist:**
- [ ] Manifest loaded successfully
- [ ] 50 files accessed
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] Each worker accesses ~12-13 files (partitioned)
- [ ] No file accessed by multiple workers

**Red Flags:**
- ❌ Wrong number of files accessed
- ❌ Files accessed by multiple workers (not partitioned)
- ❌ Throughput = 0
- ❌ Manifest not loaded (uses --file-size instead)

---

#### Test 46: Layout: Import manifest, SHARED, 4 workers (O_DIRECT, live stats)

**Command:**
```bash
iopulse layout_dir --layout-manifest regression.layout_manifest --duration 2s \
  --threads 4 --file-distribution shared --write-percent 100 --random \
  --direct --live-interval 1s
```

**What It Tests:**
- Layout manifest import
- SHARED mode (all workers access all files)
- O_DIRECT with layout

**Expected Behavior:**
- Reads manifest file
- Loads 50 files from manifest
- All 4 workers can access all 50 files
- File locking may be used

**Validation Checklist:**
- [ ] Manifest loaded successfully
- [ ] 50 files accessed
- [ ] write_ops > 0
- [ ] write_bytes > 0
- [ ] errors = 0
- [ ] Throughput > 0
- [ ] All workers contribute to operations
- [ ] Files may be accessed by multiple workers (shared)

**Red Flags:**
- ❌ Wrong number of files accessed
- ❌ Only 1 worker active (others blocked)
- ❌ Throughput = 0

---


### Phase 8: JSON Output Tests (O_DIRECT)

**Purpose:** Verify JSON output format and features.

#### Test 43: JSON output - basic (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s --json-output test43.json
```

**What It Tests:**
- JSON output file creation
- Basic JSON structure
- Time-series data

**Expected Behavior:**
- JSON file created (test43.json)
- Valid JSON format
- Contains time-series data
- Contains aggregate statistics

**Validation Checklist:**
- [ ] JSON file created
- [ ] JSON is valid (jq . test43.json succeeds)
- [ ] Contains "time_series" array
- [ ] Contains "aggregate" object
- [ ] Time-series has multiple snapshots (live-interval=1s)
- [ ] write_ops > 0 in aggregate
- [ ] errors = 0

**Red Flags:**
- ❌ JSON file not created
- ❌ Invalid JSON (jq fails)
- ❌ Missing time_series or aggregate
- ❌ Empty time_series array

---

#### Test 44: JSON output - per-worker (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine io_uring --queue-depth 32 --threads 4 --direct --live-interval 1s \
  --json-output test44.json --json-per-worker
```

**What It Tests:**
- Per-worker statistics in JSON
- --json-per-worker flag
- Worker-level detail

**Expected Behavior:**
- JSON file created
- Each time-series snapshot contains "workers" array
- 4 worker entries per snapshot
- Each worker has individual statistics

**Validation Checklist:**
- [ ] JSON file created
- [ ] JSON is valid
- [ ] time_series[0].workers array exists
- [ ] workers array has 4 entries
- [ ] Each worker has read_ops, write_ops, etc.
- [ ] Sum of worker stats = aggregate stats

**Red Flags:**
- ❌ No workers array (flag not working)
- ❌ Wrong number of workers
- ❌ Worker stats don't sum to aggregate

---

#### Test 45: JSON output - histogram (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --live-interval 1s \
  --json-output test45.json --json-histogram
```

**What It Tests:**
- Histogram export to separate JSON file
- --json-histogram flag
- Latency distribution data

**Expected Behavior:**
- Main JSON file created (test45.json)
- Histogram JSON file created (test45_histogram.json)
- Histogram contains latency buckets
- Histogram shows distribution of operation latencies

**Validation Checklist:**
- [ ] Main JSON file created
- [ ] Histogram JSON file created (test45_histogram.json)
- [ ] Both files are valid JSON
- [ ] Histogram has "buckets" array
- [ ] Buckets have counts and ranges
- [ ] Bucket counts sum to total operations

**Red Flags:**
- ❌ Histogram file not created (flag not working)
- ❌ Invalid JSON in histogram file
- ❌ Empty buckets array
- ❌ Bucket counts don't sum to total ops

---

#### Test 46: JSON output - no live display (O_DIRECT)

**Command:**
```bash
iopulse test.dat --file-size 1G --duration 2s --write-percent 100 --random \
  --engine sync --threads 4 --direct --no-live --json-output test46.json
```

**What It Tests:**
- JSON output without live display
- --no-live flag
- Monitoring thread still runs for JSON

**Expected Behavior:**
- No live stats displayed to console
- JSON file still created
- Time-series data still collected
- Monitoring thread runs in background

**Validation Checklist:**
- [ ] JSON file created
- [ ] JSON is valid
- [ ] time_series array has multiple snapshots
- [ ] No live stats in console output
- [ ] write_ops > 0
- [ ] errors = 0

**Red Flags:**
- ❌ Empty time_series array (monitoring didn't run)
- ❌ Live stats shown in console (--no-live not working)
- ❌ JSON file not created

---


---

## Common Validation Patterns

### Pattern 1: Verify Actual I/O Happened

**Check:**
```bash
# In test output, look for:
Aggregate Results:
  Duration: 2.00s
  Read:  X ops (Y GB) - Z IOPS
  Write: X ops (Y GB) - Z IOPS
  Throughput: Z MB/s  ← MUST BE > 0
```

**If throughput = 0:**
- File doesn't exist
- File is empty
- No actual I/O happened (silent failure)

---

### Pattern 2: Verify Operation Type

**Write-only test:**
```
Read:  0 ops (0.00 GB) - 0.00K IOPS  ← Should be 0
Write: X ops (Y GB) - Z IOPS         ← Should be > 0
```

**Read-only test:**
```
Read:  X ops (Y GB) - Z IOPS         ← Should be > 0
Write: 0 ops (0.00 GB) - 0.00K IOPS  ← Should be 0
```

**Mixed test (70/30):**
```
Read:  X ops (Y GB) - Z IOPS         ← Should be ~70%
Write: X ops (Y GB) - Z IOPS         ← Should be ~30%
```

---

### Pattern 3: Verify No Errors

**Check:**
```bash
# In test output, look for:
Errors: 0  ← MUST BE 0

# Also check for error messages:
grep -i "error\|failed\|panic" test_output.txt
# Should return nothing
```

---

### Pattern 4: Verify Performance

**Check:**
```bash
# Compare IOPS to baseline:
# sync engine: ~267K IOPS (±50%)
# io_uring: ~363K IOPS (±50%)
# libaio: ~301K IOPS (±50%)
# mmap: ~1.26M IOPS (±50%)

# If IOPS is outside range:
# - Too low: throttling, bug, or hardware issue
# - Too high: fake I/O, buffered when expecting O_DIRECT
```

---

### Pattern 5: Verify File Contents

**For write pattern tests:**
```bash
# Random pattern:
od -An -N1000 test.dat | tr ' ' '\n' | sort -u | wc -l
# Should be ~256 (all byte values present)

# Zeros pattern:
od -An -N1000 test.dat | tr ' ' '\n' | sort -u
# Should show only "0" or "000000"

# Ones pattern:
od -An -tx1 -N1000 test.dat | tr ' ' '\n' | sort -u
# Should show only "ff"

# Sequential pattern:
od -An -tx1 -N256 test.dat
# Should show: 00 01 02 03 ... fe ff
```

---

## Automated Validation Script

To automate validation, create a script that checks each test output:

```bash
#!/bin/bash
# validate_test_results.sh

TEST_OUTPUT="$1"

# Check 1: Throughput > 0
if grep -q "Throughput: 0.00 MB/s" "$TEST_OUTPUT"; then
    echo "❌ FAIL: Throughput = 0 (no actual I/O)"
    exit 1
fi

# Check 2: No errors
if ! grep -q "Errors: 0" "$TEST_OUTPUT"; then
    echo "❌ FAIL: Errors detected"
    exit 1
fi

# Check 3: Operations > 0
if grep -q "Total: 0 ops" "$TEST_OUTPUT"; then
    echo "❌ FAIL: No operations performed"
    exit 1
fi

# Check 4: No error messages
if grep -qi "error\|failed\|panic" "$TEST_OUTPUT"; then
    echo "❌ FAIL: Error messages in output"
    exit 1
fi

echo "✅ PASS: Basic validation checks passed"
exit 0
```

---

## Summary: What Makes A Test "Pass"

A test truly passes when:

1. **Exit code = 0** (command succeeded)
2. **Throughput > 0** (actual I/O happened)
3. **Operations > 0** (work was done)
4. **Errors = 0** (no failures)
5. **Correct operation type** (read/write/mixed as expected)
6. **Correct ratio** (for mixed tests, within ±5%)
7. **Performance reasonable** (within ±50% of baseline)
8. **Test-specific behavior** (auto-fill, verification, etc. as expected)

**A test that exits with code 0 but has throughput = 0 is a FAILED test, not a passed test.**

---

## Using This Guide

### During Test Runs

1. Run regression test suite
2. Check summary: X/46 tests passed
3. For each test, verify:
   - Exit code = 0
   - Throughput > 0
   - Operations > 0
   - Errors = 0
4. For failed tests, check test output against validation checklist
5. Investigate any red flags

### After Code Changes

1. Run regression tests
2. Compare results to previous run
3. Check for performance regressions (>10% slower)
4. Check for behavior changes (coverage, ratios, etc.)
5. Investigate any differences

### When Adding New Tests

1. Document what the test is supposed to do
2. Document expected behavior
3. Create validation checklist
4. Add to this guide

---

**This guide ensures IOPulse is trustworthy and reliable.**

