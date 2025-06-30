#!/usr/bin/env python3
"""
Gantt Chart Generator for Solana Block DAG Analysis
Creates timeline visualization showing transaction execution across tracks.
"""

import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.patches as patches
import numpy as np
import argparse
from pathlib import Path

def load_dag_data(csv_file):
    """Load DAG CSV data."""
    df = pd.read_csv(csv_file)
    # Ensure proper column names
    expected_cols = ['track', 'start_cus', 'end_cus']
    if not all(col in df.columns for col in expected_cols):
        raise ValueError(f"CSV must contain columns: {expected_cols}")
    
    # Add transaction index if not present
    if 'tx_index' not in df.columns:
        df['tx_index'] = df.index
    
    # Handle on_critical_path column (default to False if not present)
    if 'on_critical_path' not in df.columns:
        df['on_critical_path'] = False
    else:
        # Convert string boolean to actual boolean
        df['on_critical_path'] = df['on_critical_path'].astype(str).str.lower().isin(['true', '1', 'yes'])
    
    # Calculate duration
    df['duration'] = df['end_cus'] - df['start_cus']
    
    return df

def create_gantt_chart(df, output_file=None, title="Transaction Execution Timeline"):
    """Create a Gantt chart from DAG data."""
    
    # Set up the plot
    fig, ax = plt.subplots(figsize=(16, 10))
    
    # Get unique tracks and sort them numerically
    tracks = sorted(df['track'].unique())
    track_colors = plt.cm.Set3(np.linspace(0, 1, len(tracks)))
    
    # Create color map for tracks
    color_map = {track: color for track, color in zip(tracks, track_colors)}
    
    # Calculate x-axis limits first
    min_start = df['start_cus'].min() / 1_000_000
    max_end = df['end_cus'].max() / 1_000_000
    x_range = max_end - min_start
    x_padding = x_range * 0.05  # 5% padding
    
    # Plot each transaction as a horizontal bar
    for _, row in df.iterrows():
        track = row['track']
        start = row['start_cus'] / 1_000_000  # Convert to millions for readability
        duration = row['duration'] / 1_000_000
        on_critical_path = row['on_critical_path']
        
        # Choose color: red for critical path, track color otherwise
        color = 'red' if on_critical_path else color_map[track]
        alpha = 0.9 if on_critical_path else 0.7
        
        # Create rectangle for this transaction
        rect = patches.Rectangle(
            (start, track - 0.4),  # (x, y) - position
            duration,              # width
            0.8,                  # height
            linewidth=2 if on_critical_path else 1,
            edgecolor='darkred' if on_critical_path else 'black',
            facecolor=color,
            alpha=alpha
        )
        ax.add_patch(rect)
        
        # Add transaction index as text if duration is large enough
        if duration > x_range * 0.02:  # Only if >2% of total range
            text_color = 'white' if on_critical_path else 'black'
            ax.text(start + duration/2, track, f"{row['tx_index']}", 
                   ha='center', va='center', fontsize=8, fontweight='bold', color=text_color)
    
    # Set axis limits properly
    ax.set_xlim(min_start - x_padding, max_end + x_padding)
    ax.set_ylim(-0.5, len(tracks) - 0.5)
    ax.set_yticks(range(len(tracks)))
    ax.set_yticklabels([f'track {track}' for track in tracks])
    ax.set_xlabel('Compute Units (millions)')
    ax.set_ylabel('Execution track')
    ax.set_title(title)
    ax.grid(True, alpha=0.3, axis='x')
    
    # Add legend
    legend_elements = [patches.Patch(color=color_map[track], label=f'track {track}') 
                      for track in tracks]
    # Add critical path legend entry
    legend_elements.append(patches.Patch(color='red', label='critical path'))
    ax.legend(handles=legend_elements, loc='upper right', bbox_to_anchor=(1.15, 1))
    
    plt.tight_layout()
    
    if output_file:
        plt.savefig(output_file, dpi=300, bbox_inches='tight')
        print(f"Gantt chart saved to {output_file}")
    else:
        plt.show()

def create_detailed_gantt(df, output_file=None, max_transactions=100):
    """Create a detailed Gantt chart with transaction labels."""
    
    # Limit number of transactions for readability
    if len(df) > max_transactions:
        print(f"Warning: Showing only first {max_transactions} transactions for readability")
        df = df.head(max_transactions)
    
    fig, ax = plt.subplots(figsize=(20, 12))
    
    # Sort by start time for better visualization
    df_sorted = df.sort_values('start_cus')
    
    # Get unique tracks and sort them numerically
    tracks = sorted(df['track'].unique())
    track_colors = plt.cm.tab10(np.linspace(0, 1, len(tracks)))
    color_map = {track: color for track, color in zip(tracks, track_colors)}
    
    # Calculate x-axis limits
    min_start = df['start_cus'].min() / 1_000_000
    max_end = df['end_cus'].max() / 1_000_000
    x_range = max_end - min_start
    x_padding = x_range * 0.05
    
    for _, row in df_sorted.iterrows():
        track = row['track']
        start = row['start_cus'] / 1_000_000
        duration = row['duration'] / 1_000_000
        tx_idx = row['tx_index']
        on_critical_path = row['on_critical_path']
        
        # Choose color: red for critical path, track color otherwise
        color = 'red' if on_critical_path else color_map[track]
        alpha = 0.9 if on_critical_path else 0.8
        
        # Create rectangle
        rect = patches.Rectangle(
            (start, track - 0.35),
            duration,
            0.7,
            linewidth=2 if on_critical_path else 1,
            edgecolor='darkred' if on_critical_path else 'black',
            facecolor=color,
            alpha=alpha
        )
        ax.add_patch(rect)
        
        # Always add transaction index
        text_color = 'white' if on_critical_path else 'black'
        font_weight = 'bold'
        ax.text(start + duration/2, track, f"T{tx_idx}", 
               ha='center', va='center', fontsize=7, fontweight=font_weight, color=text_color)
    
    # Set axis limits properly
    ax.set_xlim(min_start - x_padding, max_end + x_padding)
    ax.set_ylim(-0.5, len(tracks) - 0.5)
    ax.set_yticks(range(len(tracks)))
    ax.set_yticklabels([f'track {track}' for track in tracks])
    ax.set_xlabel('Compute Units (millions)')
    ax.set_ylabel('Execution track')
    ax.set_title(f'Detailed Transaction Timeline ({len(df_sorted)} transactions)')
    ax.grid(True, alpha=0.3, axis='x')
    
    # Statistics text
    total_time = df['end_cus'].max() / 1_000_000
    total_work = df['duration'].sum() / 1_000_000
    efficiency = total_work / (total_time * len(tracks)) if total_time > 0 else 0
    critical_path_count = df['on_critical_path'].sum()
    
    stats_text = f"Total Time: {total_time:.1f}M CUs\nTotal Work: {total_work:.1f}M CUs\nEfficiency: {efficiency:.1%}\ncritical path TXs: {critical_path_count}"
    ax.text(0.02, 0.98, stats_text, transform=ax.transAxes, 
           verticalalignment='top', bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.8))
    
    # Add legend
    legend_elements = [patches.Patch(color=color_map[track], label=f'track {track}') 
                      for track in tracks]
    legend_elements.append(patches.Patch(color='red', label='critical path'))
    ax.legend(handles=legend_elements, loc='upper left', bbox_to_anchor=(1.02, 1))
    
    plt.tight_layout()
    
    if output_file:
        plt.savefig(output_file, dpi=300, bbox_inches='tight')
        print(f"Detailed Gantt chart saved to {output_file}")
    else:
        plt.show()

def print_summary_stats(df):
    """Print summary statistics about the DAG data."""
    print("=== DAG Analysis Summary ===")
    print(f"Total transactions: {len(df)}")
    print(f"tracks used: {sorted(df['track'].unique())}")
    print(f"Total execution time: {df['end_cus'].max() / 1_000_000:.1f}M CUs")
    print(f"Total work done: {df['duration'].sum() / 1_000_000:.1f}M CUs")
    
    # critical path statistics
    critical_path_txs = df[df['on_critical_path'] == True]
    if len(critical_path_txs) > 0:
        critical_path_work = critical_path_txs['duration'].sum() / 1_000_000
        print(f"critical path: {len(critical_path_txs)} transactions, {critical_path_work:.1f}M CUs")
        print(f"Critical path ratio: {critical_path_work / (df['duration'].sum() / 1_000_000):.1%}")
    else:
        print("critical path: No transactions marked")
    
    print("\n=== Per-track Statistics ===")
    track_stats = df.groupby('track').agg({
        'duration': ['count', 'sum', 'mean'],
        'start_cus': 'min',
        'end_cus': 'max'
    }).round(0)
    
    for track in sorted(df['track'].unique()):
        track_data = df[df['track'] == track]
        tx_count = len(track_data)
        total_work = track_data['duration'].sum() / 1_000_000
        avg_tx_size = track_data['duration'].mean() / 1_000_000
        utilization = total_work / (df['end_cus'].max() / 1_000_000) * 100
        critical_path_count = track_data['on_critical_path'].sum()
        
        print(f"track {track}: {tx_count} txs, {total_work:.1f}M CUs total, "
              f"{avg_tx_size:.2f}M avg, {utilization:.1f}% utilization, "
              f"{critical_path_count} on critical path")

def main():
    parser = argparse.ArgumentParser(description='Generate Gantt chart from DAG CSV data')
    parser.add_argument('csv_file', help='CSV file from block-dag-analyzer with --dag-csv flag')
    parser.add_argument('--output', '-o', help='Output file for the chart (PNG/PDF)')
    parser.add_argument('--detailed', action='store_true', 
                       help='Create detailed chart with transaction labels')
    parser.add_argument('--max-transactions', type=int, default=100,
                       help='Maximum transactions to show in detailed view')
    parser.add_argument('--title', default='Transaction Execution Timeline',
                       help='Chart title')
    parser.add_argument('--stats-only', action='store_true',
                       help='Only print statistics, no chart')
    
    args = parser.parse_args()
    
    # Load data
    try:
        df = load_dag_data(args.csv_file)
        print(f"Loaded {len(df)} transactions from {args.csv_file}")
    except Exception as e:
        print(f"Error loading data: {e}")
        return 1
    
    # Print statistics
    print_summary_stats(df)
    
    if args.stats_only:
        return 0
    
    # Generate chart
    try:
        if args.detailed:
            create_detailed_gantt(df, args.output, args.max_transactions)
        else:
            create_gantt_chart(df, args.output, args.title)
    except Exception as e:
        print(f"Error creating chart: {e}")
        return 1
    
    return 0

if __name__ == "__main__":
    exit(main()) 