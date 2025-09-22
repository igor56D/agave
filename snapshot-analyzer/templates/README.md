# SQL Templates

This directory contains SQL template files that can be used with the `run-query` subcommand.

## Available Templates

### staleness.sql
Analyzes accounts that have been accessed within the last N epochs.

**Parameters:**
- `current_epoch` (required): Current epoch number
- `lookback_epochs` (required): Number of epochs to look back

**Example usage:**
```bash
agave-snapshot-analyzer run-query -d database.db -t staleness -p current_epoch=600 -p lookback_epochs=50
```

### most_active.sql
Shows the accounts with the highest activity counts.

**Parameters:**
- `limit` (required): Number of top accounts to return

**Example usage:**
```bash
agave-snapshot-analyzer run-query -d database.db -t most_active -p limit=20
```

### owner_analysis.sql
Shows statistics grouped by account owner.

**Parameters:**
- `min_accounts` (required): Minimum number of accounts an owner must have to be included

**Example usage:**
```bash
agave-snapshot-analyzer run-query -d database.db -t owner_analysis -p min_accounts=5
```

### size_distribution.sql
Shows distribution of accounts by size ranges.

**Parameters:**
None required.

**Example usage:**
```bash
agave-snapshot-analyzer run-query -d database.db -t size_distribution
```

## Template Format

Templates use `{{parameter_name}}` syntax for parameter substitution. Parameters are provided via `-p key=value` command line arguments.

## Creating Custom Templates

You can create your own SQL templates by:
1. Creating a `.sql` file in this directory
2. Using `{{parameter_name}}` placeholders for dynamic values
3. Adding comments to document required parameters

Example template:
```sql
-- My Custom Query
-- Parameters:
--   min_balance: Minimum account balance in lamports

SELECT account, lamports, owner 
FROM accounts 
WHERE lamports >= {{min_balance}}
ORDER BY lamports DESC;
```
