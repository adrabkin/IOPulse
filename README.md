# IOPulse

High-performance IO profiling tool written in Rust.

## Overview

IOPulse is a high-performance IO load generation and profiling tool designed to measure storage system performance accurately. The core design principle is to minimize tool overhead so that IO operations are bound by the storage system being tested, not the tool itself.

## Features

- **Multiple IO Engines**: sync (pread/pwrite), io_uring, libaio, mmap
- **Direct IO Support**: O_DIRECT bypasses page cache for true storage performance measurement
- **Flexible Targets**: Files, directories, block devices, network filesystems (NFS, Lustre)
- **Random Distributions**: Uniform, Zipf, Pareto, Gaussian for realistic access patterns
- **File Distribution Modes**: Shared, partitioned, per-worker file access strategies
- **Directory Tree Testing**: Generate and test complex directory structures with layout manifests
- **Think Time**: Fixed, adaptive, or periodic delays to simulate application processing
- **Data Verification**: Write and verify data integrity with multiple patterns
- **Kernel Hints**: fadvise/madvise support for cache optimization
- **CPU/NUMA Affinity**: Pin workers to specific cores or NUMA nodes
- **Distributed Mode**: Coordinate tests across multiple nodes with synchronized start
- **Comprehensive Statistics**: Latency histograms, percentiles, per-worker metrics
- **Multiple Output Formats**: Live stats, JSON, CSV, heatmaps, Prometheus metrics

## Quick Start

```bash
# Build
cargo build --release

# Simple write test (60 seconds, 4 threads, 4K blocks)
iopulse /data/test.dat --file-size 1G --threads 4 --duration 60s --write-percent 100

# Random read with Zipf distribution (hot/cold data pattern)
iopulse /data/test.dat --file-size 10G --threads 8 --duration 120s \
  --read-percent 100 --random --distribution zipf --zipf-theta 1.2

# Mixed workload with io_uring and O_DIRECT
iopulse /data/test.dat --file-size 10G --threads 8 --duration 60s \
  --read-percent 70 --write-percent 30 --random \
  --engine io_uring --queue-depth 32 --direct

# High-throughput streaming with NUMA affinity
iopulse /data/test.dat --file-size 100G --threads 32 --duration 60s \
  --write-percent 100 --block-size 1M --engine mmap --numa-zones 0,1
```

## Platform Support

IOPulse is designed for Linux systems:
- Linux kernel 5.1+ (for io_uring support)
- Rust 1.70+

**Docker Support**: Run on any platform (macOS, Windows, Linux) using Docker. See [README.Docker.md](README.Docker.md) for details.

## Installation

### Native Linux Build (ubuntu)

```bash
#install build-essentials
sudo apt update
sudo apt install build-essential

# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build release binary
cargo build --release

# Binary location: target/release/iopulse
```

### Docker (Any Platform)

```bash
# Build Docker image
docker build -t iopulse:latest .

# Or use the convenience script
./docker-run.sh --help
```

See [README.Docker.md](README.Docker.md) for complete Docker usage instructions.

## IO Engines

| Engine | Description | O_DIRECT | Async |
|--------|-------------|----------|-------|
| sync | pread/pwrite syscalls | Yes | No |
| io_uring | Linux io_uring (5.1+) | Yes | Yes |
| libaio | Linux AIO | Yes | Yes |
| mmap | Memory-mapped IO | No | No |

## Documentation

- **[User Guide](docs/User_Guide.md)** - Complete usage instructions, all CLI options, real-world workload examples
- **[Technical Architecture](docs/Technical_Architecture.md)** - Internal design, hot path optimizations, implementation details
- **[Random Distributions Guide](docs/random_distributions_guide.md)** - Zipf, Pareto, Gaussian distribution usage
- **[Performance Tuning Guide](docs/performance_tuning_guide.md)** - Optimization recommendations and benchmarks
- **[Distributed Mode Specification](docs/DISTRIBUTED_MODE_SPECIFICATION.md)** - Multi-node testing architecture

## Example Workloads

**Database (OLTP):**
```bash
iopulse /data/db.dat --file-size 100G --block-size 16k --threads 32 \
  --read-percent 70 --write-percent 30 --random \
  --distribution zipf --zipf-theta 1.2 \
  --engine io_uring --queue-depth 32 --direct --duration 300s
```

**CDN/Cache:**
```bash
iopulse /data/cache.dat --file-size 1T --block-size 64k --threads 32 \
  --read-percent 95 --write-percent 5 --random \
  --distribution zipf --zipf-theta 1.5 \
  --engine io_uring --queue-depth 64 --duration 600s
```

**Metadata Benchmark:**
```bash
iopulse /data/tree --file-size 4k --dir-depth 3 --dir-width 10 \
  --total-files 100000 --threads 16 \
  --file-distribution partitioned --duration 300s --write-percent 100
```

## Output Options

```bash
# JSON output with time-series
iopulse test.dat --file-size 1G --duration 60s --write-percent 100 \
  --json-output results.json

# CSV output
iopulse test.dat --file-size 1G --duration 60s --write-percent 100 \
  --csv-output results.csv

# Heatmap (block access distribution)
iopulse test.dat --file-size 1G --duration 60s --write-percent 100 \
  --random --heatmap

# Prometheus metrics - Future Enhancemen
iopulse test.dat --file-size 1G --duration 60s --write-percent 100 \
  --prometheus --prometheus-port 9090
```

## License

MIT License - see [LICENSE](LICENSE) for details.
