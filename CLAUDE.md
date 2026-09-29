# IOPulse

## Regression Testing (MANDATORY)

After any change to iopulse source code (Rust code in `src/`), you MUST run the
full regression test and performance check workflow before considering the work done.

### Three Validation Layers

| Layer | Script | What it validates |
|-------|--------|-------------------|
| **1. Direct I/O Regression** | `run_direct_io_tests.sh` | 39 O_DIRECT tests pass/fail + absolute baseline comparison (±10%) |
| **2. Competitive Comparison** | `run_buffered_io_tests.sh` | 19 three-way tests (iopulse vs fio vs elbencho) — iopulse must be ≥90% of both tools |
| **3. Baseline Check** | `check_performance.sh` | Detects regressions/improvements vs recorded baselines |

The comparison suite covers **both** Direct I/O and Buffered I/O workloads:
- **Direct I/O (10 tests)**: core engines, workloads, queue depths, block sizes
- **Buffered I/O (9 tests)**: engines, auto-fill, NUMA, layout

### Full Workflow

```bash
# 1. Build the release binary
cargo build --release

# 2. Run direct I/O regression tests (requires sudo for cache drops)
sudo bash tests/regression/run_direct_io_tests.sh

# 3. Check direct I/O against baselines
bash tests/regression/check_performance.sh <direct_results_dir>

# 4. Run competitive comparison (direct + buffered vs fio + elbencho)
sudo bash tests/regression/run_buffered_io_tests.sh

# 5. Check competitive ratios
bash tests/regression/check_buffered.sh <buffered_results_dir>
```

Or run everything via the umbrella:
```bash
sudo bash tests/regression/run_all_tests.sh
```

### What to do with results

- **All pass**: Commit and push.
- **Direct I/O improvements**: Run check_performance.sh with `--update-baselines`.
- **Direct I/O regressions**: Investigate and fix. Do NOT update baselines to hide regressions.
- **Competitive regression** (iopulse < 90% of fio or elbencho): This is a tuning issue.
  File a bead and fix the code path — iopulse should match or beat industry tools on
  every workload. Do NOT weaken the threshold.
- **Hardware mismatch (exit 2)**: Run `init_baseline.py` with a clean run's
  results to establish baselines for the new hardware.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed |
| 1 | Regressions detected (baseline or competitive) |
| 2 | Hardware mismatch or missing tool (fio/elbencho not installed) |

### Prerequisites

- `fio` and `elbencho` must be installed (comparison suite requires both)
- `sudo` access for page cache drops between tests
- Release binary built: `cargo build --release`

### What counts as "source code changes"

Run regression tests after modifying anything in:
- `src/` (Rust source)
- `Cargo.toml` (dependency changes)

Do NOT need to run for changes to:
- `tests/regression/` (test scripts themselves)
- `docs/`, `README.md`, comments-only changes
