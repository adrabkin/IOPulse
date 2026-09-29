# IOPulse Technical Architecture

## Overview

IOPulse is a high-performance IO profiling tool written in Rust. This document describes the architectural decisions, implementation details, and design rationale for engineers working with or extending the codebase.

## Core Design Principles

1. **Zero-cost abstractions**: Trait-based polymorphism without runtime overhead
2. **Lock-free hot paths**: Atomic operations and message passing minimize contention
3. **Memory efficiency**: Pre-allocated buffers, no allocations in hot paths
4. **Unified architecture**: Standalone and distributed modes share identical code paths
5. **Fail-safe defaults**: Abort on errors by default, explicit opt-in for continue modes

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         IOPulse CLI                             │
│                    (Argument Parsing)                           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Configuration                              │
│              (TOML Parser + Validator)                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Unified Coordinator Path                        │
│    (Standalone auto-launches localhost service)                 │
└─────┬──────────────────────┬────────────────────┬───────────────┘
      │                      │                    │
      ▼                      ▼                    ▼
┌──────────┐          ┌──────────┐         ┌──────────┐
│ Worker 1 │          │ Worker 2 │   ...   │ Worker N │
└────┬─────┘          └────┬─────┘         └────┬─────┘
     │                     │                     │
     ▼                     ▼                     ▼
┌──────────────────────────────────────────────────────┐
│              IO Engine Abstraction                   │
│  (io_uring | libaio | sync | mmap)                   │
└──────────────────────────────────────────────────────┘
```

## Unified Architecture

IOPulse uses a single code path for both standalone and distributed modes. This eliminates code divergence and ensures consistent behavior.

### Standalone Mode Execution

When running in standalone mode:

1. Main process finds an available port (9999-10099)
2. Spawns a localhost service process (`iopulse --mode service`)
3. Creates `DistributedCoordinator` with `host_list = ["localhost:PORT"]`
4. Executes the same protocol as distributed mode
5. Cleans up service process on completion

This approach provides:
- Single code path for all execution modes
- Consistent statistics collection and output
- Simplified testing and maintenance

### Distributed Mode Execution

Distributed mode uses the same coordinator logic with multiple remote nodes:

```
Coordinator                          Node Service
-----------                          ------------
Connect via TCP ------------------>  Accept connection
Send CONFIG message --------------->  Parse configuration
                                      Spawn N worker threads
                                      Initialize IO engines
Wait for READY <--------------------  Send READY message
Send START message ---------------->  Wait until start_timestamp
                                      BEGIN IO operations
Receive HEARTBEAT <-----------------  Send HEARTBEAT (every 1s)
Send HEARTBEAT_ACK ---------------->  Reset dead man's switch
Send STOP message ----------------->  Signal workers to stop
Wait for RESULTS <------------------  Send RESULTS message
```

## IO Engine Abstraction

The `IOEngine` trait provides a uniform interface for all IO backends:

```rust
pub trait IOEngine: Send {
    fn init(&mut self, config: &EngineConfig) -> Result<()>;
    fn submit(&mut self, op: IOOperation) -> Result<()>;
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>>;
    fn cleanup(&mut self) -> Result<()>;
    fn capabilities(&self) -> EngineCapabilities;
}
```

### Engine Implementations

| Engine | Async | Batch Submit | Max QD | Use Case |
|--------|-------|--------------|--------|----------|
| sync | No | No | 1 | Baseline, always available |
| io_uring | Yes | Yes | 1024+ | Highest performance (Linux 5.1+) |
| libaio | Yes | Yes | 256 | Legacy async (Linux) |
| mmap | No | No | 1 | Memory-mapped workloads |

### Smart Engine Selection

For queue depth 1, IOPulse automatically uses the sync engine regardless of configuration. Async engines have overhead that provides no benefit at QD=1:

```rust
let effective_engine = if workload.queue_depth == 1 {
    match workload.engine {
        EngineType::Libaio | EngineType::IoUring => EngineType::Sync,
        _ => workload.engine,
    }
} else {
    workload.engine
};
```

## Hot Path Optimizations

### 1. Cache-Line Aligned Counters

Statistics counters are aligned to 64-byte cache lines to prevent false sharing between worker threads:

```rust
#[repr(align(64))]
pub struct AlignedCounter {
    value: AtomicU64,
    _padding: [u8; 56],
}
```

Each counter occupies its own cache line, eliminating cache invalidation when multiple threads update different counters.

### 2. Lock-Free Statistics

Basic counters use `Ordering::Relaxed` atomic operations:

```rust
#[inline]
pub fn add(&self, val: u64) {
    self.value.fetch_add(val, Ordering::Relaxed);
}
```

Relaxed ordering is sufficient because:
- No ordering guarantees needed between different counters
- Statistics are aggregated after test completion
- Performance is critical (called for every IO operation)

### 3. Fast Timing

IOPulse uses direct `clock_gettime` calls instead of `std::time::Instant`:

```rust
#[inline(always)]
pub fn now() -> Self {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts); }
    let nanos = (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64);
    Self { nanos }
}
```

Measured overhead: ~15-20ns per call vs ~25-30ns for `std::time::Instant`.

### 4. Pre-Allocated Buffer Pool

Buffers are pre-allocated at initialization to avoid allocations in the hot path:

```rust
pub struct BufferPool {
    buffers: Vec<AlignedBuffer>,
    available: VecDeque<usize>,
    buffer_size: usize,
    alignment: usize,
}
```

Buffer operations are O(1):
- `get()`: Pop from front of deque
- `return_buffer()`: Push to back of deque

For random write patterns, buffers are pre-filled with random data at initialization to avoid regenerating random data for every write.

### 5. Block-Based Offset Generation

Distributions generate block numbers, not byte offsets:

```rust
pub trait Distribution: Send {
    fn next_block(&mut self, num_blocks: u64) -> u64;
}
```

The worker converts to byte offset: `offset = block_num * block_size`

This ensures:
- Offsets are naturally aligned to block size (required for O_DIRECT)
- No runtime alignment overhead
- Single multiplication vs division + multiplication + alignment

### 6. Batched Duration Checking

Duration is checked every N operations to reduce `clock_gettime` overhead:

```rust
const DURATION_CHECK_INTERVAL: usize = 100;
let mut ops_since_duration_check = 0;

loop {
    // ... perform IO ...
    
    ops_since_duration_check += 1;
    if ops_since_duration_check >= DURATION_CHECK_INTERVAL {
        if self.should_stop() { break; }
        ops_since_duration_check = 0;
    }
}
```

At 100K+ IOPS, checking every 100 ops means ~1ms between checks.

### 7. Adaptive Live Stats Updates

Update frequency adapts to workload characteristics:

```rust
let live_stats_update_interval = if matches!(engine, EngineType::Mmap) || !direct {
    1000  // High-IOPS: mmap or buffered (500K-3M IOPS)
} else {
    1     // Low-IOPS: O_DIRECT (<100K IOPS)
};
```

High-IOPS scenarios update every 1000 ops to minimize overhead. Low-IOPS scenarios update every operation for precision.

## Statistics Collection

### SimpleHistogram

IOPulse uses a custom histogram implementation optimized for latency tracking:

```rust
pub struct SimpleHistogram {
    buckets: [u64; 112],  // 28 log2 levels × 4 sub-buckets
    num_samples: u64,
    total_nanos: u64,
    min_nanos: u64,
    max_nanos: u64,
}
```

Properties:
- Fixed 112-bucket array (no dynamic allocation)
- Logarithmic buckets: log2(latency) × 4
- Covers 0 to 2^28 microseconds (~268 seconds)
- O(1) recording and percentile queries
- ~900 bytes per histogram

Recording is the hot path:

```rust
#[inline(always)]
pub fn record(&mut self, latency: Duration) {
    let nanos = latency.as_nanos() as u64;
    self.num_samples += 1;
    self.total_nanos += nanos;
    // ... min/max updates ...
    let bucket_idx = calculate_bucket(nanos);
    self.buckets[bucket_idx] += 1;
}
```

### Per-Worker Statistics

Each worker maintains its own `WorkerStats` instance:

```rust
pub struct WorkerStats {
    read_ops: AlignedCounter,
    write_ops: AlignedCounter,
    read_bytes: AlignedCounter,
    write_bytes: AlignedCounter,
    errors: AlignedCounter,
    io_latency: SimpleHistogram,
    read_latency: SimpleHistogram,
    write_latency: SimpleHistogram,
    pub metadata: MetadataStats,
    // ...
}
```

Statistics are aggregated after test completion by merging histograms and summing counters.

### Time-Series Collection

For JSON/CSV output, statistics are collected every second via heartbeats:

1. Workers update shared snapshots every 1K ops (high-IOPS) or every op (low-IOPS)
2. Node service aggregates worker snapshots every 1 second
3. Coordinator receives cumulative values in heartbeats
4. Coordinator calculates deltas for per-second rates

Delta calculation:
```
Heartbeat 1: read_ops = 1000 (cumulative)
Heartbeat 2: read_ops = 2500 (cumulative)
Delta: 2500 - 1000 = 1500 ops in 1 second
IOPS: 1500 ops/s
```

## Worker Execution Loop

The main execution loop is async-aware, allowing multiple operations in flight:

```rust
loop {
    // Phase 1: Fill queue up to queue_depth
    while in_flight_ops.len() < queue_depth && !should_stop() {
        let op_type = select_operation_type();
        let in_flight_op = prepare_and_submit_operation(op_type)?;
        in_flight_ops.push(in_flight_op);
    }
    
    // Phase 2: Poll for completions
    if !in_flight_ops.is_empty() {
        process_completions(&mut in_flight_ops)?;
    }
    
    // Phase 3: Check duration periodically
    ops_since_duration_check += 1;
    if ops_since_duration_check >= DURATION_CHECK_INTERVAL {
        if should_stop() && in_flight_ops.is_empty() { break; }
        ops_since_duration_check = 0;
    }
    
    // Phase 4: Sample resources periodically
    // Phase 5: Update live stats snapshot
}
```

This design:
- Maximizes queue utilization for async engines
- Minimizes syscall overhead through batching
- Maintains precise latency measurement per operation

## File Distribution Modes

IOPulse supports three file distribution strategies that control how workers access files.

### Shared Mode

All workers access all files. Each worker independently selects offsets using its distribution:

```rust
// All workers use full file range
let offset = distribution.next_block(total_blocks) * block_size;
```

Use case: Testing concurrent access patterns, lock contention, shared file workloads.

### Partitioned Mode

File space is divided among workers with no overlap:

```rust
// Worker N gets exclusive region
let blocks_per_worker = total_blocks / num_workers;
let start_block = worker_id * blocks_per_worker;
let end_block = start_block + blocks_per_worker;

// Distribution generates within worker's partition
let local_block = distribution.next_block(blocks_per_worker);
let offset = (start_block + local_block) * block_size;
```

In distributed mode, partitioning is global across all nodes:
- 3 nodes × 16 threads = 48 workers
- Worker 0-15 on node 0, workers 16-31 on node 1, workers 32-47 on node 2
- Each worker gets 1/48th of the file

Use case: Maximum aggregate bandwidth, parallel IO without conflicts, HPC workloads.

### Per-Worker Mode

Each worker creates and uses its own file:

```rust
// Worker N creates test.dat.workerN
let file_path = format!("{}.worker{}", base_path, worker_id);
```

Use case: Testing aggregate creation rate, per-file performance, isolated workloads.

## Random Distribution Implementations

### Uniform Distribution

Equal probability for all blocks. Uses xoshiro256++ PRNG for speed:

```rust
impl Distribution for UniformDistribution {
    fn next_block(&mut self, num_blocks: u64) -> u64 {
        self.rng.gen_range(0..num_blocks)
    }
}
```

### Zipf Distribution

Power law distribution for hot/cold data patterns:

- P(k) ∝ 1 / k^theta
- theta = 0.5: More uniform
- theta = 1.2: Realistic workload (default)
- theta = 2.0: Highly skewed

Uses inverse transform sampling with pre-computed CDF for O(log N) generation.

### Pareto Distribution

80/20 rule simulation:

- h = 0.1: 90/10 split
- h = 0.2: 80/20 split (default)
- h = 0.5: 67/33 split

### Gaussian Distribution

Normal distribution for locality of reference:

- stddev: Controls spread (default: 0.1 = 10% of file)
- center: Controls hotspot location (default: 0.5 = middle)

## Memory Alignment

O_DIRECT requires aligned buffers. IOPulse uses custom allocation:

```rust
pub struct AlignedBuffer {
    ptr: *mut u8,
    size: usize,
    alignment: usize,
    layout: Layout,
}

impl AlignedBuffer {
    pub fn new(size: usize, alignment: usize) -> Self {
        let layout = Layout::from_size_align(size, alignment).unwrap();
        let ptr = unsafe { alloc(layout) };
        // ...
    }
}
```

Default alignment:
- O_DIRECT: 4096 bytes (page-aligned)
- Buffered IO: 512 bytes (sector-aligned)

## Think Time Implementation

Think time simulates application processing between IO operations.

### Fixed Think Time

```rust
if think_time_config.duration_us > 0 {
    match think_time_config.mode {
        ThinkTimeMode::Sleep => {
            std::thread::sleep(Duration::from_micros(duration_us));
        }
        ThinkTimeMode::Spin => {
            let end = Instant::now() + Duration::from_micros(duration_us);
            while Instant::now() < end {
                std::hint::spin_loop();
            }
        }
    }
}
```

Sleep mode yields CPU (lower precision, lower CPU usage). Spin mode busy-waits (higher precision, uses CPU).

### Adaptive Think Time

Scales think time based on measured IO latency:

```rust
if let Some(adaptive_percent) = think_time_config.adaptive_percent {
    let adaptive_us = (last_io_latency_us * adaptive_percent as u64) / 100;
    let total_us = think_time_config.duration_us + adaptive_us;
    // Apply total_us think time
}
```

### Think Every N Blocks

Reduces overhead by applying think time periodically:

```rust
ops_since_think += 1;
if ops_since_think >= think_every_n_blocks {
    apply_think_time();
    ops_since_think = 0;
}
```

## Data Verification

The verification subsystem ensures data integrity by writing and reading known patterns.

### Verification Patterns

```rust
pub enum VerificationPattern {
    Zeros,           // All zero bytes
    Ones,            // All 0xFF bytes
    Random(u64),     // Deterministic random with seed
    Sequential,      // 0x00, 0x01, ..., 0xFF, 0x00, ...
}
```

### Pattern Generation

Random pattern uses a simple LCG for deterministic, reproducible data:

```rust
fn fill_random(buffer: &mut [u8], seed: u64) {
    let mut state = seed;
    for byte in buffer.iter_mut() {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        *byte = (state >> 16) as u8;
    }
}
```

The seed is derived from the file offset, ensuring the same offset always produces the same data. This allows verification without storing written data.

### Verification Flow

1. Write: Fill buffer with pattern based on offset, write to storage
2. Read: Read from storage, regenerate expected pattern, compare byte-by-byte
3. Report: Track verify_ops and verify_failures in statistics

## Heatmap and Coverage Tracking

When `--heatmap` is enabled, IOPulse tracks block access patterns.

### Implementation

```rust
pub struct HeatmapTracker {
    buckets: Vec<AtomicU64>,      // Access count per bucket
    num_buckets: usize,
    blocks_per_bucket: u64,
    unique_blocks: AtomicU64,     // Distinct blocks accessed
    rewrite_count: AtomicU64,     // Blocks written multiple times
}
```

Each IO operation updates the appropriate bucket:

```rust
fn record_access(&self, block_num: u64) {
    let bucket_idx = (block_num / self.blocks_per_bucket) as usize;
    self.buckets[bucket_idx].fetch_add(1, Ordering::Relaxed);
}
```

### Coverage Calculation

Coverage = unique_blocks / total_blocks × 100%

Unique block tracking uses a bitmap for O(1) lookup:

```rust
fn mark_block(&self, block_num: u64) -> bool {
    let byte_idx = (block_num / 8) as usize;
    let bit_idx = (block_num % 8) as u32;
    let mask = 1u8 << bit_idx;
    
    let old = self.bitmap[byte_idx].fetch_or(mask, Ordering::Relaxed);
    (old & mask) == 0  // Returns true if block was new
}
```

### Performance Impact

Heatmap tracking adds 5-10% overhead due to:
- Atomic operations per IO
- Bitmap updates for coverage
- Memory for bucket array

Recommended for workload analysis, not peak performance testing.

## Protocol Serialization

Distributed mode uses MessagePack (rmp-serde) for message serialization:

- Supports all serde features (rename_all, default, etc.)
- Compact binary format
- Fast serialization/deserialization

Message framing:
```
[4 bytes: message length (little-endian u32)][N bytes: MessagePack message]
```

Histograms within messages use bincode for efficiency:
```rust
let io_latency_histogram = bincode::serialize(stats.io_latency())?;
```

## Error Handling

### Fail-Safe Defaults

- IO errors abort the test by default
- Worker failures abort the test
- Configuration errors prevent test start

### Continue Modes (Opt-In)

```rust
pub struct RuntimeConfig {
    pub continue_on_error: bool,
    pub max_errors: Option<usize>,
    pub continue_on_worker_failure: bool,
}
```

When enabled:
- Errors are counted in statistics
- Test continues until completion criteria met
- max_errors threshold triggers abort if exceeded

## Resource Tracking

CPU and memory utilization are tracked via `/proc/self/stat`:

```rust
pub struct ResourceStats {
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub peak_memory_bytes: u64,
}
```

Sampling occurs every 10K operations to minimize overhead.

## File Handling

### Auto-Refill

Empty files are automatically filled when:
- Read operations are requested
- mmap engine is used (cannot map empty files)

This prevents silent failures from reading empty files.

### Preallocation

For O_DIRECT, files must have allocated blocks. IOPulse:
1. Detects O_DIRECT mode
2. Forces preallocation if file_size is specified
3. Uses fallocate() for efficient space allocation

### Layout Manifest

For directory tree testing, a manifest tracks file paths:

```rust
pub struct LayoutManifest {
    pub header: ManifestHeader,
    pub file_entries: Vec<FileEntry>,
}
```

Manifests can be:
- Generated from layout configuration
- Loaded from file
- Exported for reuse

## Dependencies

All dependencies are MIT or Apache-2.0 licensed:

| Dependency | Purpose |
|------------|---------|
| clap | CLI argument parsing |
| tokio | Async runtime for distributed mode |
| io-uring | io_uring kernel interface |
| hdrhistogram | High-precision histograms (optional) |
| serde/rmp-serde | Serialization |
| rand/rand_xoshiro | Fast PRNG |
| anyhow/thiserror | Error handling |

## Performance Characteristics

### Measured Overhead

| Component | Overhead |
|-----------|----------|
| FastInstant::now() | ~15-20ns |
| SimpleHistogram::record() | ~5-10ns |
| AlignedCounter::add() | ~2-5ns |
| BufferPool::get/return | ~5-10ns |
| Distribution::next_block() | ~10-30ns |

### Scalability

- Workers: Linear scaling up to CPU core count
- Distributed nodes: Designed for 100+ nodes (protocol overhead <1% of storage bandwidth)
- Queue depth: Effective up to 256 for most workloads

### Network Overhead (Distributed Mode)

| Message | Size | Frequency |
|---------|------|-----------|
| CONFIG | ~2 KB | Once |
| HEARTBEAT | ~1-2 KB | Every 1s |
| RESULTS | ~5-10 KB | Once |

Total for 3 nodes, 5 second test: ~54 KB (negligible vs storage bandwidth)
