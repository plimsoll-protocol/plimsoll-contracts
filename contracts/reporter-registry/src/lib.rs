//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
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
    Metadata(Address),
    Stake(Address),
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
    InvalidStakeAmount = 6,
    InsufficientStake = 7,
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
pub struct StakeChanged {
    #[topic]
    pub reporter: Address,
    pub old_stake: i128,
    pub new_stake: i128,
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

fn require_registered(env: &Env, reporter: &Address) {
    if !env
        .storage()
        .persistent()
        .has(&DataKey::Reporter(reporter.clone()))
    {
        panic_with_error!(env, RegistryError::ReporterNotFound);
    }
}

fn read_stake(env: &Env, reporter: &Address) -> i128 {
    let key = DataKey::Stake(reporter.clone());
    let stake = env.storage().persistent().get::<_, i128>(&key);
    if stake.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    stake.unwrap_or(0)
}

fn write_stake(env: &Env, reporter: &Address, old_stake: i128, new_stake: i128) {
    let key = DataKey::Stake(reporter.clone());
    if new_stake == 0 {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, &new_stake);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    bump_instance(env);
    StakeChanged {
        reporter: reporter.clone(),
        old_stake,
        new_stake,
    }
    .publish(env);
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
        require_registered(&env, &reporter);
        env.storage()
            .persistent()
            .remove(&DataKey::Reporter(reporter.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::Metadata(reporter.clone()));
        let stake_key = DataKey::Stake(reporter.clone());
        if let Some(old_stake) = env.storage().persistent().get::<_, i128>(&stake_key) {
            env.storage().persistent().remove(&stake_key);
            if old_stake > 0 {
                StakeChanged {
                    reporter: reporter.clone(),
                    old_stake,
                    new_stake: 0,
                }
                .publish(&env);
            }
        }
        bump_instance(&env);
        ReporterRevoked { reporter }.publish(&env);
    }

    /// Top up a registered reporter's tracked stake by `amount`.
    pub fn top_up_stake(env: Env, reporter: Address, amount: i128) -> i128 {
        admin(&env).require_auth();
        require_registered(&env, &reporter);
        if amount <= 0 {
            panic_with_error!(&env, RegistryError::InvalidStakeAmount);
        }
        let old_stake = read_stake(&env, &reporter);
        let new_stake = old_stake
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, RegistryError::InvalidStakeAmount));
        write_stake(&env, &reporter, old_stake, new_stake);
        new_stake
    }

    /// Reduce a registered reporter's tracked stake by `amount`.
    pub fn reduce_stake(env: Env, reporter: Address, amount: i128) -> i128 {
        admin(&env).require_auth();
        require_registered(&env, &reporter);
        if amount <= 0 {
            panic_with_error!(&env, RegistryError::InvalidStakeAmount);
        }
        let old_stake = read_stake(&env, &reporter);
        if amount > old_stake {
            panic_with_error!(&env, RegistryError::InsufficientStake);
        }
        let new_stake = old_stake - amount;
        write_stake(&env, &reporter, old_stake, new_stake);
        new_stake
    }

    /// A reporter's tracked stake, or `0` when none is recorded.
    pub fn stake_of(env: Env, reporter: Address) -> i128 {
        read_stake(&env, &reporter)
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
        require_registered(&env, &reporter);
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
}

#[cfg(test)]
mod test;
