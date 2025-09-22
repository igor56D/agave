-- Account Owner Analysis
-- Shows statistics grouped by account owner
-- Parameters:
--   min_accounts: Minimum number of accounts an owner must have to be included (default: 1)

SELECT 
    owner,
    COUNT(*) as account_count,
    SUM(account_size) as total_bytes,
    ROUND(SUM(account_size) * 1.0 / 1000000.0, 2) as total_mb,
    ROUND(SUM(account_size) * 1.0 / 1000000000.0, 4) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(SUM(lamports) * 1.0 / 1000000000.0, 2) as total_sol,
    SUM(executable) as executable_accounts,
    SUM(total_activity_count) as total_activity,
    ROUND(AVG(account_size * 1.0), 0) as avg_account_size,
    ROUND(AVG(lamports * 1.0), 0) as avg_lamports
FROM accounts 
GROUP BY owner
HAVING COUNT(*) >= {{min_accounts}}
ORDER BY total_lamports DESC;
