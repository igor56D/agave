#!/usr/bin/env python3
"""
Block Limit Analysis for Solana Block DAG Data
Analyzes what percentage of blocks would be invalid with CU limits.
"""

import pandas as pd
import argparse
import sys

def analyze_block_limits(csv_file, cu_limit=25_000_000):
    """Analyze what percentage of blocks exceed the CU limit."""
    
    try:
        # Load the block summary data
        df = pd.read_csv(csv_file)
        
        # Verify we have the required column
        if 'max_dist_cus' not in df.columns:
            print(f"Error: CSV file must contain 'max_dist_cus' column")
            print(f"Available columns: {list(df.columns)}")
            return None
            
        total_blocks = len(df)
        if total_blocks == 0:
            print("Error: No blocks found in CSV file")
            return None
            
        # Find blocks that exceed the limit
        invalid_blocks = df[df['max_dist_cus'] > cu_limit]
        invalid_count = len(invalid_blocks)
        
        # Calculate percentage
        invalid_percentage = (invalid_count / total_blocks) * 100
        
        # Calculate statistics
        results = {
            'total_blocks': total_blocks,
            'invalid_blocks': invalid_count,
            'valid_blocks': total_blocks - invalid_count,
            'invalid_percentage': invalid_percentage,
            'valid_percentage': 100 - invalid_percentage,
            'cu_limit': cu_limit,
            'max_dist_stats': {
                'min': df['max_dist_cus'].min(),
                'max': df['max_dist_cus'].max(),
                'mean': df['max_dist_cus'].mean(),
                'median': df['max_dist_cus'].median(),
                'std': df['max_dist_cus'].std()
            }
        }
        
        return results, df, invalid_blocks
        
    except Exception as e:
        print(f"Error reading CSV file: {e}")
        return None

def print_analysis(results, show_details=False):
    """Print the analysis results."""
    
    print("=" * 60)
    print("BLOCK LIMIT ANALYSIS")
    print("=" * 60)
    print(f"CU Limit: {results['cu_limit']:,} CUs ({results['cu_limit']/1_000_000:.1f}M)")
    print(f"Total blocks analyzed: {results['total_blocks']:,}")
    print()
    
    print("VALIDITY RESULTS:")
    print(f"  Valid blocks:   {results['valid_blocks']:,} ({results['valid_percentage']:.2f}%)")
    print(f"  Invalid blocks: {results['invalid_blocks']:,} ({results['invalid_percentage']:.2f}%)")
    print()
    
    stats = results['max_dist_stats']
    print("MAX_DIST_CUS STATISTICS:")
    print(f"  Minimum:   {stats['min']:,} CUs ({stats['min']/1_000_000:.2f}M)")
    print(f"  Maximum:   {stats['max']:,} CUs ({stats['max']/1_000_000:.2f}M)")
    print(f"  Mean:      {stats['mean']:,.0f} CUs ({stats['mean']/1_000_000:.2f}M)")
    print(f"  Median:    {stats['median']:,.0f} CUs ({stats['median']/1_000_000:.2f}M)")
    print(f"  Std Dev:   {stats['std']:,.0f} CUs ({stats['std']/1_000_000:.2f}M)")
    
    if show_details and results['invalid_blocks'] > 0:
        print(f"\nWORST OFFENDERS (showing up to 10):")
        
def analyze_multiple_limits(df, limits):
    """Analyze multiple CU limits."""
    
    print("\n" + "=" * 80)
    print("MULTIPLE LIMIT ANALYSIS")
    print("=" * 80)
    print(f"{'Limit (M CUs)':<12} {'Invalid Blocks':<15} {'Invalid %':<12} {'Valid %':<10}")
    print("-" * 80)
    
    for limit in limits:
        invalid_count = len(df[df['max_dist_cus'] > limit])
        total_blocks = len(df)
        invalid_pct = (invalid_count / total_blocks) * 100
        valid_pct = 100 - invalid_pct
        
        print(f"{limit/1_000_000:<12.1f} {invalid_count:<15,} {invalid_pct:<12.2f} {valid_pct:<10.2f}")

def show_invalid_blocks(invalid_blocks, limit, max_show=10):
    """Show details of invalid blocks."""
    
    if len(invalid_blocks) == 0:
        return
        
    print(f"\nINVALID BLOCKS (exceeding {limit/1_000_000:.1f}M CUs):")
    print("-" * 80)
    
    # Sort by max_dist_cus descending to show worst first
    sorted_blocks = invalid_blocks.sort_values('max_dist_cus', ascending=False)
    
    print(f"{'Slot':<12} {'Max Dist CUs':<15} {'Total CUs':<15} {'Excess CUs':<12}")
    print("-" * 80)
    
    for _, block in sorted_blocks.head(max_show).iterrows():
        excess = block['max_dist_cus'] - limit
        print(f"{block['slot']:<12.0f} {block['max_dist_cus']:<15,.0f} "
              f"{block['total_cus']:<15,.0f} {excess:<12,.0f}")
    
    if len(invalid_blocks) > max_show:
        print(f"... and {len(invalid_blocks) - max_show} more blocks")

def main():
    parser = argparse.ArgumentParser(description='Analyze block validity under CU limits')
    parser.add_argument('csv_file', help='CSV file from block-dag-analyzer (block summary data)')
    parser.add_argument('--limit', type=int, default=25_000_000,
                       help='CU limit to test (default: 25M)')
    parser.add_argument('--multiple-limits', action='store_true',
                       help='Test multiple common limits')
    parser.add_argument('--show-invalid', action='store_true',
                       help='Show details of invalid blocks')
    parser.add_argument('--max-show', type=int, default=10,
                       help='Maximum invalid blocks to show details for')
    
    args = parser.parse_args()
    
    # Perform main analysis
    result = analyze_block_limits(args.csv_file, args.limit)
    if result is None:
        return 1
        
    results, df, invalid_blocks = result
    
    # Print main results
    print_analysis(results)
    
    # Show invalid block details if requested
    if args.show_invalid:
        show_invalid_blocks(invalid_blocks, args.limit, args.max_show)
    
    # Multiple limits analysis if requested
    if args.multiple_limits:
        common_limits = [
            20_000_000,  # 20M
            25_000_000,  # 25M
            30_000_000,  # 30M
            35_000_000,  # 35M
            40_000_000,  # 40M
            45_000_000,  # 45M
            50_000_000,  # 50M
        ]
        analyze_multiple_limits(df, common_limits)
    
    # Print summary
    print(f"\n{'='*60}")
    print(f"SUMMARY: {results['invalid_percentage']:.2f}% of blocks would be INVALID")
    print(f"         with a {args.limit/1_000_000:.1f}M CU limit")
    print(f"{'='*60}")
    
    return 0

if __name__ == "__main__":
    exit(main()) 