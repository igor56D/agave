#!/usr/bin/env python3
import argparse
import os

import numpy as np
import pandas as pd

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Analyze tx timing CSV from ledger-tool.",
    )
    parser.add_argument(
        "--input",
        default=os.path.join("..", "..", "tx_timing.csv"),
        help="Path to tx_timing.csv",
    )
    parser.add_argument(
        "--output",
        default="./output",
        help="Output directory for plots",
    )
    return parser.parse_args()


def load_data(path: str) -> pd.DataFrame:
    df = pd.read_csv(path)
    expected = {"signature", "execution_time_us", "executed_units"}
    missing = expected.difference(df.columns)
    if missing:
        raise ValueError(f"Missing columns in CSV: {sorted(missing)}")
    return df


def add_metrics(df: pd.DataFrame) -> pd.DataFrame:
    df = df.copy()
    df["execution_time_us"] = pd.to_numeric(df["execution_time_us"], errors="coerce")
    df["executed_units"] = pd.to_numeric(df["executed_units"], errors="coerce")
    df = df.dropna(subset=["execution_time_us", "executed_units"])
    df = df[(df["executed_units"] > 0) & (df["execution_time_us"] > 0)]
    df["cu_per_us"] = df["executed_units"] / df["execution_time_us"]
    return df


def plot_distribution(df: pd.DataFrame, out_dir: str) -> None:
    fig, axes = plt.subplots(1, 3, figsize=(15, 4))
    axes[0].hist(df["execution_time_us"], bins=100, color="#4C78A8", log=True)
    axes[0].set_title("Execution time (us)")
    axes[0].set_xlabel("us")
    axes[0].set_ylabel("count")

    axes[1].hist(df["executed_units"], bins=100, color="#F58518", log=True)
    axes[1].set_title("Executed units")
    axes[1].set_xlabel("CUs")
    axes[1].set_ylabel("count")

    ratio = df["cu_per_us"].dropna()
    axes[2].hist(ratio, bins=100, color="#54A24B", log=True)
    axes[2].set_title("CUs per microsecond")
    axes[2].set_xlabel("CU / us")
    axes[2].set_ylabel("count")

    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "distribution.png"), dpi=200)
    plt.close(fig)


def plot_cdf_ratio(df: pd.DataFrame, out_dir: str) -> None:
    ratio = df["cu_per_us"].dropna().sort_values()
    if ratio.empty:
        return
    y = np.linspace(0, 1, len(ratio), endpoint=False)
    fig, ax = plt.subplots(figsize=(6, 4))
    ax.plot(ratio.values, y, color="#4C78A8")
    ax.set_title("CDF of CUs per microsecond")
    ax.set_xlabel("CU / us")
    ax.set_xscale("log")
    ax.set_ylabel("cdf")
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "cdf_ratio.png"), dpi=200)
    plt.close(fig)


def plot_outliers(df: pd.DataFrame, out_dir: str) -> None:
    ratio = df["cu_per_us"].dropna()
    if ratio.empty:
        return
    low = ratio.quantile(0.1)
    high = ratio.quantile(0.9)

    low_df = df[df["cu_per_us"] <= low]
    high_df = df[df["cu_per_us"] >= high]

    fig, ax = plt.subplots(figsize=(6, 5))
    ax.scatter(
        low_df["executed_units"],
        low_df["execution_time_us"],
        s=6,
        alpha=0.6,
        label="bottom 10%",
        color="#54A24B",
    )
    ax.scatter(
        high_df["executed_units"],
        high_df["execution_time_us"],
        s=6,
        alpha=0.6,
        label="top 10%",
        color="#E45756",
    )
    ax.set_title("Outliers by CUs per microsecond")
    ax.set_xlabel("executed units")
    ax.set_ylabel("execution time (us)")
    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.legend()
    fig.tight_layout()
    fig.savefig(os.path.join(out_dir, "outliers_scatter.png"), dpi=200)
    plt.close(fig)


def write_ratio_metrics(df: pd.DataFrame, out_dir: str) -> None:
    ratio = df["cu_per_us"].dropna()
    if ratio.empty:
        return
    metrics = {
        "count": int(ratio.shape[0]),
        "mean": float(ratio.mean()),
        "median": float(ratio.median()),
        "std": float(ratio.std(ddof=1)),
        "variance": float(ratio.var(ddof=1)),
        "min": float(ratio.min()),
        "max": float(ratio.max()),
        "p01": float(ratio.quantile(0.01)),
        "p05": float(ratio.quantile(0.05)),
        "p10": float(ratio.quantile(0.10)),
        "p25": float(ratio.quantile(0.25)),
        "p50": float(ratio.quantile(0.50)),
        "p75": float(ratio.quantile(0.75)),
        "p90": float(ratio.quantile(0.90)),
        "p95": float(ratio.quantile(0.95)),
        "p99": float(ratio.quantile(0.99)),
        "p99_9": float(ratio.quantile(0.999)),
    }
    out_path = os.path.join(out_dir, "ratio_metrics.txt")
    with open(out_path, "w", encoding="utf-8") as handle:
        handle.write("metric: cu_per_us\n")
        for key in [
            "count",
            "mean",
            "median",
            "std",
            "variance",
            "min",
            "max",
            "p01",
            "p05",
            "p10",
            "p25",
            "p50",
            "p75",
            "p90",
            "p95",
            "p99",
            "p99_9",
        ]:
            value = metrics[key]
            if isinstance(value, float):
                handle.write(f"{key}: {value:.6f}\n")
            else:
                handle.write(f"{key}: {value}\n")

    best_path = os.path.join(out_dir, "best_cu_per_us.txt")
    best_threshold = df["cu_per_us"].quantile(0.99)
    best_pool = df[df["cu_per_us"] >= best_threshold]
    best_df = best_pool.sample(n=min(100, len(best_pool)), random_state=42)
    with open(best_path, "w", encoding="utf-8") as handle:
        for _, row in best_df.iterrows():
            handle.write(
                f"{row['signature']},"
                f"{row['cu_per_us']:.6f},"
                f"{int(row['executed_units'])},"
                f"{int(row['execution_time_us'])}\n"
            )

    worst_path = os.path.join(out_dir, "worst_cu_per_us.txt")
    worst_threshold = df["cu_per_us"].quantile(0.01)
    worst_pool = df[df["cu_per_us"] <= worst_threshold]
    worst_df = worst_pool.sample(n=min(100, len(worst_pool)), random_state=42)
    with open(worst_path, "w", encoding="utf-8") as handle:
        for _, row in worst_df.iterrows():
            handle.write(
                f"{row['signature']},"
                f"{row['cu_per_us']:.6f},"
                f"{int(row['executed_units'])},"
                f"{int(row['execution_time_us'])}\n"
            )


def main() -> None:
    args = parse_args()
    os.makedirs(args.output, exist_ok=True)
    df = load_data(args.input)
    df = add_metrics(df)
    plot_distribution(df, args.output)
    plot_cdf_ratio(df, args.output)
    plot_outliers(df, args.output)
    write_ratio_metrics(df, args.output)


if __name__ == "__main__":
    main()
