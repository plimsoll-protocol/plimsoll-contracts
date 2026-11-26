//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
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

    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, ContractExecutable, Env, String, Symbol,
};

const MAX_NAME_LEN: u32 = 64;
const MAX_WEBSITE_URI_LEN: u32 = 256;
const MAX_CONTACT_HASH_LEN: u32 = 64;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Reporter(Address),
    ReporterIndex,

    Metadata(Address),
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
    InvalidMetadata = 5,
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
pub struct ReporterMetadataUpdated {
    #[topic]
    pub reporter: Address,
    pub name: Option<Symbol>,
    pub website_uri: Option<String>,
    pub contact_hash: Option<Bytes>,
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
    pub executable_at_ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeExecuted {
    #[topic]
    pub wasm_hash: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeCancelled {
    #[topic]
    pub wasm_hash: BytesN<32>,
}

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

fn pending(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

fn read_index(env: &Env) -> Vec<Address> {
    let key = DataKey::ReporterIndex;
    match env.storage().persistent().get::<_, Vec<Address>>(&key) {
        Some(index) => {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
            index
        }
        None => Vec::new(env),
    }
}

fn write_index(env: &Env, index: &Vec<Address>) {
    let key = DataKey::ReporterIndex;
    env.storage().persistent().set(&key, index);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
}

/// Append `reporter` to the index unless it is already there.
fn index_add(env: &Env, reporter: &Address) {
    let mut index = read_index(env);
    if !index.contains(reporter) {
        index.push_back(reporter.clone());
        write_index(env, &index);
    }
}

/// Remove `reporter` from the index, keeping the others in order.
fn index_remove(env: &Env, reporter: &Address) {
    let mut index = read_index(env);
    if let Some(position) = index.first_index_of(reporter) {
        index.remove(position);
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
        // Indexing on every call, not only for new reporters, lets the admin
        // add reporters registered before the index existed by re-setting them.
        index_add(&env, &reporter);
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
        index_remove(&env, &reporter);

        env.storage()
            .persistent()
            .remove(&DataKey::Metadata(reporter.clone()));
        bump_instance(&env);
        ReporterRevoked { reporter }.publish(&env);
    }

    /// Replace a registered reporter's optional public metadata. The reporter
    /// authorises the change; an admin cannot edit it on their behalf. A `None`
    /// field clears that field, and clearing all three removes the entry.
    pub fn update_reporter_metadata(
        env: Env,
        reporter: Address,
        name: Option<Symbol>,
        website_uri: Option<String>,
        contact_hash: Option<Bytes>,
    ) {
        reporter.require_auth();
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Reporter(reporter.clone()))
        {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        if let Some(uri) = website_uri.as_ref() {
            if uri.is_empty() || uri.len() > MAX_WEBSITE_URI_LEN {
                panic_with_error!(&env, RegistryError::InvalidMetadata);
            }
        }
        if let Some(hash) = contact_hash.as_ref() {
            if hash.is_empty() || hash.len() > MAX_CONTACT_HASH_LEN {
                panic_with_error!(&env, RegistryError::InvalidMetadata);
            }
        }

        let key = DataKey::Metadata(reporter.clone());
        if name.is_none() && website_uri.is_none() && contact_hash.is_none() {
            env.storage().persistent().remove(&key);
        } else {
            let metadata = ReporterMetadata {
                name: name.clone(),
                website_uri: website_uri.clone(),
                contact_hash: contact_hash.clone(),
            };
            env.storage().persistent().set(&key, &metadata);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        bump_instance(&env);
        ReporterMetadataUpdated {
            reporter,
            name,
            website_uri,
            contact_hash,
        }
        .publish(&env);
    }

    /// A reporter's optional metadata, or `None` when they have published
    /// none. Reading an existing entry extends its TTL like `get_reporter`.
    pub fn get_reporter_metadata(env: Env, reporter: Address) -> Option<ReporterMetadata> {
        let key = DataKey::Metadata(reporter);
        let metadata = env.storage().persistent().get::<_, ReporterMetadata>(&key);
        if metadata.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        metadata
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

    /// Number of reporters in the index.
    pub fn reporter_count(env: Env) -> u32 {
        read_index(&env).len()
    }

    /// Up to `min(limit, 100)` indexed reporters from position `start`, in
    /// first-indexed order. Empty when `start` is past the end.
    pub fn reporters(env: Env, start: u32, limit: u32) -> Vec<Address> {
        let index = read_index(&env);
        let len = index.len();
        if start >= len {
            return Vec::new(&env);
        }
        let end = start
            .saturating_add(limit.min(MAX_REPORTERS_PAGE_LIMIT))
            .min(len);
        index.slice(start..end)
    }

    pub fn transfer_admin(env: Env, new_admin: Address) {
        admin(&env).require_auth();
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        bump_instance(&env);
        AdminTransferred { new_admin }.publish(&env);
    }

    /// Propose a wasm hash for installation after the upgrade delay. A new
    /// proposal replaces any pending one and restarts the clock.
    pub fn propose_upgrade(env: Env, wasm_hash: BytesN<32>) {
        admin(&env).require_auth();
        let executable_at_ledger = env
            .ledger()
            .sequence()
            .saturating_add(UPGRADE_DELAY_LEDGERS);
        env.storage().instance().set(
            &DataKey::PendingUpgrade,
            &PendingUpgrade {
                wasm_hash: wasm_hash.clone(),
                executable_at_ledger,
            },
        );
        bump_instance(&env);
        UpgradeProposed {
            wasm_hash,
            executable_at_ledger,
        }
        .publish(&env);
    }

    /// Install the pending wasm once its delay has elapsed.
    pub fn execute_upgrade(env: Env) {
        admin(&env).require_auth();
        let proposed = match pending(&env) {
            Some(proposed) => proposed,
            None => panic_with_error!(&env, RegistryError::UpgradeNotProposed),
        };
        if env.ledger().sequence() < proposed.executable_at_ledger {
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

    /// Cancel the pending upgrade.
    pub fn cancel_upgrade(env: Env) {
        admin(&env).require_auth();
        let proposed = match pending(&env) {
            Some(proposed) => proposed,
            None => panic_with_error!(&env, RegistryError::UpgradeNotProposed),
        };
        env.storage().instance().remove(&DataKey::PendingUpgrade);
        bump_instance(&env);
        UpgradeCancelled {
            wasm_hash: proposed.wasm_hash,
        }
        .publish(&env);
    }

    /// The pending upgrade, if any.
    pub fn pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        let proposed = pending(&env);
        if proposed.is_some() {
            bump_instance(&env);
        }
        proposed
    }

    /// The contract version, raised on every behavioural change.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }
}

#[cfg(test)]
mod test;
