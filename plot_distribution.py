#!/usr/bin/env python3

import matplotlib.pyplot as plt
import csv

def main():
    # Read the CSV data
    bytes_loaded = []
    slots = []
    
    print("Loading data...")
    with open('/Users/igor/projects/agave/snapshot-analyzer/block-usage-results.csv', 'r') as file:
        reader = csv.DictReader(file)
        for row in reader:
            slots.append(int(row['slot']))
            bytes_loaded.append(int(row['bytes_loaded']))
    
    # Convert to MB for readability
    bytes_mb = [b / 1e6 for b in bytes_loaded]
    
    # Calculate basic statistics
    mean_mb = sum(bytes_mb) / len(bytes_mb)
    sorted_mb = sorted(bytes_mb)
    median_mb = sorted_mb[len(sorted_mb) // 2]
    min_mb = min(bytes_mb)
    max_mb = max(bytes_mb)
    
    print(f"Data loaded: {len(bytes_loaded)} slots")
    print(f"Mean: {mean_mb:.2f} MB")
    print(f"Median: {median_mb:.2f} MB")
    print(f"Range: {min_mb:.2f} MB to {max_mb:.2f} MB")
    
    # Create the main distribution plot
    plt.figure(figsize=(14, 10))
    
    # Main histogram
    plt.subplot(2, 2, 1)
    plt.hist(bytes_mb, bins=50, alpha=0.7, color='skyblue', edgecolor='black')
    plt.axvline(mean_mb, color='red', linestyle='--', linewidth=2, label=f'Mean: {mean_mb:.1f} MB')
    plt.axvline(median_mb, color='green', linestyle='--', linewidth=2, label=f'Median: {median_mb:.1f} MB')
    plt.xlabel('Bytes Loaded (MB)')
    plt.ylabel('Frequency')
    plt.title('Distribution of Bytes Loaded per Slot')
    plt.legend()
    plt.grid(True, alpha=0.3)
    
    # Time series
    plt.subplot(2, 2, 2)
    plt.plot(bytes_mb, alpha=0.7, linewidth=0.8, color='purple')
    plt.xlabel('Slot Index')
    plt.ylabel('Bytes Loaded (MB)')
    plt.title('Bytes Loaded Over Time')
    plt.grid(True, alpha=0.3)
    
    # Cumulative distribution
    plt.subplot(2, 2, 3)
    sorted_values = sorted(bytes_mb)
    percentiles = [i / len(sorted_values) * 100 for i in range(1, len(sorted_values) + 1)]
    plt.plot(sorted_values, percentiles, linewidth=2, color='darkorange')
    plt.xlabel('Bytes Loaded (MB)')
    plt.ylabel('Cumulative Percentage')
    plt.title('Cumulative Distribution')
    plt.grid(True, alpha=0.3)
    
    # Statistics summary
    plt.subplot(2, 2, 4)
    plt.axis('off')
    
    # Calculate percentiles
    def percentile(data, p):
        sorted_data = sorted(data)
        index = int(p / 100 * len(sorted_data))
        return sorted_data[min(index, len(sorted_data) - 1)]
    
    stats_text = f"""Block Usage Statistics
    
Total Slots: {len(bytes_loaded):,}

Bytes Loaded (MB):
  Mean: {mean_mb:.2f}
  Median: {median_mb:.2f}
  Min: {min_mb:.2f}
  Max: {max_mb:.2f}
  Range: {max_mb - min_mb:.2f}

Percentiles:
  25th: {percentile(bytes_mb, 25):.2f} MB
  75th: {percentile(bytes_mb, 75):.2f} MB
  90th: {percentile(bytes_mb, 90):.2f} MB
  95th: {percentile(bytes_mb, 95):.2f} MB
  99th: {percentile(bytes_mb, 99):.2f} MB"""
    
    plt.text(0.1, 0.9, stats_text, fontsize=11, verticalalignment='top', 
             bbox=dict(boxstyle='round', facecolor='lightgray', alpha=0.8))
    
    plt.tight_layout()
    plt.suptitle('Solana Block Usage Analysis: Bytes Loaded Distribution', 
                 fontsize=16, fontweight='bold', y=0.98)
    
    # Save the plot
    output_file = '/Users/igor/projects/agave/block_usage_distribution.png'
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"\nVisualization saved as: {output_file}")
    
    # Also create a simple focused histogram
    plt.figure(figsize=(10, 6))
    plt.hist(bytes_mb, bins=60, alpha=0.7, color='lightcoral', edgecolor='darkred')
    plt.axvline(mean_mb, color='blue', linestyle='--', linewidth=2, label=f'Mean: {mean_mb:.1f} MB')
    plt.axvline(median_mb, color='green', linestyle='--', linewidth=2, label=f'Median: {median_mb:.1f} MB')
    
    plt.xlabel('Bytes Loaded (MB)', fontsize=12)
    plt.ylabel('Number of Slots', fontsize=12)
    plt.title('Distribution of Bytes Loaded per Slot in Solana Blocks', fontsize=14, fontweight='bold')
    plt.legend(fontsize=11)
    plt.grid(True, alpha=0.3)
    
    # Add some statistics as text
    plt.text(0.7, 0.9, f'Total Slots: {len(bytes_loaded):,}\nRange: {min_mb:.1f} - {max_mb:.1f} MB', 
             transform=plt.gca().transAxes, fontsize=10,
             bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.8))
    
    simple_output = '/Users/igor/projects/agave/simple_histogram.png'
    plt.savefig(simple_output, dpi=300, bbox_inches='tight')
    print(f"Simple histogram saved as: {simple_output}")
    
    # Show the plots
    plt.show()

if __name__ == "__main__":
    main()


