-- Read/Write Ratio Analysis
-- Shows accounts ordered by their ratio of reads to writes
-- Parameters:
--   limit: Number of accounts to return (default: 20)
--   min_activity: Minimum total activity count to include account (default: 10)

SELECT 
    account,
    account_size,
    lamports,
    owner,
    CASE WHEN executable = 1 THEN 'true' ELSE 'false' END as executable,
    read_count,
    write_count,
    total_activity_count,
    ROUND(read_count * 1.0 / MAX(write_count, 1), 2) as read_write_ratio,
    max_read_epoch,
    max_write_epoch
FROM accounts 
WHERE total_activity_count >= {{min_activity}}
ORDER BY read_write_ratio DESC
LIMIT {{limit}};
