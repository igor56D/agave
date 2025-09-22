-- Balance Range Analysis
-- Shows accounts within a specific lamport balance range
-- Parameters:
--   min_lamports: Minimum balance in lamports (default: 0)
--   max_lamports: Maximum balance in lamports (default: no limit)

SELECT 
    account,
    lamports,
    ROUND(CAST(lamports AS REAL) / 1000000000.0, 4) as sol_balance,
    owner,
    CASE WHEN executable = 1 THEN 'true' ELSE 'false' END as executable,
    account_size,
    total_activity_count
FROM accounts 
WHERE lamports >= {{min_lamports}}
  AND ({{max_lamports}} IS NULL OR lamports <= {{max_lamports}})
ORDER BY lamports DESC;
