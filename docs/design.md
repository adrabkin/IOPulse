# Design Document

## Introduction

This document describes the architectural design of IOPulse, a high-performance IO profiling tool written in Rust. The design translates the requirements into concrete modules, data structures, and interfaces that prioritize performance, extensibility, and maintainability.

## Design Principles

1. **Zero-cost abstractions**: Use Rust's trait system for polymorphism without runtime overhead
2. **Lock-free where possible**: Minimize contention using atomic operations and message passing
3. **Memory efficiency**: Pre-allocate buffers, use memory pools, avoid allocations in hot paths
4. **Modular architecture**: Clear separation of concerns with well-defined interfaces
5. **Fail-safe defaults**: Abort on errors by default, require explicit opt-in for continue modes
6. **MIT licensing**: Only use MIT-compatible dependencies, if better/more efficient alternative is found identify clearly and discuss

## High-Level Architecture

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
│                       Coordinator                               │
│         (Orchestrates Workers, Aggregates Stats)                │
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
     │                     │                     │
     ▼                     ▼                     ▼
┌──────────────────────────────────────────────────────┐
│              Target Abstraction                      │
│     (Files | Block Devices | Network FS)             │
└──────────────────────────────────────────────────────┘
```

## Module Structure

```
iopulse/
├── src/
│   ├── main.rs                    # Entry point, CLI parsing
│   ├── lib.rs                     # Library root
│   ├── config/
│   │   ├── mod.rs                 # Configuration module
│   │   ├── cli.rs                 # CLI argument definitions
│   │   ├── toml.rs                # TOML file parsing
│   │   ├── validator.rs           # Configuration validation
│   │   └── workload.rs            # Workload definition structures
│   ├── coordinator/
│   │   ├── mod.rs                 # Coordinator orchestration
│   │   ├── local.rs               # Local mode coordinator
│   │   └── distributed.rs         # Distributed mode coordinator
│   ├── worker/
│   │   ├── mod.rs                 # Worker thread implementation
│   │   ├── executor.rs            # IO execution loop
│   │   └── affinity.rs            # CPU/NUMA affinity binding
│   ├── engine/
│   │   ├── mod.rs                 # IO engine trait definition
│   │   ├── io_uring.rs            # io_uring backend
│   │   ├── libaio.rs              # libaio backend
│   │   ├── sync.rs                # Synchronous IO backend
│   │   └── mmap.rs                # Memory-mapped IO backend
│   ├── target/
│   │   ├── mod.rs                 # Target abstraction
│   │   ├── file.rs                # File target implementation
│   │   ├── block.rs               # Block device target
│   │   └── tree.rs                # Directory tree generation
│   ├── stats/
│   │   ├── mod.rs                 # Statistics collection
│   │   ├── histogram.rs           # Latency histogram (HdrHistogram)
│   │   ├── aggregator.rs          # Multi-worker aggregation
│   │   └── live.rs                # Live statistics updates
│   ├── output/
│   │   ├── mod.rs                 # Output formatting
│   │   ├── text.rs                # Human-readable text output
│   │   ├── json.rs                # JSON output
│   │   ├── csv.rs                 # CSV output
│   │   └── prometheus.rs          # Prometheus metrics endpoint
│   ├── distribution/
│   │   ├── mod.rs                 # Random distribution trait
│   │   ├── uniform.rs             # Uniform distribution
│   │   ├── zipf.rs                # Zipf distribution
│   │   ├── pareto.rs              # Pareto distribution
│   │   └── gaussian.rs            # Gaussian distribution
│   ├── network/
│   │   ├── mod.rs                 # Network interface detection
│   │   └── binding.rs             # Interface binding logic
│   ├── distributed/
│   │   ├── mod.rs                 # Distributed mode protocol
│   │   ├── protocol.rs            # Binary protocol definition
│   │   ├── client.rs              # Coordinator client
│   │   └── server.rs              # Worker service
│   └── util/
│       ├── mod.rs                 # Utility functions
│       ├── buffer.rs              # Buffer management
│       ├── verification.rs        # Data integrity checking
│       └── time.rs                # High-precision timing
├── Cargo.toml                     # Dependencies and metadata
└── README.md                      # Project documentation
```

## Core Data Structures

### Configuration

```rust
/// Complete test configuration
pub struct Config {
    pub workload: WorkloadConfig,
    pub targets: Vec<TargetConfig>,
    pub workers: WorkerConfig,
    pub output: OutputConfig,
    pub runtime: RuntimeConfig,
}

/// Workload definition with composite IO patterns
pub struct WorkloadConfig {
    pub read_percent: u8,              // 0-100
    pub write_percent: u8,             // 0-100
    pub read_distribution: Vec<IOPattern>,
    pub write_distribution: Vec<IOPattern>,
    pub queue_depth: usize,
    pub completion_mode: CompletionMode,
    pub distribution: DistributionType,
    pub think_time: Option<ThinkTimeConfig>,
}

/// Individual IO pattern within a workload
pub struct IOPattern {
    pub weight: u8,                    // Percentage within operation type
    pub access: AccessPattern,         // Sequential or Random
    pub block_size: u64,
}

/// Think time configuration
pub struct ThinkTimeConfig {
    pub duration_us: u64,
    pub mode: ThinkTimeMode,           // Sleep or Spin
    pub apply_every_n_blocks: usize,
    pub adaptive_percent: Option<u8>,  // Percentage of IO latency
}

/// Random distribution configuration
pub enum DistributionType {
    Uniform,
    Zipf { theta: f64 },
    Pareto { h: f64 },
    Gaussian { stddev: f64, center: f64 },
}

/// Completion criteria
pub enum CompletionMode {
    Duration(Duration),
    TotalBytes(u64),
    RunUntilComplete,
}

/// Target configuration
pub struct TargetConfig {
    pub path: PathBuf,
    pub target_type: TargetType,
    pub file_size: Option<u64>,
    pub num_files: Option<usize>,
    pub num_dirs: Option<usize>,
    pub tree_config: Option<TreeConfig>,
    pub distribution: FileDistribution,
    pub fadvise_flags: FadviseFlags,
    pub madvise_flags: MadviseFlags,
    pub lock_mode: FileLockMode,
}

/// File distribution strategy
pub enum FileDistribution {
    Shared,                            // All workers access all files
    Partitioned,                       // Files divided among workers
}

/// File locking mode
pub enum FileLockMode {
    None,
    Range,                             // Lock byte range per IO
    Full,                              // Lock entire file per IO
}
```

### Worker Architecture

```rust
/// Worker thread that executes IO operations
pub struct Worker {
    id: usize,
    config: Arc<Config>,
    engine: Box<dyn IOEngine>,
    targets: Vec<Box<dyn Target>>,
    stats: WorkerStats,
    rng: Box<dyn Distribution>,
    buffer_pool: BufferPool,
}

impl Worker {
    /// Main execution loop
    pub fn run(&mut self) -> Result<WorkerStats> {
        self.setup()?;
        
        loop {
            if self.should_stop() {
                break;
            }
            
            let op = self.select_operation();
            let target = self.select_target();
            let offset = self.rng.next_offset();
            let buffer = self.buffer_pool.get();
            
            let start = Instant::now();
            self.engine.submit(op, target, offset, buffer)?;
            let completions = self.engine.poll_completions()?;
            
            for completion in completions {
                let latency = start.elapsed();
                self.stats.record(completion, latency);
                
                if let Some(think_time) = &self.config.workload.think_time {
                    self.apply_think_time(think_time, latency);
                }
            }
        }
        
        Ok(self.stats)
    }
}

/// Per-worker statistics (lock-free atomic counters)
pub struct WorkerStats {
    pub read_ops: AtomicU64,
    pub write_ops: AtomicU64,
    pub read_bytes: AtomicU64,
    pub write_bytes: AtomicU64,
    pub errors: AtomicU64,
    pub latency_histogram: Arc<Mutex<Histogram>>,
    pub metadata_ops: MetadataStats,
    pub lock_latency_histogram: Option<Arc<Mutex<Histogram>>>,
}
```

### IO Engine Abstraction

```rust
/// Trait for all IO backends
pub trait IOEngine: Send {
    /// Initialize the engine with configuration
    fn init(&mut self, config: &EngineConfig) -> Result<()>;
    
    /// Submit an IO operation (non-blocking for async engines)
    fn submit(&mut self, op: IOOperation) -> Result<()>;
    
    /// Poll for completed operations
    fn poll_completions(&mut self) -> Result<Vec<IOCompletion>>;
    
    /// Cleanup and release resources
    fn cleanup(&mut self) -> Result<()>;
    
    /// Engine-specific capabilities
    fn capabilities(&self) -> EngineCapabilities;
}

/// IO operation descriptor
pub struct IOOperation {
    pub op_type: OperationType,
    pub target_fd: RawFd,
    pub offset: u64,
    pub buffer: *mut u8,
    pub length: usize,
    pub user_data: u64,              // For tracking completion
}

/// Completed IO operation
pub struct IOCompletion {
    pub user_data: u64,
    pub result: Result<usize>,       // Bytes transferred or error
    pub op_type: OperationType,
}
```

### Target Abstraction

```rust
/// Trait for IO targets (files, block devices, etc.)
pub trait Target: Send {
    /// Open/prepare the target
    fn open(&mut self, flags: OpenFlags) -> Result<()>;
    
    /// Get file descriptor for IO operations
    fn fd(&self) -> RawFd;
    
    /// Get target size in bytes
    fn size(&self) -> u64;
    
    /// Apply fadvise hints
    fn apply_fadvise(&self, flags: FadviseFlags) -> Result<()>;
    
    /// Apply file lock
    fn lock(&self, mode: FileLockMode, offset: u64, len: u64) -> Result<LockGuard>;
    
    /// Close the target
    fn close(&mut self) -> Result<()>;
}

/// File target implementation
pub struct FileTarget {
    path: PathBuf,
    fd: Option<RawFd>,
    size: u64,
    flags: OpenFlags,
}
```

### Statistics and Telemetry

```rust
/// Latency histogram using HdrHistogram
pub struct LatencyHistogram {
    histogram: hdrhistogram::Histogram<u64>,
}

impl LatencyHistogram {
    /// Record a latency sample (in nanoseconds)
    pub fn record(&mut self, latency_ns: u64) {
        self.histogram.record(latency_ns).ok();
    }
    
    /// Get percentile value
    pub fn percentile(&self, percentile: f64) -> u64 {
        self.histogram.value_at_percentile(percentile)
    }
    
    /// Merge another histogram into this one
    pub fn merge(&mut self, other: &LatencyHistogram) {
        self.histogram.add(&other.histogram).ok();
    }
}

/// Metadata operation statistics
pub struct MetadataStats {
    pub open_ops: AtomicU64,
    pub close_ops: AtomicU64,
    pub stat_ops: AtomicU64,
    pub setattr_ops: AtomicU64,
    pub mkdir_ops: AtomicU64,
    pub rmdir_ops: AtomicU64,
    pub unlink_ops: AtomicU64,
    pub rename_ops: AtomicU64,
    pub readdir_ops: AtomicU64,
    pub fsync_ops: AtomicU64,
    pub open_latency: Arc<Mutex<Histogram>>,
    pub close_latency: Arc<Mutex<Histogram>>,
    // ... latency histograms for each operation
}
```

## Key Design Decisions

### 1. IO Engine Selection

**Decision**: Use trait objects (`Box<dyn IOEngine>`) for runtime polymorphism

**Rationale**:
- Allows user to select engine at runtime via configuration
- Small performance cost acceptable (single virtual dispatch per batch)
- Cleaner than compile-time generics for this use case

**Alternative considered**: Generic `Worker<E: IOEngine>` - rejected due to code bloat and inflexible runtime selection

### 2. Statistics Collection

**Decision**: Lock-free atomic counters + per-worker histograms with periodic aggregation

**Rationale**:
- Atomic operations for simple counters (IOPS, bytes) have minimal overhead
- Histograms use mutex but updated infrequently (per IO, not per cycle)
- Aggregation happens outside hot path (periodic or at phase end)

**Alternative considered**: Lock-free histograms - rejected due to complexity and limited Rust libraries

### 3. Buffer Management

**Decision**: Pre-allocated buffer pool per worker with memory alignment

**Rationale**:
- Eliminates allocation overhead in hot path
- Aligned buffers required for O_DIRECT
- Per-worker pools avoid contention

```rust
pub struct BufferPool {
    buffers: Vec<AlignedBuffer>,
    available: VecDeque<usize>,
    alignment: usize,
}

pub struct AlignedBuffer {
    ptr: *mut u8,
    size: usize,
    alignment: usize,
}
```

### 4. Random Distribution Implementation

**Decision**: Trait-based distribution with pre-computed lookup tables where possible

**Rationale**:
- Zipf/Pareto computation is expensive - use rejection sampling with caching
- Gaussian uses Box-Muller transform
- Uniform uses fast PRNG (PCG or xoshiro)

```rust
pub trait Distribution: Send {
    fn next_offset(&mut self, max: u64) -> u64;
}

pub struct ZipfDistribution {
    theta: f64,
    max: u64,
    lookup_table: Vec<u64>,  // Pre-computed for common values
}
```

### 5. Think Time Implementation

**Decision**: Configurable sleep vs spin with adaptive mode

```rust
impl Worker {
    fn apply_think_time(&self, config: &ThinkTimeConfig, io_latency: Duration) {
        let duration = if let Some(pct) = config.adaptive_percent {
            io_latency.mul_f64(pct as f64 / 100.0)
        } else {
            Duration::from_micros(config.duration_us)
        };
        
        match config.mode {
            ThinkTimeMode::Sleep => thread::sleep(duration),
            ThinkTimeMode::Spin => {
                let start = Instant::now();
                while start.elapsed() < duration {
                    std::hint::spin_loop();
                }
            }
        }
    }
}
```

### 6. File Locking Implementation

**Decision**: RAII lock guards with latency tracking

```rust
pub struct LockGuard {
    fd: RawFd,
    lock_type: FileLockMode,
    acquired_at: Instant,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Unlock on drop
        let flock = libc::flock {
            l_type: libc::F_UNLCK as i16,
            l_whence: libc::SEEK_SET as i16,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        unsafe {
            libc::fcntl(self.fd, libc::F_SETLK, &flock);
        }
    }
}
```

### 7. Distributed Mode Protocol

**Decision**: Binary protocol using bincode for serialization

**Rationale**:
- Efficient binary encoding (smaller than JSON, faster than MessagePack)
- Type-safe with serde
- Simple request/response model with heartbeats

```rust
#[derive(Serialize, Deserialize)]
pub enum Message {
    ConfigureWorkload(WorkloadConfig),
    StartPhase,
    StopPhase,
    Heartbeat(WorkerStats),
    Results(WorkerStats),
    Error(String),
}
```

## Dependency Selection (MIT-Compatible)

```toml
[dependencies]
# CLI and configuration
clap = { version = "4", features = ["derive"] }  # MIT/Apache-2.0
toml = "0.8"                                      # MIT/Apache-2.0

# IO engines
io-uring = "0.6"                                  # MIT/Apache-2.0
libc = "0.2"                                      # MIT/Apache-2.0

# Statistics
hdrhistogram = "7"                                # MIT/Apache-2.0

# Serialization
serde = { version = "1", features = ["derive"] }  # MIT/Apache-2.0
serde_json = "1"                                  # MIT/Apache-2.0
bincode = "1"                                     # MIT

# Networking
tokio = { version = "1", features = ["full"] }    # MIT

# Random number generation
rand = "0.8"                                      # MIT/Apache-2.0
rand_distr = "0.4"                                # MIT/Apache-2.0

# Utilities
anyhow = "1"                                      # MIT/Apache-2.0
thiserror = "1"                                   # MIT/Apache-2.0
crossbeam = "0.8"                                 # MIT/Apache-2.0
```

## Performance Optimizations

### 1. Hot Path Optimization
- No allocations in IO submission/completion path
- Inline critical functions
- Use `#[cold]` attribute for error paths
- Profile-guided optimization (PGO) support

### 2. Cache-Line Alignment
- Align worker stats to cache lines to avoid false sharing
- Pad atomic counters to 64 bytes

```rust
#[repr(align(64))]
pub struct AlignedCounter {
    value: AtomicU64,
    _padding: [u8; 56],
}
```

### 3. NUMA Awareness
- Bind workers to NUMA nodes
- Allocate buffers on local NUMA node
- Distribute network interfaces by NUMA topology

## Error Handling Strategy

### Fail-Safe Defaults
- Abort on IO errors by default
- Abort on worker failure in distributed mode by default
- Validate all configuration before starting

### Continue Modes (Opt-In)
```rust
pub struct ErrorPolicy {
    pub continue_on_io_error: bool,
    pub continue_on_worker_failure: bool,
    pub max_errors: Option<usize>,
}
```

### Error Reporting
- Structured errors with context using `thiserror`
- Error counts in statistics
- Detailed error log with operation details

## Testing Strategy

### Unit Tests
- Each module has comprehensive unit tests
- Mock IO engines for testing worker logic
- Property-based testing for distributions

### Integration Tests
- End-to-end tests with real file IO
- Distributed mode tests with local workers
- Performance regression tests

### Benchmarks
- Criterion.rs benchmarks for hot paths
- Performance regression tests

## Next Steps

This design will be implemented according to the task breakdown in `tasks.md`, which will define the specific implementation order and milestones.


## Distributed Mode Architecture

### Execution Modes

IOPulse supports three execution modes via `--mode` parameter:

```rust
pub enum ExecutionMode {
    Standalone,    // Default: local workers only
    Coordinator,   // Orchestrate distributed test
    Worker,        // Accept commands from coordinator
}
```

### Distributed Architecture

```
Single Executable: iopulse

┌─────────────────────────────────────────────────────────────┐
│         Coordinator (iopulse --mode coordinator)            │
│                  (Control Node: 10.0.1.1)                   │
│                                                             │
│  1. Parse clients.list or --host-list                      │
│  2. Connect to all nodes (TCP port 9999)                   │
│  3. Verify protocol version                                │
│  4. Measure clock skew                                     │
│  5. Check dataset marker (if exists)                       │
│  6. Distribute CONFIG with worker assignments              │
│  7. Wait for all READY (barrier)                           │
│  8. Send START (timestamp = now + 100ms)                   │
│  9. Collect HEARTBEAT (every 1s)                           │
│ 10. Send STOP (on completion or failure)                   │
│ 11. Collect RESULTS                                        │
│ 12. Aggregate statistics                                   │
│ 13. Generate output                                        │
└────────┬──────────────┬──────────────┬──────────────────────┘
         │              │              │
         ▼              ▼              ▼
    ┌─────────┐    ┌─────────┐    ┌─────────┐
    │ Node 1  │    │ Node 2  │    │ Node 3  │
    │10.0.1.10│    │10.0.1.11│    │10.0.1.12│
    │         │    │         │    │         │
    │ iopulse │    │ iopulse │    │ iopulse │
    │ --mode  │    │ --mode  │    │ --mode  │
    │ worker  │    │ worker  │    │ worker  │
    └────┬────┘    └────┬────┘    └────┬────┘
         │              │              │
    ┌────┴────┐    ┌────┴────┐    ┌────┴────┐
    │16 Workers│   │16 Workers│   │16 Workers│
    │(threads) │   │(threads) │   │(threads) │
    └──────────┘   └──────────┘   └──────────┘
         │              │              │
         ▼              ▼              ▼
    ┌─────────────────────────────────────────┐
    │      Shared Storage (NFS/Lustre)        │
    │         /mnt/nfs/test.dat               │
    └─────────────────────────────────────────┘
```

### Distributed Protocol

#### Message Types

```rust
#[derive(Serialize, Deserialize)]
pub enum Message {
    // Coordinator → Node
    Config {
        workload: WorkloadConfig,
        worker_assignments: Vec<WorkerAssignment>,
        protocol_version: u32,
    },
    Start {
        timestamp_ns: u64,  // Absolute start time
    },
    Stop,
    HeartbeatAck,
    
    // Node → Coordinator
    Ready {
        node_id: String,
        num_workers: usize,
        protocol_version: u32,
        clock_offset_ns: i64,
    },
    Heartbeat {
        node_id: String,
        elapsed_ns: u64,
        aggregate_stats: WorkerStats,
        per_worker_stats: Option<Vec<WorkerStats>>,
    },
    Results {
        node_id: String,
        aggregate_stats: WorkerStats,
        per_worker_stats: Vec<WorkerStats>,
    },
    Error {
        node_id: String,
        message: String,
    },
}

pub struct WorkerAssignment {
    pub worker_id: usize,      // Global worker ID
    pub file_range: Option<(usize, usize)>,  // For PARTITIONED mode
    pub offset_range: Option<(u64, u64)>,    // For PARTITIONED single file
}
```

#### Message Framing

```rust
// Wire format: [4-byte length][message bytes]
pub struct MessageFrame {
    length: u32,        // Message length in bytes
    payload: Vec<u8>,   // Bincode-serialized message
}
```

#### Protocol Flow

```
Phase 1: Connection & Preparation
  Coordinator → Node: CONFIG (workload + assignments)
  Node → Coordinator: READY (when prepared)
  
Phase 2: Synchronized Start
  Coordinator: Wait for all READY (barrier)
  Coordinator → All Nodes: START (timestamp)
  Nodes: Wait until local_time >= timestamp
  Nodes: Begin IO simultaneously
  
Phase 3: Execution & Monitoring
  Nodes → Coordinator: HEARTBEAT (every 1s)
  Coordinator → Nodes: HEARTBEAT_ACK
  Coordinator: Monitor for failures (3-miss = failed)
  
Phase 4: Completion
  Coordinator → All Nodes: STOP
  Nodes: Complete in-flight operations
  Nodes → Coordinator: RESULTS (final stats)
  Coordinator: Aggregate and output
```

### Layout_Manifest and Dataset Markers

#### Layout_Manifest Design

```rust
pub struct LayoutManifest {
    pub header: ManifestHeader,
    pub file_paths: Vec<PathBuf>,
}

pub struct ManifestHeader {
    pub generated_at: DateTime<Utc>,
    pub depth: usize,
    pub width: usize,
    pub total_files: usize,
    pub total_directories: usize,
}

impl LayoutManifest {
    /// Parse from file
    pub fn from_file(path: &Path) -> Result<Self>;
    
    /// Export to file
    pub fn to_file(&self, path: &Path) -> Result<()>;
    
    /// Calculate hash for marker
    pub fn hash(&self) -> u64;
}
```

#### Dataset Marker Design

```rust
pub struct DatasetMarker {
    pub config_hash: u64,
    pub created_at: DateTime<Utc>,
    pub total_files: usize,
    pub total_size: u64,
    pub parameters: HashMap<String, String>,
    pub layout_manifest_path: Option<String>,
    pub layout_manifest_hash: Option<u64>,
}

impl DatasetMarker {
    /// Read marker from directory
    pub fn read(dir: &Path) -> Result<Option<Self>>;
    
    /// Write marker to directory
    pub fn write(&self, dir: &Path) -> Result<()>;
    
    /// Check if marker matches current config
    pub fn matches(&self, config: &TargetConfig, manifest: Option<&LayoutManifest>) -> bool;
}
```

#### Marker Workflow

```
Standalone Mode:
  1. Check for .iopulse-layout marker
  2. If exists and matches: Skip file creation
  3. If not exists or mismatch: Create files + marker
  
Distributed Mode:
  1. Coordinator checks marker (on shared storage or first node)
  2. If matches: Send CONFIG with skip_creation=true
  3. If not matches: Abort with error (use --force-recreate)
  4. Workers trust coordinator's validation
```

### File Distribution in Distributed Mode

#### Global Worker Numbering

```rust
// 3 nodes, 16 threads per node = 48 total workers
// Worker 0-15: Node 1 (10.0.1.10)
// Worker 16-31: Node 2 (10.0.1.11)
// Worker 32-47: Node 3 (10.0.1.12)

pub fn calculate_worker_id(node_index: usize, thread_index: usize, threads_per_node: usize) -> usize {
    node_index * threads_per_node + thread_index
}
```

#### PARTITIONED Mode (Single File)

```rust
// Divide file offset range among all workers globally
pub fn calculate_offset_range(
    worker_id: usize,
    total_workers: usize,
    file_size: u64
) -> (u64, u64) {
    let region_size = file_size / total_workers as u64;
    let start = worker_id as u64 * region_size;
    let end = if worker_id == total_workers - 1 {
        file_size  // Last worker gets remainder
    } else {
        start + region_size
    };
    (start, end)
}
```

#### PARTITIONED Mode (Directory Tree)

```rust
// Divide file list among all workers globally
pub fn calculate_file_range(
    worker_id: usize,
    total_workers: usize,
    total_files: usize
) -> (usize, usize) {
    let files_per_worker = total_files / total_workers;
    let start = worker_id * files_per_worker;
    let end = if worker_id == total_workers - 1 {
        total_files  // Last worker gets remainder
    } else {
        start + files_per_worker
    };
    (start, end)
}
```

### Clock Synchronization

#### Hybrid Approach

```rust
pub enum SyncMode {
    HighPrecision,    // <10ms skew, use NTP
    MediumPrecision,  // 10-50ms skew, use coordinator offsets
    Unacceptable,     // >50ms skew, abort
}

pub struct ClockSync {
    pub mode: SyncMode,
    pub max_skew_ms: f64,
    pub per_node_offset_ns: HashMap<String, i64>,
}

impl ClockSync {
    /// Measure clock skew to a node
    pub async fn measure_skew(node: &str) -> Result<i64> {
        let t1 = Instant::now();
        let node_time = send_ping(node).await?;  // Node returns its timestamp
        let t2 = Instant::now();
        let rtt = t2 - t1;
        let coordinator_time = t1 + rtt / 2;
        let skew = node_time - coordinator_time;
        Ok(skew.as_nanos() as i64)
    }
    
    /// Determine sync mode based on measured skew
    pub fn determine_mode(max_skew_ns: i64) -> SyncMode {
        let skew_ms = max_skew_ns as f64 / 1_000_000.0;
        if skew_ms < 10.0 {
            SyncMode::HighPrecision
        } else if skew_ms < 50.0 {
            SyncMode::MediumPrecision
        } else {
            SyncMode::Unacceptable
        }
    }
}
```

### Coordinator State Machine

```rust
pub enum CoordinatorState {
    Connecting,      // Connecting to nodes
    Configuring,     // Sending CONFIG
    WaitingReady,    // Waiting for all READY
    Starting,        // Sending START
    Running,         // Test executing, collecting heartbeats
    Stopping,        // Sending STOP
    Collecting,      // Collecting RESULTS
    Aggregating,     // Aggregating statistics
    Complete,        // Test complete
    Failed(String),  // Test failed
}

pub struct DistributedCoordinator {
    state: CoordinatorState,
    nodes: Vec<NodeConnection>,
    config: Arc<Config>,
    start_time: Option<Instant>,
    aggregated_stats: Option<WorkerStats>,
}

impl DistributedCoordinator {
    pub async fn run(&mut self) -> Result<WorkerStats> {
        self.connect_to_nodes().await?;
        self.verify_protocol_versions().await?;
        self.measure_clock_skew().await?;
        self.check_dataset_marker().await?;
        self.distribute_configuration().await?;
        self.wait_for_ready().await?;
        self.send_start().await?;
        self.monitor_execution().await?;
        self.collect_results().await?;
        self.aggregate_statistics()
    }
}
```

### Worker Service State Machine

```rust
pub enum WorkerState {
    Listening,       // Waiting for coordinator
    Preparing,       // Received CONFIG, preparing
    Ready,           // Sent READY, waiting for START
    Running,         // Executing IO
    Stopping,        // Received STOP, completing
    Complete,        // Sent RESULTS
}

pub struct WorkerService {
    state: WorkerState,
    config: Option<Arc<Config>>,
    workers: Vec<JoinHandle<Result<WorkerStats>>>,
    last_heartbeat_ack: Instant,
}

impl WorkerService {
    pub async fn run(&mut self) -> Result<()> {
        self.listen_for_coordinator().await?;
        self.receive_configuration().await?;
        self.prepare_workers().await?;
        self.send_ready().await?;
        self.wait_for_start().await?;
        self.execute_workload().await?;
        self.send_results().await
    }
    
    /// Dead man's switch: self-stop if no heartbeat ACK
    async fn monitor_heartbeat_ack(&self) {
        loop {
            if self.last_heartbeat_ack.elapsed() > Duration::from_secs(10) {
                eprintln!("Dead man's switch triggered: No heartbeat ACK for 10s");
                self.stop_all_workers();
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
```

### Key Design Decisions (Distributed)

#### 1. Single Executable with Modes

**Decision**: One binary, three modes via `--mode` parameter

**Rationale**:
- Simpler deployment (one binary)
- Shared code between modes
- Cleaner user experience

**Implementation**:
```rust
match cli.mode {
    ExecutionMode::Standalone => run_standalone(config),
    ExecutionMode::Coordinator => run_coordinator(config).await,
    ExecutionMode::Worker => run_worker_service(config).await,
}
```

#### 2. Barrier Synchronization with 100ms Start Delay

**Decision**: CONFIG → READY barrier → START with 100ms delay

**Rationale**:
- 100ms allows for network latency (20ms p99) + processing (10ms) + safety (70ms)
- >99.9% success rate with 100-500 nodes
- Time-series alignment excellent (100ms initial skew, then perfect)
- Adaptive mode scales automatically

**Implementation**:
```rust
// Coordinator
let start_delay = match config.start_delay {
    StartDelay::Fixed(ms) => Duration::from_millis(ms),
    StartDelay::Auto => calculate_adaptive_delay(&nodes),
};
let start_timestamp = Instant::now() + start_delay;
send_start_to_all_nodes(start_timestamp).await?;

// Worker
let start_timestamp = receive_start_message().await?;
while Instant::now() < start_timestamp {
    tokio::time::sleep(Duration::from_millis(1)).await;
}
begin_io();  // All workers start simultaneously
```

#### 3. Global Worker Partitioning

**Decision**: Partition work across all workers globally (not per-node)

**Rationale**:
- Matches user requirement: "each file touched once"
- Maximizes aggregate bandwidth
- Avoids conflicts in PARTITIONED mode
- Simple and predictable

**Implementation**:
```rust
// Coordinator calculates assignments for all workers
let total_workers = num_nodes * threads_per_node;
for worker_id in 0..total_workers {
    let assignment = match file_distribution {
        FileDistribution::Partitioned => {
            let (start, end) = calculate_file_range(worker_id, total_workers, total_files);
            WorkerAssignment { worker_id, file_range: Some((start, end)), ..}
        },
        FileDistribution::Shared => {
            WorkerAssignment { worker_id, file_range: None, ..}  // Access all files
        },
        FileDistribution::PerWorker => {
            WorkerAssignment { worker_id, unique_file: true, ..}
        },
    };
    assignments.push(assignment);
}
```

#### 4. Dataset Marker in Distributed Mode

**Decision**: Coordinator checks marker once, distributes result

**Rationale**:
- Avoid redundant checks (N nodes checking same marker)
- Coordinator has authority
- Workers trust coordinator
- Faster startup

**Implementation**:
```rust
// Coordinator
let marker = DatasetMarker::read(&target_dir)?;
let skip_creation = if let Some(marker) = marker {
    marker.matches(&config.targets[0], layout_manifest.as_ref())
} else {
    false
};

// Include in CONFIG message
let config_msg = Message::Config {
    workload: config.workload.clone(),
    worker_assignments,
    skip_file_creation: skip_creation,
    protocol_version: PROTOCOL_VERSION,
};
```

#### 5. Strict Failure Handling

**Decision**: Any node fails = entire test aborts

**Rationale**:
- Partial results are misleading
- Storage testing requires all nodes working
- Simpler implementation
- Clear pass/fail indication

**Implementation**:
```rust
// Coordinator heartbeat monitoring
async fn monitor_heartbeats(&mut self) {
    loop {
        for node in &mut self.nodes {
            if node.missed_heartbeats >= 3 {
                eprintln!("Node {} failed (missed 3 heartbeats)", node.id);
                self.send_stop_to_all_nodes().await?;
                return Err(anyhow!("Test FAILED: Node {} unreachable", node.id));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// Worker dead man's switch
async fn dead_mans_switch(&self) {
    if self.last_heartbeat_ack.elapsed() > Duration::from_secs(10) {
        eprintln!("Dead man's switch: No coordinator ACK for 10s, stopping");
        self.stop_all_workers();
    }
}
```

### Performance Considerations

#### Coordinator Scalability

**Target**: Support 1000 nodes with <5% overhead

**Optimizations**:
- Async I/O (tokio) for all network operations
- Batch heartbeat processing
- Lock-free statistics aggregation
- Minimal per-node state

**Expected overhead**:
- 100 nodes: <1% CPU, <100 MB memory
- 500 nodes: <3% CPU, <500 MB memory
- 1000 nodes: <5% CPU, <1 GB memory

#### Network Bandwidth

**Heartbeat size**: ~1 KB per node per second
- 100 nodes: 100 KB/s (negligible)
- 500 nodes: 500 KB/s (negligible)
- 1000 nodes: 1 MB/s (negligible vs storage bandwidth)

**Configuration distribution**: One-time, ~10 KB per node
**Results collection**: One-time, ~100 KB per node

**Total network overhead**: <0.1% of storage bandwidth

### Testing Strategy (Distributed)

#### Unit Tests
- Protocol message serialization/deserialization
- Worker assignment calculation
- Clock skew measurement
- Marker validation

#### Integration Tests
- 3-node local test (using localhost)
- Single file PARTITIONED mode
- Directory tree PARTITIONED mode
- Node failure simulation
- Clock synchronization validation

#### Performance Tests
- 10-node test: Measure coordinator overhead
- 100-node test: Validate scalability
- Heartbeat stress test: 1000 nodes × 1 Hz

## Implementation Order

This design will be implemented in phases:

1. **Phase 1**: Standalone tree support (Task 24k-tree)
2. **Phase 2**: Distributed protocol (Task 26)
3. **Phase 3**: Worker service (Task 27)
4. **Phase 4**: Distributed coordinator (Task 28)
5. **Phase 5**: Testing and validation

See `tasks.md` for detailed implementation tasks.
