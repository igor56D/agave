-- Most Active Accounts Analysis
-- Shows the accounts with the highest activity counts
-- Parameters:
--   limit: Number of top accounts to return (default: 20)

SELECT 
    account,
    account_size,
    lamports,
    owner,
    CASE WHEN executable = 1 THEN 'true' ELSE 'false' END as executable,
    total_activity_count,
    read_count,
    write_count,
    max_read_epoch,
    max_write_epoch
FROM accounts 
ORDER BY total_activity_count DESC 
LIMIT {{limit}};
