-- Account Staleness Analysis
-- Shows accounts that have been accessed within the last N epochs
-- Parameters:
--   current_epoch: Current epoch number
--   lookback_epochs: Number of epochs to look back

SELECT 
    COUNT(*) as account_count,
    SUM(account_size) as total_bytes,
    ROUND(SUM(account_size) * 1.0 / 1000000.0, 2) as total_mb,
    ROUND(SUM(account_size) * 1.0 / 1000000000.0, 4) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(SUM(lamports) * 1.0 / 1000000000.0, 2) as total_sol,
    SUM(executable) as executable_accounts,
    ROUND(AVG(account_size * 1.0), 0) as avg_account_size,
    ROUND(AVG(lamports * 1.0), 0) as avg_lamports
FROM accounts 
WHERE MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - {{lookback_epochs}};
