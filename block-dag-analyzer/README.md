# Solana Block DAG Analyzer

Analyzes Solana blocks to build dependency graphs from transaction account access patterns and calculate parallel execution metrics.

## Usage

**Single block:**
```bash
cargo run -- block --slot 280000000 > analysis.csv
```

**Block range:**
```bash
cargo run -- range --start-slot 280000000 --end-slot 280000010 > analysis.csv
```

**Custom chains:**
```bash
cargo run -- block --slot 280000000 --chains 8 > analysis.csv
```

## CSV Output

```csv
slot,total_cus,max_dist_cus,longest_path_cus,chain0_cus,chain1_cus,chain2_cus,chain3_cus,empty_cus
280000000,1500000,800000,600000,200000,250000,180000,170000,50000
```

## Metrics

- **total_cus**: Total compute units consumed by all transactions
- **max_dist_cus**: longest chain CU count
- **longest_path_cus**: Critical path (minimum possible execution time)
- **chainN_cus**: Compute units assigned to each execution chain
- **empty_cus**: CUs consumed on a chain waiting for next tx to be ready

## Options

- `--chains N`: Number of parallel execution chains (default: 4)
- `--rpc-url URL`: Custom RPC endpoint (default: mainnet-beta)
