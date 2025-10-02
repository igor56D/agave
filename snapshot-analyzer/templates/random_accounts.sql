-- Random Accounts (Standard DB)
-- Returns n random accounts from the `accounts` table
-- Parameters:
--   n: Number of accounts to return (default set in CLI)

SELECT
    account,
    account_size,
    lamports,
    owner,
    CASE WHEN executable = 1 THEN 'true' ELSE 'false' END as executable,
    read_count,
    write_count,
    total_activity_count,
    max_read_epoch,
    max_write_epoch
FROM accounts
ORDER BY RANDOM()
LIMIT {{n}};


