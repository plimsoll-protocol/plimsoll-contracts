//! A contract that accepts an asset only while Plimsoll says it is covered.
//!
//! This is the example in docs/INTEGRATING.md, kept as a crate so the test
//! suite compiles it and checks every edge case the guide describes. A test
//! also fails if the guide's copy of this file drifts from it.
#![no_std]

use plimsoll_types::{CoverageLedgerClient, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env,
};

/// What "covered" has to mean before this contract takes the asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    /// The coverage-ledger contract to ask.
    pub ledger: Address,
    /// The asset's Stellar Asset Contract address (as listed in the ledger).
    pub asset: Address,
    /// Least acceptable reserves / supply, in basis points: 10_000 = 100%.
    pub min_bps: u32,
    /// Oldest acceptable supply snapshot and reserve report, in seconds.
    pub max_age: u64,
    /// Weakest acceptable signer of the reserve report.
    pub min_tier: Tier,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Policy,
    Total,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GateError {
    InvalidAmount = 1,
    NotCovered = 2,
}

#[contract]
pub struct CoverageGate;

fn policy(env: &Env) -> Policy {
    env.storage()
        .instance()
        .get(&DataKey::Policy)
        .expect("policy is set in the constructor")
}

/// One cross-contract call. `is_covered` never fails: every reason the asset
/// might not qualify (no figures, stale figures, too weak a signer, not
/// enough reserves) comes back as `false`.
fn covered(env: &Env, p: &Policy) -> bool {
    CoverageLedgerClient::new(env, &p.ledger).is_covered(
        &p.asset,
        &p.min_bps,
        &p.max_age,
        &p.min_tier,
    )
}

#[contractimpl]
impl CoverageGate {
    pub fn __constructor(
        env: Env,
        ledger: Address,
        asset: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) {
        let p = Policy {
            ledger,
            asset,
            min_bps,
            max_age,
            min_tier,
        };
        env.storage().instance().set(&DataKey::Policy, &p);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
    }

    /// Record `amount` of the asset from `from`, refusing while it is not
    /// covered. A real contract would move the tokens here too.
    pub fn accept(env: Env, from: Address, amount: i128) -> i128 {
        from.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, GateError::InvalidAmount);
        }
        if !covered(&env, &policy(&env)) {
            panic_with_error!(&env, GateError::NotCovered);
        }
        let total: i128 = env.storage().instance().get(&DataKey::Total).unwrap_or(0);
        let total = total
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, GateError::InvalidAmount));
        env.storage().instance().set(&DataKey::Total, &total);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        total
    }

    /// Whether `accept` would pass the coverage check right now, so a front
    /// end can say so without simulating a call that fails.
    pub fn is_open(env: Env) -> bool {
        covered(&env, &policy(&env))
    }
}

#[cfg(test)]
mod test;
