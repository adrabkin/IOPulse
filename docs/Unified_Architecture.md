## IOPulse Unified Architecture - Visual Guide

### High-Level Flow

```
User Command
    ↓
main.rs (run_standalone)
    ↓
Auto-launch localhost service on random port
    ↓
DistributedCoordinator connects to localhost:PORT
    ↓
[SAME CODE PATH AS DISTRIBUTED MODE]
    ↓
Results returned to user
```

**Key Point:** Standalone and distributed use IDENTICAL code path!

---

### Detailed Message Flow

```
PHASE 1: SETUP
==============

Coordinator                          Node Service (localhost or remote)
-----------                          ----------------------------------
                                     [Listening on port 9999]
                                     
Connect via TCP ------------------>  Accept connection
                                     
Send CONFIG message --------------->  Receive CONFIG
  - WorkloadConfig                     - Parse configuration
  - TargetConfig                       - Spawn N worker threads
  - Worker ID range                    - Initialize IO engines
  - File list (if any)                 - Open target files
                                     
Wait for READY <--------------------  Send READY message
                                       - All workers prepared
                                       - Ready to start IO


PHASE 2: SYNCHRONIZED START
============================

Coordinator                          Node Service
-----------                          ------------

Calculate start time
(now + 2 seconds)

Send START message ---------------->  Receive START
  - start_timestamp                    - Wait until local time >= start_timestamp
                                       - BEGIN IO operations
                                       
                                       Workers start IO loop:
                                       - Generate offsets
                                       - Submit IO operations
                                       - Poll completions
                                       - Update statistics


PHASE 3: MONITORING (Every 1 second)
=====================================

Coordinator                          Node Service
-----------                          ------------

Wait for heartbeats                  Heartbeat loop (every 1 second):
                                     
                                     1. Lock shared_snapshots
                                     2. Aggregate all worker stats
                                     3. Serialize histograms (bincode)
                                     4. Sample CPU/memory (/proc/self/stat)
                                     5. Build HeartbeatMessage:
                                        - node_id
                                        - elapsed_ns
                                        - stats (CUMULATIVE)
                                        - per_worker_stats (if --json-per-worker)
                                     
Receive HEARTBEAT <-----------------  Send HEARTBEAT
  - Process cumulative stats           
  - Calculate DELTA from previous    
  - Store delta in time_series       
  - Store resource stats             
                                     
Send HEARTBEAT_ACK ---------------->  Receive ACK
                                       - Reset dead man's switch timer


PHASE 4: COMPLETION
===================

Coordinator                          Node Service
-----------                          ------------

Test duration elapsed

Send STOP message ----------------->  Receive STOP
                                       - Signal workers to stop
                                       - Wait for in-flight ops
                                       - Collect final stats
                                       
Wait for RESULTS <------------------  Send RESULTS message
  - Aggregate results                  - per_worker_stats (all workers)
  - Generate output                    - aggregate_stats (node total)
  - Write JSON/CSV                     - duration_ns
```

---

### Data Structures & Storage

#### Coordinator Side

**Time-Series Storage:**
```rust
// Per-node snapshots (DELTA values, not cumulative)
let mut time_series_snapshots: Vec<Vec<AggregatedSnapshot>> = vec![Vec::new(); num_nodes];
//   ^node_idx          ^timestamp

// Per-node resource stats
let mut time_series_resource_stats: Vec<Vec<ResourceStats>> = vec![Vec::new(); num_nodes];
//   ^node_idx                      ^timestamp

// Previous cumulative (for delta calculation)
let mut previous_cumulative: Vec<Option<AggregatedSnapshot>> = vec![None; num_nodes];
//   ^node_idx
```

**Update Frequency:** Every 1 second (when heartbeat arrives)

**Data Flow:**
1. Receive heartbeat with CUMULATIVE stats
2. Calculate DELTA: current - previous
3. Store DELTA in time_series_snapshots
4. Update previous with current cumulative
5. Store resource stats

#### Node Service Side

**Worker Stats Storage:**
```rust
// Shared between workers and heartbeat loop
let shared_snapshots: Arc<Mutex<Vec<StatsSnapshot>>> = Arc::new(Mutex::new(vec![...]));
//                                  ^worker_id
```

**Worker Update Frequency:**
- mmap engine: Every 1000 operations
- Other engines: Every 1 operation (for precision)

**Heartbeat Collection:**
1. Lock shared_snapshots
2. Aggregate all worker snapshots
3. Serialize histograms
4. Sample CPU/memory
5. Send to coordinator

---

### Message Types & Frequency

| Message | Direction | When | Frequency | Size | Contains |
|---------|-----------|------|-----------|------|----------|
| **CONFIG** | Coord → Node | Setup | Once | ~2 KB | Full test configuration |
| **READY** | Node → Coord | Setup | Once | ~100 B | Node ready signal |
| **START** | Coord → Node | Start | Once | ~50 B | Start timestamp |
| **HEARTBEAT** | Node → Coord | Running | Every 1s | ~1-2 KB | Cumulative stats, histograms, CPU/mem |
| **HEARTBEAT_ACK** | Coord → Node | Running | Every 1s | ~10 B | Acknowledgment |
| **STOP** | Coord → Node | End | Once | ~10 B | Stop signal |
| **RESULTS** | Node → Coord | End | Once | ~5-10 KB | Final stats, per-worker data |

**Total Network Traffic (3 nodes, 5 second test):**
- Setup: ~6 KB (CONFIG × 3)
- Heartbeats: ~18 KB (3 nodes × 3 KB × 5 seconds)
- Results: ~30 KB (3 nodes × 10 KB)
- **Total: ~54 KB for entire test** (negligible)

---

### Performance Data Collection

#### Worker Level (Continuous)

**What's Collected:**
```rust
pub struct StatsSnapshot {
    pub read_ops: u64,           // Cumulative
    pub write_ops: u64,          // Cumulative
    pub read_bytes: u64,         // Cumulative
    pub write_bytes: u64,        // Cumulative
    pub errors: u64,             // Cumulative
    pub read_latency: SimpleHistogram,   // Merged
    pub write_latency: SimpleHistogram,  // Merged
    pub metadata_*_ops: u64,     // Cumulative
    pub metadata_*_latency: SimpleHistogram,  // Merged
}
```

**Update Frequency:**
- Every 1000 ops (mmap) or every 1 op (others)
- Stored in `Arc<Mutex<Vec<StatsSnapshot>>>`
- Lock-free reads by heartbeat loop

#### Node Level (Every 1 Second)

**Heartbeat Loop:**
1. Lock shared_snapshots (brief)
2. Aggregate all workers:
   - Sum ops/bytes/errors
   - Merge histograms
3. Sample resource utilization:
   - Read `/proc/self/stat` for CPU
   - Read `/proc/self/status` for memory
4. Serialize histograms (bincode)
5. Send to coordinator

**Data Sent:**
- Aggregate stats (CUMULATIVE)
- Per-worker stats (CUMULATIVE, if --json-per-worker)
- CPU/memory usage
- Serialized histograms

#### Coordinator Level (Every 1 Second)

**Heartbeat Processing:**
1. Receive heartbeat from node
2. Deserialize histograms
3. Calculate DELTA from previous cumulative
4. Store DELTA in time_series_snapshots[node_idx]
5. Store resource stats
6. Update previous cumulative
7. Send ACK

**Storage:**
- Per-node time-series (DELTA values)
- Per-node resource stats
- Previous cumulative (for next delta)

---

### Data Transformation Pipeline

```
Worker Stats (Cumulative)
    ↓
Heartbeat (Cumulative, every 1s)
    ↓
Coordinator Receives (Cumulative)
    ↓
Calculate Delta (current - previous)
    ↓
Store Delta in time_series_snapshots
    ↓
JSON/CSV Output (Delta values = interval rates)
```

**Why Deltas?**
- Workers track cumulative totals (simpler, faster)
- Coordinator calculates intervals for IOPS/throughput
- Output shows per-second rates (what users expect)

**Example:**
```
Heartbeat 1: read_ops = 1000 (cumulative)
Heartbeat 2: read_ops = 2500 (cumulative)
Delta: 2500 - 1000 = 1500 ops in 1 second
IOPS: 1500 ops/s
```

---

### Standalone vs Distributed (Unified!)

#### Standalone Mode

```
User runs: ./iopulse /mnt/data/test.dat --duration 5s

main.rs:
  1. Find available port (e.g., 10004)
  2. Launch service: spawn("iopulse --mode service --listen-port 10004")
  3. Wait 500ms for service to start
  4. Create DistributedCoordinator with host_list = ["localhost:10004"]
  5. Connect to localhost:10004
  6. [SAME FLOW AS DISTRIBUTED]
  7. Collect results
  8. Kill service process
  9. Display results
```

**Key Point:** Standalone is just distributed with 1 node on localhost!

#### Distributed Mode

```
User runs: ./iopulse --mode coordinator --host-list node1:9999,node2:9999,node3:9999 /mnt/efs/test.dat

main.rs:
  1. Parse host list
  2. Create DistributedCoordinator with host_list
  3. Connect to all nodes
  4. [SAME FLOW AS STANDALONE]
  5. Collect results from all nodes
  6. Aggregate results
  7. Display results
```

**Key Point:** Same code, just different host list!

---

### Worker Statistics Update Flow

```
Worker Thread                    Shared Memory                  Heartbeat Loop
-------------                    -------------                  --------------

IO Operation completes
    ↓
Update counters:
  - read_ops++
  - read_bytes += size
  - read_latency.record(duration)
    ↓
Every 1000 ops (mmap) or
Every 1 op (others):
    ↓
Lock shared_snapshots ----------> shared_snapshots[worker_id]
Update snapshot                   = current_stats
Unlock                            
                                                                 Every 1 second:
                                                                     ↓
                                                                 Lock shared_snapshots
                                                                 Aggregate all workers
                                                                 Unlock
                                                                     ↓
                                                                 Send HEARTBEAT
```

**Locking Strategy:**
- Workers: Brief lock every 1000 ops (mmap) or 1 op (others)
- Heartbeat: Brief lock every 1 second
- Minimal contention (different frequencies)

---

### Output Generation

```
Time-Series Snapshots (stored in coordinator)
    ↓
Per-Node Data at Each Timestamp
    ↓
JSON Generation:
  - time_series[].nodes[] = per-node stats
  - time_series[].aggregate = sum of all nodes
  - final_summary.aggregate = total
  - final_summary.per_worker = all workers (with node_id)
    ↓
CSV Generation:
  - Per-node files: node1.csv, node2.csv, node3.csv
  - Aggregate file: aggregate.csv (all nodes, with node_id column)
```

---