//! Reference consumer: a single-asset deposit vault that refuses deposits
//! while the asset is not covered according to the coverage ledger. Large
//! withdrawals require a secondary approver, regardless of coverage status.
#![no_std]

use plimsoll_types::{
    CoverageLedgerClient, Tier, WithdrawalRequest, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    CoverageLedgerClient, CoverageStatus, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, BytesN, Env, String, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultConfig {
    pub admin: Address,
    pub ledger: Address,
    pub asset: Address,
    pub min_bps: u32,
    pub max_age: u64,
    pub min_tier: Tier,
    pub approver: Address,
    pub large_withdrawal_threshold: i128,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Config,
    Balance(Address),
    WithdrawalQueue,
    Admin,
    PendingAdmin,
    PendingAdmin,
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
    InvalidThreshold = 4,
    NoPendingAdmin = 4,
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

    pub new_admin: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdsUpdated {
    pub old_min_bps: u32,
    pub old_max_age: u64,
    pub old_min_tier: Tier,
    pub new_min_bps: u32,
    pub new_max_age: u64,
    pub new_min_tier: Tier,
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
        .unwrap_or_else(|| unreachable!("config is set in the constructor"))
}

fn read_withdrawal_queue(env: &Env) -> Vec<WithdrawalRequest> {
    env.storage()
        .persistent()
        .get::<_, Vec<WithdrawalRequest>>(&DataKey::WithdrawalQueue)
        .unwrap_or_else(|| Vec::new(env))
}

fn write_withdrawal_queue(env: &Env, queue: &Vec<WithdrawalRequest>) {
    env.storage()
        .persistent()
        .set(&DataKey::WithdrawalQueue, queue);
    env.storage().persistent().extend_ttl(
        &DataKey::WithdrawalQueue,
        PERSISTENT_THRESHOLD,
        PERSISTENT_EXTEND_TO,
    );
}

/// Admin-gated: add a pending delayed withdrawal request.
fn add_withdrawal_request(env: &Env, requester: Address, amount: i128, unlock_ledger: u32) {
    let mut queue = read_withdrawal_queue(env);
    queue.push_back(WithdrawalRequest {
        requester,
        amount,
        unlock_ledger,
    });
    write_withdrawal_queue(env, &queue);
}

/// Return all pending withdrawal requests (paginated via cursor/limit).
fn get_withdrawal_queue(env: &Env, cursor: Option<u32>, limit: u32) -> Vec<(Address, i128, u32)> {
    let queue = read_withdrawal_queue(env);
    let start = cursor.unwrap_or(0);
    let page = limit.min(50u32); // max 50 per call
    let mut out: Vec<(Address, i128, u32)> = Vec::new(env);
    for (i, req) in queue.iter().enumerate() {
        if (i as u32) < start {
            continue;
        }
        if out.len() >= page {
            break;
        }
        out.push_back((req.requester.clone(), req.amount, req.unlock_ledger));
    }
    out
}

fn pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

/// The coverage check `deposit` makes, with the stored config.
fn is_open(env: &Env, cfg: &VaultConfig) -> bool {
    CoverageLedgerClient::new(env, &cfg.ledger).is_covered(
        &cfg.asset,
        &cfg.min_bps,
        &cfg.max_age,
        &cfg.min_tier,
    ) == CoverageStatus::Covered
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
        approver: Address,
        large_withdrawal_threshold: i128,
    ) {
        if large_withdrawal_threshold < 0 {
            panic_with_error!(&env, VaultError::InvalidThreshold);
        }
        admin: Address,
    ) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalDelay, &0u64);
        let cfg = VaultConfig {
            admin,
            ledger,
            asset,
            min_bps,
            max_age,
            min_tier,
            approver,
            large_withdrawal_threshold,
        };
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.storage().instance().set(&DataKey::Admin, &admin);
        bump_instance(&env);
    }

    pub fn admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin is set in the constructor")
    }

    /// Replace `min_bps`, `max_age`, and `min_tier` atomically. Only the
    /// stored admin may call this.
    pub fn update_thresholds(env: Env, min_bps: u32, max_age: u64, min_tier: Tier) {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin is set in the constructor");
        stored_admin.require_auth();
        let mut cfg = config(&env);
        let old_min_bps = cfg.min_bps;
        let old_max_age = cfg.max_age;
        let old_min_tier = cfg.min_tier.clone();
        cfg.min_bps = min_bps;
        cfg.max_age = max_age;
        cfg.min_tier = min_tier.clone();
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        ThresholdsUpdated {
            old_min_bps,
            old_max_age,
            old_min_tier,
            new_min_bps: min_bps,
            new_max_age: max_age,
            new_min_tier: min_tier,
        }
        .publish(&env);
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
        let cfg = config(&env);
        let current = balance_of(&env, &to);
        if amount > current {
            panic_with_error!(&env, VaultError::InsufficientBalance);
        }
        if amount > cfg.large_withdrawal_threshold {
            cfg.approver.require_auth();
        }
        let new_balance = current - amount;
        set_balance(&env, &to, new_balance);
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

    /// The vault's actual on-chain token balance, read straight from the
    /// asset's SAC rather than the internally tracked per-depositor sum.
    /// A pure read: touches no vault storage, so no TTL is extended.
    pub fn get_balance(env: Env) -> i128 {
        let cfg = config(&env);
        token::TokenClient::new(&env, &cfg.asset).balance(&env.current_contract_address())
    }

    pub fn config(env: Env) -> VaultConfig {
        config(&env)
    }

    /// Whether a positive deposit would pass the coverage gate right now.
    pub fn can_deposit(env: Env) -> bool {
        is_open(&env, &config(&env))
    }

    /// Add a pending delayed withdrawal request.
    /// The caller must authorise this action.
    pub fn add_withdrawal_request(env: Env, requester: Address, amount: i128, unlock_ledger: u32) {
        requester.require_auth();
        add_withdrawal_request(&env, requester, amount, unlock_ledger);
    }

    /// Return all pending withdrawal requests (paginated via cursor/limit).
    /// Returns empty vec when no requests pending.
    pub fn get_withdrawal_queue(
        env: Env,
        cursor: Option<u32>,
        limit: u32,
    ) -> Vec<(Address, i128, u32)> {
        get_withdrawal_queue(&env, cursor, limit)
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
        config(&env).admin
    }

    /// Propose a new vault admin. A new proposal replaces any pending one.
    pub fn propose_vault_admin(env: Env, new_admin: Address) {
        config(&env).admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        VaultAdminProposed { new_admin }.publish(&env);
    }

    /// Accept a pending admin proposal. Must be called by the proposed admin.
    pub fn accept_vault_admin(env: Env) {
        let new_admin = match pending_admin(&env) {
            Some(new_admin) => new_admin,
            None => panic_with_error!(&env, VaultError::NoPendingAdmin),
        };
        new_admin.require_auth();
        let mut cfg = config(&env);
        cfg.admin = new_admin.clone();
        env.storage().instance().set(&DataKey::Config, &cfg);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        VaultAdminTransferred { new_admin }.publish(&env);
    }

    /// The pending admin proposal, if any.
    pub fn pending_vault_admin(env: Env) -> Option<Address> {
        pending_admin(&env)
    }
}

#[cfg(test)]
mod test;
