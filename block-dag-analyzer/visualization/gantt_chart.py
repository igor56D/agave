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
    
    # Handle is_simple_vote column (default to False if not present)
    if 'is_simple_vote' not in df.columns:
        df['is_simple_vote'] = False
    else:
        # Convert string boolean to actual boolean
        df['is_simple_vote'] = df['is_simple_vote'].astype(str).str.lower().isin(['true', '1', 'yes'])
    
    # Calculate duration
    df['duration'] = df['end_cus'] - df['start_cus']
    
    return df

def create_gantt_chart(df, output_file=None, title="Transaction Execution Timeline"):
    """Create a Gantt chart from DAG data with vote/regular track distinction."""
    
    # Get separate track lists for vote vs regular transactions
    vote_tracks = sorted(df[df['is_simple_vote']]['track'].unique()) if df['is_simple_vote'].any() else []
    regular_tracks = sorted(df[~df['is_simple_vote']]['track'].unique()) if (~df['is_simple_vote']).any() else []
    
    # Create a mapping from (track_id, is_vote) to display position
    display_positions = {}
    current_pos = 0
    
    # Map regular tracks first (bottom of chart)
    for track in regular_tracks:
        display_positions[(track, False)] = current_pos
        current_pos += 1
    
    # Map vote tracks next (top of chart)
    for track in vote_tracks:
        display_positions[(track, True)] = current_pos
        current_pos += 1
    
    total_tracks = len(regular_tracks) + len(vote_tracks)
    
    # Set up the plot
    fig, ax = plt.subplots(figsize=(16, 10))
    
    # Create different color schemes for regular vs vote tracks (avoiding red)
    # Use blues/greens for regular tracks, oranges/purples for vote tracks
    regular_colors = plt.cm.tab10(np.linspace(0.1, 0.9, len(regular_tracks))) if regular_tracks else []
    vote_colors = plt.cm.Set3(np.linspace(0.2, 0.8, len(vote_tracks))) if vote_tracks else []
    
    # Filter out red-ish colors and replace with safe alternatives
    safe_regular_colors = []
    safe_vote_colors = []
    
    # Safe color palette avoiding reds
    safe_palette = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728', '#9467bd', '#8c564b', '#e377c2', '#7f7f7f', '#bcbd22', '#17becf']
    blue_green_palette = ['#1f77b4', '#2ca02c', '#17becf', '#9467bd', '#8c564b', '#7f7f7f', '#bcbd22']  # Exclude oranges and reds
    orange_purple_palette = ['#ff7f0e', '#9467bd', '#e377c2', '#bcbd22', '#8c564b']  # Exclude reds and blues
    
    # Assign safe colors to regular tracks (blues, greens, purples)
    for i, track in enumerate(regular_tracks):
        color_index = i % len(blue_green_palette)
        safe_regular_colors.append(blue_green_palette[color_index])
    
    # Assign safe colors to vote tracks (oranges, purples, but not red)
    for i, track in enumerate(vote_tracks):
        color_index = i % len(orange_purple_palette)
        safe_vote_colors.append(orange_purple_palette[color_index])
    
    # Create color map using display positions and safe colors
    color_map = {}
    for i, track in enumerate(regular_tracks):
        color_map[(track, False)] = safe_regular_colors[i] if i < len(safe_regular_colors) else '#1f77b4'  # Default blue
    for i, track in enumerate(vote_tracks):
        color_map[(track, True)] = safe_vote_colors[i] if i < len(safe_vote_colors) else '#ff7f0e'  # Default orange
    
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
        is_vote_tx = row['is_simple_vote']  # Use actual vote column
        
        # Get display position for this track
        display_y = display_positions[(track, is_vote_tx)]
        
        # Choose color: red for critical path, track color otherwise
        if on_critical_path:
            color = 'red'
            alpha = 0.9
        else:
            color = color_map[(track, is_vote_tx)]
            alpha = 0.6 if is_vote_tx else 0.7  # Vote transactions slightly more transparent
        
        # Create rectangle for this transaction
        rect = patches.Rectangle(
            (start, display_y - 0.4),  # (x, y) - position using display position
            duration,              # width
            0.8,                  # height
            linewidth=2 if on_critical_path else (1 if not is_vote_tx else 0.5),
            edgecolor='darkred' if on_critical_path else ('black' if not is_vote_tx else 'gray'),
            facecolor=color,
            alpha=alpha,
            linestyle='-' if not is_vote_tx else '--'  # Dashed lines for vote transactions
        )
        ax.add_patch(rect)
        
        # Add transaction index as text if duration is large enough
        if duration > x_range * 0.02:  # Only if >2% of total range
            text_color = 'white' if on_critical_path else 'black'
            ax.text(start + duration/2, display_y, f"{row['tx_index']}", 
                   ha='center', va='center', fontsize=8, fontweight='bold', color=text_color)
    
    # Set axis limits using display positions
    ax.set_xlim(min_start - x_padding, max_end + x_padding)
    ax.set_ylim(-0.5, total_tracks - 0.5)
    
    # Set Y-axis ticks at display positions
    display_positions_list = list(range(total_tracks))
    ax.set_yticks(display_positions_list)
    
    # Create labels that distinguish vote vs regular tracks
    track_labels = []
    for pos in display_positions_list:
        # Find which track corresponds to this display position
        for (track_id, is_vote), display_pos in display_positions.items():
            if display_pos == pos:
                if is_vote:
                    vote_count = df[(df['track'] == track_id) & (df['is_simple_vote'])].shape[0]
                    track_labels.append(f'vote track {track_id} ({vote_count} votes)')
                else:
                    tx_count = df[(df['track'] == track_id) & (~df['is_simple_vote'])].shape[0]
                    track_labels.append(f'track {track_id} ({tx_count} txs)')
                break
    
    ax.set_yticklabels(track_labels)
    ax.set_xlabel('Compute Units (millions)')
    ax.set_ylabel('Execution track')
    ax.set_title(title)
    ax.grid(True, alpha=0.3, axis='x')
    
    # Add legend with track type distinction
    legend_elements = []
    for track in regular_tracks:
        legend_elements.append(patches.Patch(color=color_map[(track, False)], label=f'track {track}'))
    for track in vote_tracks:
        legend_elements.append(patches.Patch(color=color_map[(track, True)], label=f'vote track {track}',
                                           linestyle='--', alpha=0.6))
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
    """Create a detailed Gantt chart with transaction labels and vote/regular track distinction."""
    
    # Get separate track lists for vote vs regular transactions
    vote_tracks = sorted(df[df['is_simple_vote']]['track'].unique()) if df['is_simple_vote'].any() else []
    regular_tracks = sorted(df[~df['is_simple_vote']]['track'].unique()) if (~df['is_simple_vote']).any() else []
    
    # Create a mapping from (track_id, is_vote) to display position
    display_positions = {}
    current_pos = 0
    
    # Map regular tracks first (bottom of chart)
    for track in regular_tracks:
        display_positions[(track, False)] = current_pos
        current_pos += 1
    
    # Map vote tracks next (top of chart)
    for track in vote_tracks:
        display_positions[(track, True)] = current_pos
        current_pos += 1
    
    total_tracks = len(regular_tracks) + len(vote_tracks)
    
    # Limit number of transactions for readability
    if len(df) > max_transactions:
        print(f"Warning: Showing only first {max_transactions} transactions for readability")
        df = df.head(max_transactions)
    
    fig, ax = plt.subplots(figsize=(20, 12))
    
    # Sort by start time for better visualization
    df_sorted = df.sort_values('start_cus')
    
    # Create different color schemes for regular vs vote tracks (avoiding red)
    # Use blues/greens for regular tracks, oranges/purples for vote tracks
    safe_regular_colors = []
    safe_vote_colors = []
    
    # Safe color palette avoiding reds
    blue_green_palette = ['#1f77b4', '#2ca02c', '#17becf', '#9467bd', '#8c564b', '#7f7f7f', '#bcbd22']  # Exclude oranges and reds
    orange_purple_palette = ['#ff7f0e', '#9467bd', '#e377c2', '#bcbd22', '#8c564b']  # Exclude reds and blues
    
    # Assign safe colors to regular tracks (blues, greens, purples)
    for i, track in enumerate(regular_tracks):
        color_index = i % len(blue_green_palette)
        safe_regular_colors.append(blue_green_palette[color_index])
    
    # Assign safe colors to vote tracks (oranges, purples, but not red)
    for i, track in enumerate(vote_tracks):
        color_index = i % len(orange_purple_palette)
        safe_vote_colors.append(orange_purple_palette[color_index])
    
    # Create color map using display positions and safe colors
    color_map = {}
    for i, track in enumerate(regular_tracks):
        color_map[(track, False)] = safe_regular_colors[i] if i < len(safe_regular_colors) else '#1f77b4'  # Default blue
    for i, track in enumerate(vote_tracks):
        color_map[(track, True)] = safe_vote_colors[i] if i < len(safe_vote_colors) else '#ff7f0e'  # Default orange
    
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
        is_vote_tx = row['is_simple_vote']  # Use actual vote column
        
        # Get display position for this track
        display_y = display_positions[(track, is_vote_tx)]
        
        # Choose color: red for critical path, track color otherwise
        if on_critical_path:
            color = 'red'
            alpha = 0.9
        else:
            color = color_map[(track, is_vote_tx)]
            alpha = 0.6 if is_vote_tx else 0.8
        
        # Create rectangle
        rect = patches.Rectangle(
            (start, display_y - 0.35),
            duration,
            0.7,
            linewidth=2 if on_critical_path else (1 if not is_vote_tx else 0.5),
            edgecolor='darkred' if on_critical_path else ('black' if not is_vote_tx else 'gray'),
            facecolor=color,
            alpha=alpha,
            linestyle='-' if not is_vote_tx else '--'
        )
        ax.add_patch(rect)
        
        # Always add transaction index with different prefix for vote transactions
        text_color = 'white' if on_critical_path else 'black'
        font_weight = 'bold'
        tx_prefix = 'V' if is_vote_tx else 'T'
        ax.text(start + duration/2, display_y, f"{tx_prefix}{tx_idx}", 
               ha='center', va='center', fontsize=7, fontweight=font_weight, color=text_color)
    
    # Set axis limits using display positions
    ax.set_xlim(min_start - x_padding, max_end + x_padding)
    ax.set_ylim(-0.5, total_tracks - 0.5)
    
    # Set Y-axis ticks at display positions
    display_positions_list = list(range(total_tracks))
    ax.set_yticks(display_positions_list)
    
    # Create labels that distinguish vote vs regular tracks
    track_labels = []
    for pos in display_positions_list:
        # Find which track corresponds to this display position
        for (track_id, is_vote), display_pos in display_positions.items():
            if display_pos == pos:
                if is_vote:
                    vote_count = df[(df['track'] == track_id) & (df['is_simple_vote'])].shape[0]
                    track_labels.append(f'vote track {track_id} ({vote_count} votes)')
                else:
                    tx_count = df[(df['track'] == track_id) & (~df['is_simple_vote'])].shape[0]
                    track_labels.append(f'track {track_id} ({tx_count} txs)')
                break
    
    ax.set_yticklabels(track_labels)
    ax.set_xlabel('Compute Units (millions)')
    ax.set_ylabel('Execution track')
    ax.set_title(f'Detailed Transaction Timeline ({len(df_sorted)} transactions)')
    ax.grid(True, alpha=0.3, axis='x')
    
    # Statistics text with vote/regular track breakdown
    total_time = df['end_cus'].max() / 1_000_000
    total_work = df['duration'].sum() / 1_000_000
    efficiency = total_work / (total_time * total_tracks) if total_time > 0 else 0
    critical_path_count = df['on_critical_path'].sum()
    
    # Calculate vote vs regular statistics using is_simple_vote column
    vote_txs = df[df['is_simple_vote']]
    regular_txs = df[~df['is_simple_vote']]
    
    stats_text = f"Total Time: {total_time:.1f}M CUs\nTotal Work: {total_work:.1f}M CUs\nEfficiency: {efficiency:.1%}\n"
    stats_text += f"Critical path TXs: {critical_path_count}\n"
    stats_text += f"Regular TXs: {len(regular_txs)} ({len(regular_tracks)} tracks)\n"
    stats_text += f"Vote TXs: {len(vote_txs)} ({len(vote_tracks)} tracks)"
    
    ax.text(0.02, 0.98, stats_text, transform=ax.transAxes, 
           verticalalignment='top', bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.8))
    
    # Add legend with track type distinction
    legend_elements = []
    for track in regular_tracks:
        legend_elements.append(patches.Patch(color=color_map[(track, False)], label=f'track {track}'))
    for track in vote_tracks:
        legend_elements.append(patches.Patch(color=color_map[(track, True)], label=f'vote track {track}',
                                           linestyle='--', alpha=0.6))
    legend_elements.append(patches.Patch(color='red', label='critical path'))
    ax.legend(handles=legend_elements, loc='upper left', bbox_to_anchor=(1.02, 1))
    
    plt.tight_layout()
    
    if output_file:
        plt.savefig(output_file, dpi=300, bbox_inches='tight')
        print(f"Detailed Gantt chart saved to {output_file}")
    else:
        plt.show()

def print_summary_stats(df):
    """Print summary statistics about the DAG data with vote/regular track breakdown."""
    
    # Get separate track lists for vote vs regular transactions
    vote_tracks = sorted(df[df['is_simple_vote']]['track'].unique()) if df['is_simple_vote'].any() else []
    regular_tracks = sorted(df[~df['is_simple_vote']]['track'].unique()) if (~df['is_simple_vote']).any() else []
    
    # Get vote and regular transaction statistics using is_simple_vote column
    vote_txs = df[df['is_simple_vote']]
    regular_txs = df[~df['is_simple_vote']]
    
    print("=== DAG Analysis Summary ===")
    print(f"Total transactions: {len(df)}")
    print(f"Regular transactions: {len(regular_txs)} on {len(regular_tracks)} tracks")
    print(f"Vote transactions: {len(vote_txs)} on {len(vote_tracks)} tracks")
    print(f"Total execution time: {df['end_cus'].max() / 1_000_000:.1f}M CUs")
    print(f"Total work done: {df['duration'].sum() / 1_000_000:.1f}M CUs")
    
    # critical path statistics
    critical_path_txs = df[df['on_critical_path'] == True]
    if len(critical_path_txs) > 0:
        critical_path_work = critical_path_txs['duration'].sum() / 1_000_000
        print(f"Critical path: {len(critical_path_txs)} transactions, {critical_path_work:.1f}M CUs")
        print(f"Critical path ratio: {critical_path_work / (df['duration'].sum() / 1_000_000):.1%}")
    else:
        print("Critical path: No transactions marked")
    
    # Separate statistics for regular tracks
    print("\n=== Regular Track Statistics ===")
    if regular_tracks:
        for track in regular_tracks:
            track_data = df[(df['track'] == track) & (~df['is_simple_vote'])]
            
            tx_count = len(track_data)
            total_work = track_data['duration'].sum() / 1_000_000
            avg_tx_size = track_data['duration'].mean() / 1_000_000 if tx_count > 0 else 0
            utilization = total_work / (df['end_cus'].max() / 1_000_000) * 100
            critical_path_count = track_data['on_critical_path'].sum()
            
            print(f"Track {track}: {tx_count} txs, {total_work:.1f}M CUs total, "
                  f"{avg_tx_size:.2f}M avg, {utilization:.1f}% utilization, "
                  f"{critical_path_count} on critical path")
        
        # Regular track totals
        regular_total_work = regular_txs['duration'].sum() / 1_000_000
        regular_avg_utilization = regular_total_work / (df['end_cus'].max() / 1_000_000) / len(regular_tracks) * 100 if regular_tracks else 0
        print(f"Regular tracks total: {len(regular_txs)} txs, {regular_total_work:.1f}M CUs, "
              f"{regular_avg_utilization:.1f}% avg utilization")
    else:
        print("No regular tracks found")
    
    print("\n=== Vote Track Statistics ===")
    if vote_tracks:
        for track in vote_tracks:
            track_data = df[(df['track'] == track) & (df['is_simple_vote'])]
            
            tx_count = len(track_data)
            total_work = track_data['duration'].sum() / 1_000_000
            avg_tx_size = track_data['duration'].mean() / 1_000_000 if tx_count > 0 else 0
            utilization = total_work / (df['end_cus'].max() / 1_000_000) * 100
            critical_path_count = track_data['on_critical_path'].sum()
            
            print(f"Vote track {track}: {tx_count} txs, {total_work:.1f}M CUs total, "
                  f"{avg_tx_size:.2f}M avg, {utilization:.1f}% utilization, "
                  f"{critical_path_count} on critical path")
        
        # Vote track totals
        vote_total_work = vote_txs['duration'].sum() / 1_000_000
        vote_avg_utilization = vote_total_work / (df['end_cus'].max() / 1_000_000) / len(vote_tracks) * 100 if vote_tracks else 0
        print(f"Vote tracks total: {len(vote_txs)} txs, {vote_total_work:.1f}M CUs, "
              f"{vote_avg_utilization:.1f}% avg utilization")
    else:
        print("No vote tracks found")

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
    
    # Print statistics (no longer need num_regular_tracks parameter)
    print_summary_stats(df)
    
    if args.stats_only:
        return 0
    
    # Generate chart (no longer need num_regular_tracks parameter)
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