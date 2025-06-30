#!/usr/bin/env python3
"""
Critical Path Distribution Analysis
Creates histogram and statistics for critical path CUs across blocks.
"""

import argparse
import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path

def load_block_data(csv_file):
    """Load block summary CSV data."""
    try:
        df = pd.read_csv(csv_file)
        
        # Check for required column
        if 'critical_path_cus' not in df.columns:
            print(f"Error: Missing 'critical_path_cus' column")
            print(f"Available columns: {list(df.columns)}")
            return None
        
        # Filter out rows with zero critical path CUs
        df_filtered = df[df['critical_path_cus'] > 0].copy()
        
        if len(df_filtered) == 0:
            print("Error: No blocks with critical_path_cus > 0 found")
            return None
        
        print(f"Loaded {len(df_filtered)} blocks with critical path data")
        return df_filtered
        
    except Exception as e:
        print(f"Error loading data: {e}")
        return None

def create_distribution_plot(df, output_file=None):
    """Create critical path CU distribution visualization."""
    
    critical_path_cus = df['critical_path_cus'] / 1_000_000  # Convert to millions
    
    # Create figure with subplots (1 row, 3 columns)
    fig, (ax1, ax2, ax3) = plt.subplots(1, 3, figsize=(18, 6))
    fig.suptitle('Critical Path CU Distribution Analysis', fontsize=16, fontweight='bold')
    
    # 1. Histogram
    ax1.hist(critical_path_cus, bins=50, alpha=0.7, edgecolor='black', color='lightcoral')
    ax1.set_xlabel('Critical Path CUs (millions)')
    ax1.set_ylabel('Number of Blocks')
    ax1.set_title('Distribution of Critical Path CUs')
    ax1.grid(True, alpha=0.3)
    
    # Add statistics lines
    mean_val = critical_path_cus.mean()
    median_val = critical_path_cus.median()
    ax1.axvline(mean_val, color='red', linestyle='--', label=f'Mean: {mean_val:.1f}M')
    ax1.axvline(median_val, color='orange', linestyle='--', label=f'Median: {median_val:.1f}M')
    ax1.legend()
    
    # 2. Box plot
    ax2.boxplot(critical_path_cus, vert=True, patch_artist=True,
                boxprops=dict(facecolor='lightblue', alpha=0.7))
    ax2.set_ylabel('Critical Path CUs (millions)')
    ax2.set_title('Box Plot of Critical Path CUs')
    ax2.grid(True, alpha=0.3)
    
    # 3. Cumulative distribution
    sorted_values = np.sort(critical_path_cus)
    cumulative = np.arange(1, len(sorted_values) + 1) / len(sorted_values)
    ax3.plot(sorted_values, cumulative * 100, linewidth=2, color='green')
    ax3.set_xlabel('Critical Path CUs (millions)')
    ax3.set_ylabel('Cumulative Percentage (%)')
    ax3.set_title('Cumulative Distribution')
    ax3.grid(True, alpha=0.3)
    
    # Add percentile lines
    percentiles = [25, 50, 75, 90, 95]
    for p in percentiles:
        value = np.percentile(critical_path_cus, p)
        ax3.axvline(value, color='red', alpha=0.5, linestyle=':')
        ax3.text(value, p + 2, f'P{p}: {value:.1f}M', rotation=45, fontsize=8)
    
    plt.tight_layout()
    
    if output_file:
        plt.savefig(output_file, dpi=300, bbox_inches='tight')
        print(f"Critical path distribution plot saved to {output_file}")
    else:
        plt.show()

def print_statistics(df):
    """Print detailed statistics about critical path CUs."""
    
    critical_path_cus = df['critical_path_cus'] / 1_000_000  # Convert to millions
    
    print("\n" + "="*60)
    print("CRITICAL PATH CU STATISTICS")
    print("="*60)
    print(f"Total blocks analyzed: {len(df):,}")
    print()
    
    print("BASIC STATISTICS (millions of CUs):")
    print(f"  Mean:     {critical_path_cus.mean():.2f}")
    print(f"  Median:   {critical_path_cus.median():.2f}")
    print(f"  Std Dev:  {critical_path_cus.std():.2f}")
    print(f"  Min:      {critical_path_cus.min():.2f}")
    print(f"  Max:      {critical_path_cus.max():.2f}")
    print(f"  Range:    {critical_path_cus.max() - critical_path_cus.min():.2f}")
    print()
    
    print("PERCENTILES (millions of CUs):")
    percentiles = [5, 10, 25, 50, 75, 90, 95, 99]
    for p in percentiles:
        value = np.percentile(critical_path_cus, p)
        print(f"  {p:2d}th:    {value:.2f}")
    print()
    
    # Critical path size categories
    very_small = len(df[critical_path_cus < 1.0])  # < 1M CUs
    small = len(df[(critical_path_cus >= 1.0) & (critical_path_cus < 5.0)])  # 1-5M CUs
    medium = len(df[(critical_path_cus >= 5.0) & (critical_path_cus < 20.0)])  # 5-20M CUs
    large = len(df[(critical_path_cus >= 20.0) & (critical_path_cus < 50.0)])  # 20-50M CUs
    very_large = len(df[critical_path_cus >= 50.0])  # 50M+ CUs
    
    total = len(df)
    print("CRITICAL PATH SIZE CATEGORIES:")
    print(f"  Very Small (<1M):     {very_small:,} ({very_small/total*100:.1f}%)")
    print(f"  Small (1-5M):         {small:,} ({small/total*100:.1f}%)")
    print(f"  Medium (5-20M):       {medium:,} ({medium/total*100:.1f}%)")
    print(f"  Large (20-50M):       {large:,} ({large/total*100:.1f}%)")
    print(f"  Very Large (50M+):    {very_large:,} ({very_large/total*100:.1f}%)")
    print()
    
    # Additional insights
    if 'total_cus' in df.columns:
        critical_path_ratio = (df['critical_path_cus'] / df['total_cus']).mean()
        print(f"Average critical path ratio: {critical_path_ratio:.1%}")
        print("(Critical path CUs as percentage of total block CUs)")
        print()
    
    print("="*60)

def main():
    parser = argparse.ArgumentParser(description='Analyze critical path CU distribution')
    parser.add_argument('csv_file', help='CSV file from block-dag-analyzer (block summary data)')
    parser.add_argument('--output', '-o', help='Output file for the plot (PNG/PDF)')
    parser.add_argument('--stats-only', action='store_true',
                       help='Only print statistics, no plot')
    
    args = parser.parse_args()
    
    # Load data
    df = load_block_data(args.csv_file)
    if df is None:
        return 1
    
    # Print statistics
    print_statistics(df)
    
    # Create plot unless stats-only mode
    if not args.stats_only:
        create_distribution_plot(df, args.output)
    
    return 0

if __name__ == "__main__":
    exit(main()) 