use {
    log::*,
    solana_account::ReadableAccount,
    solana_pubkey::Pubkey,
    solana_runtime::{
        genesis_utils::create_genesis_config,
        runtime_config::RuntimeConfig,
        snapshot_archive_info::{FullSnapshotArchiveInfo, SnapshotArchiveInfoGetter},
        snapshot_bank_utils::bank_from_snapshot_archives,
    },
    std::{collections::HashMap, path::PathBuf, sync::{atomic::AtomicBool, Arc}},
};


pub struct SnapshotParser {
    pub snapshot_path: PathBuf,
    pub temp_dir: Option<tempfile::TempDir>,
}

impl SnapshotParser {
    pub fn new(snapshot_path: impl Into<PathBuf>) -> Self {
        Self {
            snapshot_path: snapshot_path.into(),
            temp_dir: None,
        }
    }

    pub fn parse_accounts(&mut self, activity_map: &HashMap<Pubkey, crate::AccountActivity>) -> Result<(HashMap<Pubkey, u64>, usize, u64), Box<dyn std::error::Error>> {
        info!("Loading Bank from snapshot: {}", self.snapshot_path.display());
        
        // Parse the snapshot archive info
        let archive_info = FullSnapshotArchiveInfo::new_from_path(self.snapshot_path.clone())?;
        info!("Found snapshot with slot: {}", archive_info.slot());

        // Create temporary directory for unpacking
        let temp_dir = tempfile::TempDir::new()?;
        let bank_snapshots_dir = temp_dir.path().join("bank_snapshots");
        std::fs::create_dir_all(&bank_snapshots_dir)?;

        // Create account paths
        let account_paths = vec![temp_dir.path().join("accounts")];
        for path in &account_paths {
            std::fs::create_dir_all(path)?;
        }

        // Create a minimal genesis config
        let genesis_config = create_genesis_config(10_000).genesis_config;
        let runtime_config = RuntimeConfig::default();
        let exit = Arc::new(AtomicBool::new(false));

        info!("Reconstructing Bank from snapshot...");
        
        // Reconstruct the bank from the snapshot
        let (bank, _timings) = bank_from_snapshot_archives(
            &account_paths,
            &bank_snapshots_dir,
            &archive_info,
            None, // No incremental snapshot
            &genesis_config,
            &runtime_config,
            None, // debug_keys
            None, // additional_builtins
            None, // limit_load_slot_count_from_snapshot
            false, // test_hash_calculation
            false, // accounts_db_skip_shrink
            false, // accounts_db_force_initial_clean
            true,  // verify_index
            None,  // accounts_db_config
            None,  // accounts_update_notifier
            exit,
        )?;

        info!("Bank reconstructed, getting aggregate stats efficiently...");

        // Get total counts efficiently using Bank's built-in stats
        let total_stats = bank.get_total_accounts_stats()?;
        let total_accounts = total_stats.num_accounts;
        let total_bytes = total_stats.data_len as u64;

        info!("Total accounts: {}, total bytes: {}", total_accounts, total_bytes);
        info!("Querying only tracked accounts...");

        // Only query the accounts we care about
        let mut tracked_accounts = HashMap::new();
        let mut tracked_bytes = 0u64;

        for pubkey in activity_map.keys() {
            if let Some(account) = bank.get_account(pubkey) {
                let data_len = account.data().len() as u64;
                tracked_accounts.insert(*pubkey, data_len);
                tracked_bytes += data_len;
            }
        }

        // Calculate untracked stats by subtraction
        let untracked_count = total_accounts - tracked_accounts.len();
        let untracked_bytes = total_bytes - tracked_bytes;

        // Store temp_dir to keep it alive
        self.temp_dir = Some(temp_dir);

        info!(
            "Parsed {} tracked accounts ({} bytes), {} untracked accounts ({} bytes)", 
            tracked_accounts.len(), 
            tracked_bytes,
            untracked_count,
            untracked_bytes
        );
        
        Ok((tracked_accounts, untracked_count, untracked_bytes))
    }

} 