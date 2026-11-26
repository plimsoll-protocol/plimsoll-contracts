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
    PendingAdmin,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VaultError {
    InvalidAmount = 1,
    NotCovered = 2,
    InsufficientBalance = 3,
    NoPendingAdmin = 4,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposited {
    #[topic]
    pub from: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdrawn {
    #[topic]
    pub to: Address,
    pub amount: i128,
}

/// The asset is not covered by the vault's config after a deposit that
/// succeeded, so a keeper watching this vault can restate reserves.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageDropped {
    #[topic]
    pub asset: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultAdminProposed {
    #[topic]
    pub new_admin: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultAdminTransferred {
    #[topic]
    pub old_admin: Address,
    #[topic]
    pub new_admin: Address,
}

#[contract]
pub struct CoveredVault;

fn admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("admin is set in the constructor")
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
}

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
        ledger: Address,
        asset: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
        admin: Address,
    ) {
        let cfg = VaultConfig {
            ledger,
            asset,
            min_bps,
            max_age,
            min_tier,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.storage().instance().set(&DataKey::Admin, &admin);
        bump_instance(&env);
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
        Deposited { from, amount }.publish(&env);
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

    pub fn withdraw(env: Env, to: Address, amount: i128) -> i128 {
        to.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, VaultError::InvalidAmount);
        }
        let current = balance_of(&env, &to);
        if amount > current {
            panic_with_error!(&env, VaultError::InsufficientBalance);
        }
        let new_balance = current - amount;
        set_balance(&env, &to, new_balance);
        let cfg = config(&env);
        token::TokenClient::new(&env, &cfg.asset).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );
        Withdrawn { to, amount }.publish(&env);
        new_balance
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

    pub fn admin(env: Env) -> Address {
        admin(&env)
    }

    pub fn pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Propose a new admin for the vault's two-step admin transfer.
    pub fn propose_admin(env: Env, new_admin: Address) {
        admin(&env).require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        bump_instance(&env);
        VaultAdminProposed { new_admin }.publish(&env);
    }

    /// Accept a pending admin proposal, transferring admin control.
    pub fn accept_admin(env: Env) {
        let new_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .unwrap_or_else(|| panic_with_error!(&env, VaultError::NoPendingAdmin));
        new_admin.require_auth();
        let old_admin = admin(&env);
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        bump_instance(&env);
        VaultAdminTransferred {
            old_admin,
            new_admin,
        }
        .publish(&env);
    }
}

#[cfg(test)]
mod test;
