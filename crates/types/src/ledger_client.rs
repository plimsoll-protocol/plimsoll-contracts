use super::{CoverageLedgerClient, Tier};
use soroban_sdk::{Address, Env};

/// Ask a coverage-ledger whether an asset meets the supplied coverage policy.
pub fn is_covered(
    env: &Env,
    ledger: &Address,
    asset: &Address,
    min_bps: u32,
    max_age: u64,
    min_tier: Tier,
) -> bool {
    CoverageLedgerClient::new(env, ledger).is_covered(asset, &min_bps, &max_age, &min_tier)
}
