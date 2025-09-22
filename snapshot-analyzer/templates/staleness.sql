-- Account Staleness Analysis
-- Shows accounts that have been accessed within the last N epochs
-- Parameters:
--   current_epoch: Current epoch number
--   lookback_epochs: Number of epochs to look back

SELECT 
    COUNT(*) as account_count,
    SUM(account_size) as total_bytes,
    ROUND(CAST(SUM(account_size) AS REAL) / 1000000000.0, 2) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(CAST(SUM(lamports) AS REAL) / 1000000000.0, 2) as total_sol,
    SUM(executable) as executable_accounts,
    AVG(account_size) as avg_account_size,
    AVG(lamports) as avg_lamports
FROM accounts 
WHERE MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - {{lookback_epochs}};
