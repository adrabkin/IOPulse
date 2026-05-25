# Running IOPulse in Docker

IOPulse can be run in a Docker container on any platform (macOS, Windows, Linux) that supports Docker.

## Quick Start

### Option 1: Using the Convenience Script (Development/Testing)

```bash
# Build and run (first time will build the image)
./docker-run.sh --help

# Run a simple test
./docker-run.sh /data/test.dat --file-size 1G --threads 4 --duration 60s --write-percent 100

# All files are in the test-data/ directory
ls test-data/
```

**Note**: On macOS/Windows, this is suitable for development and testing only, not accurate benchmarks.

### Option 1b: Optimized for Benchmarks (Linux Only)

```bash
# For accurate performance measurements on Linux hosts
./docker-run-benchmark.sh /data/test.dat --file-size 100G --threads 8 --duration 300s --write-percent 100 --direct
```

**Warning**: This script will warn you if you're not on Linux.

### Option 2: Using Docker Directly

```bash
# Build the image
docker build -t iopulse:latest .

# Run with arguments
docker run --rm -v "$(pwd)/test-data:/data" iopulse:latest /data/test.dat --file-size 1G --duration 60s --write-percent 100
```

### Option 3: Using Docker Compose

```bash
# Build
docker-compose build

# Run with custom arguments
IOPULSE_ARGS="/data/test.dat --file-size 1G --duration 60s --write-percent 100" docker-compose run --rm iopulse

# For O_DIRECT or block device access (requires privileged mode)
IOPULSE_ARGS="/data/test.dat --file-size 1G --duration 60s --write-percent 100 --direct" docker-compose run --rm iopulse-privileged
```

## File Access

The container mounts `./test-data` to `/data` inside the container. All test files should be referenced with the `/data` prefix:

```bash
# This creates test-data/mytest.dat on your host
./docker-run.sh /data/mytest.dat --file-size 1G --duration 30s --write-percent 100

# Output files also go to test-data/
./docker-run.sh /data/test.dat --file-size 1G --duration 30s --write-percent 100 --json-output /data/results.json
```

## Examples

### Basic Write Test
```bash
./docker-run.sh /data/test.dat --file-size 1G --threads 4 --duration 60s --write-percent 100
```

### Random Read with Zipf Distribution
```bash
./docker-run.sh /data/test.dat --file-size 10G --threads 8 --duration 120s \
  --read-percent 100 --random --distribution zipf --zipf-theta 1.2
```

### Mixed Workload with JSON Output
```bash
./docker-run.sh /data/test.dat --file-size 10G --threads 8 --duration 60s \
  --read-percent 70 --write-percent 30 --random \
  --json-output /data/results.json
```

### O_DIRECT Mode (Requires Privileged)
```bash
docker run --rm --privileged -v "$(pwd)/test-data:/data" \
  iopulse:latest /data/test.dat --file-size 1G --duration 60s --write-percent 100 --direct
```

## Building from Pre-compiled Binary

If you've already built the binary on a Linux machine, you can create a simpler Dockerfile:

```dockerfile
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy pre-built binary
COPY target/release/iopulse /usr/local/bin/iopulse

RUN mkdir -p /data
WORKDIR /data

ENTRYPOINT ["/usr/local/bin/iopulse"]
CMD ["--help"]
```

Then build with:
```bash
docker build -f Dockerfile.prebuilt -t iopulse:latest .
```

## Performance Considerations ⚠️

### Critical: Platform Matters!

**Docker on Linux**: ✅ Near-native performance (<1-2% overhead)
- Containers share the host kernel
- Direct syscalls, no virtualization layer
- Accurate benchmarks

**Docker on macOS/Windows**: ⚠️ Significant overhead (30-70% slower)
- Runs inside a Linux VM (HyperKit/WSL2)
- Filesystem virtualization layer adds latency
- **NOT suitable for accurate performance benchmarks**
- OK for functional testing and development

### For Accurate Benchmarks

**On Linux hosts only:**
```bash
# Use the optimized benchmark script
./docker-run-benchmark.sh /data/test.dat --file-size 100G --duration 300s --direct
```

**On macOS/Windows:**
- Use Docker for development/testing only
- Run real benchmarks on a native Linux machine
- Consider: Remote Linux server, Linux VM with direct disk access, or cloud instance

### Docker on Linux: Optimization Flags

The `docker-run-benchmark.sh` script uses these optimizations:

1. **`--privileged`**: Required for:
   - O_DIRECT flag
   - Block device access
   - NUMA features
   - Raw device access

2. **`--network=host`**: Reduces network overhead for distributed mode

3. **`--ipc=host`**: Reduces IPC overhead

4. **`--ulimit nofile=1048576`**: Removes file descriptor limits

5. **`--security-opt apparmor=unconfined`**: Disables security overhead

### Resource Limits

Adjust CPU and memory allocation:
```bash
docker run --rm --cpus="8" --memory="8g" \
  -v "$(pwd)/test-data:/data" \
  iopulse:latest [args]
```

### Filesystem Considerations

**On Linux:**
- Bind mount directly to the storage you want to test
- Avoid Docker volumes (they add a layer)
- Example: `-v /mnt/nvme:/data` for testing NVMe

**On macOS/Windows:**
- Docker volumes are faster than bind mounts
- But still not suitable for accurate benchmarks

## Troubleshooting

### Permission Denied
If you get permission errors accessing files:
```bash
# Make sure test-data directory exists and is writable
mkdir -p test-data
chmod 777 test-data
```

### O_DIRECT Not Working
O_DIRECT requires privileged mode:
```bash
docker run --rm --privileged -v "$(pwd)/test-data:/data" iopulse:latest [args] --direct
```

### Image Not Found
Build the image first:
```bash
docker build -t iopulse:latest .
```

## Multi-Platform Images

To build for multiple architectures (amd64, arm64):

```bash
# Enable buildx
docker buildx create --use

# Build multi-platform image
docker buildx build --platform linux/amd64,linux/arm64 -t iopulse:latest --push .
```
