//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingUpgrade, ReporterInfo, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contractmeta, contracttype,
    panic_with_error, Address, BytesN, ContractExecutable, Env, String, Vec,
};

const MAX_NAME_LEN: u32 = 64;
/// Maximum number of reporters returned in a single `reporters` call.
const MAX_REPORTERS_PAGE_LIMIT: u32 = 100;

/// Contract version. Increment on every behavioural change.
const VERSION: u32 = 1;

// Contract metadata for `stellar contract info meta`
contractmeta!(key = "description", val = "Plimsoll Reporter Registry");
contractmeta!(
    key = "repository",
    val = "https://github.com/plimsoll-protocol/plimsoll-contracts"
);

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Reporter(Address),
    ReporterIndex,
    PendingUpgrade,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryError {
    ReporterNotFound = 1,
    InvalidName = 2,
    UpgradeNotProposed = 3,
    UpgradeDelayNotElapsed = 4,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterSet {
    #[topic]
    pub reporter: Address,
    pub role: Role,
    pub name: String,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterRevoked {
    #[topic]
    pub reporter: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferred {
    #[topic]
    pub new_admin: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposed {
    #[topic]
    pub wasm_hash: BytesN<32>,
    pub proposed_at_ledger: u32,
    pub proposed_by: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeExecuted {
    #[topic]
    pub wasm_hash: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeCancelled;

#[contract]
pub struct ReporterRegistry;

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

fn read_index(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get::<_, Vec<Address>>(&DataKey::ReporterIndex)
        .unwrap_or_else(|| Vec::new(env))
}

fn write_index(env: &Env, index: &Vec<Address>) {
    env.storage()
        .persistent()
        .set(&DataKey::ReporterIndex, index);
    env.storage().persistent().extend_ttl(
        &DataKey::ReporterIndex,
        PERSISTENT_THRESHOLD,
        PERSISTENT_EXTEND_TO,
    );
}

fn ensure_index_backfilled(env: &Env) {
    let key = DataKey::ReporterIndex;
    if env.storage().persistent().has(&key) {
        return;
    }
    // Backfill: iterate all Reporter keys and build the index.
    // Note: soroban-sdk doesn't provide a direct way to iterate all keys with a prefix.
    // For now, the index starts empty and is built up from new reporters.
    // A full backfill would require a migration script or iterating known reporters.
    write_index(env, &Vec::new(env));
}

fn add_reporter_to_index(env: &Env, reporter: &Address) {
    let mut index = read_index(env);
    // Only add if not already present
    if !index.iter().any(|a| a == *reporter) {
        index.push_back(reporter.clone());
        write_index(env, &index);
    }
}

fn remove_reporter_from_index(env: &Env, reporter: &Address) {
    let mut index = read_index(env);
    let len = index.len();
    if let Some(pos) = index.iter().position(|a| a == *reporter) {
        // Swap-remove: move last element to the removed position
        let last = len - 1;
        let pos_u32: u32 = pos.try_into().unwrap();
        if pos_u32 != last {
            index.set(pos_u32, index.get(last).unwrap());
        }
        index.pop_back();
        write_index(env, &index);
    }
}

#[contractimpl]
impl ReporterRegistry {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        bump_instance(&env);
    }

    pub fn admin(env: Env) -> Address {
        admin(&env)
    }

    /// Add a reporter, or change an existing reporter's role and name.
    pub fn set_reporter(env: Env, reporter: Address, role: Role, name: String) {
        admin(&env).require_auth();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            panic_with_error!(&env, RegistryError::InvalidName);
        }
        let key = DataKey::Reporter(reporter.clone());
        let is_new = !env.storage().persistent().has(&key);
        let added_at = env
            .storage()
            .persistent()
            .get::<_, ReporterInfo>(&key)
            .map(|existing| existing.added_at)
            .unwrap_or_else(|| env.ledger().timestamp());
        let info = ReporterInfo {
            role,
            name: name.clone(),
            added_at,
        };
        env.storage().persistent().set(&key, &info);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        if is_new {
            add_reporter_to_index(&env, &reporter);
        }
        bump_instance(&env);
        ReporterSet {
            reporter,
            role,
            name,
        }
        .publish(&env);
    }

    pub fn revoke(env: Env, reporter: Address) {
        admin(&env).require_auth();
        let key = DataKey::Reporter(reporter.clone());
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        env.storage().persistent().remove(&key);
        remove_reporter_from_index(&env, &reporter);
        bump_instance(&env);
        ReporterRevoked { reporter }.publish(&env);
    }

    pub fn role_of(env: Env, reporter: Address) -> Option<Role> {
        Self::get_reporter(env, reporter).map(|info| info.role)
    }

    pub fn get_reporter(env: Env, reporter: Address) -> Option<ReporterInfo> {
        let key = DataKey::Reporter(reporter);
        let info = env.storage().persistent().get::<_, ReporterInfo>(&key);
        if info.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        info
    }

    /// Total number of registered reporters.
    pub fn reporter_count(env: Env) -> u32 {
        ensure_index_backfilled(&env);
        read_index(&env).len()
    }

    /// Paginated list of reporter addresses. `limit` is capped at 100.
    pub fn reporters(env: Env, start: u32, limit: u32) -> Vec<Address> {
        ensure_index_backfilled(&env);
        let index = read_index(&env);
        let len = index.len();
        if start >= len {
            return Vec::new(&env);
        }
        let effective_limit = limit.min(MAX_REPORTERS_PAGE_LIMIT).min(len - start);
        let mut out = Vec::new(&env);
        for i in start..start + effective_limit {
            out.push_back(index.get(i).unwrap());
        }
        out
    }

    pub fn transfer_admin(env: Env, new_admin: Address) {
        admin(&env).require_auth();
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        bump_instance(&env);
        AdminTransferred { new_admin }.publish(&env);
    }

    /// Propose a contract upgrade. The upgrade can be executed after a delay.
    pub fn propose_upgrade(env: Env, wasm_hash: BytesN<32>) {
        admin(&env).require_auth();
        let proposed = PendingUpgrade {
            wasm_hash: wasm_hash.clone(),
            proposed_at_ledger: env.ledger().sequence(),
            proposed_by: admin(&env),
        };
        env.storage()
            .instance()
            .set(&DataKey::PendingUpgrade, &proposed);
        bump_instance(&env);
        UpgradeProposed {
            wasm_hash,
            proposed_at_ledger: proposed.proposed_at_ledger,
            proposed_by: proposed.proposed_by,
        }
        .publish(&env);
    }

    /// Execute a previously proposed upgrade after the delay has elapsed.
    pub fn execute_upgrade(env: Env) {
        admin(&env).require_auth();
        let proposed = match env
            .storage()
            .instance()
            .get::<_, PendingUpgrade>(&DataKey::PendingUpgrade)
        {
            Some(p) => p,
            None => panic_with_error!(&env, RegistryError::UpgradeNotProposed),
        };
        let current = env.ledger().sequence();
        if current < proposed.proposed_at_ledger + UPGRADE_DELAY_LEDGERS {
            panic_with_error!(&env, RegistryError::UpgradeDelayNotElapsed);
        }
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        bump_instance(&env);
        UpgradeExecuted {
            wasm_hash: proposed.wasm_hash.clone(),
        }
        .publish(&env);
        env.deployer()
            .update_current_contract(ContractExecutable::Wasm(proposed.wasm_hash));
    }

    /// Cancel a pending upgrade proposal.
    pub fn cancel_upgrade(env: Env) {
        admin(&env).require_auth();
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        bump_instance(&env);
        UpgradeCancelled.publish(&env);
    }

    /// Get the currently pending upgrade, if any.
    pub fn pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        let upgrade = env
            .storage()
            .instance()
            .get::<_, PendingUpgrade>(&DataKey::PendingUpgrade);
        if upgrade.is_some() {
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        }
        upgrade
    }

    /// Returns the contract version.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }
}

#[cfg(test)]
mod test;
