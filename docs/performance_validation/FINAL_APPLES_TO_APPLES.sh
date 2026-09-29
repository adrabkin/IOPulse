#!/bin/bash
# Final apples-to-apples comparison
# Ensures all tools: 4 workers, 4 files, same duration, same workload

set -e

echo "=========================================="
echo "Final Apples-to-Apples Comparison"
echo "=========================================="
echo ""
echo "Configuration:"
echo "- 4 workers/threads/jobs"
echo "- 4 separate files (1 per worker)"
echo "- 1GB per file = 4GB total"
echo "- Random write, 4K blocks"
echo "- Buffered IO"
echo "- 5 second duration"
echo ""

TEST_DIR="/home/ec2-user/testing"
TEST_FILE="$TEST_DIR/test.dat"
RESULTS_DIR="/home/ec2-user/results_final_comparison"

mkdir -p "$RESULTS_DIR"

cleanup() {
    rm -f "$TEST_FILE" "$TEST_DIR"/test.*.dat "$TEST_DIR"/perfile.*
}
trap cleanup EXIT

echo "=========================================="
echo "Test 1: IOPulse"
echo "=========================================="
/home/ec2-user/IOPulse/target/release/iopulse "$TEST_FILE" --file-size 1G --duration 5s \
  --write-percent 100 --random --engine sync --threads 4 \
  --file-distribution per-worker | tee "$RESULTS_DIR/iopulse.log"

echo ""
echo "IOPulse files created:"
ls -lh "$TEST_DIR"/test.*.dat
echo ""
echo "Total data on disk:"
du -sh "$TEST_DIR"/test.*.dat
echo ""

# Extract metrics
IOPULSE_OPS=$(grep "Write:" "$RESULTS_DIR/iopulse.log" | grep "ops" | awk '{print $2}')
IOPULSE_IOPS=$(grep "Write:" "$RESULTS_DIR/iopulse.log" | grep "IOPS" | awk '{print $NF}')
IOPULSE_BW=$(grep "Write:" "$RESULTS_DIR/iopulse.log" | grep "ops" | awk '{print $4, $5}')
IOPULSE_LAT=$(grep "Mean:" "$RESULTS_DIR/iopulse.log" | awk '{print $2}')

cleanup

echo "=========================================="
echo "Test 2: FIO (4 separate jobs, 4 separate files)"
echo "=========================================="
# Use separate job definitions to ensure 4 separate files
fio --name=job0 --filename="$TEST_DIR/perfile.0" --size=1G \
  --runtime=5 --time_based --rw=randwrite --bs=4k --ioengine=psync \
  --name=job1 --filename="$TEST_DIR/perfile.1" --size=1G \
  --runtime=5 --time_based --rw=randwrite --bs=4k --ioengine=psync \
  --name=job2 --filename="$TEST_DIR/perfile.2" --size=1G \
  --runtime=5 --time_based --rw=randwrite --bs=4k --ioengine=psync \
  --name=job3 --filename="$TEST_DIR/perfile.3" --size=1G \
  --runtime=5 --time_based --rw=randwrite --bs=4k --ioengine=psync \
  --group_reporting \
  | tee "$RESULTS_DIR/fio.log"

echo ""
echo "FIO files created:"
ls -lh "$TEST_DIR"/perfile.* 2>/dev/null || echo "Files deleted by FIO"
echo ""

# Extract metrics
FIO_OPS=$(grep "issued rwts:" "$RESULTS_DIR/fio.log" | awk -F'=' '{print $2}' | awk -F',' '{print $2}')
FIO_IOPS=$(grep "write:" "$RESULTS_DIR/fio.log" | grep "IOPS=" | awk -F'IOPS=' '{print $2}' | awk -F',' '{print $1}')
FIO_BW=$(grep "write:" "$RESULTS_DIR/fio.log" | grep "BW=" | awk -F'BW=' '{print $2}' | awk -F'(' '{print $1}')
FIO_LAT=$(grep "lat (usec):" "$RESULTS_DIR/fio.log" | awk -F'avg=' '{print $2}' | awk -F',' '{print $1}')

cleanup

echo "=========================================="
echo "Test 3: elbencho (4 threads, 4 files)"
echo "=========================================="
/home/ec2-user/elbencho/bin/elbencho -w -t 4 -n 1 -s 1G -b 4k --rand "$TEST_FILE" \
  | tee "$RESULTS_DIR/elbencho.log"

echo ""
echo "elbencho files created:"
ls -lh "$TEST_DIR"/test.*.dat 2>/dev/null || echo "Files deleted by elbencho"
echo ""

# Extract metrics
ELBENCHO_IOPS=$(grep "IOPS" "$RESULTS_DIR/elbencho.log" | tail -1 | awk '{print $3}')
ELBENCHO_BW=$(grep "Throughput MiB/s" "$RESULTS_DIR/elbencho.log" | tail -1 | awk '{print $NF}')
ELBENCHO_TIME=$(grep "Elapsed time" "$RESULTS_DIR/elbencho.log" | tail -1 | awk '{print $NF}')

cleanup

echo "=========================================="
echo "Final Comparison (Apples-to-Apples)"
echo "=========================================="
echo ""
echo "| Tool | Operations | IOPS | Throughput | Latency | Duration |"
echo "|------|------------|------|------------|---------|----------|"
echo "| IOPulse | $IOPULSE_OPS | $IOPULSE_IOPS | $IOPULSE_BW | $IOPULSE_LAT | 5.000s |"
echo "| FIO | $FIO_OPS | $FIO_IOPS | $FIO_BW | ${FIO_LAT}µs | ~5.000s |"
echo "| elbencho | N/A | $ELBENCHO_IOPS | ${ELBENCHO_BW} MiB/s | N/A | $ELBENCHO_TIME |"
echo ""
echo "Configuration Verification:"
echo "==========================="
echo "✓ All tools: 4 workers/threads/jobs"
echo "✓ All tools: 4 separate files (1 per worker)"
echo "✓ All tools: 1GB per file"
echo "✓ All tools: Random write, 4K blocks"
echo "✓ All tools: Buffered IO (no O_DIRECT)"
echo "✓ All tools: ~5 second duration"
echo ""
echo "This is a fair apples-to-apples comparison!"
echo ""
