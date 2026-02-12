# Random Access Distributions - User Guide

**IOPulse v0.1.0**  
**Date:** January 18, 2026

---

## Overview

IOPulse supports multiple random access distributions to simulate real-world storage workloads. This guide helps you choose the right distribution for your use case and provides practical examples.

---

## Quick Reference

| Distribution | Use Case | Pattern | Example Workload |
|--------------|----------|---------|------------------|
| **Uniform** | Baseline testing | Even access across file | Initial fill, stress test |
| **Zipf** | Hot/cold data | Power law, heavy at start | Database indexes, web cache |
| **Pareto** | 80/20 rule | 80% ops hit 20% of data | Business analytics, logs |
| **Gaussian** | Locality | Bell curve around center | Sequential-ish with variation |

---

## Uniform Distribution

### What It Is
Every block has equal probability of being accessed. No hot/cold pattern.

### When to Use
- **Baseline performance testing** - Measure raw storage capability
- **Initial file fill** - Write data evenly across file
- **Stress testing** - Maximum coverage of storage
- **Comparison baseline** - Compare against skewed distributions

### Example Commands

**Basic uniform test:**
```bash
iopulse /data/test.dat \
  --file-size 10G \
  --block-size 4k \
  --threads 4 \
  --duration 60s \
  --random \
  --distribution uniform \
  --read-percent 70 \
  --write-percent 30
```

**Expected behavior:**
- 85-95% coverage (with typical operation counts)
- Even distribution across file
- No hot spots

---

## Zipf Distribution

### What It Is
Power law distribution where a small percentage of blocks receive the majority of accesses. Named after linguist George Zipf.

**Mathematical property:** Block 1 is most frequent, block 2 is accessed 1/2^theta as often, block 3 is 1/3^theta as often, etc.

### When to Use

**Database Workloads:**
- Primary key lookups on hot tables
- Index scans with temporal locality
- Recent transaction data

**Web Applications:**
- CDN with popular content
- Cache with hot objects
- Session stores

**File Systems:**
- Active working set
- Hot directories and metadata

### Theta Parameter Guide

**theta = 1.2 (Default - Recommended)**
- **Pattern:** 97.72% of operations hit first 20% of file
- **Use case:** Database hot indexes, web cache
- **Example:** MySQL primary key lookups, Redis hot keys

```bash
# MySQL InnoDB simulation
iopulse /data/mysql-sim.dat \
  --file-size 50G \
  --block-size 16k \
  --threads 8 \
  --duration 300s \
  --random \
  --distribution zipf \
  --zipf-theta 1.2 \
  --read-percent 80 \
  --write-percent 20 \
  --queue-depth 32
```

**theta = 2.5 (Extreme)**
- **Pattern:** 100% of operations hit first 1% of file
- **Use case:** Extreme hot keys, real-time analytics
- **Example:** Redis counters, current hour analytics

```bash
# Redis hot keys simulation
iopulse /data/redis-sim.dat \
  --file-size 10G \
  --block-size 4k \
  --threads 16 \
  --duration 300s \
  --random \
  --distribution zipf \
  --zipf-theta 2.5 \
  --read-percent 90 \
  --write-percent 10 \
  --queue-depth 256
```

### Verified Behavior

| Theta | Top 20% Coverage | Use Case |
|-------|------------------|----------|
| 1.2 | 97.72% | Database indexes, web cache |
| 2.5 | 100% | Extreme hot keys, real-time data |

---

## Pareto Distribution

### What It Is
Implements the Pareto principle (80/20 rule) where 80% of operations access 20% of the data.

**Mathematical property:** Based on Pareto Type I distribution, adapted for bounded file offsets.

### When to Use

**Business Applications:**
- Customer data (20% of customers generate 80% of revenue)
- Product inventory (20% of products account for 80% of sales)
- Log analysis (20% of log entries are accessed 80% of the time)

**Analytics Workloads:**
- Recent data analysis
- Hot partition access
- Time-series databases

### H Parameter Guide

**h = 0.9 (Default - Classic 80/20)**
- **Pattern:** 78.76% of operations hit first 20% of file
- **Use case:** Classic Pareto principle scenarios
- **Example:** Business analytics, customer data

```bash
# Business analytics simulation
iopulse /data/analytics.dat \
  --file-size 100G \
  --block-size 8k \
  --threads 8 \
  --duration 600s \
  --random \
  --distribution pareto \
  --pareto-h 0.9 \
  --read-percent 85 \
  --write-percent 15 \
  --queue-depth 64
```

**h = 2.0 (More skewed)**
- **Pattern:** ~90% of operations hit first 10% of file
- **Use case:** Highly skewed business data
- **Example:** VIP customers, premium products

```bash
# VIP customer data simulation
iopulse /data/vip-customers.dat \
  --file-size 50G \
  --block-size 16k \
  --threads 4 \
  --duration 300s \
  --random \
  --distribution pareto \
  --pareto-h 2.0 \
  --read-percent 95 \
  --write-percent 5
```

### Verified Behavior

| H | Top 20% Coverage | Use Case |
|---|------------------|----------|
| 0.9 | 78.76% | Classic 80/20 rule |

---

## Gaussian Distribution

### What It Is
Normal (bell curve) distribution centered at a configurable point in the file. Simulates locality of reference.

### When to Use

**Spatial Locality:**
- Sequential-ish access with variation
- Working set with locality
- Scan with random jumps

**Time-Series Data:**
- Recent data access (center at end of file)
- Historical analysis (center at specific time point)

### Parameters

**stddev** - Standard deviation (spread)
- 0.05: Very tight (20-30% coverage)
- 0.1: Moderate (40-60% coverage)
- 0.2: Loose (70-90% coverage)

**center** - Center point (0.0-1.0)
- 0.0: Start of file
- 0.5: Middle of file (default)
- 1.0: End of file

### Example Commands

**Log file tail access:**
```bash
# Recent log entries (last 10% of file)
iopulse /data/logs.dat \
  --file-size 50G \
  --block-size 4k \
  --threads 4 \
  --duration 300s \
  --random \
  --distribution gaussian \
  --gaussian-stddev 0.05 \
  --gaussian-center 0.95 \
  --read-percent 100
```

**Time-series with locality:**
```bash
# Access around middle of dataset
iopulse /data/timeseries.dat \
  --file-size 100G \
  --block-size 64k \
  --threads 8 \
  --duration 600s \
  --random \
  --distribution gaussian \
  --gaussian-stddev 0.1 \
  --gaussian-center 0.5 \
  --read-percent 80 \
  --write-percent 20
```

---

## Choosing the Right Distribution

### Decision Tree

**1. Do you have hot/cold data?**
- **Yes** → Use Zipf or Pareto
- **No** → Use Uniform or Gaussian

**2. Is it power law (few items very hot)?**
- **Yes** → Use Zipf
- **No, more balanced** → Use Pareto

**3. Is there spatial locality?**
- **Yes** → Use Gaussian
- **No** → Use Zipf or Pareto

**4. Is it 80/20 rule?**
- **Yes** → Use Pareto (h=0.9)
- **No, more extreme** → Use Zipf (theta=1.2-2.5)

### Real-World Mapping

**Database (OLTP):**
- Primary key lookups → **Zipf theta=1.2-1.5**
- Index scans → **Zipf theta=1.0-1.2**
- Recent data → **Gaussian center=0.9, stddev=0.1**
- Table scans → **Sequential** (not random)

**Web Cache / CDN:**
- Popular content → **Zipf theta=1.4-1.6**
- Long tail → **Pareto h=0.9-1.2**
- Trending content → **Gaussian center=0.8, stddev=0.15**

**Object Storage:**
- Recent objects → **Pareto h=0.9**
- Archive with hot objects → **Zipf theta=1.0-1.2**
- Backup/restore → **Sequential**

**Analytics:**
- Business intelligence → **Pareto h=0.9** (80/20 rule)
- Time-series queries → **Gaussian** (temporal locality)
- Data warehouse scans → **Sequential**

---

## Understanding Coverage vs Operations

**Important:** Coverage depends on BOTH the distribution AND the number of operations.

### Example: 1GB file, 4K blocks (262,144 blocks)

**Zipf theta=1.2:**
- With 1.88M operations: 83.8% coverage
- But 97.72% of ops hit first 20% of file
- **High coverage doesn't mean uniform!**

**Why?** With enough operations, even cold blocks get accessed eventually (birthday paradox). The distribution controls WHERE operations go, not total coverage.

### Visualizing with Heatmap

Use `--heatmap` to see the actual distribution:

```bash
iopulse /data/test.dat \
  --file-size 1G \
  --block-size 4k \
  --duration 5s \
  --random \
  --distribution zipf \
  --zipf-theta 1.2 \
  --heatmap
```

**Output shows:**
- Operations per bucket (visual histogram)
- Top 20% vs bottom 80% split
- Clear power law tapering for Zipf
- Proper 80/20 split for Pareto

---

## Practical Examples

### Example 1: MySQL Database Simulation

**Scenario:** MySQL with hot indexes, 80% reads, 20% writes

```bash
iopulse /data/mysql.dat \
  --file-size 100G \
  --block-size 16k \
  --threads 16 \
  --duration 600s \
  --random \
  --distribution zipf \
  --zipf-theta 1.2 \
  --read-percent 80 \
  --write-percent 20 \
  --queue-depth 64 \
  --direct
```

**Expected:**
- 97% of operations hit hot indexes (first 20% of file)
- Realistic database access pattern
- Tests storage under concentrated load

### Example 2: E-commerce Analytics

**Scenario:** 80/20 rule - 20% of products generate 80% of queries

```bash
iopulse /data/products.dat \
  --file-size 50G \
  --block-size 8k \
  --threads 8 \
  --duration 300s \
  --random \
  --distribution pareto \
  --pareto-h 0.9 \
  --read-percent 95 \
  --write-percent 5 \
  --queue-depth 32
```

**Expected:**
- 78.76% of operations hit top 20% of products
- Simulates real business data access
- Tests cache effectiveness

### Example 3: Log File Monitoring

**Scenario:** Monitor recent log entries (last 5% of file)

```bash
iopulse /data/logs.dat \
  --file-size 100G \
  --block-size 4k \
  --threads 4 \
  --duration 300s \
  --random \
  --distribution gaussian \
  --gaussian-stddev 0.02 \
  --gaussian-center 0.98 \
  --read-percent 100
```

**Expected:**
- Operations concentrated at end of file
- Simulates tail -f behavior with random access
- Tests hot spot performance

### Example 4: CDN with Trending Content

**Scenario:** Popular content with temporal locality

```bash
iopulse /data/cdn.dat \
  --file-size 1T \
  --block-size 64k \
  --threads 32 \
  --duration 3600s \
  --random \
  --distribution zipf \
  --zipf-theta 1.5 \
  --read-percent 98 \
  --write-percent 2 \
  --queue-depth 128
```

**Expected:**
- Strong concentration on popular content
- Realistic CDN access pattern
- Tests cache and network performance

---

## Validation and Debugging

### Use Heatmap to Verify

Always use `--heatmap` when testing distributions to verify behavior:

```bash
# Test your distribution
iopulse test.dat --file-size 1G --block-size 4k --duration 5s \
  --random --distribution zipf --zipf-theta 1.2 --heatmap

# Look for:
# - Power law tapering (Zipf)
# - 80/20 split (Pareto)
# - Bell curve (Gaussian)
# - Even distribution (Uniform)
```

### Common Issues

**Issue:** "Coverage is too high (90%+), expected 20%"
- **Cause:** Too many operations relative to blocks (birthday paradox)
- **Fix:** Use larger file, shorter test, or higher theta/h

**Issue:** "Distribution looks uniform"
- **Cause:** Wrong parameters or distribution not working
- **Fix:** Use `--heatmap` to visualize, check parameters

**Issue:** "Performance is slow"
- **Cause:** Heatmap overhead (~5%)
- **Fix:** Remove `--heatmap` for production benchmarks

---

## Summary

**Zipf:** Use for power law workloads (databases, caches)  
**Pareto:** Use for 80/20 scenarios (business data, analytics)  
**Gaussian:** Use for locality (logs, time-series)  
**Uniform:** Use for baseline testing

**Key insight:** Coverage depends on operation count. Use `--heatmap` to see the actual distribution pattern, not just coverage percentage.

**Verified working:**
- ✅ Zipf theta=1.2 → 97.72% in top 20%
- ✅ Zipf theta=2.5 → 100% in top 1%
- ✅ Pareto h=0.9 → 78.76% in top 20% (80/20 rule)

---

For detailed technical information, see:
- `docs/zipf_distribution_guide.md` - Zipf technical details
- Use `--heatmap` flag for visual validation
