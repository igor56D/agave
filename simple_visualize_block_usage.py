#!/usr/bin/env python3

import matplotlib.pyplot as plt
import numpy as np
import csv
from matplotlib.ticker import FuncFormatter

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

def load_csv_data(filename):
    """Load data from CSV file"""
    slots = []
    bytes_loaded = []
    
    with open(filename, 'r') as file:
        reader = csv.DictReader(file)
        for row in reader:
            slots.append(int(row['slot']))
            bytes_loaded.append(int(row['bytes_loaded']))
    
    return np.array(slots), np.array(bytes_loaded)

def main():
    # Load the data
    print("Loading data from block-usage-results.csv...")
    slots, bytes_loaded = load_csv_data('/Users/igor/projects/agave/snapshot-analyzer/block-usage-results.csv')
    
    # Convert to MB for better readability
    bytes_mb = bytes_loaded / 1e6
    
    # Basic statistics
    print(f"\nData Summary:")
    print(f"Total slots: {len(bytes_loaded)}")
    print(f"Bytes loaded statistics:")
    print(f"  Mean: {np.mean(bytes_loaded):,.0f} bytes ({np.mean(bytes_mb):.2f} MB)")
    print(f"  Median: {np.median(bytes_loaded):,.0f} bytes ({np.median(bytes_mb):.2f} MB)")
    print(f"  Min: {np.min(bytes_loaded):,.0f} bytes ({np.min(bytes_mb):.2f} MB)")
    print(f"  Max: {np.max(bytes_loaded):,.0f} bytes ({np.max(bytes_mb):.2f} MB)")
    print(f"  Std: {np.std(bytes_loaded):,.0f} bytes ({np.std(bytes_mb):.2f} MB)")
    
    # Create a figure with multiple subplots
    fig, axes = plt.subplots(2, 2, figsize=(15, 12))
    fig.suptitle('Block Usage Analysis: Distribution of Bytes Loaded', fontsize=16, fontweight='bold')
    
    # 1. Histogram
    ax1 = axes[0, 0]
    n_bins = min(50, len(bytes_loaded) // 20)  # Adaptive number of bins
    counts, bins, patches = ax1.hist(bytes_loaded, bins=n_bins, alpha=0.7, color='skyblue', edgecolor='black')
    ax1.set_xlabel('Bytes Loaded')
    ax1.set_ylabel('Frequency')
    ax1.set_title('Distribution of Bytes Loaded (Histogram)')
    ax1.xaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax1.grid(True, alpha=0.3)
    
    # Add statistics lines
    mean_val = np.mean(bytes_loaded)
    median_val = np.median(bytes_loaded)
    ax1.axvline(mean_val, color='red', linestyle='--', linewidth=2, label=f'Mean: {mean_val/1e6:.1f}MB')
    ax1.axvline(median_val, color='green', linestyle='--', linewidth=2, label=f'Median: {median_val/1e6:.1f}MB')
    ax1.legend()
    
    # 2. Box plot equivalent (using percentiles)
    ax2 = axes[0, 1]
    percentiles = [5, 25, 50, 75, 95]
    pct_values = [np.percentile(bytes_loaded, p) for p in percentiles]
    
    # Create a violin-like plot manually
    ax2.barh(0, pct_values[4] - pct_values[0], left=pct_values[0], height=0.3, 
             alpha=0.3, color='lightcoral', label='5th-95th percentile')
    ax2.barh(0, pct_values[3] - pct_values[1], left=pct_values[1], height=0.6, 
             alpha=0.6, color='lightcoral', label='25th-75th percentile')
    ax2.axvline(pct_values[2], color='black', linewidth=3, label='Median')
    
    ax2.set_xlabel('Bytes Loaded')
    ax2.set_title('Box Plot Equivalent (Percentiles)')
    ax2.xaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax2.set_yticks([])
    ax2.grid(True, alpha=0.3)
    ax2.legend()
    
    # 3. Time series plot
    ax3 = axes[1, 0]
    ax3.plot(range(len(bytes_loaded)), bytes_loaded, alpha=0.7, linewidth=0.8, color='purple')
    ax3.set_xlabel('Slot Index')
    ax3.set_ylabel('Bytes Loaded')
    ax3.set_title('Bytes Loaded Over Time (by Slot Order)')
    ax3.yaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax3.grid(True, alpha=0.3)
    
    # Add rolling average
    window_size = max(10, len(bytes_loaded) // 100)
    rolling_mean = np.convolve(bytes_loaded, np.ones(window_size)/window_size, mode='same')
    ax3.plot(range(len(bytes_loaded)), rolling_mean, color='red', linewidth=2, 
             label=f'Rolling Mean (window={window_size})')
    ax3.legend()
    
    # 4. Cumulative distribution
    ax4 = axes[1, 1]
    sorted_values = np.sort(bytes_loaded)
    percentiles_array = np.arange(1, len(sorted_values) + 1) / len(sorted_values) * 100
    ax4.plot(sorted_values, percentiles_array, linewidth=2, color='darkorange')
    ax4.set_xlabel('Bytes Loaded')
    ax4.set_ylabel('Cumulative Percentage')
    ax4.set_title('Cumulative Distribution Function')
    ax4.xaxis.set_major_formatter(FuncFormatter(format_bytes))
    ax4.grid(True, alpha=0.3)
    
    # Add percentile markers
    percentile_marks = [25, 50, 75, 90, 95, 99]
    for p in percentile_marks:
        value = np.percentile(bytes_loaded, p)
        ax4.axvline(value, color='red', linestyle=':', alpha=0.6)
        ax4.text(value, p, f'P{p}\n{value/1e6:.1f}MB', 
                rotation=90, verticalalignment='bottom', fontsize=8)
    
    plt.tight_layout()
    
    # Save the plot
    output_file = '/Users/igor/projects/agave/block_usage_distribution.png'
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"\nVisualization saved as: {output_file}")
    
    # Create detailed histogram
    fig2, ax = plt.subplots(1, 1, figsize=(12, 8))
    
    # Create detailed histogram
    n_bins = min(100, len(bytes_mb) // 10)
    counts, bins, patches = ax.hist(bytes_mb, bins=n_bins, alpha=0.7, color='lightblue', 
                                   edgecolor='navy', linewidth=0.5)
    
    ax.set_xlabel('Bytes Loaded (MB)', fontsize=12)
    ax.set_ylabel('Frequency', fontsize=12)
    ax.set_title('Detailed Distribution of Bytes Loaded per Slot', fontsize=14, fontweight='bold')
    ax.grid(True, alpha=0.3)
    
    # Add statistics
    mean_mb = np.mean(bytes_mb)
    median_mb = np.median(bytes_mb)
    std_mb = np.std(bytes_mb)
    
    ax.axvline(mean_mb, color='red', linestyle='--', linewidth=2, label=f'Mean: {mean_mb:.1f} MB')
    ax.axvline(median_mb, color='green', linestyle='--', linewidth=2, label=f'Median: {median_mb:.1f} MB')
    ax.axvline(mean_mb + std_mb, color='orange', linestyle=':', linewidth=2, label=f'+1σ: {mean_mb + std_mb:.1f} MB')
    ax.axvline(mean_mb - std_mb, color='orange', linestyle=':', linewidth=2, label=f'-1σ: {mean_mb - std_mb:.1f} MB')
    
    ax.legend(fontsize=10)
    
    # Add text box with statistics
    stats_text = f"""Statistics:
Count: {len(bytes_loaded):,} slots
Mean: {mean_mb:.2f} MB
Median: {median_mb:.2f} MB
Std Dev: {std_mb:.2f} MB
Min: {np.min(bytes_mb):.2f} MB
Max: {np.max(bytes_mb):.2f} MB

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
    
    # Show the plots
    plt.show()
    
    # Print additional insights
    print(f"\nAdditional Insights:")
    print(f"Range: {(np.max(bytes_mb) - np.min(bytes_mb)):.2f} MB")
    print(f"Coefficient of Variation: {(std_mb / mean_mb * 100):.2f}%")
    
    # Identify outliers (values beyond 1.5 * IQR)
    Q1 = np.percentile(bytes_mb, 25)
    Q3 = np.percentile(bytes_mb, 75)
    IQR = Q3 - Q1
    lower_bound = Q1 - 1.5 * IQR
    upper_bound = Q3 + 1.5 * IQR
    outliers = bytes_mb[(bytes_mb < lower_bound) | (bytes_mb > upper_bound)]
    print(f"Outliers (beyond 1.5*IQR): {len(outliers)} slots ({len(outliers)/len(bytes_mb)*100:.2f}%)")
    
    if len(outliers) > 0:
        print(f"Outlier range: {np.min(outliers):.2f} MB to {np.max(outliers):.2f} MB")

if __name__ == "__main__":
    main()


