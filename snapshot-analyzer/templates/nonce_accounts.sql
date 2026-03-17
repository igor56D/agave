-- Nonce Account Analysis
-- Counts and analyzes accounts owned by a specific program (typically used for nonce accounts)
-- Parameters:
--   owner: Program owner address to filter by (e.g., System Program or nonce program address)

SELECT 
    COUNT(*) as total_accounts,
    SUM(account_size) as total_bytes,
    ROUND(SUM(account_size) * 1.0 / 1000000000.0, 4) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(SUM(lamports) * 1.0 / 1000000000.0, 2) as total_sol,
    SUM(executable) as executable_accounts,
    ROUND(AVG(account_size * 1.0), 2) as avg_account_size,
    ROUND(AVG(lamports * 1.0), 2) as avg_lamports_per_account,
    MIN(account_size) as min_size,
    MAX(account_size) as max_size,
    MIN(lamports) as min_lamports,
    MAX(lamports) as max_lamports
FROM accounts 
WHERE owner = '{{owner}}';
