-- Index-Only Low Activity Count
-- Counts how many index-only accounts were accessed less than or equal to n times
-- Parameters:
--   n: Threshold for total accesses (reads + writes)

SELECT
    COUNT(*) AS accounts_with_<=_n_accesses
FROM index_only_accounts
WHERE total_activity_count <= {{n}};


