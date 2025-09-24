-- Index-Only Account Staleness Breakdown
-- Detailed breakdown of staleness for index-only accounts by activity level
-- Parameters:
--   current_epoch: Current epoch number
--   lookback_epochs: Number of epochs to look back for activity

WITH staleness_categories AS (
    SELECT 
        account,
        total_activity_count,
        max_read_epoch,
        max_write_epoch,
        MAX(max_read_epoch, max_write_epoch) as max_activity_epoch,
        {{current_epoch}} - MAX(max_read_epoch, max_write_epoch) as epochs_since_activity,
        
        -- Categorize by activity level
        CASE 
            WHEN total_activity_count >= 1000 THEN 'Very High (1000+)'
            WHEN total_activity_count >= 100 THEN 'High (100-999)'
            WHEN total_activity_count >= 10 THEN 'Medium (10-99)'
            WHEN total_activity_count >= 1 THEN 'Low (1-9)'
            ELSE 'None (0)'
        END as activity_level,
        
        -- Categorize by staleness
        CASE 
            WHEN MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - {{lookback_epochs}} THEN 'Recent'
            WHEN MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - ({{lookback_epochs}} * 2) THEN 'Moderately Stale'
            WHEN MAX(max_read_epoch, max_write_epoch) >= {{current_epoch}} - ({{lookback_epochs}} * 5) THEN 'Very Stale'
            ELSE 'Ancient'
        END as staleness_category
        
    FROM index_only_accounts
)

SELECT 
    activity_level,
    staleness_category,
    COUNT(*) as account_count,
    ROUND(COUNT(*) * 100.0 / SUM(COUNT(*)) OVER (), 2) as percentage_of_total,
    
    -- Activity metrics for this group
    SUM(total_activity_count) as total_activity,
    ROUND(AVG(total_activity_count * 1.0), 2) as avg_activity_per_account,
    
    -- Staleness metrics
    ROUND(AVG(epochs_since_activity * 1.0), 1) as avg_epochs_since_activity,
    MIN(epochs_since_activity) as min_epochs_since_activity,
    MAX(epochs_since_activity) as max_epochs_since_activity

FROM staleness_categories
GROUP BY activity_level, staleness_category
ORDER BY 
    CASE activity_level
        WHEN 'Very High (1000+)' THEN 1
        WHEN 'High (100-999)' THEN 2
        WHEN 'Medium (10-99)' THEN 3
        WHEN 'Low (1-9)' THEN 4
        ELSE 5
    END,
    CASE staleness_category
        WHEN 'Recent' THEN 1
        WHEN 'Moderately Stale' THEN 2
        WHEN 'Very Stale' THEN 3
        ELSE 4
    END;
