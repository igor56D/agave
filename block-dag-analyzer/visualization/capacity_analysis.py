#!/usr/bin/env python3
"""
Capacity Analysis for Solana Block DAG
Analyzes leftover capacity with 25M CU limit.
"""

import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import argparse
from pathlib import Path

def load_capacity_data(csv_file):
    """Load capacity data from CSV."""
    df = pd.read_csv(csv_file)
    
    # Check required columns
    required_cols = ['slot', 'max_dist_cus', 'track0_cus', 'track1_cus', 'track2_cus', 'track3_cus']
    missing_cols = [col for col in required_cols if col not in df.columns]
    if missing_cols:
        raise ValueError(f"Missing required columns: {missing_cols}")
    
    return df

def calculate_capacity_metrics(df, max_limit_cus=25_000_000):
    """Calculate the two capacity metrics."""
    
    # Metric 1: Worst case capacity (25M - max_dist)
    df['worst_case_capacity'] = max_limit_cus - df['max_dist_cus']
    
    # Metric 2: Per-track capacity sum
    track_cols = ['track0_cus', 'track1_cus', 'track2_cus', 'track3_cus']
    
    # Calculate per-track leftover capacity
    for col in track_cols:
        df[f'{col}_leftover'] = max_limit_cus - df[col]
    
    # Sum all track leftovers
    leftover_cols = [f'{col}_leftover' for col in track_cols]
    df['per_track_capacity_sum'] = df[leftover_cols].sum(axis=1)
    
    return df

def create_distribution_analysis(df, output_dir=None):
    """Create comprehensive distribution analysis."""
    
    # Set up the plot with 2x2 layout instead of 2x3
    fig, axes = plt.subplots(2, 2, figsize=(15, 12))
    fig.suptitle('Block Capacity Distribution Analysis (25M CU Limit)', fontsize=16)
    
    # Metric 1: Worst Case Capacity
    worst_case = df['worst_case_capacity'] / 1_000_000  # Convert to millions
    
    # Histogram of worst case capacity with percentile lines
    axes[0, 0].hist(worst_case, bins=50, alpha=0.7, color='skyblue', edgecolor='black')
    axes[0, 0].set_title('Capacity Remaining (Worst-Case) Distribution')
    axes[0, 0].set_xlabel('Leftover Capacity (M CUs)')
    axes[0, 0].set_ylabel('Frequency')
    axes[0, 0].grid(True, alpha=0.3)
    
    # Add percentile lines to worst case histogram
    percentiles_worst = [25, 50, 75]
    colors = ['orange', 'red', 'purple']
    for p, color in zip(percentiles_worst, colors):
        percentile_val = worst_case.quantile(p/100)
        axes[0, 0].axvline(percentile_val, color=color, linestyle='--', linewidth=2, 
                          label=f'{p}th percentile: {percentile_val:.1f}M')
    axes[0, 0].legend(loc='upper right')
    
    # Cumulative distribution of worst case capacity
    sorted_worst = np.sort(worst_case)
    cumulative = np.arange(1, len(sorted_worst) + 1) / len(sorted_worst)
    axes[0, 1].plot(sorted_worst, cumulative, linewidth=2, color='navy')
    axes[0, 1].set_title('Worst-Case CDF')
    axes[0, 1].set_xlabel('Leftover Capacity (M CUs)')
    axes[0, 1].set_ylabel('Cumulative Probability')
    axes[0, 1].grid(True, alpha=0.3)
    
    # Metric 2: Per-Track Capacity Sum
    per_track_sum = df['per_track_capacity_sum'] / 1_000_000  # Convert to millions
    
    # Histogram of per-track capacity sum with percentile lines
    axes[1, 0].hist(per_track_sum, bins=50, alpha=0.7, color='lightgreen', edgecolor='black')
    axes[1, 0].set_title('Capacity Remaining (Best-Case) Distribution')
    axes[1, 0].set_xlabel('Leftover Capacity (M CUs)')
    axes[1, 0].set_ylabel('Frequency')
    axes[1, 0].grid(True, alpha=0.3)
    
    # Add percentile lines to per-track sum histogram
    for p, color in zip(percentiles_worst, colors):
        percentile_val = per_track_sum.quantile(p/100)
        axes[1, 0].axvline(percentile_val, color=color, linestyle='--', linewidth=2, 
                          label=f'{p}th percentile: {percentile_val:.1f}M')
    axes[1, 0].legend(loc='upper right')
    
    # Cumulative distribution of per-track capacity sum
    sorted_per_track = np.sort(per_track_sum)
    cumulative_per_track = np.arange(1, len(sorted_per_track) + 1) / len(sorted_per_track)
    axes[1, 1].plot(sorted_per_track, cumulative_per_track, linewidth=2, color='darkgreen')
    axes[1, 1].set_title('Best-Case CDF')
    axes[1, 1].set_xlabel('Leftover Capacity (M CUs)')
    axes[1, 1].set_ylabel('Cumulative Probability')
    axes[1, 1].grid(True, alpha=0.3)
    
    plt.tight_layout()
    
    if output_dir:
        output_path = Path(output_dir) / 'capacity_distribution_analysis.png'
        plt.savefig(output_path, dpi=300, bbox_inches='tight')
        print(f"Distribution analysis saved to {output_path}")
    else:
        plt.show()

def create_comparison_plot(df, output_dir=None):
    """Create a comparison plot between the two metrics."""
    
    fig, axes = plt.subplots(1, 2, figsize=(15, 6))
    fig.suptitle('Capacity Metrics Comparison', fontsize=14)
    
    worst_case = df['worst_case_capacity'] / 1_000_000
    per_track_sum = df['per_track_capacity_sum'] / 1_000_000
    
    # Scatter plot comparing the two metrics
    axes[0].scatter(worst_case, per_track_sum, alpha=0.6, s=20)
    axes[0].set_xlabel('Worst Case Capacity (M CUs)')
    axes[0].set_ylabel('Per-Track Sum Capacity (M CUs)')
    axes[0].set_title('Worst Case vs Per-Track Sum')
    axes[0].grid(True, alpha=0.3)
    
    # Add diagonal line for reference
    min_val = min(worst_case.min(), per_track_sum.min())
    max_val = max(worst_case.max(), per_track_sum.max())
    axes[0].plot([min_val, max_val], [min_val, max_val], 'r--', alpha=0.5, label='y=x')
    axes[0].legend()
    
    # Difference plot
    difference = per_track_sum - worst_case
    axes[1].hist(difference, bins=50, alpha=0.7, color='orange', edgecolor='black')
    axes[1].set_xlabel('Difference (Per-Track Sum - Worst Case) (M CUs)')
    axes[1].set_ylabel('Frequency')
    axes[1].set_title('Capacity Difference Distribution')
    axes[1].grid(True, alpha=0.3)
    axes[1].axvline(difference.mean(), color='red', linestyle='--', 
                    label=f'Mean: {difference.mean():.1f}M')
    axes[1].legend()
    
    plt.tight_layout()
    
    if output_dir:
        output_path = Path(output_dir) / 'capacity_comparison.png'
        plt.savefig(output_path, dpi=300, bbox_inches='tight')
        print(f"Comparison plot saved to {output_path}")
    else:
        plt.show()

def print_summary_stats(df):
    """Print comprehensive summary statistics."""
    
    worst_case = df['worst_case_capacity'] / 1_000_000
    per_track_sum = df['per_track_capacity_sum'] / 1_000_000
    difference = per_track_sum - worst_case
    
    print("=== Capacity Analysis Summary ===")
    print(f"Total blocks analyzed: {len(df)}")
    print(f"Max CU limit: 25.0M CUs")
    print()
    
    print("=== Worst Case Capacity (25M - max_dist) ===")
    print(f"Mean: {worst_case.mean():.2f}M CUs")
    print(f"Median: {worst_case.median():.2f}M CUs")
    print(f"Std Dev: {worst_case.std():.2f}M CUs")
    print(f"Min: {worst_case.min():.2f}M CUs")
    print(f"Max: {worst_case.max():.2f}M CUs")
    print(f"25th percentile: {worst_case.quantile(0.25):.2f}M CUs")
    print(f"75th percentile: {worst_case.quantile(0.75):.2f}M CUs")
    print()
    
    print("=== Per-Track Capacity Sum (Σ(25M - track_size)) ===")
    print(f"Mean: {per_track_sum.mean():.2f}M CUs")
    print(f"Median: {per_track_sum.median():.2f}M CUs")
    print(f"Std Dev: {per_track_sum.std():.2f}M CUs")
    print(f"Min: {per_track_sum.min():.2f}M CUs")
    print(f"Max: {per_track_sum.max():.2f}M CUs")
    print(f"25th percentile: {per_track_sum.quantile(0.25):.2f}M CUs")
    print(f"75th percentile: {per_track_sum.quantile(0.75):.2f}M CUs")
    print()
    
    print("=== Difference Analysis (Per-Track Sum - Worst Case) ===")
    print(f"Mean difference: {difference.mean():.2f}M CUs")
    print(f"Median difference: {difference.median():.2f}M CUs")
    print(f"Std Dev: {difference.std():.2f}M CUs")
    print(f"Min difference: {difference.min():.2f}M CUs")
    print(f"Max difference: {difference.max():.2f}M CUs")
    print()
    
    # Utilization analysis
    max_dist_pct = (df['max_dist_cus'] / 25_000_000 * 100)
    print("=== Block Utilization Analysis ===")
    print(f"Average max_dist utilization: {max_dist_pct.mean():.1f}%")
    print(f"Median max_dist utilization: {max_dist_pct.median():.1f}%")
    print(f"Max utilization: {max_dist_pct.max():.1f}%")
    print(f"Blocks over 80% utilization: {(max_dist_pct > 80).sum()} ({(max_dist_pct > 80).mean()*100:.1f}%)")
    print(f"Blocks over 90% utilization: {(max_dist_pct > 90).sum()} ({(max_dist_pct > 90).mean()*100:.1f}%)")
    print(f"Blocks over 95% utilization: {(max_dist_pct > 95).sum()} ({(max_dist_pct > 95).mean()*100:.1f}%)")

def main():
    parser = argparse.ArgumentParser(description='Analyze block capacity distributions')
    parser.add_argument('csv_file', help='CSV file with block data')
    parser.add_argument('--output-dir', '-o', help='Output directory for plots')
    parser.add_argument('--max-limit', type=int, default=25_000_000,
                       help='Maximum CU limit (default: 25M)')
    parser.add_argument('--stats-only', action='store_true',
                       help='Only print statistics, no plots')
    
    args = parser.parse_args()
    
    # Load data
    try:
        df = load_capacity_data(args.csv_file)
        print(f"Loaded {len(df)} blocks from {args.csv_file}")
    except Exception as e:
        print(f"Error loading data: {e}")
        return 1
    
    # Calculate capacity metrics
    df = calculate_capacity_metrics(df, args.max_limit)
    
    # Print statistics
    print_summary_stats(df)
    
    if args.stats_only:
        return 0
    
    # Create visualizations
    try:
        create_distribution_analysis(df, args.output_dir)
        create_comparison_plot(df, args.output_dir)
    except Exception as e:
        print(f"Error creating plots: {e}")
        return 1
    
    return 0

if __name__ == "__main__":
    exit(main()) 