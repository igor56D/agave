-- Nonce Account Analysis
-- Identifies nonce accounts: accounts owned by the System Program with exactly 80 bytes of data
-- Nonce accounts are state accounts used for durable transaction nonces

SELECT 
    COUNT(*) as total_nonce_accounts,
    SUM(account_size) as total_bytes,
    ROUND(SUM(account_size) * 1.0 / 1000000000.0, 4) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(SUM(lamports) * 1.0 / 1000000000.0, 2) as total_sol,
    ROUND(AVG(lamports * 1.0), 2) as avg_lamports_per_account,
    MIN(lamports) as min_lamports,
    MAX(lamports) as max_lamports
FROM accounts 
WHERE owner = '11111111111111111111111111111111'
  AND account_size = 80;
