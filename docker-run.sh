#!/bin/bash
# Convenience wrapper for running iopulse in Docker

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo -e "${YELLOW}Error: Docker is not running${NC}"
    exit 1
fi

# Build image if it doesn't exist
if [[ "$(docker images -q iopulse:latest 2> /dev/null)" == "" ]]; then
    echo -e "${GREEN}Building iopulse Docker image...${NC}"
    docker build -t iopulse:latest .
fi

# Create test-data directory if it doesn't exist
mkdir -p test-data

# Run iopulse with all arguments passed through
# Mount test-data directory for file access
docker run --rm \
    -v "$(pwd)/test-data:/data" \
    iopulse:latest "$@"
