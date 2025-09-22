-- Debug Token Account Lamports
-- Investigate the lamport distribution for token accounts
-- Parameters: none

SELECT 
    'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA' as owner,
    COUNT(*) as total_accounts,
    MIN(lamports) as min_lamports,
    MAX(lamports) as max_lamports,
    AVG(lamports) as avg_lamports,
    SUM(lamports) as total_lamports,
    -- Show distribution
    SUM(CASE WHEN lamports < 1000000000 THEN 1 ELSE 0 END) as accounts_under_1_sol,
    SUM(CASE WHEN lamports >= 1000000000 AND lamports < 10000000000 THEN 1 ELSE 0 END) as accounts_1_to_10_sol,
    SUM(CASE WHEN lamports >= 10000000000 THEN 1 ELSE 0 END) as accounts_over_10_sol,
    -- Show some examples of high-balance accounts
    (SELECT GROUP_CONCAT(account || ':' || lamports, ', ') 
     FROM (SELECT account, lamports FROM accounts 
           WHERE owner = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA' 
           ORDER BY lamports DESC LIMIT 5)) as top_5_accounts
FROM accounts 
WHERE owner = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA';
