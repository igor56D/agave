use {
    crate::account_loader::LoadedTransaction,
    solana_message::inner_instruction::InnerInstructionsList,
    solana_program_runtime::loaded_programs::ProgramCacheEntry,
    solana_pubkey::Pubkey,
    solana_transaction_context::TransactionReturnData,
    solana_transaction_error::TransactionResult,
    std::{collections::HashMap, sync::Arc},
};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TransactionLoadedAccountsStats {
    pub loaded_accounts_data_size: u32,
    pub loaded_accounts_count: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TransactionExecutionTimings {
    pub validate_fees_us: u64,
    pub load_us: u64,
    pub execute_us: u64,
    pub collect_balances_us: u64,
    pub filter_executable_us: u64,
    pub program_cache_us: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutedTransaction {
    pub loaded_transaction: LoadedTransaction,
    pub execution_details: TransactionExecutionDetails,
    pub programs_modified_by_tx: HashMap<Pubkey, Arc<ProgramCacheEntry>>,
}

impl ExecutedTransaction {
    pub fn was_successful(&self) -> bool {
        self.execution_details.was_successful()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionExecutionDetails {
    pub status: TransactionResult<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    pub execution_time_us: u64,
    pub execution_timings: TransactionExecutionTimings,
    /// The change in accounts data len for this transaction.
    /// NOTE: This value is valid IFF `status` is `Ok`.
    pub accounts_data_len_delta: i64,
}

impl TransactionExecutionDetails {
    pub fn was_successful(&self) -> bool {
        self.status.is_ok()
    }
}
