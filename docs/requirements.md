# Requirements Document

## Introduction

IOPulse is a high-performance IO load generation and profiling tool written in Rust. The core goal is to create an efficient IO load generator that "gets out of the way" of the test—ensuring IO operations are bound by the device/filesystem being tested, not constrained by the tool itself.

IOPulse supports multiple IO backends (io_uring, libaio, sync, mmap), targets block devices and filesystems (NFS, Lustre, XFS, raw block), and operates in both standalone and distributed modes. The architecture is modular to allow future extensions (S3, GPU direct storage, NVMe passthrough, Windows) without requiring rewrites of existing modules.

## Glossary

- **IOPulse**: The IO load generation and profiling tool being specified
- **Coordinator**: The control node in distributed mode that orchestrates Nodes and aggregates results
- **Node**: A physical or virtual host in distributed mode that runs one or more Workers
- **Worker**: A thread that executes IO operations (in standalone: thread on local host; in distributed: thread on a Node)
- **IO_Engine**: A pluggable backend that performs actual IO operations (io_uring, libaio, sync, mmap)
- **Workload**: A defined set of IO operations including read/write mix, block sizes, access patterns
- **Profile**: A complete test configuration including workload, targets, and runtime parameters
- **Target**: A file, directory, or block device that receives IO operations
- **Layout_Manifest**: A text file defining directory structure and file paths for reproducible testing
- **Latency_Histogram**: A data structure tracking IO completion time distribution using logarithmic buckets
- **Live_Stats**: Real-time statistics collected during test execution
- **Stonewall**: A synchronization point where all workers wait for the slowest to complete a phase
- **IOPS**: Input/Output Operations Per Second
- **Throughput**: Data transfer rate, typically measured in MB/s or GB/s
- **Queue_Depth**: Number of outstanding IO operations submitted but not yet completed
- **Submission_Latency**: Time from IO request creation to submission to the kernel
- **Completion_Latency**: Time from IO submission to completion notification
- **Total_Latency**: End-to-end time from request creation to completion
- **Think_Time**: Configurable delay between IO operations to simulate application processing time
- **Zipf_Distribution**: Statistical distribution following power law where small number of items are accessed frequently
- **Pareto_Distribution**: Statistical distribution following Pareto principle (80/20 rule) for hot/cold data access
- **Gaussian_Distribution**: Normal distribution with bell curve around a center point for locality testing
- **fadvise**: POSIX function to provide file access pattern hints to the kernel for cache optimization
- **madvise**: POSIX function to provide memory access pattern hints to the kernel for mmap optimization
- **File_Lock**: Exclusive or shared lock on file or byte range to control concurrent access

## Requirements

### Requirement 1: Core IO Operations

**User Story:** As a storage engineer, I want to generate various IO workloads against storage targets, so that I can measure and analyze storage performance characteristics.

#### Acceptance Criteria

1. THE IOPulse SHALL support sequential read operations with configurable block sizes from 512 bytes to 64MB
2. THE IOPulse SHALL support sequential write operations with configurable block sizes from 512 bytes to 64MB
3. THE IOPulse SHALL support random read operations with configurable block sizes and offset alignment
4. THE IOPulse SHALL support random write operations with configurable block sizes and offset alignment
5. THE IOPulse SHALL support mixed read/write workloads with configurable read percentage (0-100%)
6. WHEN a workload specifies multiple block sizes, THE IOPulse SHALL distribute IO operations according to a configurable block size distribution
7. THE IOPulse SHALL support configurable IO queue depth from 1 to 1024 outstanding operations
8. WHEN direct IO is requested, THE IOPulse SHALL bypass the page cache for all IO operations
9. THE IOPulse SHALL support runtime duration as a completion criterion (run for N seconds)
10. THE IOPulse SHALL support total IO amount as a completion criterion (transfer N bytes total)
11. THE IOPulse SHALL support "run until complete" mode where the test completes when all specified operations finish (e.g., read all 100,000 files once)
12. THE IOPulse SHALL report time-to-completion when running in "run until complete" mode

### Requirement 1a: File Distribution Strategies

**User Story:** As a storage engineer, I want to control how files are distributed among workers, so that I can simulate different real-world access patterns.

#### Acceptance Criteria

1. THE IOPulse SHALL support "shared" file distribution where every worker accesses every target file
2. THE IOPulse SHALL support "partitioned" file distribution where target files are divided among workers with no overlap
3. WHEN using shared distribution, THE IOPulse SHALL coordinate access to prevent workers from interfering with each other's measurements
4. WHEN using partitioned distribution, THE IOPulse SHALL evenly divide files among workers (with remainder files assigned to early workers)
5. THE IOPulse SHALL support configurable file access order within each worker (sequential or random)
6. WHEN multiple workers access the same file in shared mode, THE IOPulse SHALL support configurable offset strategies (same offsets, different offsets, or interleaved)
7. THE IOPulse SHALL support "per-worker-files" distribution where each worker operates on its own uniquely named file
8. WHEN using per-worker-files distribution, THE IOPulse SHALL generate file names with worker ID suffix (e.g., testfile.0, testfile.1, testfile.2)
9. WHEN using per-worker-files distribution, THE IOPulse SHALL create files of equal size as specified in configuration
10. THE IOPulse SHALL support configurable file naming patterns for per-worker files (suffix, prefix, or custom template)

### Requirement 1b: Composite Workload Definition

**User Story:** As a storage engineer, I want to define complex mixed workloads with nested IO distributions, so that I can accurately simulate real-world application IO patterns.

#### Acceptance Criteria

1. THE IOPulse SHALL support defining a top-level read/write percentage split (e.g., 80% read, 20% write)
2. THE IOPulse SHALL support defining sub-distributions within each operation type specifying access pattern and block size combinations
3. WHEN a sub-distribution is defined, THE IOPulse SHALL support specifying: percentage weight, access pattern (random/sequential), and block size
4. THE IOPulse SHALL validate that sub-distribution percentages within each operation type sum to 100%
5. THE IOPulse SHALL select operations according to the defined probability distributions during test execution
6. WHEN writes are included in a shared-file workload, THE IOPulse SHALL support "partitioned-write-regions" mode where each worker owns exclusive write offset ranges
7. WHEN writes are included in a shared-file workload, THE IOPulse SHALL support "append-only" mode where workers append to separate files or use atomic append operations
8. IF a workload definition would cause write conflicts in distributed mode, THEN THE IOPulse SHALL warn the user and require explicit acknowledgment or automatic region partitioning
9. THE IOPulse SHALL support saving and loading composite workload definitions as named profiles

### Requirement 1c: Access Pattern Distributions

**User Story:** As a storage engineer, I want to use realistic statistical distributions for random access patterns, so that I can simulate hot/cold data scenarios and test cache effectiveness.

#### Acceptance Criteria

1. THE IOPulse SHALL support uniform random distribution as the default random access pattern
2. THE IOPulse SHALL support Zipf distribution for random offsets with configurable theta parameter (range 0.0-3.0, default 1.2)
3. THE IOPulse SHALL support Pareto distribution for random offsets with configurable h parameter (range 0.0-10.0, default 0.9)
4. THE IOPulse SHALL support Gaussian (normal) distribution for random offsets with configurable standard deviation and center point
5. WHEN Zipf distribution is selected, THE IOPulse SHALL generate offsets where a small percentage of blocks receive the majority of accesses (power law behavior)
6. WHEN Pareto distribution is selected, THE IOPulse SHALL generate offsets following the Pareto principle (e.g., 80% of accesses to 20% of data)
7. WHEN Gaussian distribution is selected, THE IOPulse SHALL generate offsets clustered around a configurable center point with specified standard deviation
8. THE IOPulse SHALL validate distribution parameters and report errors for out-of-range values
9. THE IOPulse SHALL report which distribution was used in test results and configuration output

### Requirement 1d: Think Time

**User Story:** As a storage engineer, I want to introduce configurable delays between IO operations, so that I can simulate realistic application behavior where processing occurs between IOs.

#### Acceptance Criteria

1. THE IOPulse SHALL support configurable think time (delay between IO operations) from 0 microseconds to 1 second
2. THE IOPulse SHALL support think time specified in microseconds, milliseconds, or seconds
3. THE IOPulse SHALL support "sleep" mode where the worker thread sleeps for the think time duration
4. THE IOPulse SHALL support "spin" mode where the worker thread busy-waits (CPU spin) for the think time duration
5. THE IOPulse SHALL support applying think time after every N blocks instead of after every IO operation
6. THE IOPulse SHALL support adaptive think time as a percentage of the previous IO operation's latency
7. WHEN think time is configured, THE IOPulse SHALL exclude think time from latency measurements
8. WHEN think time is configured, THE IOPulse SHALL report effective IOPS accounting for think time delays
9. THE IOPulse SHALL support different think time configurations per operation type (read vs write)

### Requirement 2: IO Engine Abstraction

**User Story:** As a developer, I want IOPulse to support multiple IO backends through a pluggable architecture, so that I can test different IO submission mechanisms and extend the tool with new backends.

#### Acceptance Criteria

1. THE IO_Engine SHALL define a common trait interface for all IO backends including: setup, init, submit, poll_completions, and cleanup operations
2. THE IOPulse SHALL support the io_uring backend for Linux systems with kernel 5.1+
3. THE IOPulse SHALL support the libaio backend for Linux systems
4. THE IOPulse SHALL support synchronous IO (pread/pwrite) as a baseline backend
5. THE IOPulse SHALL support memory-mapped IO (mmap) backend
6. WHEN an IO_Engine is registered, THE IOPulse SHALL make it available for selection via configuration
7. THE IO_Engine interface SHALL support both synchronous and asynchronous completion models
8. WHEN a new IO_Engine is added, THE existing engines and core logic SHALL remain unmodified

### Requirement 2a: Kernel IO Hints

**User Story:** As a storage engineer, I want to provide access pattern hints to the kernel, so that I can optimize cache behavior and test filesystem responsiveness to hints.

#### Acceptance Criteria

1. THE IOPulse SHALL support posix_fadvise() hints for file IO operations
2. THE IOPulse SHALL support POSIX_FADV_SEQUENTIAL hint to indicate sequential access pattern
3. THE IOPulse SHALL support POSIX_FADV_RANDOM hint to indicate random access pattern
4. THE IOPulse SHALL support POSIX_FADV_WILLNEED hint to prefetch data into page cache
5. THE IOPulse SHALL support POSIX_FADV_DONTNEED hint to drop data from page cache
6. THE IOPulse SHALL support POSIX_FADV_NOREUSE hint to indicate one-time access
7. THE IOPulse SHALL support madvise() hints for memory-mapped IO operations
8. THE IOPulse SHALL support MADV_SEQUENTIAL, MADV_RANDOM, MADV_WILLNEED, and MADV_DONTNEED for mmap operations
9. THE IOPulse SHALL support MADV_HUGEPAGE and MADV_NOHUGEPAGE hints to control transparent huge page usage
10. THE IOPulse SHALL allow multiple fadvise/madvise hints to be combined (e.g., sequential + willneed)
11. THE IOPulse SHALL apply fadvise hints after file open and before IO operations begin
12. THE IOPulse SHALL apply madvise hints after mmap and before IO operations begin
13. THE IOPulse SHALL report which hints were applied in test configuration output

### Requirement 2b: File Locking Modes

**User Story:** As a storage engineer, I want to apply file locks during IO operations, so that I can measure locking overhead and test distributed lock manager performance.

#### Acceptance Criteria

1. THE IOPulse SHALL support three file locking modes: none, range, and full
2. WHEN locking mode is "none", THE IOPulse SHALL perform no file locking (default behavior)
3. WHEN locking mode is "range", THE IOPulse SHALL acquire an exclusive lock on the specific byte range before each IO operation using fcntl() with F_SETLKW
4. WHEN locking mode is "full", THE IOPulse SHALL acquire an exclusive lock on the entire file before each IO operation
5. THE IOPulse SHALL release locks immediately after each IO operation completes
6. THE IOPulse SHALL use blocking locks (F_SETLKW) that wait for lock acquisition
7. THE IOPulse SHALL track lock acquisition time separately from IO operation time
8. THE IOPulse SHALL report lock acquisition latency statistics (min, avg, max, percentiles) when locking is enabled
9. THE IOPulse SHALL report lock acquisition failures and errors
10. WHEN file locking is enabled in distributed mode, THE IOPulse SHALL coordinate locks across all workers
11. THE IOPulse SHALL support configuring locking mode per workload phase

### Requirement 3: Target Support

**User Story:** As a storage engineer, I want to test various storage targets including filesystems and block devices, so that I can profile different storage configurations.

#### Acceptance Criteria

1. THE IOPulse SHALL support regular files on local filesystems (XFS, ext4, etc.) as targets
2. THE IOPulse SHALL support files on network filesystems (NFS, Lustre) as targets
3. THE IOPulse SHALL support raw block devices as targets
4. WHEN targeting a file, THE IOPulse SHALL create the file if it does not exist
5. WHEN targeting a file, THE IOPulse SHALL support pre-allocation to avoid fragmentation
6. THE IOPulse SHALL support multiple targets per workload with configurable distribution
7. WHEN multiple targets are specified, THE IOPulse SHALL distribute workers across targets according to configuration

### Requirement 3a: Directory Tree Generation

**User Story:** As a storage engineer, I want to generate directory trees with configurable depth and width, so that I can test filesystem metadata performance and simulate realistic directory structures.

#### Acceptance Criteria

1. THE IOPulse SHALL support configurable directory tree depth (number of nested directory levels)
2. THE IOPulse SHALL support configurable directory width (number of subdirectories per directory level)
3. THE IOPulse SHALL support configurable number of files per directory
4. THE IOPulse SHALL evenly distribute files across the generated directory tree
5. THE IOPulse SHALL support configurable file sizes for generated files (fixed size or size distribution)
6. THE IOPulse SHALL support configurable file naming patterns (sequential, random, prefixed)
7. WHEN generating directory trees, THE IOPulse SHALL report metadata operation statistics (mkdir, create, stat times)

### Requirement 3b: Custom Tree Definition

**User Story:** As a storage engineer, I want to define custom directory and file structures from a definition file, so that I can recreate specific filesystem layouts for reproducible testing.

#### Acceptance Criteria

1. THE IOPulse SHALL support reading directory/file structure definitions from a tree definition file
2. THE tree definition file SHALL support specifying directories with their full paths
3. THE tree definition file SHALL support specifying files with their paths and sizes
4. THE tree definition file SHALL support comments for documentation
5. WHEN a tree definition file is provided, THE IOPulse SHALL create the specified structure before running IO operations
6. THE IOPulse SHALL validate tree definition files and report errors for invalid entries
7. THE IOPulse SHALL support exporting an existing directory structure to a tree definition file format

### Requirement 3c: Directory Tree with Layout_Manifest Support

**User Story:** As a storage engineer, I want to generate directory trees with a specified total number of files and save/reuse layout definitions, so that I can test metadata performance with reproducible file structures.

#### Acceptance Criteria

**Tree Generation with Total Files:**
1. THE IOPulse SHALL support `--total-files` parameter to specify total number of files to generate
2. WHEN `--total-files` is specified with `--dir-depth` and `--dir-width`, THE IOPulse SHALL calculate files_per_dir automatically
3. THE calculation SHALL be: `files_per_dir = ceil(total_files / total_directories)`
4. THE IOPulse SHALL distribute files evenly across all leaf and intermediate directories
5. IF total_files is not evenly divisible, THE IOPulse SHALL distribute remainder files to early directories

**Layout_Manifest Precedence:**
6. WHEN `--layout-manifest` is provided, THE IOPulse SHALL use the layout manifest as the definitive structure
7. WHEN `--layout-manifest` is provided, THE IOPulse SHALL ignore `--dir-depth`, `--dir-width`, `--total-files`, and `--num-files` parameters
8. THE IOPulse SHALL log a warning if layout manifest is provided with conflicting parameters
9. THE layout manifest file SHALL contain one file path per line (relative to root directory)
10. THE layout manifest file SHALL support comments (lines starting with #)
11. THE layout manifest file SHALL support blank lines (ignored during parsing)

**Layout_Manifest Export:**
12. THE IOPulse SHALL support `--export-layout-manifest <path>` parameter to save generated tree structure
13. WHEN `--export-layout-manifest` is specified, THE IOPulse SHALL generate the tree and save file list to specified path
14. THE exported layout manifest SHALL include: header comment with generation parameters, total file count, and one file path per line
15. THE exported layout manifest SHALL be usable as input via `--layout-manifest` in subsequent tests
16. THE IOPulse SHALL report: "Layout manifest exported to <path> (N files)"
17. THE layout manifest file extension SHALL be `.layout_manifest` or `.lm`

**File Distribution Modes:**
18. THE IOPulse SHALL support SHARED mode where all workers access all files in the tree
19. THE IOPulse SHALL support PARTITIONED mode where files are divided among workers
20. THE IOPulse SHALL support PER_WORKER mode where each worker creates its own tree
21. IN PARTITIONED mode, THE Coordinator SHALL assign file ranges to workers (e.g., Worker 0: files 0-999)
22. IN PARTITIONED mode, each file SHALL be accessed by exactly one worker
23. THE Worker SHALL select files from its assigned range during execution
24. THE IOPulse SHALL report file distribution strategy and file count in output
25. WHEN a directory tree is generated, THE IOPulse SHALL create an in-memory file list of all paths
26. THE Worker SHALL support iterating through file list instead of single target

**Example Usage:**
```bash
# Generate tree and export layout manifest
iopulse /mnt/nfs/tree --dir-depth 3 --dir-width 10 --total-files 1000000 \
  --export-layout-manifest tree_1M.layout_manifest --duration 0

# Reuse layout manifest (faster, no regeneration)
iopulse /mnt/nfs/tree --layout-manifest tree_1M.layout_manifest --duration 30s

# Layout manifest overrides other parameters (warning logged)
iopulse /mnt/nfs/tree --layout-manifest tree_1M.lm --dir-depth 5  # depth=5 ignored
```

**Layout_Manifest Format:**
```
# IOPulse Layout Manifest
# Generated: 2026-01-24 10:30:00
# Parameters: depth=3, width=10, total_files=1000000
# Total files: 1000000
#
dir_0000/file_000000
dir_0000/file_000001
dir_0000/dir_0000/file_000000
...
```

### Requirement 4: Worker Architecture

**User Story:** As a performance engineer, I want IOPulse to efficiently utilize multiple CPU cores, so that I can generate maximum IO load without the tool becoming a bottleneck.

#### Acceptance Criteria

1. THE IOPulse SHALL support configurable number of worker threads with no artificial upper limit
2. WHEN thread count exceeds CPU core count, THE IOPulse SHALL warn the user but proceed with the requested configuration
3. WHEN workers are created, THE IOPulse SHALL support optional CPU core affinity binding
4. WHEN workers are created, THE IOPulse SHALL support optional NUMA node binding
5. THE Worker SHALL maintain independent statistics counters using atomic operations for lock-free updates
6. THE Worker SHALL support rate limiting to cap IOPS or throughput per worker
7. WHEN a stonewall is configured, THE IOPulse SHALL record statistics at the point when the first worker completes
8. THE Worker SHALL track per-operation latency and update the Latency_Histogram

### Requirement 4a: Network Interface Awareness

**User Story:** As a storage engineer with multi-homed systems, I want IOPulse to intelligently distribute workers across network interfaces, so that I can maximize network throughput and test multi-path configurations.

#### Acceptance Criteria

1. THE IOPulse SHALL detect available network interfaces on the system
2. THE IOPulse SHALL support explicit network interface binding for workers via configuration
3. WHEN multiple network interfaces are available and auto-balance is enabled, THE IOPulse SHALL distribute workers across interfaces
4. WHEN distributing workers across interfaces, THE IOPulse SHALL consider NUMA topology to bind workers to CPUs local to their assigned network interface
5. THE IOPulse SHALL support specifying interface-to-target mappings for multi-path configurations
6. THE IOPulse SHALL report per-interface throughput statistics when multiple interfaces are in use
7. WHEN network interface binding fails, THE IOPulse SHALL report the error and fall back to default routing

### Requirement 5: Distributed Mode

**User Story:** As a storage administrator, I want to run IOPulse across multiple hosts, so that I can generate aggregate load exceeding single-host capabilities and test distributed storage systems.

#### Acceptance Criteria

1. THE IOPulse SHALL support a Coordinator mode that orchestrates remote Worker nodes
2. THE IOPulse SHALL support a Worker service mode that accepts instructions from a Coordinator
3. WHEN in distributed mode, THE Coordinator SHALL read target node addresses from a configuration file or CLI argument
4. WHEN in distributed mode, THE Coordinator SHALL distribute workload configuration to all Worker nodes
5. WHEN in distributed mode, THE Worker nodes SHALL send periodic heartbeat messages with summary statistics to the Coordinator
6. WHEN in distributed mode, THE Worker nodes SHALL send complete results to the Coordinator upon phase completion
7. THE distributed communication protocol SHALL use a binary format for efficiency
8. WHEN a Worker node becomes unreachable, THE Coordinator SHALL abort the test and report the failure
9. THE final results SHALL clearly indicate the number of active Worker nodes vs configured Worker nodes
10. THE Coordinator SHALL aggregate statistics from all Worker nodes for unified reporting

### Requirement 5a: Connection Management

**User Story:** As a storage administrator, I want IOPulse to connect to worker nodes using a simple static configuration, so that I can run distributed tests without complex setup.

#### Acceptance Criteria

1. THE Coordinator SHALL read worker node addresses from `--host-list` CLI argument (comma-separated IPs/hostnames)
2. THE Coordinator SHALL read worker node addresses from `clients.list` file (one host per line) if `--host-list` not provided
3. THE `clients.list` file SHALL support comments (lines starting with #) and blank lines
4. THE Coordinator SHALL establish TCP connections to worker nodes on configurable port (default 9999, via `--worker-port`)
5. THE Coordinator SHALL send protocol version in first message to each worker node
6. THE Worker nodes SHALL verify protocol version matches before accepting coordinator
7. IF protocol version mismatch, THE Worker node SHALL reject connection with error message including expected and actual versions
8. THE Coordinator SHALL retry failed connections with exponential backoff (1s, 2s, 4s, max 3 attempts)
9. THE Coordinator SHALL maintain persistent connections for test duration
10. THE Coordinator SHALL abort test if any worker node connection fails after all retries
11. THE Coordinator SHALL report connection status for each worker node during initialization

**Example:**
```bash
# Using command line
iopulse-coordinator --host-list 10.0.1.10,10.0.1.11,10.0.1.12 --threads 16 ...

# Using clients.list file
# clients.list:
# 10.0.1.10
# 10.0.1.11
# 10.0.1.12
iopulse-coordinator --clients-file clients.list --threads 16 ...

# Result: 3 nodes × 16 workers = 48 total workers
```

### Requirement 5b: Worker Failure Handling

**User Story:** As a storage engineer, I want distributed tests to abort cleanly if any worker node fails, so that I get reliable results or clear failure indication.

#### Acceptance Criteria

1. THE Worker nodes SHALL send heartbeat messages to Coordinator every 1 second
2. THE Coordinator SHALL send heartbeat acknowledgments to worker nodes
3. WHEN a Worker node misses 3 consecutive heartbeats, THE Coordinator SHALL consider it failed
4. WHEN any Worker node fails, THE Coordinator SHALL immediately send STOP message to all worker nodes
5. THE Coordinator SHALL wait up to 10 seconds for worker nodes to acknowledge STOP
6. THE Coordinator SHALL report test as FAILED with list of failed worker nodes
7. THE Worker nodes SHALL implement "dead man's switch": self-stop if no heartbeat ACK for 10 seconds
8. THE final results SHALL clearly show: "Test FAILED: Node X (10.0.1.10) unreachable at HH:MM:SS"
9. THE Coordinator SHALL NOT support continue-on-failure mode
10. THE Coordinator SHALL collect partial statistics from responsive worker nodes before aborting

### Requirement 5c: Synchronized Execution

**User Story:** As a storage engineer, I want all distributed workers to start IO simultaneously, so that I can measure storage behavior under realistic concurrent load.

#### Acceptance Criteria

1. THE Coordinator SHALL send CONFIG message to all worker nodes before test start
2. THE Worker nodes SHALL prepare for test (create files, allocate buffers, open targets)
3. THE Worker nodes SHALL send READY message to coordinator when preparation complete
4. THE Coordinator SHALL wait for all worker nodes to send READY before proceeding
5. THE Coordinator SHALL send START message with target start timestamp
6. THE start timestamp SHALL be: coordinator_time + 2 seconds (allows for network latency and preparation)
7. THE Worker nodes SHALL wait until local_time >= start_timestamp before beginning IO
8. ALL workers on all nodes SHALL begin IO operations simultaneously (within synchronization window)
9. THE Coordinator SHALL send STOP message when test duration expires or completion criteria met
10. THE Worker nodes SHALL complete in-flight operations within 10 seconds of receiving STOP
11. THE Worker nodes SHALL send RESULTS message with final statistics to coordinator
12. THE Coordinator SHALL report if any worker node fails to synchronize within timeout
13. THE Coordinator SHALL support configurable synchronization delay (default 2 seconds, range 1-10 seconds)

### Requirement 5d: Distributed File Distribution with Node/Worker Hierarchy

**User Story:** As a storage engineer, I want to control how files are distributed among distributed nodes and their workers, so that I can simulate different access patterns across multiple hosts.

#### Acceptance Criteria

**Node and Worker Hierarchy:**
1. THE Coordinator SHALL distribute work to Nodes (hosts)
2. EACH Node SHALL run N workers (threads) as configured by `--threads` parameter
3. THE Coordinator SHALL calculate total workers as: num_nodes × threads_per_node
4. THE Coordinator SHALL assign work to workers globally (across all nodes)
5. THE Coordinator SHALL report: "Connected to N nodes, M workers total (M = N × threads_per_node)"

**File Distribution Modes:**
6. THE Coordinator SHALL support three file distribution modes: SHARED, PARTITIONED, PER_WORKER

7. FOR SHARED mode in distributed:
   a) All workers (across all nodes) generate offsets across entire file range (single file) or access all files (tree)
   b) Coordinator does NOT partition (allows overlap and concurrent access)
   c) Use case: Lock contention, cache coherency, concurrent access testing

8. FOR PARTITIONED mode in distributed with single file:
   a) Coordinator divides file offset range among ALL workers globally
   b) Worker 0 (node 1, thread 0): offsets 0 to file_size/total_workers
   c) Worker 47 (node 3, thread 15): offsets 47×file_size/48 to file_size (for 3 nodes × 16 threads)
   d) Use case: Maximum aggregate bandwidth, no conflicts

9. FOR PARTITIONED mode in distributed with directory tree:
   a) Coordinator assigns file ranges to workers globally
   b) Worker 0 (node 1, thread 0): files 0 to total_files/total_workers - 1
   c) Worker 47 (node 3, thread 15): files 47×total_files/48 to total_files - 1
   d) Each file accessed by exactly one worker across all nodes
   e) Use case: Metadata benchmarking, each file touched once

10. FOR PER_WORKER mode in distributed:
    a) Each worker creates uniquely named file: test.dat.nodeX.workerY
    b) No coordination needed between workers
    c) Use case: Aggregate file creation rate, maximum throughput

11. THE Coordinator SHALL validate that file distribution mode is compatible with workload
12. THE Coordinator SHALL report file distribution strategy in test output
13. THE Coordinator SHALL include per-worker file ranges in distributed configuration

**Example:**
```bash
# 3 nodes, 16 threads per node = 48 total workers
# 1M files, PARTITIONED mode
# Worker 0 (node 1, thread 0): files 0-20,832
# Worker 1 (node 1, thread 1): files 20,833-41,665
# ...
# Worker 47 (node 3, thread 15): files 979,167-999,999
```

### Requirement 5e: Clock Synchronization

**User Story:** As a storage engineer, I want time-series data from distributed workers to align accurately, so that I can analyze aggregate performance over time.

#### Acceptance Criteria

1. THE Coordinator SHALL assume NTP/PTP synchronization may be configured (user responsibility)
2. THE Coordinator SHALL measure clock skew to each worker node during initialization
3. THE Coordinator SHALL use the following synchronization strategy:
   a) IF skew < 10ms: Use absolute timestamps (high precision mode)
   b) IF skew 10-50ms: Use coordinator-based time offsets (medium precision mode)
   c) IF skew > 50ms: Abort test with error (unacceptable synchronization)
4. THE Coordinator SHALL report synchronization method and measured accuracy
5. THE Worker nodes SHALL report elapsed time since test start (not absolute timestamps)
6. THE Coordinator SHALL calculate worker timestamps as: coordinator_start_time + worker_elapsed_time
7. THE Coordinator SHALL adjust all worker timestamps to coordinator time in aggregated results
8. THE Coordinator SHALL re-measure clock skew every 60 seconds during long-running tests
9. THE Coordinator SHALL log maximum observed clock skew in test results
10. THE Coordinator SHALL support `--require-ntp` flag to enforce <10ms synchronization

**Synchronization Accuracy:**
- With NTP: 1-10ms (excellent)
- Without NTP: 10-50ms (acceptable)
- >50ms: Test aborted (unacceptable)

### Requirement 5f: Aggregate Performance Metrics

**User Story:** As a storage engineer, I want to see aggregate IOPS and bandwidth across all distributed nodes and workers, so that I can measure total storage system performance and identify bottlenecks.

#### Acceptance Criteria

1. THE Coordinator SHALL calculate aggregate IOPS across all workers on all nodes
2. THE Coordinator SHALL calculate aggregate bandwidth (MB/s, GB/s) across all workers on all nodes
3. THE Coordinator SHALL calculate aggregate latency percentiles across all workers
4. THE Coordinator SHALL identify slowest worker and slowest node (straggler detection)
5. THE Coordinator SHALL report per-node aggregate statistics (sum of all workers on that node)
6. THE Coordinator SHALL report per-worker statistics when `--show-per-worker` flag is used
7. THE Coordinator SHALL calculate efficiency: actual_aggregate / (num_workers × avg_per_worker)
8. THE Coordinator SHALL include aggregate metrics in all output formats (text, JSON, CSV)
9. THE Coordinator SHALL report network saturation if detected (per-interface stats)
10. THE Coordinator SHALL calculate time-to-completion for run-until-complete mode across all workers

**Example Output:**
```
Aggregate Results (3 nodes, 48 workers):
  Total IOPS: 2.4M (800K per node avg)
  Total Bandwidth: 9.6 GB/s (3.2 GB/s per node avg)
  Slowest node: node2 (750K IOPS, -6% vs avg)
  Slowest worker: node2-worker7 (45K IOPS, -10% vs avg)
```

### Requirement 5g: Graceful Shutdown

**User Story:** As a storage engineer, I want distributed tests to shut down cleanly when interrupted, so that I can collect partial results and clean up resources.

#### Acceptance Criteria

1. WHEN the Coordinator receives SIGINT/SIGTERM, THE Coordinator SHALL send STOP to all worker nodes
2. THE Coordinator SHALL wait up to 30 seconds for worker nodes to complete in-flight operations
3. THE Coordinator SHALL collect partial results from all responsive worker nodes
4. THE Coordinator SHALL report which worker nodes completed gracefully vs forcefully terminated
5. THE Worker nodes SHALL support cleanup operations on shutdown (close files, release locks)
6. THE Coordinator SHALL support `--cleanup` flag to trigger distributed cleanup of test files
7. WHEN `--cleanup` is specified, THE Worker nodes SHALL delete created files and directories
8. THE Coordinator SHALL report cleanup status for each worker node
9. THE Coordinator SHALL save partial results even if test was interrupted
10. THE final output SHALL indicate: "Test INTERRUPTED at HH:MM:SS, partial results collected from N/M nodes"

### Requirement 6: Statistics and Telemetry

**User Story:** As a performance analyst, I want comprehensive IO telemetry, so that I can deeply analyze storage behavior and identify performance characteristics.

#### Acceptance Criteria

1. THE IOPulse SHALL track IOPS for each operation type (read, write, mixed)
2. THE IOPulse SHALL track throughput in bytes per second for each operation type
3. THE IOPulse SHALL track submission latency (time from request creation to kernel submission)
4. THE IOPulse SHALL track completion latency (time from submission to completion)
5. THE IOPulse SHALL track total latency (end-to-end time)
6. THE Latency_Histogram SHALL use logarithmic buckets with sub-microsecond precision for low latencies
7. THE IOPulse SHALL calculate latency percentiles (p50, p90, p95, p99, p99.9, p99.99)
8. THE IOPulse SHALL track queue depth over time as a time series
9. THE IOPulse SHALL track IO size distribution during the test
10. THE IOPulse SHALL track CPU utilization during the test
11. WHEN live statistics are enabled, THE IOPulse SHALL update statistics at configurable intervals (default 1 second)
12. THE IOPulse SHALL track minimum, maximum, average, and standard deviation for latency metrics

### Requirement 6a: Metadata Operation Statistics

**User Story:** As a storage engineer testing NFS and distributed filesystems, I want detailed metadata operation metrics, so that I can analyze filesystem overhead and identify metadata bottlenecks.

#### Acceptance Criteria

1. THE IOPulse SHALL track IOPS and latency histograms for open() operations
2. THE IOPulse SHALL track IOPS and latency histograms for close() operations
3. THE IOPulse SHALL track IOPS and latency histograms for stat()/getattr() operations
4. THE IOPulse SHALL track IOPS and latency histograms for setattr() operations (chmod, chown, utime)
5. THE IOPulse SHALL track IOPS and latency histograms for mkdir() operations
6. THE IOPulse SHALL track IOPS and latency histograms for rmdir() operations
7. THE IOPulse SHALL track IOPS and latency histograms for unlink() operations
8. THE IOPulse SHALL track IOPS and latency histograms for rename() operations
9. THE IOPulse SHALL track IOPS and latency histograms for readdir() operations
10. THE IOPulse SHALL track IOPS and latency histograms for fsync()/fdatasync() operations
11. THE IOPulse SHALL aggregate metadata statistics separately from data IO statistics
12. THE IOPulse SHALL include metadata operation breakdown in all output formats (text, JSON, CSV, Prometheus)
13. WHEN running metadata-focused workloads, THE IOPulse SHALL report aggregate metadata IOPS and average latency

### Requirement 6b: Per-Worker and Per-Thread Statistics

**User Story:** As a performance analyst, I want per-worker and per-thread statistics, so that I can identify stragglers, hotspots, and system-level issues across distributed tests.

#### Acceptance Criteria

1. THE IOPulse SHALL track and report aggregate statistics across all workers (default view)
2. THE IOPulse SHALL track per-worker statistics including IOPS, throughput, and latency for each worker host
3. THE IOPulse SHALL support optional per-thread statistics within each worker for deep-dive analysis
4. WHEN per-thread statistics are enabled, THE IOPulse SHALL track IOPS, throughput, and latency per thread
5. THE IOPulse SHALL include worker/thread identifiers in per-worker and per-thread statistics
6. THE JSON output SHALL support hierarchical statistics: aggregate → per-worker → per-thread
7. THE CSV output SHALL support per-worker time-series data with worker identifier columns
8. WHEN Prometheus metrics are enabled, THE IOPulse SHALL expose per-worker metrics with worker labels for Grafana dashboards
9. THE IOPulse SHALL support configurable statistics granularity: aggregate-only, per-worker, or per-thread
10. WHEN in distributed mode, THE Coordinator SHALL collect and aggregate per-worker statistics from all remote Workers

### Requirement 7: Configuration

**User Story:** As a user, I want to configure IOPulse through both command-line arguments and configuration files, so that I can quickly run simple tests or define complex reproducible profiles.

#### Acceptance Criteria

1. THE IOPulse SHALL accept workload parameters via command-line arguments
2. THE IOPulse SHALL accept workload parameters via TOML configuration files
3. WHEN both CLI arguments and config file are provided, THE CLI arguments SHALL override config file values
4. THE configuration file SHALL support defining multiple workload phases in sequence
5. THE configuration file SHALL support variable substitution for parameterized profiles
6. THE IOPulse SHALL validate all configuration parameters before starting execution
7. IF invalid configuration is detected, THEN THE IOPulse SHALL report specific validation errors and exit
8. THE IOPulse SHALL support a dry-run mode that validates configuration without executing IO

### Requirement 8: Output and Reporting

**User Story:** As a user, I want IOPulse to output results in multiple formats, so that I can integrate with various analysis tools and monitoring systems.

#### Acceptance Criteria

1. THE IOPulse SHALL output results in human-readable text format to stdout
2. THE IOPulse SHALL output results in JSON format to a file
3. THE IOPulse SHALL output results in CSV format to a file
4. WHEN Prometheus metrics are enabled, THE IOPulse SHALL expose metrics via HTTP endpoint
5. THE JSON output SHALL include all collected statistics including full latency histograms
6. THE CSV output SHALL include time-series data for live statistics
7. WHEN a test completes, THE IOPulse SHALL output a summary including aggregate IOPS, throughput, and latency percentiles
8. THE IOPulse SHALL support configurable output verbosity levels

### Requirement 9: Data Integrity

**User Story:** As a storage engineer, I want to optionally verify data integrity during IO operations, so that I can detect storage corruption or data loss.

#### Acceptance Criteria

1. THE IOPulse SHALL support optional data verification for read operations
2. WHEN verification is enabled, THE IOPulse SHALL write deterministic patterns that can be verified on read
3. THE IOPulse SHALL support multiple verification patterns (zeros, ones, random with seed, sequential)
4. IF data verification fails, THEN THE IOPulse SHALL log the failure details including offset and expected vs actual data
5. WHEN verification is enabled, THE IOPulse SHALL track verification failure count in statistics

### Requirement 10: Performance Optimization

**User Story:** As a performance engineer, I want IOPulse to have minimal overhead, so that measured performance reflects actual storage capabilities.

#### Acceptance Criteria

1. THE IOPulse SHALL pre-allocate IO buffers during initialization to avoid allocation during test execution
2. THE IOPulse SHALL use memory-aligned buffers for direct IO operations
3. THE IOPulse SHALL minimize memory allocations in the hot path during IO execution
4. THE IOPulse SHALL use lock-free data structures for statistics collection where possible
5. WHEN using io_uring, THE IOPulse SHALL support registered buffers and fixed files for reduced syscall overhead
6. THE IOPulse SHALL support batch submission of multiple IO operations in a single syscall where the backend supports it

### Requirement 11: Error Handling

**User Story:** As a user, I want IOPulse to handle errors gracefully, so that I can understand failures and the tool remains stable under adverse conditions.

#### Acceptance Criteria

1. IF an IO operation fails, THEN THE IOPulse SHALL by default abort the test and report the error with operation details
2. THE IOPulse SHALL support a configurable "continue-on-io-error" mode that allows tests to proceed after IO failures
3. WHEN continue-on-io-error is enabled, THE IOPulse SHALL log each error with full operation details (offset, size, operation type, error code)
4. WHEN continue-on-io-error is enabled, THE IOPulse SHALL track error counts by error type in statistics
5. WHEN continue-on-io-error is enabled, THE IOPulse SHALL prominently report the error count in final results
6. THE IOPulse SHALL support configurable error thresholds (e.g., abort after N errors even in continue mode)
7. IF a target becomes unavailable, THEN THE IOPulse SHALL report the failure and abort by default
8. THE IOPulse SHALL implement graceful shutdown on SIGINT/SIGTERM signals
9. WHEN interrupted, THE IOPulse SHALL complete in-flight operations and output partial results
10. THE final results SHALL clearly indicate if any errors occurred during the test

### Requirement 12: Extensibility

**User Story:** As a developer, I want IOPulse to have a modular architecture, so that new features can be added without modifying existing code.

#### Acceptance Criteria

1. THE IOPulse architecture SHALL separate IO engine, statistics, configuration, and output concerns into distinct modules
2. WHEN a new IO_Engine is implemented, THE existing codebase SHALL require no modifications
3. WHEN a new output format is implemented, THE existing codebase SHALL require no modifications
4. THE IOPulse SHALL define clear trait interfaces for extensible components
5. THE IOPulse SHALL support future addition of S3/object storage backend without core changes
6. THE IOPulse SHALL support future addition of GPU direct storage backend without core changes

### Requirement 13: Licensing Compatibility

**User Story:** As a project maintainer, I want IOPulse to use MIT licensing, so that the tool can be freely used, modified, and distributed in both open source and commercial contexts.

#### Acceptance Criteria

1. THE IOPulse source code SHALL be released under the MIT license
2. THE IOPulse SHALL only depend on libraries with MIT-compatible licenses (MIT, Apache 2.0, BSD, ISC, Zlib, Unlicense)
3. IF a dependency has a copyleft license (GPL, LGPL, AGPL), THEN THE IOPulse SHALL NOT include that dependency
4. THE IOPulse SHALL maintain a dependency audit documenting the license of each direct dependency
5. WHEN selecting libraries during design and implementation, THE license compatibility SHALL be verified before adoption


### Requirement 14: Dataset Layout Markers

**User Story:** As a storage engineer running large-scale tests with millions of files, I want IOPulse to automatically track dataset layouts and skip recreation when the configuration hasn't changed, so that I can iterate quickly on test runs without waiting for dataset recreation.

#### Acceptance Criteria

**Marker Creation:**
1. WHEN IOPulse creates or modifies files, THE IOPulse SHALL create a `.iopulse-layout` marker file in the target directory
2. THE marker file SHALL contain a configuration hash that uniquely identifies the dataset layout
3. THE configuration hash SHALL include: file sizes, file count, directory structure, block size, layout_manifest path (if used)
4. THE marker file SHALL include human-readable metadata: creation timestamp, file count, total size, generation parameters

**Marker Validation:**
5. WHEN IOPulse starts a test, THE IOPulse SHALL read the `.iopulse-layout` marker if it exists
6. WHEN the marker's configuration hash matches the current configuration, THE IOPulse SHALL skip file creation/validation and proceed immediately
7. WHEN the marker's configuration hash does not match, THE IOPulse SHALL warn the user and require explicit confirmation or `--force-recreate` flag
8. WHEN validating against a marker, THE IOPulse SHALL only read the marker file (O(1) operation), not check individual files
9. THE IOPulse SHALL report: "Using existing layout (verified via marker, N files)"

**Marker Override Flags:**
10. THE IOPulse SHALL support a `--ignore-layout-marker` flag that bypasses marker checks and trusts existing files
11. THE IOPulse SHALL support a `--force-recreate` flag that ignores markers and recreates the dataset
12. WHEN `--force-recreate` is used, THE IOPulse SHALL delete old marker and create new one

**Integration with Layout_Manifest:**
13. WHEN a layout_manifest is used, THE marker SHALL include the layout_manifest path and file hash
14. WHEN reusing a layout_manifest, THE IOPulse SHALL check if marker exists and matches
15. IF marker matches layout_manifest, THE IOPulse SHALL skip file creation (files already exist)
16. IF marker doesn't match, THE IOPulse SHALL warn: "Layout changed, use --force-recreate to regenerate"

**Distributed Mode Integration:**
17. IN distributed mode, THE Coordinator SHALL check the marker file before distributing configuration
18. THE Coordinator SHALL read marker from shared storage (NFS/Lustre) or from first node
19. THE Coordinator SHALL distribute marker validation result to all worker nodes
20. THE Worker nodes SHALL trust coordinator's marker validation (no redundant checks)
21. WHEN marker validation fails in distributed mode, THE Coordinator SHALL abort before connecting to worker nodes

**Example Workflow:**
```bash
# First run: Generate tree, create marker
iopulse /mnt/nfs/tree --dir-depth 3 --dir-width 10 --total-files 1000000 --duration 0
# Creates: .iopulse-layout marker
# Output: "Created 1,000,000 files, marker saved"

# Second run: Reuse existing structure (fast)
iopulse /mnt/nfs/tree --dir-depth 3 --dir-width 10 --total-files 1000000 --duration 60s
# Reads: .iopulse-layout marker
# Output: "Using existing layout (verified via marker, 1000000 files)"
# Skips: File creation (saves minutes/hours)

# Distributed mode: Coordinator checks once
iopulse --mode coordinator --host-list 10.0.1.10,10.0.1.11 \
  --layout-manifest tree_1M.layout_manifest --duration 60s
# Coordinator reads: .iopulse-layout marker
# Coordinator validates: Hash matches
# Coordinator tells workers: "Skip creation, files exist"
# Workers: Trust coordinator, begin IO immediately
```

**Marker File Format:**
```
# IOPulse Layout Marker
# Created: 2026-01-24 10:30:00 UTC
# Config Hash: a3f5b2c8d1e9f4a7
#
# Parameters:
#   dir_depth: 3
#   dir_width: 10
#   total_files: 1000000
#   layout_manifest: tree_1M.layout_manifest (hash: b4e6c3d9)
#
# Dataset:
#   Total files: 1000000
#   Total size: 1.0 TB
#   Total directories: 1110
```
