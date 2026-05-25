# Regression Test Validation Report

**Date:** January 27, 2026  
**Test Run:** run_20260126_051809  
**Methodology:** Data-driven validation using baseline.json + intent analysis  
**Tests:** 50 total

---

## Validation Approach

1. **Data Verification:** Compare actual results vs baseline.json (±10% tolerance)
2. **Intent Verification:** Ensure test behavior matches its purpose
3. **Correctness Verification:** Throughput > 0, errors = 0, correct operation types
4. **Anomaly Detection:** Flag suspicious results for investigation

---

## Test-by-Test Validation


### Test 1: sync engine - write (O_DIRECT)

**Intent:** Verify synchronous I/O engine works with O_DIRECT

**Baseline (from baseline.json):**
- IOPS: 5,350
- Throughput: 20 MB/s
- Latency p50: 512µs
- Latency p99: 512µs

**Actual Results:**
- IOPS: 5,170
- Throughput: 20.18 MB/s
- Latency p50: 512µs
- Latency p99: 512µs

**Performance Comparison:**
- IOPS: 5,170 / 5,350 = 96.6% of baseline ✅ (within 10%)
- Throughput: 20.18 / 20 = 100.9% of baseline ✅
- Latency p50: 512 / 512 = 100% ✅
- Latency p99: 512 / 512 = 100% ✅

**Correctness Verification:**
- ✅ Throughput > 0 (20.18 MB/s - actual I/O happened)
- ✅ Write-only (write_ops=10,335, read_ops=0)
- ✅ Errors = 0
- ✅ 4 workers active (all reporting)

**Intent Verification:**
- ✅ Sync engine used (confirmed in config)
- ✅ O_DIRECT used (confirmed in config)
- ✅ Write workload (100% writes)

**Result:** ✅ PASS - Performance and behavior match baseline

---

### Test 2: io_uring engine - write (O_DIRECT)

**Intent:** Verify async io_uring engine with O_DIRECT and QD=32

**Baseline (from baseline.json):**
- IOPS: 83,090
- Throughput: 324 MB/s
- Latency p50: 1,024µs
- Latency p99: 2,048µs

**Actual Results:**
- IOPS: 44,780
- Throughput: 174.94 MB/s
- Latency p50: 2,048µs
- Latency p99: 4,096µs

**Performance Comparison:**
- IOPS: 44,780 / 83,090 = 53.9% of baseline ❌ (46% regression)
- Throughput: 174.94 / 324 = 54.0% of baseline ❌ (46% regression)
- Latency p50: 2,048 / 1,024 = 200% (2x slower) ❌
- Latency p99: 4,096 / 2,048 = 200% (2x slower) ❌

**Correctness Verification:**
- ✅ Throughput > 0 (174.94 MB/s - actual I/O happened)
- ✅ Write-only (write_ops=89,632, read_ops=0)
- ✅ Errors = 0
- ✅ 4 workers active

**Intent Verification:**
- ✅ io_uring engine used (confirmed in config)
- ✅ QD=32 (confirmed in config)
- ✅ O_DIRECT used
- ⚠️ Performance 46% below baseline (significant regression)

**Result:** ⚠️ FUNCTIONAL PASS, PERFORMANCE REGRESSION

**Investigation Needed:** Why is io_uring 46% slower than baseline?

---

### Test 3: libaio engine - write (O_DIRECT)

**Intent:** Verify async libaio engine with O_DIRECT and QD=32

**Baseline (from baseline.json):**
- IOPS: 74,670
- Throughput: 291 MB/s
- Latency p50: 1,024µs
- Latency p99: 2,048µs

**Actual Results:**
- IOPS: 59,480
- Throughput: 232.36 MB/s
- Latency p50: 1,024µs
- Latency p99: 2,048µs

**Performance Comparison:**
- IOPS: 59,480 / 74,670 = 79.7% of baseline ⚠️ (20% regression)
- Throughput: 232.36 / 291 = 79.8% of baseline ⚠️ (20% regression)
- Latency p50: 1,024 / 1,024 = 100% ✅
- Latency p99: 2,048 / 2,048 = 100% ✅

**Correctness Verification:**
- ✅ Throughput > 0 (232.36 MB/s)
- ✅ Write-only (write_ops=132,693, read_ops=0)
- ✅ Errors = 0
- ✅ 4 workers active

**Intent Verification:**
- ✅ libaio engine used (confirmed in config)
- ✅ QD=32 (confirmed in config)
- ✅ O_DIRECT used
- ⚠️ Performance 20% below baseline (borderline regression)

**Result:** ⚠️ FUNCTIONAL PASS, PERFORMANCE REGRESSION

**Investigation Needed:** Why is libaio 20% slower than baseline?

---

### Test 4: mmap engine - write (buffered, auto-fill)

**Intent:** Verify mmap engine with auto-fill on empty file

**Baseline (from baseline.json):**
- IOPS: 2,900,000
- Throughput: 11,325 MB/s
- Latency p99: 4µs

**Actual Results:**
- IOPS: 2,820,000
- Throughput: 10,760 MB/s
- Latency p99: 4µs

**Performance Comparison:**
- IOPS: 2,820,000 / 2,900,000 = 97.2% of baseline ✅ (within 10%)
- Throughput: 10,760 / 11,325 = 95.0% of baseline ✅ (within 10%)
- Latency p99: 4 / 4 = 100% ✅

**Correctness Verification:**
- ✅ Throughput > 0 (10.76 GB/s)
- ✅ Write-only (write_ops=6,230,551, read_ops=0)
- ✅ Errors = 0
- ✅ 4 workers active

**Intent Verification:**
- ✅ mmap engine used (confirmed in config)
- ✅ Auto-fill triggered (4 "Filling with random data" messages)
- ✅ Buffered I/O (no O_DIRECT)
- ⚠️ All 4 workers filled same file (inefficient but functional)

**Result:** ✅ PASS - Performance and behavior match baseline

**Note:** All 4 workers filling the same 1GB file is inefficient (should be coordinated), but functionally correct.

---

### Test 5: sync engine - write (buffered)

**Intent:** Verify sync engine with buffered I/O (no O_DIRECT)

**Baseline (from baseline.json):**
- IOPS: 286,900
- Throughput: 1,116 MB/s
- Latency p50: 8µs
- Latency p99: 32µs

**Actual Results:**
- IOPS: 296,800
- Throughput: 1,130 MB/s
- Latency p50: 8µs
- Latency p99: 32µs

**Performance Comparison:**
- IOPS: 296,800 / 286,900 = 103.5% of baseline ✅ (within 10%)
- Throughput: 1,130 / 1,116 = 101.3% of baseline ✅
- Latency p50: 8 / 8 = 100% ✅
- Latency p99: 32 / 32 = 100% ✅

**Correctness Verification:**
- ✅ Throughput > 0 (1.13 GB/s)
- ✅ Write-only (write_ops=593,618, read_ops=0)
- ✅ Errors = 0
- ✅ 4 workers active

**Intent Verification:**
- ✅ Sync engine used
- ✅ Buffered I/O (no O_DIRECT in config)
- ✅ Much faster than O_DIRECT (297K vs 5K IOPS)

**Result:** ✅ PASS - Performance matches baseline, buffered I/O working correctly

---


### Test 7: 100% read workload (O_DIRECT, pre-fill)

**Intent:** Verify read-only workload works, file must be pre-filled (CRITICAL TEST - previously had silent failure bug)

**Baseline (from baseline.json):**
- IOPS: 5,310
- Throughput: 20 MB/s

**Actual Results (Phase 1 - write to fill file):**
- IOPS: 4,740
- Throughput: 18.52 MB/s
- Write ops: 4,747
- Read ops: 0

**Actual Results (Phase 2 - read test):**
- IOPS: 302,980
- Throughput: 1.16 GB/s (1,160 MB/s)
- Read ops: 606,148
- Write ops: 0

**Performance Comparison (Phase 2):**
- IOPS: 302,980 / 5,310 = 5,706% of baseline ❌ (57x faster!)
- Throughput: 1,160 / 20 = 5,800% of baseline ❌ (58x faster!)

**Correctness Verification:**
- ✅ Phase 1: File filled (write_ops=4,747, throughput=18.52 MB/s)
- ✅ Phase 2: Throughput > 0 (1.16 GB/s) **CRITICAL - proves file was read!**
- ✅ Phase 2: Read-only (read_ops=606,148, write_ops=0)
- ✅ Phase 2: Errors = 0
- ✅ 4 workers active in both phases

**Intent Verification:**
- ✅ Two-phase test (write then read)
- ✅ File pre-filled in Phase 1
- ✅ Read-only in Phase 2
- ❌ Performance 57x faster than baseline (anomaly)

**Analysis:**
- **Functional:** ✅ PASS - Test works correctly, file is read
- **Performance:** ❌ ANOMALY - Reads are 57x faster than baseline
- **P0 Fix:** ✅ VERIFIED - Throughput > 0 proves file was read (no silent failure)

**Investigation Needed:**
- Why are reads 57x faster than baseline?
- Is O_DIRECT actually being used for reads?
- Is page cache being bypassed?
- Compare Phase 1 write (4.7K IOPS) vs Phase 2 read (303K IOPS) - 64x difference!

**Result:** ✅ FUNCTIONAL PASS (P0 fix verified), ⚠️ PERFORMANCE ANOMALY

---


### Test 8: Mixed 70/30 read/write (O_DIRECT)

**Intent:** Verify mixed workload with 70% reads, 30% writes

**Baseline (from baseline.json):**
- IOPS: 5,330
- Throughput: 20 MB/s

**Actual Results (Phase 2 - mixed test):**
- IOPS: 16,090
- Throughput: 62.86 MB/s
- Read ops: 22,505 (69.9%)
- Write ops: 9,699 (30.1%)

**Performance Comparison:**
- IOPS: 16,090 / 5,330 = 302% of baseline ❌ (3x faster)
- Throughput: 62.86 / 20 = 314% of baseline ❌ (3x faster)

**Correctness Verification:**
- ✅ Phase 1: File filled (write_ops=4,692)
- ✅ Phase 2: Throughput > 0 (62.86 MB/s)
- ✅ Phase 2: Both reads and writes (read_ops=22,505, write_ops=9,699)
- ✅ Phase 2: Errors = 0
- ✅ Read ratio: 22,505 / 32,204 = 69.9% (target: 70%) ✅ Perfect!
- ✅ Write ratio: 9,699 / 32,204 = 30.1% (target: 30%) ✅ Perfect!

**Intent Verification:**
- ✅ Mixed workload working correctly
- ✅ Ratio accuracy is excellent (69.9% / 30.1%)
- ❌ Performance 3x faster than baseline (anomaly)

**Result:** ✅ FUNCTIONAL PASS (ratio perfect), ⚠️ PERFORMANCE ANOMALY

---

### Test 9: Mixed 50/50 read/write (O_DIRECT)

**Intent:** Verify balanced mixed workload (50/50)

**Baseline (from baseline.json):**
- IOPS: 5,410
- Throughput: 21 MB/s

**Actual Results (Phase 2 - mixed test):**
- IOPS: 10,310
- Throughput: 40.27 MB/s
- Read ops: 10,342 (50.1%)
- Write ops: 10,289 (49.9%)

**Performance Comparison:**
- IOPS: 10,310 / 5,410 = 190.6% of baseline ❌ (91% faster)
- Throughput: 40.27 / 21 = 191.8% of baseline ❌ (92% faster)

**Correctness Verification:**
- ✅ Phase 1: File filled (write_ops=4,961)
- ✅ Phase 2: Throughput > 0 (40.27 MB/s)
- ✅ Phase 2: Both reads and writes (read_ops=10,342, write_ops=10,289)
- ✅ Phase 2: Errors = 0
- ✅ Read ratio: 10,342 / 20,631 = 50.1% (target: 50%) ✅ Perfect!
- ✅ Write ratio: 10,289 / 20,631 = 49.9% (target: 50%) ✅ Perfect!

**Intent Verification:**
- ✅ Balanced mixed workload working correctly
- ✅ Ratio accuracy is excellent (50.1% / 49.9%)
- ❌ Performance 91% faster than baseline (anomaly)

**Result:** ✅ FUNCTIONAL PASS (ratio perfect), ⚠️ PERFORMANCE ANOMALY

---

## Summary of Findings So Far

### Functional Correctness: ✅ ALL PASS
- All tests have throughput > 0 (actual I/O happening)
- All tests have errors = 0
- All tests have correct operation types
- Mixed tests have perfect ratios (69.9%/30.1%, 50.1%/49.9%)
- P0 fix verified working (Test 7 throughput > 0)

### Performance Analysis: ⚠️ ANOMALIES DETECTED

**O_DIRECT Write Tests (consistent with baseline):**
- Test 1 (sync): 96.6% of baseline ✅
- Test 5 (buffered): 103.5% of baseline ✅

**O_DIRECT Async Tests (regression):**
- Test 2 (io_uring): 53.9% of baseline ❌ (46% slower)
- Test 3 (libaio): 79.7% of baseline ⚠️ (20% slower)

**O_DIRECT Read/Mixed Tests (anomaly):**
- Test 7 (read): 5,706% of baseline ❌ (57x faster!)
- Test 8 (mixed 70/30): 302% of baseline ❌ (3x faster)
- Test 9 (mixed 50/50): 191% of baseline ❌ (2x faster)

**Buffered Tests (consistent):**
- Test 4 (mmap): 97.2% of baseline ✅
- Test 5 (sync buffered): 103.5% of baseline ✅

### Pattern Identified

**Writes with O_DIRECT:** Slow (~5K IOPS) - matches baseline  
**Reads with O_DIRECT:** Fast (~300K IOPS) - 57x faster than baseline  
**Mixed with O_DIRECT:** Fast (~10-16K IOPS) - 2-3x faster than baseline

**Hypothesis:** Reads are hitting page cache (not truly O_DIRECT), or baseline was measured differently.

Should I continue validating all 50 tests, or investigate this anomaly first?