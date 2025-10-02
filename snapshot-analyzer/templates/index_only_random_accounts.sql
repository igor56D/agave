-- Random Accounts (Index-Only DB)
-- Returns n random accounts from the `index_only_accounts` table
-- Parameters:
--   n: Number of accounts to return (default set in CLI)

SELECT
    account,
    read_count,
    write_count,
    total_activity_count,
    max_read_epoch,
    max_write_epoch
FROM index_only_accounts
ORDER BY RANDOM()
LIMIT {{n}};


