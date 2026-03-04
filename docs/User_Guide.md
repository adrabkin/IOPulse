# IOPulse User Guide

**Version:** 0.1.0  
**Date:** January 28, 2026

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Installation](#installation)
3. [Basic Usage](#basic-usage)
4. [IO Engines](#io-engines)
5. [Direct IO](#direct-io)
6. [Access Patterns](#access-patterns)
7. [Random Distributions](#random-distributions)
8. [File Distribution Modes](#file-distribution-modes)
9. [Directory Tree Testing](#directory-tree-testing)
10. [Think Time](#think-time)
11. [Data Verification](#data-verification)
12. [Output Options](#output-options)
13. [CPU and NUMA Affinity](#cpu-and-numa-affinity)
14. [Distributed Mode](#distributed-mode)
15. [Real-World Workload Examples](#real-world-workload-examples)
16. [CLI Reference](#cli-reference)

---

## Quick Start

Run a 60-second random write test with 4 threads:

```bash
iopulse /data/test.dat \
  --file-size 1G \
  --block-size 4k \
  --threads 4 \
  --duration 60s \
  --write-percent 100 \
  --random
```

Run a mixed read/write test with io_uring:

```bash
iopulse /data/test.dat \
  --file-size 10G \
  --block-size 8k \
  --threads 8 \
  --duration 120s \
  --read-percent 70 \
  --write-percent 30 \
  --random \
  --engine io_uring \
  --queue-depth 32
```

---

## Installation

Build from source:

```bash
cargo build --release
```

The binary is located at `target/release/iopulse`.

---

## Basic Usage

### Target Path

IOPulse accepts a file, directory, or block device as the target:

```bash
# Single file
iopulse /data/test.dat --file-size 1G --duration 60s --write-percent 100

# Directory (creates files within)
iopulse /data/testdir --file-size 100M --num-files 10 --duration 60s --write-percent 100

# Block device (requires appropriate permissions)
iopulse /dev/nvme0n1 --duration 60s --read-percent 100
```

### Completion Modes

IOPulse supports three completion modes (exactly one required):

**Duration-based:**
```bash
iopulse test.dat --file-size 1G --duration 60s --write-percent 100
iopulse test.dat --file-size 1G --duration 5m --write-percent 100
iopulse test.dat --file-size 1G --duration 1h --write-percent 100
```

**Byte-based:**
```bash
iopulse test.dat --file-size 1G --total-bytes 10G --write-percent 100
```

**Run until complete:**
```bash
iopulse test.dat --file-size 1G --run-until-complete --write-percent 100
```

### Block Size

Specify block size with suffixes (k, M, G). Default is 4k if not specified:

```bash
iopulse test.dat --file-size 1G --duration 60s --write-percent 100              # Uses default 4k
iopulse test.dat --file-size 1G --block-size 4k --duration 60s --write-percent 100
iopulse test.dat --file-size 1G --block-size 64k --duration 60s --write-percent 100
iopulse test.dat --file-size 1G --block-size 1M --duration 60s --write-percent 100
```

### Thread Count

```bash
iopulse test.dat --file-size 1G --threads 1 --duration 60s --write-percent 100   # Single-threaded
iopulse test.dat --file-size 1G --threads 16 --duration 60s --write-percent 100  # 16 threads
iopulse test.dat --file-size 1G --threads 128 --duration 60s --write-percent 100 # High concurrency
```

### Read/Write Mix

```bash
# Write-only
iopulse test.dat --file-size 1G --write-percent 100 --duration 60s

# Read-only
iopulse test.dat --file-size 1G --read-percent 100 --duration 60s

# Mixed (must sum to 100)
iopulse test.dat --file-size 1G --read-percent 70 --write-percent 30 --duration 60s
```

---

## IO Engines

IOPulse supports four IO engines, each with different characteristics.

### sync (Default)

Uses pread/pwrite system calls. Works on all systems.

```bash
iopulse test.dat --file-size 1G --engine sync --duration 60s --write-percent 100
```

Characteristics:
- Synchronous operations (one IO at a time per thread)
- Works with O_DIRECT
- Reliable baseline for comparison

### io_uring

Uses Linux io_uring interface (Linux 5.1+). Supports asynchronous IO with queue depth.

```bash
iopulse test.dat --file-size 1G --engine io_uring --queue-depth 32 --duration 60s --write-percent 100
```

Characteristics:
- Asynchronous operations
- Supports queue depth 1-1024
- Works with O_DIRECT
- Highest performance for storage-bound workloads

### libaio

Uses Linux AIO interface. Supports asynchronous IO with queue depth.

```bash
iopulse test.dat --file-size 1G --engine libaio --queue-depth 32 --duration 60s --write-percent 100
```

Characteristics:
- Asynchronous operations
- Supports queue depth 1-1024
- Works with O_DIRECT
- Requires libaio library

### mmap

Uses memory-mapped IO.

```bash
iopulse test.dat --file-size 1G --engine mmap --duration 60s --write-percent 100
```

Characteristics:
- Memory-mapped file access
- Highest IOPS for buffered workloads
- Does not support O_DIRECT
- File must have content (IOPulse auto-fills empty files)

---

## Direct IO

O_DIRECT bypasses the page cache, measuring true storage performance.

```bash
iopulse test.dat --file-size 1G --direct --duration 60s --write-percent 100
```

Requirements:
- Block size must be aligned (typically 512 bytes or 4K)
- File must exist (IOPulse handles this automatically)
- Not compatible with mmap engine

Use O_DIRECT when:
- Measuring actual storage device performance
- Testing without page cache effects
- Validating that IO operations reach storage

Use buffered IO when:
- Testing application-level performance
- Measuring page cache effectiveness
- Maximum IOPS testing

### O_SYNC

O_SYNC ensures data is written to storage before the write call returns:

```bash
iopulse test.dat --file-size 1G --sync --duration 60s --write-percent 100
```

Can be combined with O_DIRECT:

```bash
iopulse test.dat --file-size 1G --direct --sync --duration 60s --write-percent 100
```

---

## Access Patterns

### Sequential Access

Default behavior (no flags needed):

```bash
iopulse test.dat --file-size 1G --duration 60s --write-percent 100
```

### Random Access

```bash
iopulse test.dat --file-size 1G --random --duration 60s --write-percent 100
```

### fadvise Hints

Provide hints to the kernel about access patterns:

```bash
# Sequential access hint
iopulse test.dat --file-size 1G --fadvise seq --duration 60s --read-percent 100

# Random access hint
iopulse test.dat --file-size 1G --fadvise rand --duration 60s --read-percent 100

# Pre-fetch data
iopulse test.dat --file-size 1G --fadvise willneed --duration 60s --read-percent 100

# Drop from cache after use
iopulse test.dat --file-size 1G --fadvise dontneed --duration 60s --read-percent 100

# Multiple hints
iopulse test.dat --file-size 1G --fadvise rand,noreuse --duration 60s --read-percent 100
```

### madvise Hints (mmap engine)

```bash
# Sequential hint
iopulse test.dat --file-size 1G --engine mmap --madvise seq --duration 60s --read-percent 100

# Random hint
iopulse test.dat --file-size 1G --engine mmap --madvise rand --duration 60s --read-percent 100

# Use huge pages
iopulse test.dat --file-size 1G --engine mmap --madvise hugepage --duration 60s --read-percent 100
```

---

## Random Distributions

When using `--random`, IOPulse supports four distribution types to simulate real-world access patterns.

### Uniform Distribution (Default)

Every block has equal probability of access:

```bash
iopulse test.dat --file-size 1G --random --distribution uniform --duration 60s --write-percent 100
```

Use for: Baseline testing, stress testing, initial file fill.

### Zipf Distribution

Power law distribution where a small percentage of blocks receive most accesses:

```bash
# Default theta=1.2 (97.72% of operations hit first 20% of file)
iopulse test.dat --file-size 1G --random --distribution zipf --duration 60s --read-percent 100

# Higher theta = more concentrated (theta=2.5: 100% hit first 1%)
iopulse test.dat --file-size 1G --random --distribution zipf --zipf-theta 2.5 --duration 60s --read-percent 100
```

Use for: Database indexes, web caches, CDN content, hot/cold data patterns.

### Pareto Distribution

Implements the 80/20 rule (80% of operations access 20% of data):

```bash
# Default h=0.9 (78.76% of operations hit first 20% of file)
iopulse test.dat --file-size 1G --random --distribution pareto --duration 60s --read-percent 100

# Higher h = more skewed
iopulse test.dat --file-size 1G --random --distribution pareto --pareto-h 2.0 --duration 60s --read-percent 100
```

Use for: Business analytics, customer data, product inventory access.

### Gaussian Distribution

Bell curve distribution centered at a configurable point:

```bash
# Center at middle of file, moderate spread
iopulse test.dat --file-size 1G --random --distribution gaussian \
  --gaussian-stddev 0.1 --gaussian-center 0.5 --duration 60s --read-percent 100

# Center at end of file (recent data access)
iopulse test.dat --file-size 1G --random --distribution gaussian \
  --gaussian-stddev 0.05 --gaussian-center 0.95 --duration 60s --read-percent 100
```

Parameters:
- `--gaussian-stddev`: Spread (0.05 = tight, 0.2 = loose)
- `--gaussian-center`: Center point (0.0 = start, 0.5 = middle, 1.0 = end)

Use for: Log file tail access, time-series data, spatial locality patterns.

### Visualizing Distributions

Use `--heatmap` to see the actual access distribution:

```bash
iopulse test.dat --file-size 1G --random --distribution zipf --zipf-theta 1.2 \
  --duration 10s --read-percent 100 --heatmap
```

---

## File Distribution Modes

Control how workers access files.

### Shared (Default)

All workers access all files:

```bash
iopulse test.dat --file-size 1G --threads 4 --file-distribution shared --duration 60s --write-percent 100
```

Use for: Testing concurrent access, lock contention, shared file workloads.

**⚠️ Write Conflict Detection:** When using shared distribution with random writes and multiple workers, IOPulse will detect potential data corruption scenarios and require explicit handling. See [Write Conflict Detection](#write-conflict-detection) below.

### Partitioned

Files/regions divided among workers (no overlap):

```bash
iopulse test.dat --file-size 1G --threads 4 --file-distribution partitioned --duration 60s --write-percent 100
```

With 4 threads on a 1GB file:
- Thread 0: offsets 0-256MB
- Thread 1: offsets 256MB-512MB
- Thread 2: offsets 512MB-768MB
- Thread 3: offsets 768MB-1GB

Use for: Maximum aggregate bandwidth, parallel IO without conflicts, HPC workloads.

### Per-Worker

Each worker creates and uses its own file:

```bash
iopulse test.dat --file-size 1G --threads 4 --file-distribution per-worker --duration 60s --write-percent 100
```

Creates:
- test.dat.worker0
- test.dat.worker1
- test.dat.worker2
- test.dat.worker3

Use for: Testing aggregate creation rate, per-file performance, isolated workloads.

### Write Conflict Detection

IOPulse automatically detects configurations that may cause data corruption when multiple workers write to shared files without coordination.

**Risky Configuration (will error):**
```bash
# This will ERROR - multiple workers writing random offsets to shared file
iopulse /data/test --file-size 1G --threads 8 \
  --write-percent 100 --random --file-distribution shared --duration 60s
```

**Error Message:**
```
⚠️  WARNING: Potential write conflicts detected!

Configuration:
  - File distribution: shared (all workers access same files)
  - Write operations: 100%
  - Access pattern: random
  - Locking: none
  - Workers: 8

This configuration may cause data corruption because multiple workers
can write to the same file offsets simultaneously without coordination.

Options to resolve:

  1. Add --lock-mode range
     Tests lock contention (realistic but slower)

  2. Use --file-distribution partitioned
     Each worker gets exclusive files (no conflicts, faster)

  3. Add --allow-write-conflicts
     Benchmark mode: measure raw performance, accept data corruption
```

**Safe Options:**

**Option 1: Add file locking** (tests lock contention)
```bash
iopulse /data/test --file-size 1G --threads 8 \
  --write-percent 100 --random --file-distribution shared \
  --lock-mode range --duration 60s
```

**Option 2: Use partitioned distribution** (no conflicts)
```bash
iopulse /data/test --file-size 1G --threads 8 \
  --write-percent 100 --random --file-distribution partitioned --duration 60s
```

**Option 3: Benchmark mode** (explicitly allow conflicts)
```bash
iopulse /data/test --file-size 1G --threads 8 \
  --write-percent 100 --random --file-distribution shared \
  --allow-write-conflicts --duration 60s
```

**When Detection Triggers:**

The validation triggers when ALL of these conditions are met:
- File distribution is `shared`
- Write percentage > 0
- Access pattern is `random`
- No file locking enabled
- Multiple workers (threads > 1)

**Safe Scenarios (no warning):**
- Read-only workloads (`--read-percent 100`)
- Sequential writes (no `--random` flag)
- Single worker (`--threads 1`)
- Locking enabled (`--lock-mode range` or `--lock-mode full`)
- Partitioned or per-worker distribution

**Applies to All Target Types:**
- Single files
- Multiple files (`--num-files`, `--num-dirs`)
- Directory layouts (`--dir-depth`, `--dir-width`)
- With or without O_DIRECT (`--direct`)
- Single-node and multi-node distributed mode

---

## Directory Tree Testing

Test with multiple files organized in directory structures.

### Basic Multi-File Testing

```bash
# 10 files in a directory
iopulse /data/testdir --file-size 100M --num-files 10 --duration 60s --write-percent 100

# 10 files across 5 directories
iopulse /data/testdir --file-size 100M --num-files 10 --num-dirs 5 --duration 60s --write-percent 100
```

### Directory Tree Structure

Create nested directory trees:

```bash
# 3 levels deep, 10 subdirectories per level, 1000 total files
iopulse /data/tree --file-size 1M --dir-depth 3 --dir-width 10 --total-files 1000 \
  --duration 60s --write-percent 100
```

### Layout Manifest

Save and reuse directory structures:

**Export a layout:**
```bash
iopulse /data/tree --dir-depth 3 --dir-width 10 --total-files 100000 \
  --export-layout-manifest tree_100k.layout_manifest --run-until-complete --write-percent 100
```

**Reuse a layout:**
```bash
iopulse /data/tree --layout-manifest tree_100k.layout_manifest --duration 60s --read-percent 100
```

Benefits:
- Skip tree generation on subsequent runs
- Reproducible testing with exact same structure
- Share layouts across team members

---

## Think Time

Simulate application processing time between IO operations.

### Fixed Think Time

```bash
# 100 microseconds between IOs
iopulse test.dat --file-size 1G --think-time 100us --duration 60s --write-percent 100

# 1 millisecond between IOs
iopulse test.dat --file-size 1G --think-time 1ms --duration 60s --write-percent 100
```

### Think Time Mode

**Sleep mode (default):** Yields CPU during think time:
```bash
iopulse test.dat --file-size 1G --think-time 100us --think-mode sleep --duration 60s --write-percent 100
```

**Spin mode:** Busy-waits (more precise timing, uses CPU):
```bash
iopulse test.dat --file-size 1G --think-time 100us --think-mode spin --duration 60s --write-percent 100
```

### Think Every N Blocks

Apply think time every N operations:

```bash
# Think time every 10 blocks
iopulse test.dat --file-size 1G --think-time 1ms --think-every 10 --duration 60s --write-percent 100
```

### Adaptive Think Time

Scale think time based on IO latency:

```bash
# Add 50% of IO latency as think time
iopulse test.dat --file-size 1G --think-adaptive-percent 50 --duration 60s --write-percent 100

# Base think time + adaptive
iopulse test.dat --file-size 1G --think-time 50us --think-adaptive-percent 25 --duration 60s --write-percent 100
```

---

## Data Verification

Verify data integrity by writing and reading known patterns.

### Write with Pattern

```bash
# Write zeros
iopulse test.dat --file-size 1G --verify --verify-pattern zeros --write-percent 100 --run-until-complete

# Write random (deterministic based on offset)
iopulse test.dat --file-size 1G --verify --verify-pattern random --write-percent 100 --run-until-complete
```

### Read and Verify

```bash
# Verify zeros
iopulse test.dat --file-size 1G --verify --verify-pattern zeros --read-percent 100 --run-until-complete

# Verify random
iopulse test.dat --file-size 1G --verify --verify-pattern random --read-percent 100 --run-until-complete
```

### Available Patterns

- `zeros`: All zero bytes
- `ones`: All 0xFF bytes
- `sequential`: Sequential bytes (0x00, 0x01, ..., 0xFF, 0x00, ...)
- `random`: Deterministic random based on offset (reproducible)

### Write Buffer Pattern

Control the pattern used for write operations (separate from verification):

```bash
# Random data (default, defeats deduplication)
iopulse test.dat --file-size 1G --write-pattern random --write-percent 100 --duration 60s

# Zeros (compressible, dedup-friendly)
iopulse test.dat --file-size 1G --write-pattern zeros --write-percent 100 --duration 60s
```

---

## Output Options

### Live Statistics

**Enable live stats (default):**
```bash
iopulse test.dat --file-size 1G --duration 60s --write-percent 100
```

**Custom interval:**
```bash
iopulse test.dat --file-size 1G --live-interval 500ms --duration 60s --write-percent 100
```

**Disable live stats:**
```bash
iopulse test.dat --file-size 1G --no-live --duration 60s --write-percent 100
```

### Latency Statistics

```bash
# Show latency summary
iopulse test.dat --file-size 1G --show-latency --duration 60s --write-percent 100

# Show latency histogram
iopulse test.dat --file-size 1G --show-histogram --duration 60s --write-percent 100

# Show percentiles
iopulse test.dat --file-size 1G --show-percentiles --duration 60s --write-percent 100
```

### JSON Output

```bash
# Output to file
iopulse test.dat --file-size 1G --json-output results.json --duration 60s --write-percent 100

# Output to directory (creates timestamped files)
iopulse test.dat --file-size 1G --json-output /data/results/ --duration 60s --write-percent 100

# Custom aggregate name
iopulse test.dat --file-size 1G --json-output /data/results/ --json-name my_test --duration 60s --write-percent 100

# Include full histogram (112 buckets)
iopulse test.dat --file-size 1G --json-output results.json --json-histogram --duration 60s --write-percent 100

# Include per-worker stats
iopulse test.dat --file-size 1G --json-output results.json --per-worker-output --duration 60s --write-percent 100

# Custom time-series interval
iopulse test.dat --file-size 1G --json-output results.json --json-interval 500ms --duration 60s --write-percent 100
```

### CSV Output

```bash
iopulse test.dat --file-size 1G --csv-output results.csv --duration 60s --write-percent 100
```

### Heatmap Output

Visualize block access distribution:

```bash
# Enable heatmap (100 buckets default)
iopulse test.dat --file-size 1G --heatmap --duration 60s --write-percent 100 --random

# Custom bucket count
iopulse test.dat --file-size 1G --heatmap --heatmap-buckets 50 --duration 60s --write-percent 100 --random
```

Note: Heatmap adds 5-10% overhead. Use for analysis, not peak performance testing.

### Prometheus Metrics

```bash
# Enable Prometheus endpoint on default port 9090
iopulse test.dat --file-size 1G --prometheus --duration 60s --write-percent 100

# Custom port
iopulse test.dat --file-size 1G --prometheus --prometheus-port 9091 --duration 60s --write-percent 100
```

---

## CPU and NUMA Affinity

### CPU Core Affinity

Pin workers to specific CPU cores:

```bash
# Pin to cores 0-3
iopulse test.dat --file-size 1G --threads 4 --cpu-cores 0,1,2,3 --duration 60s --write-percent 100

# Pin to core range
iopulse test.dat --file-size 1G --threads 8 --cpu-cores 0-7 --duration 60s --write-percent 100
```

### NUMA Zone Affinity

Pin workers to specific NUMA nodes:

```bash
# Pin to NUMA node 0
iopulse test.dat --file-size 1G --threads 16 --numa-zones 0 --duration 60s --write-percent 100

# Pin to both NUMA nodes
iopulse test.dat --file-size 1G --threads 32 --numa-zones 0,1 --duration 60s --write-percent 100
```

NUMA affinity provides significant performance gains (50-120%) for:
- Large block sizes (1M+)
- mmap engine
- High thread counts (32+)

---

## Distributed Mode

Run coordinated tests across multiple nodes.

### Architecture

```
Coordinator (control node)
    │
    ├── Node 1 (worker service) ── 16 worker threads
    ├── Node 2 (worker service) ── 16 worker threads
    └── Node 3 (worker service) ── 16 worker threads
                                   ─────────────────
                                   48 total workers
```

### Start Worker Services

On each worker node:

```bash
iopulse --mode service --listen-port 9999
```

### Run Coordinator

```bash
# Using host list
iopulse --mode coordinator \
  --host-list 10.0.1.10:9999,10.0.1.11:9999,10.0.1.12:9999 \
  /mnt/nfs/test.dat \
  --file-size 100G \
  --threads 16 \
  --duration 60s \
  --write-percent 100

# Using clients file
iopulse --mode coordinator \
  --clients-file clients.txt \
  /mnt/nfs/test.dat \
  --file-size 100G \
  --threads 16 \
  --duration 60s \
  --write-percent 100
```

clients.txt format:
```
10.0.1.10:9999
10.0.1.11:9999
10.0.1.12:9999
```

### Distributed File Distribution

```bash
# All workers access all files
iopulse --mode coordinator --host-list ... \
  --file-distribution shared ...

# Each file touched by exactly one worker
iopulse --mode coordinator --host-list ... \
  --file-distribution partitioned ...

# Each worker creates own files
iopulse --mode coordinator --host-list ... \
  --file-distribution per-worker ...
```

---

## Real-World Workload Examples

The following examples demonstrate how to simulate specific real-world workloads using IOPulse's features.

### MySQL/PostgreSQL OLTP Database

Characteristics:
- Random access with hot data (Zipf distribution)
- 70/30 read/write ratio
- 8K or 16K block size (database page)
- Query processing time between IOs

```bash
iopulse /data/db_test.dat \
  --file-size 100G \
  --block-size 16k \
  --threads 32 \
  --duration 300s \
  --read-percent 70 \
  --write-percent 30 \
  --random \
  --distribution zipf \
  --zipf-theta 1.2 \
  --engine io_uring \
  --queue-depth 32 \
  --think-time 50us \
  --direct \
  --numa-zones 0,1
```

### Redis/Memcached Cache

Characteristics:
- Extreme hot key access (high Zipf theta)
- Read-heavy (90/10)
- Small block size (4K)
- High concurrency

```bash
iopulse /data/cache_test.dat \
  --file-size 10G \
  --block-size 4k \
  --threads 64 \
  --duration 300s \
  --read-percent 90 \
  --write-percent 10 \
  --random \
  --distribution zipf \
  --zipf-theta 2.0 \
  --engine io_uring \
  --queue-depth 128
```

### CDN/Object Storage

Characteristics:
- Popular content access (Zipf)
- Read-heavy (95/5)
- Medium block size (64K)
- High throughput

```bash
iopulse /data/cdn_test.dat \
  --file-size 1T \
  --block-size 64k \
  --threads 32 \
  --duration 600s \
  --read-percent 95 \
  --write-percent 5 \
  --random \
  --distribution zipf \
  --zipf-theta 1.5 \
  --engine io_uring \
  --queue-depth 64
```

### Log File Analysis

Characteristics:
- Access concentrated at end of file (recent logs)
- Read-only
- Sequential-ish with variation (Gaussian)

```bash
iopulse /data/logs_test.dat \
  --file-size 50G \
  --block-size 4k \
  --threads 4 \
  --duration 300s \
  --read-percent 100 \
  --random \
  --distribution gaussian \
  --gaussian-stddev 0.05 \
  --gaussian-center 0.95
```

### Business Analytics (80/20 Rule)

Characteristics:
- 80% of queries hit 20% of data (Pareto)
- Read-heavy
- Medium block size

```bash
iopulse /data/analytics_test.dat \
  --file-size 100G \
  --block-size 8k \
  --threads 16 \
  --duration 600s \
  --read-percent 85 \
  --write-percent 15 \
  --random \
  --distribution pareto \
  --pareto-h 0.9 \
  --engine io_uring \
  --queue-depth 32
```

### High-Throughput Streaming

Characteristics:
- Large block sizes
- Sequential or random
- Maximum bandwidth

```bash
iopulse /data/stream_test.dat \
  --file-size 100G \
  --block-size 1M \
  --threads 32 \
  --duration 60s \
  --write-percent 100 \
  --random \
  --engine mmap \
  --numa-zones 0,1
```

### HPC Parallel IO (MPI-IO Style)

Characteristics:
- Partitioned access (no overlap)
- Large blocks
- High thread count

```bash
iopulse /data/hpc_test.dat \
  --file-size 500G \
  --block-size 1M \
  --threads 128 \
  --duration 300s \
  --write-percent 100 \
  --file-distribution partitioned \
  --engine io_uring \
  --queue-depth 32 \
  --direct
```

### Metadata-Intensive Workload

Characteristics:
- Many small files
- Directory tree structure
- Create/stat/delete operations

```bash
iopulse /data/metadata_test \
  --file-size 4k \
  --dir-depth 3 \
  --dir-width 10 \
  --total-files 100000 \
  --threads 16 \
  --duration 300s \
  --write-percent 100 \
  --file-distribution partitioned
```

### Data Integrity Validation

Write data with verification pattern, then verify:

```bash
# Step 1: Write with pattern
iopulse /data/integrity_test.dat \
  --file-size 10G \
  --block-size 4k \
  --threads 4 \
  --write-percent 100 \
  --random \
  --verify \
  --verify-pattern random \
  --run-until-complete

# Step 2: Read and verify
iopulse /data/integrity_test.dat \
  --file-size 10G \
  --block-size 4k \
  --threads 4 \
  --read-percent 100 \
  --random \
  --verify \
  --verify-pattern random \
  --run-until-complete
```

### Distributed Metadata Benchmark

Test metadata operations across multiple nodes:

```bash
# Start services on nodes
# Node 1: iopulse --mode service --listen-port 9999
# Node 2: iopulse --mode service --listen-port 9999
# Node 3: iopulse --mode service --listen-port 9999

# Run coordinator
iopulse --mode coordinator \
  --host-list 10.0.1.10:9999,10.0.1.11:9999,10.0.1.12:9999 \
  /mnt/nfs/tree \
  --dir-depth 3 \
  --dir-width 10 \
  --total-files 1000000 \
  --threads 16 \
  --duration 300s \
  --write-percent 100 \
  --file-distribution partitioned
```

---

## CLI Reference

### Execution Mode Options

| Option | Description | Default |
|--------|-------------|---------|
| `--mode` | Execution mode: standalone, coordinator, service | standalone |
| `--listen-port` | Port for service mode | 9999 |
| `--host-list` | Comma-separated node addresses for coordinator | - |
| `--clients-file` | File with node addresses (one per line) | - |
| `--worker-port` | Port to connect to on worker nodes | 9999 |

### Basic Options

| Option | Description | Default |
|--------|-------------|---------|
| `-t, --threads` | Number of worker threads | 1 |
| `-b, --block-size` | Block size (e.g., 4k, 1M) | 4k |
| `-s, --file-size` | File size (e.g., 1G, 100M) | - |
| `-d, --duration` | Test duration (e.g., 60s, 5m) | - |
| `--total-bytes` | Total bytes to transfer | - |
| `--run-until-complete` | Run until all operations complete | false |

### Workload Options

| Option | Description | Default |
|--------|-------------|---------|
| `--random` | Use random offsets | false |
| `--read-percent` | Read percentage (0-100) | - |
| `--write-percent` | Write percentage (0-100) | - |
| `-q, --queue-depth` | IO queue depth (1-1024) | 1 |
| `--write-pattern` | Write buffer pattern: zeros, ones, random, sequential | random |

### Distribution Options

| Option | Description | Default |
|--------|-------------|---------|
| `--distribution` | Distribution type: uniform, zipf, pareto, gaussian | uniform |
| `--zipf-theta` | Zipf theta parameter (0.0-3.0) | 1.2 |
| `--pareto-h` | Pareto h parameter (0.0-10.0) | 0.9 |
| `--gaussian-stddev` | Gaussian standard deviation | - |
| `--gaussian-center` | Gaussian center point (0.0-1.0) | 0.5 |

### Think Time Options

| Option | Description | Default |
|--------|-------------|---------|
| `--think-time` | Think time between IOs (e.g., 100us, 1ms) | - |
| `--think-mode` | Think time mode: sleep, spin | sleep |
| `--think-every` | Apply think time every N blocks | 1 |
| `--think-adaptive-percent` | Adaptive think time as % of IO latency | - |

### IO Engine Options

| Option | Description | Default |
|--------|-------------|---------|
| `--engine` | IO engine: sync, io_uring, libaio, mmap | sync |
| `--direct` | Use O_DIRECT (bypass page cache) | false |
| `--sync` | Use O_SYNC | false |
| `--fadvise` | fadvise hints: seq, rand, willneed, dontneed, noreuse | - |
| `--madvise` | madvise hints: seq, rand, willneed, dontneed, hugepage, nohugepage | - |

### File Distribution Options

| Option | Description | Default |
|--------|-------------|---------|
| `--file-distribution` | Distribution strategy: shared, partitioned, per-worker | shared |
| `-n, --num-files` | Number of files per directory | - |
| `-N, --num-dirs` | Number of directories | - |
| `--dir-depth` | Directory tree depth | - |
| `--dir-width` | Subdirectories per level | - |
| `--total-files` | Total files to generate | - |
| `--layout-manifest` | Input layout manifest file | - |
| `--export-layout-manifest` | Output layout manifest file | - |
| `--lock-mode` | File locking: none, range, full | none |

### Target Options

| Option | Description | Default |
|--------|-------------|---------|
| `--preallocate` | Pre-allocate file space | false |
| `--truncate-to-size` | Truncate files to size on creation | false |
| `--refill` | Fill pre-allocated files with pattern data | false |
| `--refill-pattern` | Pattern for refill: zeros, ones, random, sequential | random |
| `--no-refill` | Disable automatic file filling for read tests | false |

### Output Options

| Option | Description | Default |
|--------|-------------|---------|
| `--json-output` | JSON output file or directory | - |
| `--json-name` | Name for aggregate JSON file | aggregate |
| `--json-histogram` | Generate separate histogram file | false |
| `--per-worker-output` | Include per-worker stats in output | false |
| `--no-aggregate` | Skip aggregate file generation | false |
| `--json-interval` | Polling interval for time-series | 1s |
| `--csv-output` | CSV output file | - |
| `--prometheus` | Enable Prometheus metrics | false |
| `--prometheus-port` | Prometheus port | 9090 |
| `--heatmap` | Enable block access heatmap | false |
| `--heatmap-buckets` | Number of heatmap buckets | 100 |
| `--show-latency` | Show latency statistics | false |
| `--show-histogram` | Show latency histogram | false |
| `--show-percentiles` | Show latency percentiles | false |
| `--live-interval` | Live statistics interval | - |
| `--no-live` | Disable live statistics | false |

### CPU/NUMA Options

| Option | Description | Default |
|--------|-------------|---------|
| `--cpu-cores` | CPU cores to bind workers to | - |
| `--numa-zones` | NUMA zones to bind workers to | - |

### Error Handling Options

| Option | Description | Default |
|--------|-------------|---------|
| `--continue-on-error` | Continue on IO errors | false |
| `--max-errors` | Maximum errors before aborting | - |

### Data Integrity Options

| Option | Description | Default |
|--------|-------------|---------|
| `--verify` | Enable data verification | false |
| `--verify-pattern` | Verification pattern: zeros, ones, random, sequential | - |

### Other Options

| Option | Description | Default |
|--------|-------------|---------|
| `-c, --config` | TOML configuration file | - |
| `--dry-run` | Validate configuration without executing | false |
| `--debug` | Enable debug output | false |

---

## Additional Resources

- [Random Distributions Guide](random_distributions_guide.md) - Detailed distribution documentation
- [Performance Tuning Guide](performance_tuning_guide.md) - Optimization recommendations
- [Technical Architecture](Technical_Architecture.md) - Internal design documentation
- [Distributed Mode Specification](DISTRIBUTED_MODE_SPECIFICATION.md) - Distributed mode details
