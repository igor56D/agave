-- Index-Only Account Staleness Analysis
-- Shows staleness analysis for accounts that exist in the activity index but not in the snapshot
-- These are accounts that had activity but are no longer present in the current snapshot
-- Parameters:
--   current_epoch: Current epoch number
--   lookback_epochs: Number of epochs to look back for activity

SELECT
    COUNT(*) as index_only_account_count,
    
    -- Activity-based metrics
    SUM(read_count) as total_reads,
    SUM(write_count) as total_writes,
    SUM(total_activity_count) as total_activity,
    
    -- Average activity per account
    ROUND(AVG(read_count * 1.0), 2) as avg_reads_per_account,
    ROUND(AVG(write_count * 1.0), 2) as avg_writes_per_account,
    ROUND(AVG(total_activity_count * 1.0), 2) as avg_total_activity_per_account,
    
    -- Staleness metrics (accounts with recent activity)
    SUM(CASE WHEN max_read_epoch >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) as recently_read_accounts,
    SUM(CASE WHEN max_write_epoch >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) as recently_written_accounts,
    SUM(CASE WHEN MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) as recently_active_accounts,
    
    -- Staleness percentages
    ROUND(
        SUM(CASE WHEN max_read_epoch >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) * 100.0 / COUNT(*), 2
    ) as recently_read_percentage,
    ROUND(
        SUM(CASE WHEN max_write_epoch >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) * 100.0 / COUNT(*), 2
    ) as recently_written_percentage,
    ROUND(
        SUM(CASE WHEN MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - {{lookback_epochs}} THEN 1 ELSE 0 END) * 100.0 / COUNT(*), 2
    ) as recently_active_percentage,
    
    -- Activity distribution
    MIN(max_read_epoch) as oldest_read_epoch,
    MAX(max_read_epoch) as newest_read_epoch,
    MIN(max_write_epoch) as oldest_write_epoch,
    MAX(max_write_epoch) as newest_write_epoch,
    MIN(MAX(max_read_epoch, max_write_epoch)) as oldest_activity_epoch,
    MAX(MAX(max_read_epoch, max_write_epoch)) as newest_activity_epoch

FROM index_only_accounts;
