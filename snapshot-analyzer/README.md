# Agave Snapshot Analyzer

A CLI tool for analyzing account staleness in Solana networks using a pre-built account access index.

## Overview

This tool determines how much account data has been accessed over various time periods by:

- Loading account data from a Solana snapshot archive
- Reading a pre-built index of account access data (sorted by slot)
- Iterating through accounts in chronological order (newest to oldest access)
- Tracking cumulative account sizes as we cross time boundaries
- Reporting data totals at 1, 2, 4, and 8 month intervals
- Showing the total snapshot size for context

## How It Works

1. **Snapshot Loading**: Parses snapshot to build account map with data sizes and calculates total size
2. **Index Loading**: Reads bincode-serialized vector of `AccountSlotEntry` structs
3. **Boundary Calculation**: Determines slot boundaries for 1/2/4/8 months ago
4. **Iteration**: Processes index entries from newest to oldest access slot
5. **Checkpoint Tracking**: Records cumulative sizes when crossing time boundaries
6. **Report Generation**: Outputs total snapshot size and all checkpoints

## Input Format

The tool expects a pre-built index file containing a bincode-serialized vector of:

```rust
pub struct AccountSlotEntry {
    pub account: Pubkey,
    pub highest_slot: u64,
}
```

The vector must be sorted from high to low slot (newest to oldest access).

## Installation

```bash
cd snapshot-analyzer
cargo build --release
```

## Usage

### Basic Usage

```bash
./target/release/agave-snapshot-analyzer \
    --snapshot snapshot-270000000-hash.tar.zst \
    --index account-access-index.bin \
    --current-slot 275000000
```

### Controlling Thread Count

You can control the number of threads used for parallel processing in several ways:

**1. Command Line Option (Recommended):**
```bash
# Use 8 threads
./target/release/agave-snapshot-analyzer --threads 8 --snapshot ... --index ...

# Use 1 thread (disable parallelism)  
./target/release/agave-snapshot-analyzer -j 1 --snapshot ... --index ...
```

**2. Environment Variable:**
```bash
# Set globally for the process
export RAYON_NUM_THREADS=16
./target/release/agave-snapshot-analyzer --snapshot ... --index ...
```

**3. Default Behavior:**
- Uses all available CPU cores if no thread count is specified
- Automatically detects the number of logical cores on your system

### Command Line Options

- `--snapshot PATH`: Path to the snapshot archive file (required)
- `--index PATH`: Path to the bincode account access index file (required)  
- `--current-slot SLOT`: Current slot number (can be auto-detected from filename)
- `--threads NUM` or `-j NUM`: Number of threads for parallel processing (default: number of CPU cores)

## Example Output

```
INFO  [agave_snapshot_analyzer] Using default thread pool with 16 threads
INFO  [agave_snapshot_analyzer] Calculating total snapshot size using parallel processing...
INFO  [agave_snapshot_analyzer] Total snapshot size calculated in 0.85s
INFO  [agave_snapshot_analyzer] Building account map from 2,150,000 accounts using parallel processing...
INFO  [agave_snapshot_analyzer] Built account map with 2,150,000 entries in 1.23s

=== Solana Account Staleness Analysis ===

Total snapshot size: 456.78 GB
Using pre-built account access index
Analysis based on cumulative account sizes by access recency

Checkpoints (accounts accessed within each time period):
------------------------------------------------------------
1 months ago (slot 271234567): 123.45 GB
2 months ago (slot 268765432): 234.56 GB  
4 months ago (slot 263827160): 345.67 GB
8 months ago (slot 254950616): 456.78 GB

Growth between checkpoints:
----------------------------------------
1 to 2 months: +111.11 GB
2 to 4 months: +111.11 GB
4 to 8 months: +111.11 GB
```

Data older than 8 months = Total snapshot size - 8 months checkpoint

## Performance Characteristics

- **Memory Usage**: Scales with snapshot size + index size (typically 1-10 GB RAM)
- **Disk I/O**: Sequential reads of snapshot and index files
- **Processing Speed**: Very fast - parallel processing with DashMap and rayon
- **Storage**: Requires snapshot file + pre-built index file
- **Threading**: Parallel account map building and size calculations

### Thread Count Recommendations

- **Default (auto)**: Good for most cases - uses all CPU cores
- **High-memory systems**: Consider reducing threads if memory usage is too high
- **I/O-bound systems**: More threads may not help if disk is the bottleneck
- **Debugging**: Use `--threads 1` to disable parallelism for easier debugging
- **Benchmarking**: Try different thread counts to find optimal performance for your hardware

Example performance impact:
```bash
# Single-threaded (baseline)
time ./agave-snapshot-analyzer -j 1 --snapshot ... --index ...

# Multi-threaded (should be faster)
time ./agave-snapshot-analyzer -j 8 --snapshot ... --index ...
```

## Technical Notes

### Time Boundary Calculation
- Assumes ~2.5 slots per second on Solana mainnet
- Uses 30 days per month approximation
- Calculates slot boundaries working backwards from current slot

### Index Requirements
- Must be bincode-serialized vector of `AccountSlotEntry`
- Must be sorted from high to low slot (newest first)
- Accounts not in snapshot are ignored (no error)

## Index Creation

The account access index should be created by another tool that:
1. Processes historical ledger data (e.g., from Old Faithful)
2. Tracks the highest slot each account was accessed
3. Sorts results by slot (high to low)
4. Serializes using bincode

Example index creation pattern:
```rust
let entries: Vec<AccountSlotEntry> = /* collect from ledger data */;
entries.sort_by(|a, b| b.highest_slot.cmp(&a.highest_slot)); // high to low
let file = File::create("account-access-index.bin")?;
bincode::serialize_into(file, &entries)?;
```

## Contributing

This implementation is much simpler than CAR file processing approaches:
1. **Index Validation**: Ensure proper sorting and format
2. **Progress Reporting**: Better progress indicators for large datasets  
3. **Memory Optimization**: Streaming for very large indices
4. **Error Handling**: Robust handling of malformed indices

## Comparison to Previous Approach

| Aspect | Old (CAR Files) | New (Pre-built Index) |
|--------|-----------------|----------------------|
| Data Source | Stream CAR files | Read bincode index |
| Network Usage | Downloads GB of data | None (local files only) |
| Implementation | Complex CAR/CBOR parsing | Simple iteration + HashMap |
| Performance | Slow (parsing overhead) | Very fast (direct access) |
| Dependencies | Many (CAR, CBOR, etc.) | Minimal (bincode only) |
| Reliability | Network-dependent | Local file only |

## License

This project follows the same license as the Agave project. 