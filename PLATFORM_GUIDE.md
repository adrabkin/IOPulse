# IOPulse Platform Guide

Quick reference for running IOPulse on different platforms.

## TL;DR

| Platform | Build Method | Benchmark Accuracy | Use Case |
|----------|-------------|-------------------|----------|
| **Linux Native** | `cargo build --release` | ✅ 100% Accurate | Production benchmarks |
| **Docker on Linux** | `./docker-run-benchmark.sh` | ✅ ~98% Accurate | Production benchmarks |
| **Docker on Mac/Win** | `./docker-run.sh` | ⚠️ 30-70% slower | Development/testing only |
| **macOS Native** | ❌ Won't compile | N/A | Not supported |
| **Windows Native** | ❌ Won't compile | N/A | Not supported |

## Detailed Platform Guide

### Linux (Recommended for Benchmarks)

**Native Build:**
```bash
cargo build --release
./target/release/iopulse /data/test.dat --file-size 100G --duration 300s --write-percent 100
```

**Docker (nearly identical performance):**
```bash
./docker-run-benchmark.sh /data/test.dat --file-size 100G --duration 300s --write-percent 100
```

**Why Linux?**
- IOPulse uses Linux-specific syscalls (io_uring, O_DIRECT, posix_fallocate, etc.)
- Production storage systems run on Linux
- Accurate measurements require Linux kernel

---

### macOS (Development Only)

**For Development/Testing:**
```bash
# Build Docker image
docker build -t iopulse:latest .

# Run tests (NOT accurate for benchmarks)
./docker-run.sh /data/test.dat --file-size 1G --duration 30s --write-percent 100
```

**For Accurate Benchmarks:**

Option A: **Remote Linux Server**
```bash
# Build on Linux
ssh user@linux-server "cd /path/to/iopulse && cargo build --release"

# Run benchmarks
ssh user@linux-server "/path/to/iopulse/target/release/iopulse /data/test.dat ..."
```

Option B: **Linux VM with Direct Disk Access**
- Use UTM, Parallels, or VMware Fusion
- Create Linux VM (Ubuntu 22.04+ recommended)
- Pass through a physical disk or create a large virtual disk
- Build and run natively in the VM

Option C: **Cloud Linux Instance**
```bash
# Example: AWS EC2
aws ec2 run-instances --instance-type c5.4xlarge --image-id ami-ubuntu-22.04 ...
ssh ubuntu@instance
git clone https://github.com/yourusername/iopulse
cd iopulse
cargo build --release
./target/release/iopulse /data/test.dat --file-size 100G ...
```

**Why Not Native macOS?**
- Missing Linux syscalls: io_uring, posix_fallocate, fdatasync, O_DIRECT
- Different filesystem semantics (APFS vs ext4/xfs)
- Not representative of production Linux storage

---

### Windows (Development Only)

**For Development/Testing:**
```powershell
# Build Docker image
docker build -t iopulse:latest .

# Run tests (NOT accurate for benchmarks)
docker run --rm -v ${PWD}/test-data:/data iopulse:latest /data/test.dat --file-size 1G --duration 30s --write-percent 100
```

**For Accurate Benchmarks:**

Option A: **WSL2 with Direct Disk Access**
```bash
# In WSL2 Ubuntu
cd /mnt/c/path/to/iopulse
cargo build --release

# Mount a physical disk in WSL2
# (requires Windows 11 or Windows 10 with specific updates)
wsl --mount \\.\PHYSICALDRIVE1

# Run benchmarks
./target/release/iopulse /mnt/wsl/PHYSICALDRIVE1/test.dat ...
```

Option B: **Cloud Linux Instance** (same as macOS Option C)

---

## Performance Impact Summary

### Docker Overhead by Platform

| Platform | Overhead | Suitable for Benchmarks? |
|----------|----------|-------------------------|
| Linux | <1-2% | ✅ Yes |
| macOS | 30-70% | ❌ No |
| Windows (WSL2) | 20-50% | ⚠️ Maybe (depends) |

### Why Docker on Mac/Windows is Slow

1. **Virtualization Layer**: Docker Desktop runs a Linux VM
2. **Filesystem Translation**: 
   - macOS: VirtioFS or gRPC-FUSE
   - Windows: Plan 9 filesystem protocol
3. **IO Path**: Container → VM → Host OS → Storage
4. **Not Representative**: You're benchmarking the VM, not real hardware

### When Docker Overhead Doesn't Matter

Docker on Mac/Windows is fine for:
- ✅ Verifying commands work
- ✅ Testing new features
- ✅ Development and debugging
- ✅ Generating sample output files
- ✅ Learning the tool

Docker on Mac/Windows is NOT suitable for:
- ❌ Performance benchmarks
- ❌ Comparing storage systems
- ❌ Capacity planning
- ❌ SLA validation
- ❌ Regression testing

---

## Recommended Workflows

### Scenario 1: Mac Developer, Linux Production

```bash
# On Mac: Develop and test functionality
./docker-run.sh /data/test.dat --file-size 1G --duration 10s --write-percent 100

# On Linux: Run real benchmarks
ssh prod-server "cd /path/to/iopulse && cargo build --release"
ssh prod-server "./target/release/iopulse /data/test.dat --file-size 100G --duration 300s ..."
```

### Scenario 2: All Linux

```bash
# Build once
cargo build --release

# Run benchmarks
./target/release/iopulse /data/test.dat --file-size 100G --duration 300s --write-percent 100

# Or use Docker for isolation
./docker-run-benchmark.sh /data/test.dat --file-size 100G --duration 300s --write-percent 100
```

### Scenario 3: CI/CD Pipeline

```yaml
# .github/workflows/benchmark.yml
name: Benchmark
on: [push]
jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Build
        run: cargo build --release
      - name: Run Benchmark
        run: |
          ./target/release/iopulse /tmp/test.dat \
            --file-size 10G --duration 60s --write-percent 100 \
            --json-output results.json
      - name: Upload Results
        uses: actions/upload-artifact@v3
        with:
          name: benchmark-results
          path: results.json
```

---

## FAQ

**Q: Can I get accurate benchmarks on my Mac?**
A: No, not with Docker. Use a Linux VM with direct disk access or a remote Linux server.

**Q: Is Docker on Linux as fast as native?**
A: Yes, within 1-2%. Docker containers share the host kernel on Linux.

**Q: Why not support macOS natively?**
A: macOS lacks critical Linux syscalls (io_uring, O_DIRECT semantics, posix_fallocate) and has fundamentally different filesystem behavior. Supporting it would require significant platform-specific code that wouldn't be representative of production Linux systems.

**Q: What about Windows native?**
A: Same issues as macOS. Windows has its own IO APIs (IOCP, overlapped IO) that are completely different from Linux.

**Q: Can I use Docker on Mac for development?**
A: Absolutely! It's perfect for testing features, verifying commands, and development. Just don't use it for performance measurements.

**Q: What Linux distro should I use?**
A: Ubuntu 22.04+, Debian 12+, RHEL 9+, or any modern distro with kernel 5.1+. The kernel version matters more than the distro.

---

## Quick Decision Tree

```
Need to run IOPulse?
│
├─ For accurate benchmarks?
│  │
│  ├─ Yes → Use Linux (native or Docker on Linux)
│  │
│  └─ No (just testing) → Docker on any platform is fine
│
└─ What platform do you have?
   │
   ├─ Linux → cargo build --release (best)
   │          or ./docker-run-benchmark.sh (nearly as good)
   │
   ├─ macOS → ./docker-run.sh (testing only)
   │          or use remote Linux server (for benchmarks)
   │
   └─ Windows → docker run ... (testing only)
                or use WSL2/cloud Linux (for benchmarks)
```
