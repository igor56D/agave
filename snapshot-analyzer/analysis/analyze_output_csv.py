#!/usr/bin/env python3
"""Analyze snapshot slot CSV data and generate plots/reports.

Input CSV columns:
    slot,account_count,missing_account_count,read_data_size_bytes,write_data_size_bytes,total_data_size_bytes
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import statistics
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

import matplotlib.pyplot as plt


EXPECTED_COLUMNS = {
    "slot",
    "account_count",
    "missing_account_count",
    "read_data_size_bytes",
    "write_data_size_bytes",
    "total_data_size_bytes",
}


@dataclass(frozen=True)
class Row:
    slot: int
    account_count: int
    missing_account_count: int
    read_data_size_bytes: int
    write_data_size_bytes: int
    total_data_size_bytes: int

    @property
    def estimated_total_account_data_bytes(self) -> int:
        return (
            self.total_data_size_bytes
            + (200 * self.missing_account_count)
            + (128 * self.account_count)
        )


def percentile(sorted_values: list[float], p: float) -> float:
    if not sorted_values:
        return float("nan")
    if p <= 0:
        return float(sorted_values[0])
    if p >= 100:
        return float(sorted_values[-1])
    index = (len(sorted_values) - 1) * (p / 100.0)
    lo = math.floor(index)
    hi = math.ceil(index)
    if lo == hi:
        return float(sorted_values[lo])
    weight = index - lo
    return (1.0 - weight) * float(sorted_values[lo]) + weight * float(sorted_values[hi])


def read_rows(path: Path) -> list[Row]:
    with path.open("r", newline="") as f:
        reader = csv.DictReader(f)
        if reader.fieldnames is None:
            raise ValueError("Input CSV is empty or missing a header row.")
        missing_columns = EXPECTED_COLUMNS.difference(reader.fieldnames)
        if missing_columns:
            missing = ", ".join(sorted(missing_columns))
            raise ValueError(f"Input CSV missing required column(s): {missing}")

        rows: list[Row] = []
        for line_num, raw in enumerate(reader, start=2):
            try:
                rows.append(
                    Row(
                        slot=int(raw["slot"]),
                        account_count=int(raw["account_count"]),
                        missing_account_count=int(raw["missing_account_count"]),
                        read_data_size_bytes=int(raw["read_data_size_bytes"]),
                        write_data_size_bytes=int(raw["write_data_size_bytes"]),
                        total_data_size_bytes=int(raw["total_data_size_bytes"]),
                    )
                )
            except (TypeError, ValueError) as exc:
                raise ValueError(f"Invalid numeric value at CSV line {line_num}: {exc}") from exc
    rows.sort(key=lambda r: r.slot)
    return rows


def series(rows: Iterable[Row], attr: str) -> list[int]:
    return [getattr(row, attr) for row in rows]


def write_aggregate_stats(rows: list[Row], output_path: Path) -> None:
    slots = series(rows, "slot")
    account_count = series(rows, "account_count")
    missing_count = series(rows, "missing_account_count")
    data_size = series(rows, "total_data_size_bytes")
    estimated = [r.estimated_total_account_data_bytes for r in rows]
    write_bytes = series(rows, "write_data_size_bytes")

    metric_map = {
        "account_count": account_count,
        "missing_account_count": missing_count,
        "total_data_size_bytes": data_size,
        "estimated_total_account_data_bytes": estimated,
        "write_data_size_bytes": write_bytes,
    }

    percentiles = [50, 75, 90, 95, 99, 99.5, 99.9]

    summary = {
        "row_count": len(rows),
        "slot_range": {"min": min(slots), "max": max(slots)},
        "metrics": {},
    }

    for metric_name, values in metric_map.items():
        sorted_values = sorted(values)
        summary["metrics"][metric_name] = {
            "min": min(values),
            "max": max(values),
            "mean": statistics.fmean(values),
            "median": statistics.median(values),
            "stdev": statistics.pstdev(values),
            "sum": sum(values),
            "percentiles": {str(p): percentile(sorted_values, p) for p in percentiles},
        }

    with output_path.open("w") as f:
        json.dump(summary, f, indent=2)
        f.write("\n")


def write_top_1000(rows: list[Row], output_path: Path) -> None:
    top_rows = sorted(
        rows,
        key=lambda r: r.estimated_total_account_data_bytes,
        reverse=True,
    )[:1000]

    with output_path.open("w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(
            [
                "rank",
                "slot",
                "account_count",
                "missing_account_count",
                "total_data_size_bytes",
                "estimated_total_account_data_bytes",
            ]
        )
        for rank, row in enumerate(top_rows, start=1):
            writer.writerow(
                [
                    rank,
                    row.slot,
                    row.account_count,
                    row.missing_account_count,
                    row.total_data_size_bytes,
                    row.estimated_total_account_data_bytes,
                ]
            )


def write_top_1000_write_bytes(rows: list[Row], output_path: Path) -> None:
    """Top 1000 slots by write_data_size_bytes (all slots)."""
    top_rows = sorted(
        rows,
        key=lambda r: r.write_data_size_bytes,
        reverse=True,
    )[:1000]

    with output_path.open("w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(
            [
                "rank",
                "slot",
                "account_count",
                "missing_account_count",
                "write_data_size_bytes",
            ]
        )
        for rank, row in enumerate(top_rows, start=1):
            writer.writerow(
                [
                    rank,
                    row.slot,
                    row.account_count,
                    row.missing_account_count,
                    row.write_data_size_bytes,
                ]
            )


def make_plots(rows: list[Row], output_dir: Path) -> None:
    slots = series(rows, "slot")
    account_count = series(rows, "account_count")
    missing_count = series(rows, "missing_account_count")
    data_size = series(rows, "total_data_size_bytes")
    write_bytes = series(rows, "write_data_size_bytes")
    estimated = [r.estimated_total_account_data_bytes for r in rows]

    plt.style.use("seaborn-v0_8-darkgrid")

    # Plot 1: total_data_size_bytes for each slot.
    fig, ax = plt.subplots(figsize=(14, 6))
    ax.plot(slots, data_size, linewidth=1.0, color="tab:blue")
    ax.set_title("Total Data Size Bytes by Slot")
    ax.set_xlabel("Slot")
    ax.set_ylabel("total_data_size_bytes")
    fig.tight_layout()
    fig.savefig(output_dir / "total_data_size_bytes_by_slot.png", dpi=150)
    plt.close(fig)

    # Plot 1b: write_data_size_bytes for each slot (raw write bytes, missing accounts not estimated).
    fig, ax = plt.subplots(figsize=(14, 6))
    ax.plot(slots, write_bytes, linewidth=1.0, color="tab:orange")
    ax.set_title("Write Data Size Bytes by Slot")
    ax.set_xlabel("Slot")
    ax.set_ylabel("write_data_size_bytes")
    fig.tight_layout()
    fig.savefig(output_dir / "write_data_size_bytes_by_slot.png", dpi=150)
    plt.close(fig)

    # Plot 2: account_count and missing_account_count together.
    fig, ax_left = plt.subplots(figsize=(14, 6))
    line1 = ax_left.plot(
        slots, account_count, linewidth=1.0, color="tab:green", label="account_count"
    )
    ax_left.set_xlabel("Slot")
    ax_left.set_ylabel("account_count", color="tab:green")
    ax_left.tick_params(axis="y", labelcolor="tab:green")

    ax_right = ax_left.twinx()
    line2 = ax_right.plot(
        slots,
        missing_count,
        linewidth=1.0,
        color="tab:red",
        label="missing_account_count",
    )
    ax_right.set_ylabel("missing_account_count", color="tab:red")
    ax_right.tick_params(axis="y", labelcolor="tab:red")

    lines = line1 + line2
    labels = [l.get_label() for l in lines]
    ax_left.legend(lines, labels, loc="upper right")
    ax_left.set_title("Account Count and Missing Account Count by Slot")
    fig.tight_layout()
    fig.savefig(output_dir / "account_and_missing_counts_by_slot.png", dpi=150)
    plt.close(fig)

    # Plot 3: estimated total account data by slot.
    fig, ax = plt.subplots(figsize=(14, 6))
    ax.plot(slots, estimated, linewidth=1.0, color="tab:purple")
    ax.set_title(
        "Estimated Total Account Data by Slot\n"
        "(total_data_size_bytes + 200 * missing_account_count + 128 * account_count)"
    )
    ax.set_xlabel("Slot")
    ax.set_ylabel("estimated_total_account_data_bytes")
    fig.tight_layout()
    fig.savefig(output_dir / "estimated_total_account_data_by_slot.png", dpi=150)
    plt.close(fig)

    # Plot 4: top 10/5/1 percentiles by estimate.
    sorted_estimated = sorted(float(v) for v in estimated)
    threshold_90 = percentile(sorted_estimated, 90.0)  # top 10%
    threshold_95 = percentile(sorted_estimated, 95.0)  # top 5%
    threshold_99 = percentile(sorted_estimated, 99.0)  # top 1%

    top_10_points = [(r.slot, r.estimated_total_account_data_bytes) for r in rows if r.estimated_total_account_data_bytes >= threshold_90]
    top_5_points = [(r.slot, r.estimated_total_account_data_bytes) for r in rows if r.estimated_total_account_data_bytes >= threshold_95]
    top_1_points = [(r.slot, r.estimated_total_account_data_bytes) for r in rows if r.estimated_total_account_data_bytes >= threshold_99]

    fig, ax = plt.subplots(figsize=(14, 6))
    ax.plot(slots, estimated, color="lightgray", linewidth=0.8, label="all slots")

    if top_10_points:
        x10, y10 = zip(*top_10_points)
        ax.scatter(x10, y10, s=8, alpha=0.35, color="tab:blue", label="top 10%")
    if top_5_points:
        x5, y5 = zip(*top_5_points)
        ax.scatter(x5, y5, s=10, alpha=0.45, color="tab:orange", label="top 5%")
    if top_1_points:
        x1, y1 = zip(*top_1_points)
        ax.scatter(x1, y1, s=14, alpha=0.75, color="tab:red", label="top 1%")

    ax.axhline(y=threshold_90, color="tab:blue", linestyle="--", linewidth=1.0, alpha=0.7)
    ax.axhline(y=threshold_95, color="tab:orange", linestyle="--", linewidth=1.0, alpha=0.7)
    ax.axhline(y=threshold_99, color="tab:red", linestyle="--", linewidth=1.0, alpha=0.7)

    ax.set_title("Top 10%, 5%, and 1% Slots by Estimated Total Account Data")
    ax.set_xlabel("Slot")
    ax.set_ylabel("estimated_total_account_data_bytes")
    ax.legend(loc="upper left")
    fig.tight_layout()
    fig.savefig(output_dir / "top_percentile_estimated_account_data_by_slot.png", dpi=150)
    plt.close(fig)


def parse_args() -> argparse.Namespace:
    script_path = Path(__file__).resolve()
    repo_root = script_path.parents[2]
    default_input = repo_root / "output.csv"
    default_output_dir = script_path.parent / "results"

    parser = argparse.ArgumentParser(
        description="Generate plots and aggregate reports for snapshot analyzer CSV output."
    )
    parser.add_argument(
        "--input",
        type=Path,
        default=default_input,
        help=f"Path to input CSV (default: {default_input})",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=default_output_dir,
        help=f"Directory for generated plots/reports (default: {default_output_dir})",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    input_path: Path = args.input
    output_dir: Path = args.output_dir

    if not input_path.exists():
        raise FileNotFoundError(f"Input CSV not found: {input_path}")

    output_dir.mkdir(parents=True, exist_ok=True)
    rows = read_rows(input_path)
    if not rows:
        raise ValueError("Input CSV has no data rows.")

    make_plots(rows, output_dir)
    write_aggregate_stats(rows, output_dir / "aggregate_stats.json")
    write_top_1000(rows, output_dir / "top_1000_slots_by_estimated_size.csv")
    write_top_1000_write_bytes(
        rows, output_dir / "top_1000_slots_by_write_data_size.csv"
    )

    print(f"Done. Wrote plots and reports to: {output_dir}")


if __name__ == "__main__":
    main()
