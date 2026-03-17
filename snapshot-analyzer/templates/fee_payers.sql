-- Fee Payer Analysis
-- Identifies potential fee payers: accounts owned by the System Program with 0 data size
-- A fee payer is a pre-funded native SOL account with no executable code

SELECT 
    COUNT(*) as total_fee_payers,
    SUM(lamports) as total_lamports,
    ROUND(SUM(lamports) * 1.0 / 1000000000.0, 2) as total_sol,
    ROUND(AVG(lamports * 1.0), 2) as avg_lamports_per_account,
    MIN(lamports) as min_lamports,
    MAX(lamports) as max_lamports,
    ROUND(SUM(lamports) * 1.0 / COUNT(*) / 1000000000.0, 6) as avg_sol_per_account
FROM accounts 
WHERE owner = '11111111111111111111111111111111'
  AND account_size = 0;
