-- Account Size Distribution Analysis
-- Shows distribution of accounts by size ranges
-- No parameters required

SELECT 
    CASE 
        WHEN account_size = 0 THEN '0 bytes'
        WHEN account_size <= 100 THEN '1-100 bytes'
        WHEN account_size <= 1000 THEN '101-1K bytes'
        WHEN account_size <= 10000 THEN '1K-10K bytes'
        WHEN account_size <= 100000 THEN '10K-100K bytes'
        WHEN account_size <= 1000000 THEN '100K-1M bytes'
        ELSE '1M+ bytes'
    END as size_range,
    COUNT(*) as account_count,
    SUM(account_size) as total_bytes,
    ROUND(CAST(SUM(account_size) AS REAL) / 1000000000.0, 2) as total_gb,
    SUM(lamports) as total_lamports,
    ROUND(CAST(SUM(lamports) AS REAL) / 1000000000.0, 2) as total_sol,
    SUM(executable) as executable_accounts
FROM accounts 
GROUP BY 
    CASE 
        WHEN account_size = 0 THEN '0 bytes'
        WHEN account_size <= 100 THEN '1-100 bytes'
        WHEN account_size <= 1000 THEN '101-1K bytes'
        WHEN account_size <= 10000 THEN '1K-10K bytes'
        WHEN account_size <= 100000 THEN '10K-100K bytes'
        WHEN account_size <= 1000000 THEN '100K-1M bytes'
        ELSE '1M+ bytes'
    END
ORDER BY MIN(account_size);
