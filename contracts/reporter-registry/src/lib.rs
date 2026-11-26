//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, ContractExecutable, Env, String, Symbol, Vec,
};

const MAX_NAME_LEN: u32 = 64;
const MAX_WEBSITE_URI_LEN: u32 = 256;
const MAX_CONTACT_HASH_LEN: u32 = 64;
/// Holding period (in seconds) within which distinct watcher disputes accrue
/// toward `dispute_quorum` (24 hours).
pub const DISPUTE_HOLDING_PERIOD_SECS: u64 = 86_400;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Reporter(Address),
    Metadata(Address),
    PendingUpgrade,
    DisputeQuorum,
    Dispute(BytesN<32>),
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DisputeStatus {
    Held = 1,
    Rejected = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeRecord {
    pub report_id: BytesN<32>,
    pub watchers: Vec<Address>,
    pub opened_at: u64,
    pub status: DisputeStatus,
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
    InvalidQuorum = 6,
    DuplicateDispute = 7,
    DisputeAlreadyRejected = 8,
    HoldingPeriodElapsed = 9,
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

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeQuorumSet {
    pub quorum: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportDisputed {
    #[topic]
    pub report_id: BytesN<32>,
    pub watcher: Address,
    pub dispute_count: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRejected {
    #[topic]
    pub report_id: BytesN<32>,
    pub dispute_count: u32,
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
        let key = DataKey::Reporter(reporter.clone());
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        env.storage().persistent().remove(&key);
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

    /// Set the number of distinct watchers that must dispute a report within the
    /// holding period to automatically reject it. Must be at least 1.
    pub fn set_dispute_quorum(env: Env, quorum: u32) {
        admin(&env).require_auth();
        if quorum == 0 {
            panic_with_error!(&env, RegistryError::InvalidQuorum);
        }
        env.storage()
            .instance()
            .set(&DataKey::DisputeQuorum, &quorum);
        bump_instance(&env);
        DisputeQuorumSet { quorum }.publish(&env);
    }

    /// Current dispute quorum (defaults to 1).
    pub fn dispute_quorum(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::DisputeQuorum)
            .unwrap_or(1)
    }

    /// Record a watcher's dispute against `report_id`. When `dispute_quorum`
    /// distinct watchers have disputed the same report within the holding
    /// period, the report is automatically rejected and `ReportRejected` is
    /// emitted.
    pub fn dispute_report(env: Env, watcher: Address, report_id: BytesN<32>) -> DisputeStatus {
        watcher.require_auth();
        let now = env.ledger().timestamp();
        let key = DataKey::Dispute(report_id.clone());
        let mut record = match env.storage().persistent().get::<_, DisputeRecord>(&key) {
            Some(existing) => {
                if existing.status == DisputeStatus::Rejected {
                    panic_with_error!(&env, RegistryError::DisputeAlreadyRejected);
                }
                if now
                    > existing
                        .opened_at
                        .saturating_add(DISPUTE_HOLDING_PERIOD_SECS)
                {
                    panic_with_error!(&env, RegistryError::HoldingPeriodElapsed);
                }
                if existing.watchers.contains(&watcher) {
                    panic_with_error!(&env, RegistryError::DuplicateDispute);
                }
                existing
            }
            None => DisputeRecord {
                report_id: report_id.clone(),
                watchers: Vec::new(&env),
                opened_at: now,
                status: DisputeStatus::Held,
            },
        };

        record.watchers.push_back(watcher.clone());
        let dispute_count = record.watchers.len();
        let quorum = Self::dispute_quorum(env.clone());
        if dispute_count >= quorum {
            record.status = DisputeStatus::Rejected;
        }

        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);

        ReportDisputed {
            report_id: report_id.clone(),
            watcher,
            dispute_count,
        }
        .publish(&env);

        if record.status == DisputeStatus::Rejected {
            ReportRejected {
                report_id,
                dispute_count,
            }
            .publish(&env);
        }

        record.status
    }

    /// Return the dispute record for `report_id`, if any.
    pub fn get_dispute(env: Env, report_id: BytesN<32>) -> Option<DisputeRecord> {
        let key = DataKey::Dispute(report_id);
        let record = env.storage().persistent().get::<_, DisputeRecord>(&key);
        if record.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        record
    }
}

#[cfg(test)]
mod test;
