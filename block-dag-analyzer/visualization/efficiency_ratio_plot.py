#!/usr/bin/env python3
"""
Efficiency Ratio Analysis for Solana Block DAG Data
Plots the distribution of critical_path_cus / max_dist_cus ratio.
"""

import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import argparse
from pathlib import Path

def load_and_analyze_data(csv_file):
    """Load data and calculate efficiency ratio."""
    
    try:
        df = pd.read_csv(csv_file)
        
        # Check required columns
        required_cols = ['critical_path_cus', 'max_dist_cus', 'total_cus']
        missing_cols = [col for col in required_cols if col not in df.columns]
        if missing_cols:
            print(f"Error: Missing required columns: {missing_cols}")
            print(f"Available columns: {list(df.columns)}")
            return None
        
        # Determine number of tracks from the CSV columns
        track_cols = [col for col in df.columns if col.startswith('track') and col.endswith('_cus')]
        num_tracks = len(track_cols)
        
        if num_tracks == 0:
            print("Error: No track columns found in CSV (expected trackN_cus columns)")
            return None
        
        print(f"Detected {num_tracks} tracks from columns: {track_cols}")
        
        # Filter out rows where max_dist_cus is 0 to avoid division by zero
        df_filtered = df[df['max_dist_cus'] > 0].copy()
        
        if len(df_filtered) == 0:
            print("Error: No blocks with max_dist_cus > 0 found")
            return None
        
        # Calculate optimal minimum execution time
        df_filtered['optimal_min_cus'] = df_filtered[['critical_path_cus', 'total_cus']].apply(
            lambda row: max(row['critical_path_cus'], row['total_cus'] / num_tracks), axis=1
        )
        
        # Calculate efficiency ratio using the improved formula
        df_filtered['efficiency_ratio'] = df_filtered['optimal_min_cus'] / df_filtered['max_dist_cus']
        
        # Filter out any impossible ratios (should be <= 1.0 in theory)
        df_filtered = df_filtered[df_filtered['efficiency_ratio'] <= 1.5]  # Allow some tolerance for edge cases
        
        print(f"Loaded {len(df_filtered)} valid blocks for analysis")
        print(f"Efficiency ratio range: {df_filtered['efficiency_ratio'].min():.3f} to {df_filtered['efficiency_ratio'].max():.3f}")
        print(f"Using formula: max(critical_path_cus, total_cus/{num_tracks}) / max_dist_cus")
        
        return df_filtered
        
    except Exception as e:
        print(f"Error loading data: {e}")
        return None

def create_efficiency_plots(df, output_dir=None):
    """Create multiple visualizations of the efficiency ratio."""
    
    efficiency_ratio = df['efficiency_ratio']
    
    # Create a figure with multiple subplots
    fig, ((ax1, ax2), (ax3, ax4)) = plt.subplots(2, 2, figsize=(15, 12))
    fig.suptitle('Efficiency Ratio Analysis: max(critical path, Total CUs/# tracks) / Longest Thread', fontsize=16, fontweight='bold')
    
    # 1. Histogram
    ax1.hist(efficiency_ratio, bins=50, alpha=0.7, edgecolor='black', color='skyblue')
    ax1.set_xlabel('Efficiency Ratio (Optimal Min / Longest Thread)')
    ax1.set_ylabel('Number of Blocks')
    ax1.set_title('Distribution of Efficiency Ratios')
    ax1.grid(True, alpha=0.3)
    ax1.set_ylim(bottom=0)  # Start y-axis at 0
    ax1.set_xlim(left=0)    # Start x-axis at 0
    
    # Add vertical lines for statistics
    mean_ratio = efficiency_ratio.mean()
    median_ratio = efficiency_ratio.median()
    ax1.axvline(mean_ratio, color='red', linestyle='--', label=f'Mean: {mean_ratio:.3f}')
    ax1.axvline(median_ratio, color='orange', linestyle='--', label=f'Median: {median_ratio:.3f}')
    ax1.legend()
    
    # 2. Box plot
    ax2.boxplot(efficiency_ratio, vert=True, patch_artist=True, 
                boxprops=dict(facecolor='lightgreen', alpha=0.7))
    ax2.set_ylabel('Efficiency Ratio')
    ax2.set_title('Box Plot of Efficiency Ratios')
    ax2.grid(True, alpha=0.3)
    ax2.set_ylim(bottom=0)  # Start y-axis at 0
    
    # 3. Cumulative distribution
    sorted_ratios = np.sort(efficiency_ratio)
    cumulative = np.arange(1, len(sorted_ratios) + 1) / len(sorted_ratios)
    ax3.plot(sorted_ratios, cumulative * 100, linewidth=2, color='purple')
    ax3.set_xlabel('Efficiency Ratio')
    ax3.set_ylabel('Cumulative Percentage (%)')
    ax3.set_title('Cumulative Distribution')
    ax3.grid(True, alpha=0.3)
    ax3.set_xlim(left=0)    # Start x-axis at 0
    ax3.set_ylim(bottom=0)  # Start y-axis at 0
    
    # Add some percentile lines
    percentiles = [25, 50, 75, 90, 95]
    for p in percentiles:
        value = np.percentile(efficiency_ratio, p)
        ax3.axvline(value, color='red', alpha=0.5, linestyle=':')
        ax3.text(value, p + 2, f'P{p}: {value:.3f}', rotation=45, fontsize=8)
    
    # 4. Scatter plot vs total CUs (if available)
    if 'total_cus' in df.columns:
        scatter = ax4.scatter(df['total_cus'] / 1_000_000, efficiency_ratio, 
                            alpha=0.6, s=20, c=efficiency_ratio, cmap='viridis')
        ax4.set_xlabel('Total CUs (millions)')
        ax4.set_ylabel('Efficiency Ratio')
        ax4.set_title('Efficiency vs Block Size')
        ax4.grid(True, alpha=0.3)
        ax4.set_xlim(left=0)    # Start x-axis at 0
        ax4.set_ylim(bottom=0)  # Start y-axis at 0
        plt.colorbar(scatter, ax=ax4, label='Efficiency Ratio')
    else:
        # Alternative: ratio vs slot number
        ax4.scatter(df['slot'], efficiency_ratio, alpha=0.6, s=20, color='coral')
        ax4.set_xlabel('Slot Number')
        ax4.set_ylabel('Efficiency Ratio')
        ax4.set_title('Efficiency Over Time')
        ax4.grid(True, alpha=0.3)
        ax4.set_ylim(bottom=0)  # Start y-axis at 0
    
    plt.tight_layout()
    
    if output_dir:
        output_path = Path(output_dir) / 'efficiency_ratio_analysis.png'
        plt.savefig(output_path, dpi=300, bbox_inches='tight')
        print(f"Plot saved to {output_path}")
    else:
        plt.show()

def print_statistics(df):
    """Print detailed statistics about the efficiency ratio."""
    
    efficiency_ratio = df['efficiency_ratio']
    
    print("\n" + "="*60)
    print("EFFICIENCY RATIO STATISTICS")
    print("="*60)
    print(f"Total blocks analyzed: {len(df):,}")
    print(f"Formula: max(critical_path_cus, total_cus/num_tracks) / max_dist_cus")
    print()
    
    # Show breakdown of which constraint dominated
    critical_path_dominated = len(df[df['critical_path_cus'] >= df['total_cus'] / len([c for c in df.columns if c.startswith('track') and c.endswith('_cus')])])
    total_work_dominated = len(df) - critical_path_dominated
    
    print("CONSTRAINT ANALYSIS:")
    print(f"  critical path dominated:  {critical_path_dominated:,} ({critical_path_dominated/len(df)*100:.1f}%)")
    print(f"  Work Distribution dominated: {total_work_dominated:,} ({total_work_dominated/len(df)*100:.1f}%)")
    print()
    
    print("BASIC STATISTICS:")
    print(f"  Mean:     {efficiency_ratio.mean():.4f}")
    print(f"  Median:   {efficiency_ratio.median():.4f}")
    print(f"  Std Dev:  {efficiency_ratio.std():.4f}")
    print(f"  Min:      {efficiency_ratio.min():.4f}")
    print(f"  Max:      {efficiency_ratio.max():.4f}")
    print()
    
    print("PERCENTILES:")
    percentiles = [5, 10, 25, 50, 75, 90, 95, 99]
    for p in percentiles:
        value = np.percentile(efficiency_ratio, p)
        print(f"  {p:2d}th:    {value:.4f}")
    print()
    
    # Efficiency categories
    very_efficient = len(df[efficiency_ratio >= 0.9])
    efficient = len(df[(efficiency_ratio >= 0.7) & (efficiency_ratio < 0.9)])
    moderate = len(df[(efficiency_ratio >= 0.5) & (efficiency_ratio < 0.7)])
    inefficient = len(df[efficiency_ratio < 0.5])
    
    total = len(df)
    print("EFFICIENCY CATEGORIES:")
    print(f"  Very Efficient (≥0.9):    {very_efficient:,} ({very_efficient/total*100:.1f}%)")
    print(f"  Efficient (0.7-0.9):      {efficient:,} ({efficient/total*100:.1f}%)")
    print(f"  Moderate (0.5-0.7):       {moderate:,} ({moderate/total*100:.1f}%)")
    print(f"  Inefficient (<0.5):       {inefficient:,} ({inefficient/total*100:.1f}%)")
    print()
    
    # Interpretation
    mean_ratio = efficiency_ratio.mean()
    if mean_ratio >= 0.8:
        interpretation = "EXCELLENT - Very high parallelization efficiency"
    elif mean_ratio >= 0.6:
        interpretation = "GOOD - Decent parallelization efficiency"
    elif mean_ratio >= 0.4:
        interpretation = "MODERATE - Room for improvement in parallelization"
    else:
        interpretation = "POOR - Significant parallelization bottlenecks"
    
    print(f"INTERPRETATION: {interpretation}")
    print("="*60)

def main():
    parser = argparse.ArgumentParser(description='Analyze and plot efficiency ratios')
    parser.add_argument('csv_file', help='CSV file from block-dag-analyzer (block summary data)')
    parser.add_argument('--output', '-o', help='Output directory for plots')
    parser.add_argument('--stats-only', action='store_true',
                       help='Only print statistics, no plots')
    
    args = parser.parse_args()
    
    # Load and analyze data
    df = load_and_analyze_data(args.csv_file)
    if df is None:
        return 1
    
    # Print statistics
    print_statistics(df)
    
    # Create plots unless stats-only mode
    if not args.stats_only:
        create_efficiency_plots(df, args.output)
    
    return 0

if __name__ == "__main__":
    exit(main()) 