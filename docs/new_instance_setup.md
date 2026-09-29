# New Gas Town Instance Setup

Complete setup for a new machine that shares Gas Town configuration
with the primary instance. After setup, you can switch between machines
by pushing/pulling (see `docs/multi_machine_workflow.md`).

Base directory: `/data/gt` (adjust if different on your machine)

## Prerequisites

```bash
# Go 1.21+ (for building gt)
# See https://go.dev/doc/install

# Rust (for building iopulse)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Git SSH key must be configured for github.com
ssh -T git@github.com   # should show "Hi <username>!"
```

## Step 1: Kill Any Existing Installation

```bash
# Stop GT services if running
gt down 2>/dev/null
pkill -f "gt daemon" 2>/dev/null
pkill -f "dolt sql-server" 2>/dev/null

# Remove existing installation
rm -rf /data/gt
```

## Step 2: Build and Install the gt Binary

```bash
# Clone gastown source (from your backup, which has your config)
cd /tmp
git clone git@github.com:adrabkin/gastown_entropy_backup.git gastown-build
cd gastown-build

# Build gt
SKIP_UPDATE_CHECK=1 make install

# Verify
gt --version

# Clean up build dir (we'll clone it properly into the town later)
rm -rf /tmp/gastown-build
```

## Step 3: Create the Town

```bash
gt install /data/gt
cd /data/gt
```

## Step 4: Import Town Configuration

```bash
cd /data/gt

# Clone the backup repo temporarily to get town config
git clone git@github.com:adrabkin/gastown_entropy_backup.git /tmp/gt-config

# Copy town configuration
cp /tmp/gt-config/town-config/CLAUDE.md .
cp /tmp/gt-config/town-config/mayor/rigs.json mayor/
cp /tmp/gt-config/town-config/mayor/town.json mayor/
cp /tmp/gt-config/town-config/mayor/overseer.json mayor/
cp /tmp/gt-config/town-config/mayor/daemon.json mayor/
cp /tmp/gt-config/town-config/settings/config.json settings/
cp /tmp/gt-config/town-config/settings/escalation.json settings/

# Copy beads routing and config
cp /tmp/gt-config/town-config/.beads/routes.jsonl .beads/
cp /tmp/gt-config/town-config/.beads/config.yaml .beads/

# Import beads backup data (issue history)
cp /tmp/gt-config/.beads/backup/* .beads/ 2>/dev/null

# Clean up
rm -rf /tmp/gt-config
```

## Step 5 (Optional): Set Up the Gastown Rig

Only needed if you want to rebuild `gt` from source on this machine
or push config changes back to the backup repo. Skip this if you're
only working on iopulse.

```bash
cd /data/gt
mkdir -p gastown/mayor
git clone git@github.com:adrabkin/gastown_entropy_backup.git gastown/mayor/rig
cd gastown/mayor/rig

# Set split remote: fetch upstream GT code, push to your backup
git remote set-url origin git@github.com:steveyegge/gastown.git
git remote set-url --push origin git@github.com:adrabkin/gastown_entropy_backup.git

# Verify
git remote -v
# origin  git@github.com:steveyegge/gastown.git (fetch)
# origin  git@github.com:adrabkin/gastown_entropy_backup.git (push)
```

## Step 6: Clone IOPulse Rig

```bash
cd /data/gt
mkdir -p iopulse/mayor
git clone git@github.com:adrabkin/IOPulse_private.git iopulse/mayor/rig
cd iopulse/mayor/rig
git checkout beta
```

## Step 7: Start Gas Town

```bash
cd /data/gt

# Dock rigs that aren't cloned on this machine (prevents startup errors)
gt rig dock ASP
gt rig dock aspinfra
gt rig dock crucible
gt rig dock forge
gt rig dock gastown

# Now start (only iopulse will have witness/refinery)
gt up
gt prime
gt rig list    # should show iopulse running, others docked
```

When you clone another rig later, undock it: `gt rig undock <rig> && gt rig start <rig>`

## Step 8: Build IOPulse and Set Up Testing

```bash
# Build
cd /data/gt/iopulse/mayor/rig
cargo build --release

# Install test tools
sudo apt install -y fio jq
cd /tmp && curl -sLO "https://github.com/breuner/elbencho/releases/download/v3.0-37/elbencho-static_amd64.deb"
sudo dpkg -i elbencho-static_amd64.deb

# Create test directories
sudo mkdir -p /data/iopulse_testing /data/iopulse_regression_results
sudo chown $USER:$USER /data/iopulse_testing /data/iopulse_regression_results
```

## Step 9: Initialize Baselines for This Hardware

```bash
cd /data/gt/iopulse/mayor/rig

# Run direct I/O tests (takes ~5 minutes)
sudo bash tests/regression/run_direct_io_tests.sh
# Note the results directory printed at the end

# Initialize baselines (will detect new hardware automatically)
python3 tests/regression/init_baseline.py /data/iopulse_regression_results/direct_run_YYYYMMDD_HHMMSS

# Verify baselines
bash tests/regression/check_performance.sh /data/iopulse_regression_results/direct_run_YYYYMMDD_HHMMSS

# Run competitive comparison (takes ~5 minutes)
sudo bash tests/regression/run_buffered_io_tests.sh
bash tests/regression/check_buffered.sh /data/iopulse_regression_results/buffered_run_YYYYMMDD_HHMMSS
```

## Step 10: Verify Everything Works

```bash
cd /data/gt
gt status           # should show services running
gt rig list         # should show rigs (iopulse + gastown cloned, others listed but not cloned)
bd stats            # should show issue counts
gt mail inbox       # should show messages (if any)
```

## Adding More Rigs Later

When you want to work on another project on this machine:

```bash
cd /data/gt

# Example: adding forge
mkdir -p forge/mayor
git clone git@github.com:adrabkin/forge_private.git forge/mayor/rig

# No need to run gt rig add — it's already in rigs.json from the config import
# Just undock and start:
gt rig undock forge
gt rig start forge
```

## Updating Gas Town Binary

When the upstream GT code gets updates:

```bash
cd /data/gt/gastown/mayor/rig
git pull                         # pulls from steveyegge/gastown
SKIP_UPDATE_CHECK=1 make install # rebuild
```
