#!/usr/bin/env python3

import pandas as pd
import matplotlib.pyplot as plt
import numpy as np
import seaborn as sns
from matplotlib.ticker import FuncFormatter

# Set style for better-looking plots
plt.style.use('seaborn-v0_8')
sns.set_palette("husl")

def format_bytes(x, pos):
    """Format bytes as MB or GB for axis labels"""
    if x >= 1e9:
        return f'{x/1e9:.1f}GB'
    elif x >= 1e6:
        return f'{x/1e6:.1f}MB'
    elif x >= 1e3:
        return f'{x/1e3:.1f}KB'
    else:
        return f'{x:.0f}B'

def main():
    # Read the CSV file
    print("Loading data from block-usage-results.csv...")
    df = pd.read_csv('/Users/igor/projects/agave/snapshot-analyzer/block-usage-results.csv')
    
    # Basic statistics
    print(f"\nData Summary:")
    print(f"Total slots: {len(df)}")
    print(f"Bytes loaded statistics:")
    print(f"  Mean: {df['bytes_loaded'].mean():,.0f} bytes ({df['bytes_loaded'].mean()/1e6:.2f} MB)")
    print(f"  Median: {df['bytes_loaded'].median():,.0f} bytes ({df['bytes_loaded'].median()/1e6:.2f} MB)")
    print(f"  Min: {df['bytes_loaded'].min():,.0f} bytes ({df['bytes_loaded'].min()/1e6:.2f} MB)")
    print(f"  Max: {df['bytes_loaded'].max():,.0f} bytes ({df['bytes_loaded'].max()/1e6:.2f} MB)")
    print(f"  Std: {df['bytes_loaded'].std():,.0f} bytes ({df['bytes_loaded'].std()/1e6:.2f} MB)")
    
    # Create a figure with multiple subplots
    fig, axes = plt.subplots(2, 2, figsize=(15, 12))
    fig.suptitle('Block Usage Analysis: Distribution of Bytes Loaded', fontsize=16, fontweight='bold')
    
    # 1. Histogram
    ax1 = axes[0, 0]
    n_bins = min(50, len(df) // 20)  # Adaptive number of bins
    counts, bins, patches = ax1.hist(df['bytes_loaded'], bins=n_bins, alpha=0.7, color='skyblue', edgecolor='black')
    ax1.set_xlabel('Bytes Loaded')
    ax1.set_ylabel('Frequency')
    ax1.set_title('Distribution of Bytes Loaded (Histogram)')
    ax1.xaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax1.grid(True, alpha=0.3)
    
    # Add statistics text to histogram
    mean_val = df['bytes_loaded'].mean()
    median_val = df['bytes_loaded'].median()
    ax1.axvline(mean_val, color='red', linestyle='--', linewidth=2, label=f'Mean: {mean_val/1e6:.1f}MB')
    ax1.axvline(median_val, color='green', linestyle='--', linewidth=2, label=f'Median: {median_val/1e6:.1f}MB')
    ax1.legend()
    
    # 2. Box plot
    ax2 = axes[0, 1]
    box_plot = ax2.boxplot(df['bytes_loaded'], patch_artist=True)
    box_plot['boxes'][0].set_facecolor('lightcoral')
    ax2.set_ylabel('Bytes Loaded')
    ax2.set_title('Box Plot of Bytes Loaded')
    ax2.yaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax2.grid(True, alpha=0.3)
    
    # 3. Time series plot (assuming slots are in chronological order)
    ax3 = axes[1, 0]
    ax3.plot(range(len(df)), df['bytes_loaded'], alpha=0.7, linewidth=0.8, color='purple')
    ax3.set_xlabel('Slot Index')
    ax3.set_ylabel('Bytes Loaded')
    ax3.set_title('Bytes Loaded Over Time (by Slot Order)')
    ax3.yaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax3.grid(True, alpha=0.3)
    
    # Add rolling average
    window_size = max(10, len(df) // 100)
    rolling_mean = df['bytes_loaded'].rolling(window=window_size, center=True).mean()
    ax3.plot(range(len(df)), rolling_mean, color='red', linewidth=2, 
             label=f'Rolling Mean (window={window_size})')
    ax3.legend()
    
    # 4. Cumulative distribution
    ax4 = axes[1, 1]
    sorted_values = np.sort(df['bytes_loaded'])
    percentiles = np.arange(1, len(sorted_values) + 1) / len(sorted_values) * 100
    ax4.plot(sorted_values, percentiles, linewidth=2, color='darkorange')
    ax4.set_xlabel('Bytes Loaded')
    ax4.set_ylabel('Cumulative Percentage')
    ax4.set_title('Cumulative Distribution Function')
    ax4.xaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax4.grid(True, alpha=0.3)
    
    # Add percentile markers
    percentile_marks = [25, 50, 75, 90, 95, 99]
    for p in percentile_marks:
        value = np.percentile(df['bytes_loaded'], p)
        ax4.axvline(value, color='red', linestyle=':', alpha=0.6)
        ax4.text(value, p, f'P{p}\n{value/1e6:.1f}MB', 
                rotation=90, verticalalignment='bottom', fontsize=8)
    
    plt.tight_layout()
    
    # Save the plot
    output_file = '/Users/igor/projects/agave/block_usage_distribution.png'
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"\nVisualization saved as: {output_file}")
    
    # Create an additional detailed histogram with statistics
    fig2, ax = plt.subplots(1, 1, figsize=(12, 8))
    
    # Convert to MB for better readability
    bytes_mb = df['bytes_loaded'] / 1e6
    
    # Create histogram with more details
    n_bins = min(100, len(df) // 10)
    counts, bins, patches = ax.hist(bytes_mb, bins=n_bins, alpha=0.7, color='lightblue', 
                                   edgecolor='navy', linewidth=0.5)
    
    # Color code the histogram bars based on value ranges
    cm = plt.cm.viridis
    for i, (count, bin_edge) in enumerate(zip(counts, bins[:-1])):
        patches[i].set_facecolor(cm(bin_edge / max(bytes_mb)))
    
    ax.set_xlabel('Bytes Loaded (MB)', fontsize=12)
    ax.set_ylabel('Frequency', fontsize=12)
    ax.set_title('Detailed Distribution of Bytes Loaded per Slot', fontsize=14, fontweight='bold')
    ax.grid(True, alpha=0.3)
    
    # Add statistics
    mean_mb = bytes_mb.mean()
    median_mb = bytes_mb.median()
    std_mb = bytes_mb.std()
    
    ax.axvline(mean_mb, color='red', linestyle='--', linewidth=2, label=f'Mean: {mean_mb:.1f} MB')
    ax.axvline(median_mb, color='green', linestyle='--', linewidth=2, label=f'Median: {median_mb:.1f} MB')
    ax.axvline(mean_mb + std_mb, color='orange', linestyle=':', linewidth=2, label=f'+1σ: {mean_mb + std_mb:.1f} MB')
    ax.axvline(mean_mb - std_mb, color='orange', linestyle=':', linewidth=2, label=f'-1σ: {mean_mb - std_mb:.1f} MB')
    
    ax.legend(fontsize=10)
    
    # Add text box with statistics
    stats_text = f"""Statistics:
    Count: {len(df):,} slots
    Mean: {mean_mb:.2f} MB
    Median: {median_mb:.2f} MB
    Std Dev: {std_mb:.2f} MB
    Min: {bytes_mb.min():.2f} MB
    Max: {bytes_mb.max():.2f} MB
    
    Percentiles:
    25th: {np.percentile(bytes_mb, 25):.2f} MB
    75th: {np.percentile(bytes_mb, 75):.2f} MB
    95th: {np.percentile(bytes_mb, 95):.2f} MB
    99th: {np.percentile(bytes_mb, 99):.2f} MB"""
    
    ax.text(0.02, 0.98, stats_text, transform=ax.transAxes, fontsize=9,
            verticalalignment='top', bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.8))
    
    # Save the detailed plot
    detailed_output = '/Users/igor/projects/agave/block_usage_detailed_distribution.png'
    plt.savefig(detailed_output, dpi=300, bbox_inches='tight')
    print(f"Detailed visualization saved as: {detailed_output}")
    
    # Show both plots
    plt.show()
    
    # Print additional insights
    print(f"\nAdditional Insights:")
    print(f"Range: {(bytes_mb.max() - bytes_mb.min()):.2f} MB")
    print(f"Coefficient of Variation: {(std_mb / mean_mb * 100):.2f}%")
    
    # Identify outliers (values beyond 1.5 * IQR)
    Q1 = bytes_mb.quantile(0.25)
    Q3 = bytes_mb.quantile(0.75)
    IQR = Q3 - Q1
    lower_bound = Q1 - 1.5 * IQR
    upper_bound = Q3 + 1.5 * IQR
    outliers = bytes_mb[(bytes_mb < lower_bound) | (bytes_mb > upper_bound)]
    print(f"Outliers (beyond 1.5*IQR): {len(outliers)} slots ({len(outliers)/len(df)*100:.2f}%)")
    
    if len(outliers) > 0:
        print(f"Outlier range: {outliers.min():.2f} MB to {outliers.max():.2f} MB")

if __name__ == "__main__":
    main()


