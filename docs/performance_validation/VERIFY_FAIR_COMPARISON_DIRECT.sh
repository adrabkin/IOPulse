#!/bin/bash
# Verification script to ensure fair comparison with O_DIRECT
# Goal: All tools write the same amount of data with O_DIRECT

set -e

echo "=========================================="
echo "Verification: Fair Comparison with O_DIRECT"
echo "=========================================="
echo ""

TEST_DIR="/home/ec2-user/testing"
TEST_FILE="$TEST_DIR/test.dat"
DURATION="2s"
RESULTS_DIR="/home/ec2-user/results_verification_direct"

mkdir -p "$RESULTS_DIR"

cleanup() {
    rm -f "$TEST_FILE" "$TEST_DIR"/test.*.dat
}
trap cleanup EXIT

echo "Test Goal: Ensure all tools write same amount of data with O_DIRECT"
echo "Target: 4GB total (4 workers × 1GB per worker)"
echo "Mode: O_DIRECT (bypass page cache, true storage performance)"
echo ""

echo "=========================================="
echo "Test 1: IOPulse - Per-Worker, 4 workers, O_DIRECT"
echo "=========================================="
/home/ec2-user/IOPulse/target/release/iopulse "$TEST_FILE" --file-size 1G --duration "$DURATION" \
  --write-percent 100 --random --engine libaio --queue-depth 32 --threads 4 \
  --file-distribution per-worker --direct | tee "$RESULTS_DIR/iopulse_perworker_direct.log"

echo ""
echo "Files created by IOPulse:"
ls -lh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""
echo "Total size:"
du -sh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""
echo "Detailed sizes:"
du -h "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""

# Extract key metrics
IOPULSE_OPS=$(grep "Write:" "$RESULTS_DIR/iopulse_perworker_direct.log" | grep "ops" | awk '{print $2}')
IOPULSE_BYTES=$(grep "Write:" "$RESULTS_DIR/iopulse_perworker_direct.log" | grep "ops" | awk '{print $4}')
IOPULSE_IOPS=$(grep "Write:" "$RESULTS_DIR/iopulse_perworker_direct.log" | grep "IOPS" | awk '{print $NF}')
IOPULSE_LAT=$(grep "Mean:" "$RESULTS_DIR/iopulse_perworker_direct.log" | awk '{print $2}')

echo "IOPulse Results:"
echo "  Operations: $IOPULSE_OPS"
echo "  Bytes: $IOPULSE_BYTES"
echo "  IOPS: $IOPULSE_IOPS"
echo "  Latency (mean): $IOPULSE_LAT"
echo ""

cleanup

echo "=========================================="
echo "Test 2: FIO - Per-File, 4 jobs, O_DIRECT"
echo "=========================================="
fio --name=perfile --filename="$TEST_FILE" --size=1G \
  --runtime=2 --time_based --rw=randwrite --bs=4k \
  --ioengine=libaio --iodepth=32 --numjobs=4 --group_reporting --direct=1 \
  --file_service_type=sequential \
  | tee "$RESULTS_DIR/fio_perfile_direct.log"

echo ""
echo "Files created by FIO:"
ls -lh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""
echo "Total size:"
du -sh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""

# Extract key metrics
FIO_OPS=$(grep "issued rwts:" "$RESULTS_DIR/fio_perfile_direct.log" | awk -F'=' '{print $2}' | awk -F',' '{print $2}')
FIO_BYTES=$(grep "write:" "$RESULTS_DIR/fio_perfile_direct.log" | grep "IOPS=" | awk '{print $NF}' | sed 's/[()]//g' | awk -F'/' '{print $1}')
FIO_IOPS=$(grep "write:" "$RESULTS_DIR/fio_perfile_direct.log" | grep "IOPS=" | awk -F'IOPS=' '{print $2}' | awk -F',' '{print $1}')
FIO_LAT=$(grep "clat (usec):" "$RESULTS_DIR/fio_perfile_direct.log" | awk -F'avg=' '{print $2}' | awk -F',' '{print $1}')

echo "FIO Results:"
echo "  Operations: $FIO_OPS"
echo "  Bytes: $FIO_BYTES"
echo "  IOPS: $FIO_IOPS"
echo "  Latency (mean): ${FIO_LAT}µs"
echo ""

cleanup

echo "=========================================="
echo "Test 3: elbencho - Per-File, 4 threads, O_DIRECT"
echo "=========================================="
/home/ec2-user/elbencho/bin/elbencho -w -t 4 -N 1 -s 1G -b 4k --rand --direct "$TEST_FILE" \
  | tee "$RESULTS_DIR/elbencho_perfile_direct.log"

echo ""
echo "Files created by elbencho:"
ls -lh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""
echo "Total size:"
du -sh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "No files"
echo ""

# Extract key metrics
ELBENCHO_IOPS=$(grep "IOPS" "$RESULTS_DIR/elbencho_perfile_direct.log" | tail -1 | awk '{print $3}')
ELBENCHO_BYTES=$(grep "Total MiB" "$RESULTS_DIR/elbencho_perfile_direct.log" | tail -1 | awk '{print $NF}')
ELBENCHO_TIME=$(grep "Elapsed time" "$RESULTS_DIR/elbencho_perfile_direct.log" | tail -1 | awk '{print $NF}')

echo "elbencho Results:"
echo "  IOPS: $ELBENCHO_IOPS"
echo "  Bytes: ${ELBENCHO_BYTES} MiB"
echo "  Time: $ELBENCHO_TIME"
echo ""

cleanup

echo "=========================================="
echo "Comparison Summary (O_DIRECT)"
echo "=========================================="
echo ""
echo "| Tool | Operations | Bytes Written | IOPS | Latency | Time |"
echo "|------|------------|---------------|------|---------|------|"
echo "| IOPulse | $IOPULSE_OPS | $IOPULSE_BYTES | $IOPULSE_IOPS | $IOPULSE_LAT | 2.000s |"
echo "| FIO | $FIO_OPS | $FIO_BYTES | $FIO_IOPS | ${FIO_LAT}µs | ~2.000s |"
echo "| elbencho | N/A | ${ELBENCHO_BYTES} MiB | $ELBENCHO_IOPS | N/A | $ELBENCHO_TIME |"
echo ""
echo "Verification Checks:"
echo "==================="
echo ""
echo "1. Did all tools create 4 files?"
echo "2. Are file sizes approximately 1GB each?"
echo "3. Is total data written approximately 4GB?"
echo "4. Are IOPS calculations correct (ops / time)?"
echo "5. Is latency reasonable for O_DIRECT (~100-1000µs expected)?"
echo ""
echo "Critical: O_DIRECT bypasses page cache, so performance should be lower"
echo "Expected IOPS: 1-10K (vs 100K-1M for buffered)"
echo ""
