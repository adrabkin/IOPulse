# IOPulse Enhancement Backlog
**Date:** January 24, 2026  
**Purpose:** Track future enhancements deferred from initial implementation

---

## Distributed Mode Enhancements

### 1. Distributed Data Verification (Deferred from Requirement 9)

**Description:** Coordinate data verification across distributed workers

**Use Case:**
- Verify data integrity in distributed tests
- Detect storage corruption across nodes
- Validate write-then-read patterns

**Requirements:**
- Coordinator distributes random seed to all workers
- All workers use same seed for deterministic patterns
- Coordinator aggregates verification failure counts
- Report per-node verification statistics

**Priority:** Low  
**Effort:** 4-6 hours

**Rationale for deferral:** Data verification is rarely used in storage benchmarking. Most tests focus on performance, not integrity. Can be added later if needed.

---

### 2. Network Interface Awareness in Distributed Mode (Deferred from Requirement 4a)

**Description:** Track and report per-interface statistics across distributed nodes

**Use Case:**
- Multi-homed nodes with multiple network interfaces
- Validate interface load balancing
- Debug network bottlenecks
- Multi-path configurations

**Requirements:**
- Worker nodes report available interfaces in READY message
- Coordinator collects per-interface statistics from all nodes
- Coordinator aggregates per-interface throughput globally
- Report: "Interface eth0: 40 GB/s (across 10 nodes)"

**Priority:** Medium  
**Effort:** 6-8 hours

**Rationale for deferral:** IOPulse already tracks aggregate throughput (what matters). Per-interface stats are useful for debugging but not essential for initial distributed mode. OS tools (iftop, nload) can provide this info if needed. Nice to have for ensuring multiple interfaces are utilized.

---

### 3. Worker Registration/Deregistration API

**Description:** Dynamic worker management without restarting coordinator

**Use Case:**
- Add nodes to running test
- Remove nodes from running test
- Scale up/down dynamically

**Requirements:**
- REST API on coordinator
- Register endpoint: POST /workers with node address
- Deregister endpoint: DELETE /workers/{node_id}
- Automatic workload rebalancing

**Priority:** Medium  
**Effort:** 8-10 hours

---

### 2. Dynamic Node Discovery Protocol

**Description:** Automatic discovery of available worker nodes

**Use Case:**
- Large clusters (100+ nodes)
- Cloud environments (auto-scaling)
- Avoid manual host list management

**Requirements:**
- Multicast or broadcast discovery
- Worker nodes announce availability
- Coordinator discovers and connects
- Health checking and auto-removal

**Priority:** Low  
**Effort:** 10-12 hours

---

### 3. TLS Encryption

**Description:** Encrypted communication for sensitive environments

**Use Case:**
- Cross-datacenter testing
- Untrusted networks
- Compliance requirements

**Requirements:**
- TLS 1.3 support
- Certificate management
- Minimal performance overhead (<5%)

**Priority:** Low  
**Effort:** 6-8 hours

---

### 4. Authentication Tokens

**Description:** Token-based authentication for coordinator/worker connections

**Use Case:**
- Multi-tenant environments
- Shared infrastructure
- Access control

**Requirements:**
- Token generation and validation
- Token expiration
- Token revocation

**Priority:** Low  
**Effort:** 4-6 hours

---

### 5. Chaos Testing / Failure Injection

**Description:** Inject failures to test distributed mode reliability

**Use Case:**
- Validate failure handling
- Test coordinator resilience
- Chaos engineering

**Requirements:**
- `--chaos-mode` flag
- Random node failures
- Random network delays
- Random message drops
- Configurable failure rates

**Priority:** Low  
**Effort:** 8-10 hours

---

### 6. Worker-to-Worker Communication

**Description:** Direct peer-to-peer communication between workers

**Use Case:**
- Distributed lock manager (DLM) testing
- Coordination without coordinator bottleneck
- Advanced distributed filesystem testing

**Requirements:**
- Peer discovery via coordinator
- Direct TCP connections between workers
- Lock coordination protocol
- Fallback to coordinator-mediated

**Priority:** Low  
**Effort:** 12-15 hours

---

### 7. Coordinator High Availability

**Description:** Backup coordinator for failover

**Use Case:**
- Long-running tests (days/weeks)
- Critical testing scenarios
- Avoid single point of failure

**Requirements:**
- Primary/backup coordinator election
- State replication
- Automatic failover
- Worker reconnection

**Priority:** Low  
**Effort:** 15-20 hours

---

### 8. Multi-Coordinator Support

**Description:** Multiple coordinators for >1000 nodes

**Use Case:**
- Massive scale testing (1000+ nodes)
- Coordinator becomes bottleneck
- Hierarchical coordination

**Requirements:**
- Coordinator hierarchy (master + sub-coordinators)
- Sub-coordinator aggregation
- Master coordinator final aggregation
- Scalability to 10,000+ nodes

**Priority:** Low  
**Effort:** 20-25 hours

---

## Standalone Mode Enhancements

### 9. S3/Object Storage Backend

**Description:** Support for cloud object storage testing

**Use Case:**
- S3 performance testing
- Object storage benchmarking
- Cloud-native workloads

**Requirements:**
- S3 API integration
- Multipart upload support
- Object versioning
- Bucket operations

**Priority:** Medium  
**Effort:** 15-20 hours

---

### 10. GPU Direct Storage

**Description:** NVIDIA GPUDirect Storage support

**Use Case:**
- AI/ML workload simulation
- GPU-accelerated IO
- High-performance computing

**Requirements:**
- cuFile API integration
- GPU memory buffers
- Direct GPU-to-storage transfers

**Priority:** Low  
**Effort:** 20-25 hours

---

### 11. Windows Support

**Description:** Native Windows IO engines

**Use Case:**
- Windows storage testing
- Cross-platform compatibility
- Enterprise Windows environments

**Requirements:**
- IOCP (IO Completion Ports) engine
- Windows async file IO
- Windows-specific optimizations

**Priority:** Low  
**Effort:** 15-20 hours

---

### 12. NVMe Passthrough

**Description:** Direct NVMe command submission

**Use Case:**
- NVMe-specific testing
- Bypass filesystem overhead
- Ultra-low latency testing

**Requirements:**
- NVMe admin/IO command support
- Direct queue pair access
- NVMe-specific statistics

**Priority:** Low  
**Effort:** 20-25 hours

---

## Output and Monitoring Enhancements

### 13. Prometheus Metrics Exporter

**Description:** Export real-time metrics in Prometheus format

**Use Case:**
- Integration with existing monitoring infrastructure
- Long-running tests with historical tracking
- Alerting on performance thresholds
- Multi-test correlation

**Requirements:**
- `/metrics` HTTP endpoint on coordinator
- Prometheus text format output
- Metrics updated from heartbeat deltas
- Labels: node_id, worker_id, operation_type
- Metrics: iops, throughput, latency percentiles, errors

**Architecture:**
- Coordinator already calculates deltas from heartbeats
- Add PrometheusExporter consumer that reads delta snapshots
- Update metrics on each heartbeat (1/sec)
- Expose via HTTP endpoint

**Priority:** High  
**Effort:** 6-8 hours

**Dependencies:** Task 40/44 (per-worker time-series collection)

**Example Metrics:**
```
iopulse_read_iops{node="node1:9999",worker="0"} 1250.5
iopulse_write_iops{node="node1:9999",worker="0"} 450.2
iopulse_latency_p99_microseconds{node="node1:9999",worker="0",op="read"} 125.0
```

---

### 14. Live CSV Output

**Description:** Write CSV rows as heartbeats arrive (streaming output)

**Use Case:**
- Monitor test progress in real-time
- Tail CSV file during long tests
- Immediate data availability for analysis
- No waiting for test completion

**Requirements:**
- `--live-csv` flag to enable
- CSV file written incrementally (append mode)
- Flush after each row for immediate visibility
- Works with both aggregate and per-worker modes

**Architecture:**
- Coordinator already has CSV writer
- Already calculates deltas from heartbeats
- Just need to write immediately instead of buffering
- Trivial implementation (~10 lines)

**Priority:** Medium  
**Effort:** 2-3 hours

**Dependencies:** Task 40/44 (per-worker time-series collection)

**Example Usage:**
```bash
# Start test with live CSV
./iopulse /mnt/data/test.dat --duration 300s --live-csv /mnt/data/live.csv

# In another terminal, watch progress
tail -f /mnt/data/live.csv
```

---

### 15. Real-Time Dashboard

**Description:** Web-based real-time monitoring dashboard

**Use Case:**
- Long-running tests
- Remote monitoring
- Visual analysis
- Multi-node visualization

**Requirements:**
- Web server with dashboard
- Real-time updates (WebSocket)
- Charts and graphs (IOPS, throughput, latency)
- Per-node/worker breakdown
- Responsive design (mobile-friendly)

**Architecture:**
- Coordinator runs HTTP server
- WebSocket endpoint for real-time updates
- Send delta snapshots to connected clients
- Browser renders charts using Chart.js or similar
- No database needed (streaming only)

**Priority:** Medium  
**Effort:** 12-15 hours

**Dependencies:** Task 40/44 (per-worker time-series collection)

**Features:**
- Live IOPS/throughput graphs
- Latency heatmaps
- Per-node breakdown
- Per-worker breakdown (optional)
- Error counters
- Resource utilization (CPU/memory)

---

### 16. Grafana Integration

**Description:** Native Grafana dashboard templates

**Use Case:**
- Enterprise monitoring
- Historical analysis
- Alerting
- Multi-test correlation

**Requirements:**
- Prometheus metrics (Enhancement #13)
- Pre-built Grafana dashboards
- Alert rule templates
- Documentation

**Priority:** Medium  
**Effort:** 4-6 hours (after Enhancement #13)

**Dependencies:** Enhancement #13 (Prometheus exporter)

---

### 17. InfluxDB Line Protocol Output

**Description:** Export metrics in InfluxDB line protocol format

**Use Case:**
- Time-series database storage
- Long-term historical analysis
- Integration with InfluxDB/Telegraf
- Custom dashboards

**Requirements:**
- InfluxDB line protocol format
- Batch writes for efficiency
- Configurable flush interval
- Support for tags and fields

**Architecture:**
- Similar to Prometheus exporter
- Read delta snapshots from coordinator
- Format as line protocol
- Write to InfluxDB via HTTP API or file

**Priority:** Low  
**Effort:** 6-8 hours

**Dependencies:** Task 40/44 (per-worker time-series collection)

**Example Output:**
```
iopulse,node=node1:9999,worker=0,op=read iops=1250.5,throughput_mbps=156.3,latency_p99_us=125.0 1643723400000000000
```

---

### 18. Streaming API / Consumer Trait

**Description:** Pluggable output architecture for extensibility

**Use Case:**
- Custom output formats
- Third-party integrations
- Research and experimentation
- Future-proofing

**Requirements:**
- `SnapshotConsumer` trait for output plugins
- Coordinator manages list of consumers
- Each heartbeat delta sent to all consumers
- Consumers can be buffered or unbuffered

**Architecture:**
```rust
trait SnapshotConsumer {
    fn consume(&mut self, snapshot: &AggregatedSnapshot) -> Result<()>;
    fn buffering_mode(&self) -> BufferingMode;  // Buffered | Immediate
    fn finalize(&mut self) -> Result<()>;  // Called at test end
}

// Coordinator:
for consumer in &mut consumers {
    consumer.consume(&delta_snapshot)?;
}
```

**Priority:** Low  
**Effort:** 4-6 hours

**Benefits:**
- Clean separation of concerns
- Easy to add new outputs
- Testable in isolation
- Plugin architecture foundation

---

### 19. Custom Metrics / User-Defined Metrics

**Description:** Allow users to define and track custom metrics

**Use Case:**
- Application-specific metrics
- Custom latency buckets
- Business logic metrics
- Research experiments

**Requirements:**
- Protocol extension for custom metrics
- HashMap<String, f64> in WorkerStatsSnapshot
- Aggregation in coordinator
- Output in JSON/CSV/Prometheus

**Architecture:**
```rust
// Protocol extension:
pub struct WorkerStatsSnapshot {
    // ... existing fields ...
    pub custom_metrics: Option<HashMap<String, f64>>,
}

// User API (future):
worker.record_custom_metric("cache_hit_rate", 0.95);
```

**Priority:** Low  
**Effort:** 8-10 hours

**Dependencies:** Protocol versioning

---

## Testing Enhancements

### 15. Automated Performance Regression

**Description:** Continuous performance monitoring

**Use Case:**
- Detect performance regressions
- Track performance over time
- CI/CD integration

**Requirements:**
- Baseline performance database
- Automated comparison
- Regression detection (>10% degradation)
- CI/CD pipeline integration

**Priority:** Medium  
**Effort:** 8-10 hours

---

### 16. Workload Templates

**Description:** Pre-defined workload profiles for common scenarios

**Use Case:**
- Quick testing without configuration
- Best practices
- Standardized benchmarks

**Requirements:**
- Template library (OLTP, OLAP, web cache, etc.)
- Template selection via CLI
- Template customization

**Priority:** Low  
**Effort:** 4-6 hours

---

## Total Backlog

**High Priority:** 1 item (Prometheus exporter)  
**Medium Priority:** 6 items (Network awareness, Live CSV, Dashboard, Grafana, InfluxDB, Regression)  
**Low Priority:** 14 items  

**Total Estimated Effort:** 250+ hours

---

## Phase 3 Extensibility Notes

**Current Architecture Strengths:**
- ✅ Clean separation: Nodes collect, coordinator processes
- ✅ Protocol is extensible (optional fields)
- ✅ Delta calculation centralized (single source of truth)
- ✅ Minimal overhead (10.8 KB/sec network, 54 KB memory, 132µs CPU)
- ✅ Streaming-ready (heartbeats every 1 second)

**What Makes Phase 3 Easy:**
- Coordinator already calculates deltas from cumulative heartbeat data
- `AggregatedSnapshot` is output-agnostic (can feed any consumer)
- Multiple consumers can read same delta data (no recalculation)
- Adding new outputs only touches coordinator (zero node changes)

**Implementation Pattern:**
```rust
// Coordinator heartbeat processing (happens once)
let delta_snapshot = calculate_delta(cumulative, previous);

// Multiple consumers (no recalculation needed)
csv_writer.append_snapshot(&delta_snapshot);           // CSV
json_builder.add_snapshot(&delta_snapshot);            // JSON
prometheus_exporter.update(&delta_snapshot);           // Prometheus
live_csv_writer.append_snapshot(&delta_snapshot);      // Live CSV
influxdb_writer.write(&delta_snapshot);                // InfluxDB
websocket_server.broadcast(&delta_snapshot);           // Dashboard
```

**Future Improvements:**
- Streaming API (SnapshotConsumer trait) for pluggable outputs
- Custom metrics support (user-defined metrics in protocol)
- Output plugins (load .so files for custom formats)

---

## Review Process

**To promote from backlog to requirements:**
1. User requests feature
2. Review use case and priority
3. Create detailed requirements
4. Add to requirements.md
5. Create implementation tasks
6. Implement and test

**To add new items:**
1. Describe feature and use case
2. Estimate effort
3. Assign priority
4. Add to this document

---

## Cosmetic Issues

### 17. Fix "Prepared X files (0 filled)" Message in Distributed Mode

**Description:** Misleading message when files are actually filled

**Issue:**
- Distributed write-only tests show: "Filling file region with random pattern..."
- Then show: "Prepared 1 files (0 filled)"
- File IS filled, but count reports 0

**Root Cause:**
- `preallocate_region()` returns `(1, if fill { 1 } else { 0 })`
- `fill` parameter is based on `has_reads` (false for write-only)
- But FileTarget auto-fills when `offset_range` is set (for XFS lazy allocation)
- So file gets filled, but function returns 0 for files_filled

**Fix:**
- Track whether refill actually happened in `preallocate_region()`
- Return accurate count regardless of `fill` parameter
- Or change message to be more accurate

**Priority:** Low (cosmetic only)  
**Effort:** 1 hour

**Impact:** None (functional behavior correct, just misleading message)



---

## File Distribution Enhancements

### 3. Per-Worker File Distribution for Directory Layouts (Task 45)

**Description:** Support `--file-distribution per-worker` with directory layouts and layout manifests

**Use Case:**
- Isolated worker testing with directory structures
- Aggregate file creation rate benchmarking
- Avoiding lock contention in metadata tests
- Testing per-worker file performance

**Current Behavior:**
- Per-worker distribution only works with single-file targets
- Flag is silently ignored when used with `--num-files`, `--num-dirs`, or layout manifests
- Users must use partitioned or shared distribution for layouts

**Expected Behavior:**
- Each worker gets its own copy of all files with `.workerN` suffix
- Example: 10 files × 5 dirs × 8 workers = 400 total files
- File naming: `file_000000.worker0`, `file_000000.worker1`, etc.
- Each worker only accesses files with its suffix (complete isolation)

**Requirements:**
- Modify `LayoutGenerator` to accept `num_workers` parameter
- Generate files with worker suffixes when per-worker mode enabled
- Coordinator passes total worker count to layout generator
- Workers filter file list to only their files (`.workerN` suffix)
- Layout manifests support per-worker files (include worker count in header)
- Works in both standalone and distributed modes

**Implementation Components:**
1. `src/target/layout.rs`: Add `num_workers` to `LayoutConfig`, modify `create_files()`
2. `src/distributed/coordinator.rs`: Pass worker count to generator, filter file lists per node
3. `src/worker/mod.rs`: Filter file list to only worker's files
4. `src/target/layout_manifest.rs`: Add `num_workers` to manifest header
5. `src/config/mod.rs`: Add `num_workers` field to `LayoutConfig`

**Testing:**
- Unit tests for `LayoutGenerator` with `num_workers`
- Integration tests for simple layouts, complex trees, manifests
- Distributed mode testing
- Verify file count = base_files × num_workers
- Verify worker isolation (each worker only accesses its files)

**Priority:** Medium  
**Effort:** 6-8 hours  
**Risk:** LOW (isolated changes, follows existing partitioned mode pattern)

**Rationale for deferral:** Feature gap discovered during testing. Per-worker distribution is useful but not critical - users can work around with partitioned mode. Clean implementation requires coordination between multiple components. Well-scoped task with clear requirements and low risk.

**Documentation:** See `docs/working_docs/NEXT_SESSION_START_HERE_FEB_01.md` for detailed implementation plan.

**Related Requirements:** 1a.7-1a.10, 5d.10

