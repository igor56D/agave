# Tx Timing Analysis

Simple analysis utilities for `ledger-tool --record-tx-timing-csv` output.

## Setup

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

## Run

```bash
python analyze_tx_timing.py --input ../../tx_timing.csv --output ./output
```

Outputs plots to `./output/`:

- `distribution.png` — histograms of execution time, CUs, and time/CU ratio
- `cdf_ratio.png` — CDF of time/CU ratio
- `outliers_scatter.png` — scatter of bottom/top 10% by time/CU ratio

## CSV columns

Timing columns are in microseconds and are per-transaction measurements.
Cost columns are in compute units (CUs) unless noted.

- `signature` — base58 transaction signature
- `execution_time_us` — total time from validation through execution for this transaction
- `validate_fees_us` — nonce and fee-payer validation time
- `load_us` — account loading time
- `execute_us` — execution pipeline time for the transaction
- `collect_balances_us` — balance collection time (pre + post)
- `filter_executable_us` — time to identify executable program accounts
- `program_cache_us` — program cache replenish time
- `executed_units` — actual compute units consumed during execution
- `cost_signature` — signature verification cost
- `cost_write_lock` — write-lock cost
- `cost_data_bytes` — instruction data size cost
- `cost_programs_execution` — execution cost component (uses `executed_units`)
- `cost_loaded_accounts_data_size` — loaded accounts data size cost
- `cost_allocated_accounts_data_size` — allocated account data size in bytes
- `cost_total` — total cost used by the cost model
