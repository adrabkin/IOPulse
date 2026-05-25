#!/bin/bash
# Verification script to ensure IOPulse is performing real IO
# Goal: Prove that high IOPS numbers are legitimate

set -e

echo "=========================================="
echo "Verification: IOPulse is Performing Real IO"
echo "=========================================="
echo ""

TEST_DIR="/home/ec2-user/testing"
TEST_FILE="$TEST_DIR/test.dat"
RESULTS_DIR="/home/ec2-user/results_io_verification"

mkdir -p "$RESULTS_DIR"

cleanup() {
    rm -f "$TEST_FILE" "$TEST_DIR"/test.*.dat
}
trap cleanup EXIT

echo "Test 1: Monitor disk I/O during IOPulse test"
echo "============================================="
echo "Starting iostat in background..."
iostat -x 1 10 > "$RESULTS_DIR/iostat_iopulse.log" &
IOSTAT_PID=$!

sleep 1
echo "Running IOPulse test..."
/home/ec2-user/IOPulse/target/release/iopulse "$TEST_FILE" --file-size 1G --duration 5s \
  --write-percent 100 --random --engine sync --threads 4 \
  --file-distribution per-worker | tee "$RESULTS_DIR/iopulse_monitored.log"

wait $IOSTAT_PID
echo ""
echo "Disk I/O stats:"
grep "nvme" "$RESULTS_DIR/iostat_iopulse.log" | head -10
echo ""

# Check file sizes
echo "Files created:"
ls -lh "$TEST_DIR"/test.*.dat
echo ""
echo "Actual disk usage (not sparse):"
du -h "$TEST_DIR"/test.*.dat
echo ""

# Verify files have actual data (not all zeros)
echo "Checking if files contain actual data (sampling first 1MB)..."
for f in "$TEST_DIR"/test.*.dat; do
    # Count unique bytes in first 1MB
    UNIQUE=$(dd if="$f" bs=1M count=1 2>/dev/null | od -An -tu1 | tr -s ' ' '\n' | sort -u | wc -l)
    echo "$(basename $f): $UNIQUE unique byte values (should be ~256 for random data)"
done
echo ""

cleanup

echo "=========================================="
echo "Test 2: Compare with FIO (same monitoring)"
echo "=========================================="
echo "Starting iostat in background..."
iostat -x 1 10 > "$RESULTS_DIR/iostat_fio.log" &
IOSTAT_PID=$!

sleep 1
echo "Running FIO test..."
fio --name=perfile --filename="$TEST_FILE" --size=1G \
  --runtime=5 --time_based --rw=randwrite --bs=4k \
  --ioengine=psync --numjobs=4 --group_reporting \
  --file_service_type=sequential \
  | tee "$RESULTS_DIR/fio_monitored.log"

wait $IOSTAT_PID
echo ""
echo "Disk I/O stats:"
grep "nvme" "$RESULTS_DIR/iostat_fio.log" | head -10
echo ""

cleanup

echo "=========================================="
echo "Test 3: Verify pwrite syscalls with strace"
echo "=========================================="
echo "Running IOPulse with strace (1 second test)..."
strace -c -e trace=pwrite,pread /home/ec2-user/IOPulse/target/release/iopulse "$TEST_FILE" --file-size 100M --duration 1s \
  --write-percent 100 --random --engine sync --threads 1 \
  --file-distribution per-worker 2>&1 | tee "$RESULTS_DIR/strace_iopulse.log"

echo ""
echo "Syscall summary:"
grep "pwrite" "$RESULTS_DIR/strace_iopulse.log" | tail -5
echo ""

cleanup

echo "=========================================="
echo "Analysis"
echo "=========================================="
echo ""
echo "Verification Results:"
echo "===================="
echo ""
echo "1. Disk I/O Activity:"
echo "   - Check iostat logs for actual disk writes"
echo "   - IOPulse and FIO should show similar disk utilization"
echo ""
echo "2. File Contents:"
echo "   - Files should contain ~256 unique byte values (random data)"
echo "   - If only a few unique values, data might not be random"
echo ""
echo "3. Syscall Count:"
echo "   - strace should show thousands of pwrite calls"
echo "   - Number of pwrites should match operation count"
echo ""
echo "If all checks pass, IOPulse performance is legitimate!"
echo ""
