//! Reference consumer: a single-asset deposit vault that refuses deposits
//! while the asset is not covered according to the coverage ledger.
//! Withdrawals are never blocked.
#![no_std]

use plimsoll_types::{
    CoverageLedgerClient, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO,
    PERSISTENT_THRESHOLD,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, Env,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultConfig {
    pub ledger: Address,
    pub asset: Address,
    pub min_bps: u32,
    pub max_age: u64,
    pub min_tier: Tier,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Config,
    Balance(Address),
    Admin,
    WithdrawalDelay,
    PendingWithdrawal(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingWithdrawal {
    amount: i128,
    available_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VaultError {
    InvalidAmount = 1,
    NotCovered = 2,
    InsufficientBalance = 3,
    InvalidDelay = 4,
    NoPendingWithdrawal = 5,
    WithdrawalDelayNotElapsed = 6,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposited {
    #[topic]
    pub from: Address,
    pub amount: i128,
    pub balance_after: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdrawn {
    #[topic]
    pub to: Address,
    pub amount: i128,
    pub balance_after: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalQueued {
    #[topic]
    pub to: Address,
    pub amount: i128,
    pub available_at: u64,
}

/// The asset is not covered by the vault's config after a deposit that
/// succeeded, so a keeper watching this vault can restate reserves.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageDropped {
    #[topic]
    pub asset: Address,
}

#[contract]
pub struct CoveredVault;

fn config(env: &Env) -> VaultConfig {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .expect("config is set in the constructor")
}

/// The coverage check `deposit` makes, with the stored config.
fn is_open(env: &Env, cfg: &VaultConfig) -> bool {
    CoverageLedgerClient::new(env, &cfg.ledger).is_covered(
        &cfg.asset,
        &cfg.min_bps,
        &cfg.max_age,
        &cfg.min_tier,
    )
}

fn balance_of(env: &Env, who: &Address) -> i128 {
    let key = DataKey::Balance(who.clone());
    let balance = env.storage().persistent().get::<_, i128>(&key);
    if balance.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    balance.unwrap_or(0)
}

fn withdrawal_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::WithdrawalDelay)
        .unwrap_or(0)
}

fn pending_withdrawal(env: &Env, who: &Address) -> Option<PendingWithdrawal> {
    let key = DataKey::PendingWithdrawal(who.clone());
    let pending = env.storage().persistent().get(&key);
    if pending.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    pending
}

fn set_balance(env: &Env, who: &Address, amount: i128) {
    let key = DataKey::Balance(who.clone());
    if amount == 0 {
        env.storage().persistent().remove(&key);
        return;
    }
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
}

#[contractimpl]
impl CoveredVault {
    pub fn __constructor(
        env: Env,
        admin: Address,
        ledger: Address,
        asset: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalDelay, &0u64);
        let cfg = VaultConfig {
            ledger,
            asset,
            min_bps,
            max_age,
            min_tier,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
    }

    pub fn deposit(env: Env, from: Address, amount: i128) -> i128 {
        from.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, VaultError::InvalidAmount);
        }
        let cfg = config(&env);
        if !is_open(&env, &cfg) {
            panic_with_error!(&env, VaultError::NotCovered);
        }
        token::TokenClient::new(&env, &cfg.asset).transfer(
            &from,
            env.current_contract_address(),
            &amount,
        );
        let new_balance = balance_of(&env, &from)
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, VaultError::InvalidAmount));
        set_balance(&env, &from, new_balance);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        Deposited {
            from,
            amount,
            balance_after: new_balance,
        }
        .publish(&env);
        // Ask again now that the deposit is in the books. A deposit writes no
        // figure `is_covered` reads, so a ledger that behaves as SPEC.md says
        // answers as it did at the gate; this fires only when the answer moves
        // between the two reads, which is what a keeper needs to hear about.
        if !is_open(&env, &cfg) {
            CoverageDropped {
                asset: cfg.asset.clone(),
            }
            .publish(&env);
        }
        new_balance
    }

    pub fn request_withdraw(env: Env, to: Address, amount: i128) -> u64 {
        to.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, VaultError::InvalidAmount);
        }
        let current = balance_of(&env, &to);
        if amount > current {
            panic_with_error!(&env, VaultError::InsufficientBalance);
        }
        let available_at = match env.ledger().timestamp().checked_add(withdrawal_delay(&env)) {
            Some(available_at) => available_at,
            None => panic_with_error!(&env, VaultError::InvalidDelay),
        };
        let key = DataKey::PendingWithdrawal(to.clone());
        env.storage().persistent().set(
            &key,
            &PendingWithdrawal {
                amount,
                available_at,
            },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        WithdrawalQueued {
            to,
            amount,
            available_at,
        }
        .publish(&env);
        available_at
    }

    pub fn execute_withdraw(env: Env, to: Address) -> i128 {
        to.require_auth();
        let pending = match pending_withdrawal(&env, &to) {
            Some(pending) => pending,
            None => panic_with_error!(&env, VaultError::NoPendingWithdrawal),
        };
        if env.ledger().timestamp() < pending.available_at {
            panic_with_error!(&env, VaultError::WithdrawalDelayNotElapsed);
        }
        let current = balance_of(&env, &to);
        if pending.amount > current {
            panic_with_error!(&env, VaultError::InsufficientBalance);
        }
        let new_balance = current - pending.amount;
        set_balance(&env, &to, new_balance);
        env.storage()
            .persistent()
            .remove(&DataKey::PendingWithdrawal(to.clone()));
        let cfg = config(&env);
        token::TokenClient::new(&env, &cfg.asset).transfer(
            &env.current_contract_address(),
            &to,
            &pending.amount,
        );
        Withdrawn {
            to,
            amount: pending.amount,
            balance_after: new_balance,
        }
        .publish(&env);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        new_balance
    }

    pub fn set_withdrawal_delay(env: Env, delay_seconds: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin is set in the constructor");
        admin.require_auth();
        if env
            .ledger()
            .timestamp()
            .checked_add(delay_seconds)
            .is_none()
        {
            panic_with_error!(&env, VaultError::InvalidDelay);
        }
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalDelay, &delay_seconds);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
    }

    pub fn withdrawal_delay_seconds(env: Env) -> u64 {
        withdrawal_delay(&env)
    }

    pub fn balance(env: Env, of: Address) -> i128 {
        balance_of(&env, &of)
    }

    pub fn config(env: Env) -> VaultConfig {
        config(&env)
    }

    /// Whether a positive deposit would pass the coverage gate right now.
    pub fn can_deposit(env: Env) -> bool {
        is_open(&env, &config(&env))
    }
}

#[cfg(test)]
mod test;
