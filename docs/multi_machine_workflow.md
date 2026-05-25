# Multi-Machine Gas Town Workflow

How to develop across two machines without conflicts. Assumes you will NOT
work on both machines simultaneously.

## Before Leaving a Machine

```bash
# 1. Dock active rigs (stops witnesses/refineries, blocks new slings)
gt rig dock iopulse
gt rig dock forge
# Skip any already docked (ASP, aspinfra, crucible, gastown)

# 2. Push beads backup for each rig you worked on
cd /data/dev/gt/iopulse/mayor/rig
bd backup
git add .beads/backup/ && git commit -m "beads sync" && git push

cd /data/dev/gt/forge/mayor/rig
bd backup
git add .beads/backup/ && git commit -m "beads sync" && git push

# Repeat for any other active rigs (crucible, ASP, etc.)

# 3. Push gastown (town-level beads + config)
cd /data/dev/gt/gastown/mayor/rig
bd backup
git add .beads/backup/ && git commit -m "beads sync" && git push

# 4. (Optional) Send yourself a handoff note for context
gt handoff -m "Switched to other machine. Working on: <brief context>"
```

## First Time Setup on a New Machine

### Install Gas Town

```bash
mkdir -p /data/dev/gt
cd /data/dev/gt

# Clone gastown from YOUR backup repo (has your beads + config)
mkdir -p gastown/mayor
git clone git@github.com:adrabkin/gastown_entropy_backup.git gastown/mayor/rig
cd gastown/mayor/rig

# Set split remote: pull upstream GT code, push to your backup
git remote set-url origin git@github.com:steveyegge/gastown.git
git remote set-url --push origin git@github.com:adrabkin/gastown_entropy_backup.git

# Build and install gt
SKIP_UPDATE_CHECK=1 make install

# Bootstrap Gas Town
cd /data/dev/gt
gt init
gt prime
```

### Clone Rigs You Need

```bash
# IOPulse
mkdir -p /data/dev/gt/iopulse/mayor
git clone git@github.com:adrabkin/IOPulse_private.git iopulse/mayor/rig
cd iopulse/mayor/rig && git checkout beta
gt rig add iopulse

# Forge
mkdir -p /data/dev/gt/forge/mayor
git clone git@github.com:adrabkin/forge_private.git forge/mayor/rig
gt rig add forge

# Crucible
mkdir -p /data/dev/gt/crucible/mayor
git clone git@github.com:adrabkin/crucible_private.git crucible/mayor/rig
gt rig add crucible

# Add others as needed
```

### Install Test Dependencies (for IOPulse)

```bash
sudo apt install -y fio
cd /tmp && curl -sLO "https://github.com/breuner/elbencho/releases/download/v3.0-37/elbencho-static_amd64.deb"
sudo dpkg -i elbencho-static_amd64.deb
```

## Returning to a Machine

```bash
# 1. Pull latest code + beads for each rig
cd /data/dev/gt/gastown/mayor/rig && git pull
cd /data/dev/gt/iopulse/mayor/rig && git pull
# Repeat for any other rigs

# 2. Prime GT
cd /data/dev/gt
gt prime

# 3. Undock the rig you want to work on
gt rig undock iopulse
gt rig start iopulse

# 4. Check your hook/mail for context
gt hook
gt mail inbox
```

## Remote Configuration Reference

### Gastown (split remote)

```
origin (fetch): git@github.com:steveyegge/gastown.git              ← upstream GT code
origin (push):  git@github.com:adrabkin/gastown_entropy_backup.git  ← your private backup
```

- `git pull` gets latest GT code from upstream
- `git push` backs up your config + beads to your private repo

### Rig Repos

| Rig | Remote | Branch |
|-----|--------|--------|
| iopulse | adrabkin/IOPulse_private | beta |
| forge | adrabkin/forge_private | main |
| crucible | adrabkin/crucible_private | main |
| gastown | steveyegge/gastown (fetch) / adrabkin/gastown_entropy_backup (push) | main |

## Key Rules

- **Always dock before leaving** — prevents polecats/witnesses from running
  unattended and consuming API credits
- **`bd backup` is critical** — without it, issue state doesn't make it into git
- **Push before switching** — code and beads must be pushed to travel between machines
- **Pull before working** — always pull on arrival to get the latest state
- **No simultaneous work** — if both machines modify the same rig, you'll get merge conflicts
