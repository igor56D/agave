# Snapshot Analyzer Python Analysis

Python helpers for analyzing CSV output with plots and percentile reports.

## Requirements

- Python 3.10+

## Input CSV

Expected columns:

```text
slot,account_count,missing_account_count,read_data_size_bytes,write_data_size_bytes,total_data_size_bytes
```

## Usage

From the repo root (one-time setup):

```bash
bash snapshot-analyzer/analysis/setup_venv.sh
source snapshot-analyzer/analysis/.venv/bin/activate
```

Run analysis:

```bash
python snapshot-analyzer/analysis/analyze_output_csv.py \
  --input output.csv \
  --output-dir snapshot-analyzer/analysis/results
```

If you prefer manual setup:

```bash
python3 -m venv snapshot-analyzer/analysis/.venv
source snapshot-analyzer/analysis/.venv/bin/activate
python -m pip install --upgrade pip
python -m pip install -r snapshot-analyzer/analysis/requirements.txt
```

## Outputs

Generated under `snapshot-analyzer/analysis/results` by default:

- `total_data_size_bytes_by_slot.png`
- `write_data_size_bytes_by_slot.png`
- `account_and_missing_counts_by_slot.png`
- `estimated_total_account_data_by_slot.png`
- `top_percentile_estimated_account_data_by_slot.png`
- `aggregate_stats.json` (aggregate metrics + percentiles)
- `top_1000_slots_by_estimated_size.csv` (sorted by estimated size descending)
- `top_1000_slots_by_write_data_size.csv` (sorted by write bytes descending)

Estimate used for total account data:

```text
total_data_size_bytes + 200 * missing_account_count + 128 * account_count
```

Additional metrics are computed for `write_data_size_bytes` and exposed under the key `write_data_size_bytes` in `aggregate_stats.json`. These metrics are based purely on the recorded write bytes per slot; they do not estimate or add bytes for missing accounts.
