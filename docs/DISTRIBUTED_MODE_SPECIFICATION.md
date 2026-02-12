# IOPulse Distributed Mode Specification
**Date:** January 24, 2026  
**Status:** APPROVED - Ready for Implementation  
**Last Updated:** January 24, 2026 (consolidated all decisions)  
**Purpose:** Single source of truth for distributed mode

---

## Executive Summary

IOPulse distributed mode enables testing across multiple nodes with coordinated workload execution. Key features:

- ✅ **Single executable** with three modes (standalone, coordinator, worker)
- ✅ **100ms start delay** (data-driven, adaptive mode available)
- ✅ **Layout_Manifest** for reproducible tree structures
- ✅ **Dataset markers** (skip recreation on subsequent runs)
- ✅ **Node/Worker hierarchy** (nodes × threads = total workers)
- ✅ **Strict failure handling** (any node fails = test aborts)
- ✅ **Hybrid clock sync** (1-50ms precision)

**Total effort:** 34-44 hours across 5 phases

---

## Terminology

### Standalone Mode
- **Worker** = Thread executing IO operations on local host
- **Coordinator** = Local coordinator spawning worker threads
- **Example:** 16 workers = 16 threads on 1 host

### Distributed Mode
- **Node** = Physical/virtual host (e.g., 10.0.1.10)
- **Worker** = Thread on a node executing IO operations
- **Coordinator** = Control node orchestrating all nodes
- **Total Workers** = num_nodes × threads_per_node
- **Example:** 48 workers = 3 nodes × 16 threads per node

---

## Architecture Overview

```
Single Executable: iopulse

Three Modes:
1. Standalone (default) - Local testing with worker threads
2. Coordinator - Orchestrates distributed testing
3. Worker - Runs on nodes, accepts commands from coordinator

┌─────────────────────────────────────────────────────────────┐
│              Coordinator (iopulse --mode coordinator)       │
│                    (Control Node: 10.0.1.1)                 │
│                                                             │
│  - Reads clients.list or --host-list                        │
│  - Connects to all nodes                                    │
│  - Distributes configuration                                │
│  - Coordinates synchronized start (100ms delay)             │
│  - Aggregates results                                       │
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
    ┌────┴─────┐   ┌────┴─────┐   ┌────┴─────┐
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

---

## Execution Modes

### Mode 1: Standalone (Default)
```bash
iopulse /mnt/data/test.dat --threads 16 --duration 60s
```
- Single host, multiple worker threads
- No network communication
- Current implementation (working)

### Mode 2: Coordinator
```bash
iopulse --mode coordinator \
  --host-list 10.0.1.10,10.0.1.11,10.0.1.12 \
  --threads 16 \
  /mnt/nfs/test.dat \
  --duration 60s
```
- Orchestrates distributed test
- Connects to worker nodes
- Aggregates results
- Runs on control node

### Mode 3: Worker
```bash
iopulse --mode worker --listen-port 9999
```
- Runs on each worker node
- Accepts commands from coordinator
- Spawns worker threads
- Sends results to coordinator

---

## Key Features

### 1. Layout_Manifest (Tree File Definitions)

**Purpose:** Save and reuse directory tree structures

**Generate and export:**
```bash
iopulse /mnt/nfs/tree \
  --dir-depth 3 \
  --dir-width 10 \
  --total-files 1000000 \
  --export-layout-manifest tree_1M.layout_manifest \
  --duration 0
```

**Reuse:**
```bash
iopulse /mnt/nfs/tree \
  --layout-manifest tree_1M.layout_manifest \
  --duration 60s
```

**Benefits:**
- Skip tree generation (saves time)
- Reproducible testing (exact same structure)
- Shareable (distribute to team)

### 2. Dataset Layout Markers

**Purpose:** Skip file creation on subsequent runs

**First run:**
```bash
iopulse /mnt/nfs/tree --dir-depth 3 --total-files 1000000 --duration 60s
# Creates: 1M files + .iopulse-layout marker
# Time: 10 minutes (file creation)
```

**Second run:**
```bash
iopulse /mnt/nfs/tree --dir-depth 3 --total-files 1000000 --duration 60s
# Reads: .iopulse-layout marker
# Verifies: Hash matches (same config)
# Skips: File creation
# Time: 1 second (marker read) + 60s (test)
```

**Savings:** 10 minutes → 1 second (600× faster startup)

### 3. Node/Worker Hierarchy

**Terminology:**
- **Node** = Host (e.g., 10.0.1.10)
- **Worker** = Thread on a node
- **Total Workers** = nodes × threads_per_node

**Example:**
```bash
# 3 nodes, 16 threads per node
iopulse --mode coordinator --host-list 10.0.1.10,10.0.1.11,10.0.1.12 --threads 16

# Result: 48 total workers
# Worker 0-15: Node 1
# Worker 16-31: Node 2
# Worker 32-47: Node 3
```

### 4. File Distribution Modes

**SHARED:** All workers access all files
```bash
--file-distribution shared
# All 48 workers access all 1M files
# Use case: Concurrent access, lock contention
```

**PARTITIONED:** Each file touched once
```bash
--file-distribution partitioned
# Worker 0: files 0-20,832
# Worker 47: files 979,167-999,999
# Use case: Metadata benchmark, maximum bandwidth
```

**PER_WORKER:** Each worker creates own files
```bash
--file-distribution per-worker
# Worker 0: test.dat.node1.worker0
# Worker 47: test.dat.node3.worker15
# Use case: Aggregate creation rate
```

### 5. Synchronized Start (100ms)

**Why 100ms:**
- Network latency: 20ms (p99 cross-AZ)
- Coordinator processing: 10ms (100 nodes)
- Safety margin: 70ms (2.2×)
- Success rate: >99.9%
- Time-series alignment: Excellent

**Adaptive mode:**
```bash
--start-delay auto
# Measures network latency
# Calculates optimal delay
# Scales automatically
```

### 6. Clock Synchronization (Hybrid)

**Strategy:**
- <10ms skew: Use NTP (high precision)
- 10-50ms skew: Use coordinator offsets (medium precision)
- >50ms skew: Abort test (unacceptable)

**Implementation:**
- Coordinator measures skew during init
- Workers report elapsed time (not absolute)
- Coordinator adjusts timestamps
- Time-series data aligns perfectly

---

## Protocol Messages

### Message Types

```rust
enum Message {
    // Coordinator → Node
    Config(WorkloadConfig),      // Send test configuration
    Start(u64),                  // Start IO at timestamp
    Stop,                        // Stop IO immediately
    HeartbeatAck,               // Acknowledge heartbeat
    
    // Node → Coordinator
    Ready(NodeInfo),            // Node prepared, ready to start
    Heartbeat(NodeStats),       // Periodic statistics update
    Results(NodeStats),         // Final statistics
    Error(String),              // Error occurred
}

struct NodeInfo {
    node_id: String,            // IP or hostname
    num_workers: usize,         // Threads on this node
    protocol_version: u32,      // For compatibility check
}

struct NodeStats {
    node_id: String,
    per_worker_stats: Vec<WorkerStats>,
    aggregate_stats: WorkerStats,
}
```

### Protocol Flow

```
Coordinator                     Node 1                      Node 2
    |                              |                           |
    |-------- CONFIG ------------->|                           |
    |-------- CONFIG ------------------------------------->|
    |                              |                           |
    |                         [Prepare]                   [Prepare]
    |                              |                           |
    |<------- READY --------------|                           |
    |<------- READY ------------------------------------|
    |                              |                           |
    |-- START(T+100ms) ----------->|                           |
    |-- START(T+100ms) --------------------------------->|
    |                              |                           |
    |                    [Wait until T+100ms]        [Wait until T+100ms]
    |                         [Begin IO]                 [Begin IO]
    |                              |                           |
    |<----- HEARTBEAT(stats) ------|                           |
    |<----- HEARTBEAT(stats) ----------------------------|
    |-- HEARTBEAT_ACK ------------>|                           |
    |-- HEARTBEAT_ACK ---------------------------------->|
    |                              |                           |
    |         [Test duration expires]                          |
    |                              |                           |
    |-------- STOP --------------->|                           |
    |-------- STOP ----------------------------------->|
    |                              |                           |
    |                      [Complete in-flight]        [Complete in-flight]
    |                              |                           |
    |<----- RESULTS(stats) --------|                           |
    |<----- RESULTS(stats) -----------------------------|
    |                              |                           |
    |      [Aggregate results]                                 |
    |      [Display output]                                    |
```

---

## Complete Workflow Example

### Scenario: 1M File Metadata Benchmark on 3 Nodes

**Step 1: Generate tree and export (run once)**
```bash
# On any node (standalone mode)
iopulse /mnt/nfs/tree \
  --dir-depth 3 \
  --dir-width 10 \
  --total-files 1000000 \
  --export-layout-manifest tree_1M.layout_manifest \
  --duration 0

# Output:
# "Generating directory tree..."
# "Created 1,110 directories"
# "Created 1,000,000 files"
# "Layout manifest exported to tree_1M.layout_manifest"
# "Dataset marker created: .iopulse-layout"
# Time: ~10 minutes
```

**Step 2: Start worker service on each node**
```bash
# On node 10.0.1.10:
iopulse --mode worker --listen-port 9999

# On node 10.0.1.11:
iopulse --mode worker --listen-port 9999

# On node 10.0.1.12:
iopulse --mode worker --listen-port 9999

# Output: "Worker service listening on port 9999"
```

**Step 3: Run distributed test (coordinator)**
```bash
# On coordinator node:
iopulse --mode coordinator \
  --host-list 10.0.1.10,10.0.1.11,10.0.1.12 \
  --threads 16 \
  --layout-manifest tree_1M.layout_manifest \
  --duration 60s \
  --write-percent 100 \
  --random \
  --file-distribution partitioned \
  --start-delay 100ms

# Output:
# "Connecting to 3 nodes..."
# "Connected to 10.0.1.10 (protocol v1, 16 workers)"
# "Connected to 10.0.1.11 (protocol v1, 16 workers)"
# "Connected to 10.0.1.12 (protocol v1, 16 workers)"
# "Total: 3 nodes, 48 workers"
# "Clock skew: max 8ms (high precision mode)"
# "Using existing layout (verified via marker, 1000000 files)"
# "File distribution: partitioned (20833 files per worker)"
# "Start delay: 100ms"
# "Waiting for all nodes to be ready..."
# "All nodes ready, starting in 100ms..."
# "Test running..."
# [Live statistics every 1 second]
# "Test complete!"
# 
# Aggregate Results (3 nodes, 48 workers):
#   Total IOPS: 2.4M
#   Total Bandwidth: 9.6 GB/s
#   Per-node: 800K IOPS avg
#   Slowest node: node2 (750K IOPS, -6%)
#   Time-to-completion: 60.1s

# Time: 1 second (marker read) + 60s (test) = 61s total
```

---

## File Distribution Examples

### Example 1: Single File, PARTITIONED Mode

**Setup:**
- 3 nodes, 16 threads per node = 48 workers
- File: /mnt/nfs/test.dat (100GB)
- Mode: PARTITIONED

**Distribution:**
```
Worker 0 (node 1, thread 0):  offsets 0 - 2.08GB
Worker 1 (node 1, thread 1):  offsets 2.08GB - 4.17GB
...
Worker 15 (node 1, thread 15): offsets 31.25GB - 33.33GB
Worker 16 (node 2, thread 0):  offsets 33.33GB - 35.42GB
...
Worker 47 (node 3, thread 15): offsets 97.92GB - 100GB
```

**Result:** Each worker writes to exclusive region, no conflicts

### Example 2: Directory Tree, PARTITIONED Mode

**Setup:**
- 3 nodes, 16 threads per node = 48 workers
- Tree: 1M files
- Mode: PARTITIONED

**Distribution:**
```
Worker 0 (node 1, thread 0):  files 0 - 20,832
Worker 1 (node 1, thread 1):  files 20,833 - 41,665
...
Worker 15 (node 1, thread 15): files 312,499 - 333,331
Worker 16 (node 2, thread 0):  files 333,332 - 354,164
...
Worker 47 (node 3, thread 15): files 979,167 - 999,999
```

**Result:** Each file accessed by exactly one worker

### Example 3: Directory Tree, SHARED Mode

**Setup:**
- 3 nodes, 16 threads per node = 48 workers
- Tree: 1M files
- Mode: SHARED

**Distribution:**
```
All 48 workers: access all 1M files (random selection)
```

**Result:** Files accessed multiple times, concurrent access, lock contention

### Example 4: Per-Worker Files

**Setup:**
- 3 nodes, 16 threads per node = 48 workers
- Mode: PER_WORKER

**Distribution:**
```
Worker 0 (node 1, thread 0):  test.dat.node1.worker0
Worker 1 (node 1, thread 1):  test.dat.node1.worker1
...
Worker 47 (node 3, thread 15): test.dat.node3.worker15
```

**Result:** 48 separate files, no coordination needed

---

## Layout_Manifest Format

### File Format

```
# IOPulse Layout Manifest
# Generated: 2026-01-24 10:30:00 UTC
# Parameters: depth=3, width=10, total_files=1000000
# Total files: 1000000
# Total directories: 1110
# Files per directory: 900 (avg)
#
dir_0000/file_000000
dir_0000/file_000001
...
dir_0000/file_000899
dir_0001/file_000000
...
dir_0000/dir_0000/file_000000
...
```

### Usage Examples

**Generate and export:**
```bash
iopulse /mnt/nfs/tree \
  --dir-depth 3 \
  --dir-width 10 \
  --total-files 1000000 \
  --export-layout-manifest tree_1M.layout_manifest \
  --duration 0

# Output: "Layout manifest exported to tree_1M.layout_manifest (1000000 files)"
```

**Reuse layout manifest:**
```bash
iopulse /mnt/nfs/tree \
  --layout-manifest tree_1M.layout_manifest \
  --duration 60s \
  --threads 16 \
  --file-distribution partitioned

# Output: "Using layout manifest: tree_1M.layout_manifest (1000000 files)"
# Output: "File distribution: partitioned (62500 files per worker)"
```

**Layout manifest overrides:**
```bash
iopulse /mnt/nfs/tree \
  --layout-manifest tree_1M.layout_manifest \
  --dir-depth 5 \
  --total-files 500000

# Warning: "Layout manifest provided, ignoring --dir-depth and --total-files"
# Uses structure from tree_1M.layout_manifest (1M files, depth=3)
```

---

## Key Design Decisions

### 1. Single Executable with Modes

**Decision:** One binary with `--mode` parameter

**Rationale:**
- Simpler deployment (one binary to distribute)
- Cleaner user experience
- Shared code between modes

**Implementation:**
```rust
match cli.mode {
    Mode::Standalone => run_standalone(),
    Mode::Coordinator => run_coordinator(),
    Mode::Worker => run_worker_service(),
}
```

### 2. Connection Management

**Decision:** Simple TCP connections, no security overhead

**Rationale:**
- Storage testing is typically on trusted networks
- Authentication/encryption adds latency
- Version checking sufficient for compatibility
- Can add security later if needed

**Implementation:**
- TCP server on each node (port 9999)
- Coordinator connects to all nodes
- Binary protocol (bincode serialization)
- Protocol version in first message

### 3. Failure Handling

**Decision:** Strict abort on any node failure

**Rationale:**
- Partial results are misleading
- Storage testing requires all nodes working
- Simpler implementation (no partial aggregation logic)
- Clear pass/fail indication

**Implementation:**
- Heartbeat every 1 second
- 3-miss timeout = failed
- Coordinator sends STOP to all nodes
- Workers have dead man's switch (self-stop if no ACK)

### 4. Synchronization

**Decision:** Barrier synchronization with 100ms start delay

**Rationale:**
- Realistic concurrent load requires simultaneous start
- 100ms allows for network latency (20ms p99) + coordinator processing (10ms) + safety (70ms)
- >99.9% success rate with 100-500 nodes
- Time-series alignment excellent (100ms initial skew, then perfect)
- Adaptive mode scales automatically

**Implementation:**
- CONFIG → READY barrier → START with timestamp
- Workers wait until local_time >= start_timestamp
- All workers begin IO simultaneously (within 100ms)
- Configurable: `--start-delay 100ms` or `--start-delay auto`

### 5. File Distribution

**Decision:** Global partitioning across all workers

**Rationale:**
- Matches user's "each file touched once" requirement
- Maximizes aggregate bandwidth
- Avoids conflicts in PARTITIONED mode
- Realistic for metadata benchmarking

**Implementation:**
- Coordinator calculates: total_workers = nodes × threads
- PARTITIONED: Worker N gets files N×(total/workers) to (N+1)×(total/workers)
- SHARED: All workers access all files (overlap allowed)
- PER_WORKER: Each worker creates unique files

### 6. Layout_Manifest

**Decision:** Use layout_manifest for tree file definitions

**Rationale:**
- Descriptive name (layout + manifest)
- Reusable across tests (save generation time)
- Reproducible testing (exact same structure)
- Overrides depth/width (definitive structure)

**Implementation:**
- `--layout-manifest <file>` for input
- `--export-layout-manifest <file>` for output
- File extension: `.layout_manifest` or `.lm`
- Format: One file path per line with header

### 7. Clock Synchronization

**Decision:** Hybrid NTP + coordinator-based

**Rationale:**
- Best accuracy when NTP available (1-10ms)
- Graceful degradation without NTP (10-50ms)
- Abort if unacceptable (>50ms)
- User-friendly (works with or without NTP)

**Implementation:**
- Measure clock skew during init
- Use absolute timestamps if <10ms skew
- Use coordinator-relative if 10-50ms skew
- Abort if >50ms skew

---

## Requirements Summary

**Total requirements:** 14 (1, 1a-1d, 2, 2a-2b, 3, 3a-3c, 4, 4a, 5, 5a-5g, 6, 6a-6b, 7-14)

**New for distributed mode:**
- Requirement 3c: Layout_Manifest support (26 criteria)
- Requirement 5a: Connection management (14 criteria)
- Requirement 5b: Worker failure handling (10 criteria)
- Requirement 5c: Synchronized execution (16 criteria)
- Requirement 5d: Distributed file distribution (13 criteria)
- Requirement 5e: Clock synchronization (10 criteria)
- Requirement 5f: Aggregate metrics (10 criteria)
- Requirement 5g: Graceful shutdown (10 criteria)
- Requirement 14: Dataset markers (updated, 16 criteria)

**Total new acceptance criteria:** 125

---

## Implementation Tasks

### Phase 1: Standalone Tree Support (6-8 hours)

**Task 24k-tree:** Directory Tree File Distribution
- Add --total-files, --layout-manifest, --export-layout-manifest
- Integrate TreeGenerator with Coordinator
- Implement file distribution modes
- Integrate with dataset markers (Requirement 14)
- Test with 1000 files, 16 workers

### Phase 2: Distributed Protocol (4-6 hours)

**Task 26:** Distributed Protocol
- Define message types
- Implement bincode serialization
- Protocol version checking
- Message framing

### Phase 3: Worker Service (8-10 hours)

**Task 27:** Worker Mode Implementation
- Add --mode worker to CLI
- Implement TCP server
- Spawn worker threads
- Heartbeat and dead man's switch
- Dataset marker integration

### Phase 4: Distributed Coordinator (10-12 hours)

**Task 28:** Coordinator Mode Implementation
- Add --mode coordinator to CLI
- Parse host list
- Connect to nodes
- Coordinate synchronized start (100ms)
- Aggregate results
- Clock skew measurement

### Phase 5: Testing (6-8 hours)

- 3 nodes, 48 workers
- Single file and directory tree
- Layout_manifest reuse
- Dataset marker validation
- Failure scenarios
- Clock synchronization

**Total:** 34-44 hours

---

## Success Criteria

Distributed mode is complete when:

**Functional:**
- ✅ Single executable with three modes
- ✅ Can connect to 10 nodes
- ✅ Can run 160 workers (10 nodes × 16 threads)
- ✅ All workers start within 100ms
- ✅ Can handle 1M files in PARTITIONED mode
- ✅ Each file touched exactly once
- ✅ Layout_manifest export and reuse works
- ✅ Dataset markers skip recreation
- ✅ Node failure triggers clean abort

**Performance:**
- ✅ Coordinator overhead <5%
- ✅ Heartbeat processing <1% CPU
- ✅ Network bandwidth <1% of storage bandwidth
- ✅ Start delay: 100ms (>99.9% success rate)

**Usability:**
- ✅ Single binary to deploy
- ✅ Simple CLI
- ✅ Clear error messages
- ✅ Comprehensive documentation

---

## Reference Documents

**Source of Truth:**
- `.kiro/specs/iopulse/requirements.md` - All requirements
- `.kiro/specs/iopulse/tasks.md` - All implementation tasks

**Supporting Documentation:**
- `DISTRIBUTED_MODE_SPECIFICATION.md` - This document (consolidated spec)
- `START_DELAY_ANALYSIS.md` - 100ms timing analysis and rationale
- `ENHANCEMENT_BACKLOG.md` - Future features (deferred)
- `REQUIREMENTS_UPDATE_SUMMARY.md` - Summary of changes made

---

## Next Steps

1. ✅ **Requirements approved** - All changes incorporated in requirements.md
2. ⏳ **Update design.md** - Add distributed architecture
3. ⏳ **Begin Phase 1** - Implement Task 24k-tree
4. ⏳ **Test Phase 1** - Validate tree support
5. ⏳ **Begin Phase 2-4** - Implement distributed mode
6. ⏳ **Test Phase 5** - Comprehensive validation

**Ready to proceed with design phase?**
