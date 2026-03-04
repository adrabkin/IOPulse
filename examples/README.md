# IOPulse Configuration Examples

This directory contains example TOML configuration files for IOPulse.

## Basic Configuration

`basic_config.toml` - A simple single-phase configuration demonstrating:
- 70/30 read/write mix
- Composite IO patterns with different block sizes
- Zipf distribution for hot/cold data access
- 4 worker threads
- 60 second duration

Usage:
```bash
iopulse -c examples/basic_config.toml
```

## Multi-Phase Configuration

`multi_phase_config.toml` - A complex multi-phase test demonstrating:
- Phase 1: Sequential write warmup (30s)
- Phase 2: Random read with Zipf distribution (5m)
- Phase 3: Mixed workload with Pareto distribution (10m)
- Stonewall synchronization between phases
- JSON and CSV output

Usage:
```bash
iopulse -c examples/multi_phase_config.toml
```

## Configuration Structure

### Single-Phase Configuration

```toml
[workload]
read_percent = 70
write_percent = 30
queue_depth = 32

[workload.completion_mode]
mode = "duration"  # or "total_bytes" or "run_until_complete"
seconds = 60

[workload.distribution]
type = "zipf"  # or "uniform", "pareto", "gaussian"
theta = 1.2

[[targets]]
path = "/path/to/target"
file_size = 1073741824

[workers]
threads = 4

[output]
show_latency = true

[runtime]
continue_on_error = false
```

### Multi-Phase Configuration

```toml
[[targets]]
path = "/path/to/target"

[workers]
threads = 8

[[phases]]
name = "phase1"

[phases.workload]
read_percent = 100
write_percent = 0

[phases.workload.completion_mode]
mode = "duration"
seconds = 60

[[phases]]
name = "phase2"
# ... next phase configuration
```

## CLI Override

CLI arguments always take precedence over configuration file values:

```bash
# Override threads and duration from config file
iopulse -c examples/basic_config.toml --threads 8 --duration 120s

# Override distribution
iopulse -c examples/basic_config.toml --distribution pareto --pareto-h 0.9

# Override output options
iopulse -c examples/basic_config.toml --json-output results.json --prometheus
```

## Validation

Use `--dry-run` to validate configuration without executing:

```bash
iopulse -c examples/basic_config.toml --dry-run
```
