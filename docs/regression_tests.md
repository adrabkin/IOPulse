# IOPulse Regression & Competitive Testing Guide

## Prerequisites

```bash
# Install fio
sudo apt install -y fio

# Install elbencho
cd /tmp && curl -sLO "https://github.com/breuner/elbencho/releases/download/v3.0-37/elbencho-static_amd64.deb"
sudo dpkg -i elbencho-static_amd64.deb

# Verify
fio --version       # expect fio-3.x
elbencho --version  # expect 3.0.x
```

## Build

```bash
cd /path/to/iopulse
cargo build --release
```

## Step 1: Direct I/O Regression Tests

Runs 39 O_DIRECT tests (engines, workloads, distributions, queue depths, edge cases).
Requires sudo for page cache drops between tests.

```bash
sudo bash tests/regression/run_direct_io_tests.sh
```

Note the results directory printed at the end (e.g., `direct_run_YYYYMMDD_HHMMSS`).

## Step 2: Check Direct I/O Against Baselines

On a **new machine** (first run), initialize baselines:

```bash
# This will show "hardware mismatch" — expected on new hardware.
# Initialize baselines for this machine:
python3 tests/regression/init_baseline.py /data/iopulse_regression_results/direct_run_YYYYMMDD_HHMMSS
```

Then check performance (should show all pass since baselines match the run):

```bash
bash tests/regression/check_performance.sh /data/iopulse_regression_results/direct_run_YYYYMMDD_HHMMSS
```

On **subsequent runs** (same machine), just check:

```bash
bash tests/regression/check_performance.sh /data/iopulse_regression_results/direct_run_YYYYMMDD_HHMMSS
```

## Step 3: Competitive Comparison (vs fio + elbencho)

Runs 19 three-way comparison tests — iopulse vs fio vs elbencho on equivalent workloads.
Covers both Direct I/O (10 tests) and Buffered I/O (9 tests).

```bash
sudo bash tests/regression/run_buffered_io_tests.sh
```

Note the results directory (e.g., `buffered_run_YYYYMMDD_HHMMSS`).

## Step 4: Check Competitive Ratios

```bash
bash tests/regression/check_buffered.sh /data/iopulse_regression_results/buffered_run_YYYYMMDD_HHMMSS
```

Pass criteria: iopulse must be ≥90% of both fio and elbencho IOPS on every test.

## Step 5: Variability Check (Run Comparison a Second Time)

Run the comparison suite again and compare results to check for run-to-run variability:

```bash
sudo bash tests/regression/run_buffered_io_tests.sh
bash tests/regression/check_buffered.sh /data/iopulse_regression_results/buffered_run_YYYYMMDD_HHMMSS
```

If different tests fail on each run, the variability is system noise, not code regressions.
If the same tests fail consistently, those are real performance gaps to investigate.

## Quick Reference

| Script | Purpose |
|--------|---------|
| `tests/regression/run_direct_io_tests.sh` | 39 O_DIRECT regression tests |
| `tests/regression/run_buffered_io_tests.sh` | 19 competitive comparison tests (direct + buffered) |
| `tests/regression/run_all_tests.sh` | Umbrella: runs both suites |
| `tests/regression/check_performance.sh` | Check direct I/O against baselines (±10%) |
| `tests/regression/check_buffered.sh` | Check competitive ratios (≥90% of fio/elbencho) |
| `tests/regression/init_baseline.py` | Initialize baselines for new hardware |
| `tests/regression/compare_buffered.py` | Three-way IOPS/throughput/latency comparison |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed |
| 1 | Regressions detected |
| 2 | Hardware mismatch or missing tool (fio/elbencho) |

## Interpreting Results

- **All pass**: Code is good. Commit and push.
- **Direct I/O improvements**: Run `check_performance.sh --update-baselines` to record them.
- **Direct I/O regressions**: Investigate and fix. Do NOT update baselines to hide regressions.
- **Competitive regression** (iopulse < 90% of fio or elbencho): Tuning issue — file a bug and fix.
- **Hardware mismatch**: Run `init_baseline.py` to establish baselines for the new machine.
