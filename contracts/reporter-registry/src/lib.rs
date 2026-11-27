//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingReporter, PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO,
    INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, ContractExecutable, Env, String, Symbol, Vec,
};

const MAX_NAME_LEN: u32 = 64;
const MAX_WEBSITE_URI_LEN: u32 = 256;
const MAX_CONTACT_HASH_LEN: u32 = 64;
/// Most category tags a reporter may self-assign at once.
const MAX_TAGS: u32 = 5;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Reporter(Address),
    Metadata(Address),
    Stake(Address),
    PendingUpgrade,
    PendingReporter(Address),
    /// A reporter's self-assigned category tags.
    Tags(Address),
    /// Reverse index: reporters currently holding a given category tag.
    Category(Symbol),
    /// The coverage-ledger contract trusted to report post/retract outcomes
    /// for reputation scoring. Kept separate from `Admin` since it names a
    /// contract, not an account, and may be rotated independently.
    Ledger,
    /// A reporter's reputation score. Kept in its own key, like `Stake`,
    /// rather than added to `ReporterInfo`: changing that struct's field
    /// list would change the encoding of every existing reporter entry.
    Score(Address),
    DisplayName(Address),
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
    InvalidConfirmWindow = 6,
    PendingRegistrationNotFound = 7,
    RegistrationExpired = 8,
    InvalidStakeAmount = 6,
    InsufficientStake = 7,
    TooManyTags = 8,
    UntrustedLedger = 9,
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
pub struct ReporterRegistrationProposed {
    #[topic]
    pub reporter: Address,
    pub role: Role,
    pub name: String,
    pub expires_at_ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterRegistrationConfirmed {
    #[topic]
    pub reporter: Address,
    pub role: Role,
    pub name: String,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterRegistrationExpired {
    #[topic]
    pub reporter: Address,
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
pub struct ReporterTagsUpdated {
    #[topic]
    pub reporter: Address,
    pub tags: Vec<Symbol>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerSet {
    #[topic]
    pub ledger: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReputationChanged {
    #[topic]
    pub reporter: Address,
    pub old_score: i32,
    pub new_score: i32,
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

/// Remove a reporter's registration, metadata and stake, emitting the same
/// events `revoke` does. Caller is responsible for the admin auth check.
fn deregister_one(env: &Env, reporter: Address) {
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
            .publish(env);
        }
    }
    bump_instance(env);
    ReporterRevoked { reporter }.publish(env);
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

fn add_to_category(env: &Env, tag: &Symbol, reporter: &Address) {
    let key = DataKey::Category(tag.clone());
    let mut index = env
        .storage()
        .persistent()
        .get::<_, Vec<Address>>(&key)
        .unwrap_or_else(|| Vec::new(env));
    if !index.contains(reporter) {
        index.push_back(reporter.clone());
        env.storage().persistent().set(&key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
}

fn remove_from_category(env: &Env, tag: &Symbol, reporter: &Address) {
    let key = DataKey::Category(tag.clone());
    let mut index = match env.storage().persistent().get::<_, Vec<Address>>(&key) {
        Some(index) => index,
        None => return,
    };
    if let Some(i) = index
        .iter()
        .position(|indexed| indexed == *reporter)
        .and_then(|i| u32::try_from(i).ok())
    {
        index.remove(i);
    }
    if index.is_empty() {
        env.storage().persistent().remove(&key);
    } else {
        env.storage().persistent().set(&key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
}

fn read_score(env: &Env, reporter: &Address) -> i32 {
    let key = DataKey::Score(reporter.clone());
    let score = env.storage().persistent().get::<_, i32>(&key);
    if score.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    score.unwrap_or(0)
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

    /// Create a pending reporter registration that the reporter must confirm
    /// within `confirm_window_ledgers` before becoming active.
    pub fn register_reporter(
        env: Env,
        reporter: Address,
        role: Role,
        name: String,
        confirm_window_ledgers: u32,
    ) {
        admin(&env).require_auth();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            panic_with_error!(&env, RegistryError::InvalidName);
        }
        if confirm_window_ledgers == 0 {
            panic_with_error!(&env, RegistryError::InvalidConfirmWindow);
        }
        let expires_at_ledger = env
            .ledger()
            .sequence()
            .saturating_add(confirm_window_ledgers);
        let key = DataKey::PendingReporter(reporter.clone());
        let entry = PendingReporter {
            role,
            name: name.clone(),
            expires_at_ledger,
        };
        env.storage().persistent().set(&key, &entry);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
        ReporterRegistrationProposed {
            reporter,
            role,
            name,
            expires_at_ledger,
        }
        .publish(&env);
    }

    /// Confirm a pending reporter registration. Must be called by the reporter
    /// themselves before `expires_at_ledger`.
    pub fn confirm_registration(env: Env, reporter: Address) {
        reporter.require_auth();
        let pending_key = DataKey::PendingReporter(reporter.clone());
        let pending = env
            .storage()
            .persistent()
            .get::<_, PendingReporter>(&pending_key)
            .unwrap_or_else(|| panic_with_error!(&env, RegistryError::PendingRegistrationNotFound));
        if env.ledger().sequence() > pending.expires_at_ledger {
            panic_with_error!(&env, RegistryError::RegistrationExpired);
        }
        env.storage().persistent().remove(&pending_key);

        let key = DataKey::Reporter(reporter.clone());
        let added_at = env
            .storage()
            .persistent()
            .get::<_, ReporterInfo>(&key)
            .map(|existing| existing.added_at)
            .unwrap_or_else(|| env.ledger().timestamp());
        let info = ReporterInfo {
            role: pending.role,
            name: pending.name.clone(),
            added_at,
        };
        env.storage().persistent().set(&key, &info);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
        ReporterRegistrationConfirmed {
            reporter,
            role: pending.role,
            name: pending.name,
        }
        .publish(&env);
    }

    /// Remove an expired pending registration. Returns true if an expired
    /// entry was removed, false otherwise.
    pub fn cleanup_expired_registration(env: Env, reporter: Address) -> bool {
        let key = DataKey::PendingReporter(reporter.clone());
        let Some(pending) = env.storage().persistent().get::<_, PendingReporter>(&key) else {
            return false;
        };
        if env.ledger().sequence() <= pending.expires_at_ledger {
            return false;
        }
        env.storage().persistent().remove(&key);
        bump_instance(&env);
        ReporterRegistrationExpired { reporter }.publish(&env);
        true
    }

    /// Read a pending reporter registration if one exists and has not expired.
    pub fn get_pending_reporter(env: Env, reporter: Address) -> Option<PendingReporter> {
        let key = DataKey::PendingReporter(reporter);
        let pending = env.storage().persistent().get::<_, PendingReporter>(&key)?;
        if env.ledger().sequence() > pending.expires_at_ledger {
            return None;
        }
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        Some(pending)
    }

    pub fn revoke(env: Env, reporter: Address) {
        admin(&env).require_auth();
        require_registered(&env, &reporter);
        deregister_one(&env, reporter);
    }

    /// Revoke every reporter in `addresses` that is currently registered.
    /// Addresses that are not registered are skipped rather than erroring,
    /// so a batch can be retried after a partial run without failing.
    pub fn batch_deregister(env: Env, addresses: Vec<Address>) {
        admin(&env).require_auth();
        for reporter in addresses.iter() {
            if env
                .storage()
                .persistent()
                .has(&DataKey::Reporter(reporter.clone()))
            {
                deregister_one(&env, reporter);
        env.storage()
            .persistent()
            .remove(&DataKey::Reporter(reporter.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::Metadata(reporter.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::PendingReporter(reporter.clone()));
        bump_instance(&env);
        ReporterRevoked { reporter }.publish(&env);
            .remove(&DataKey::DisplayName(reporter.clone()));
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

    /// Rotate a registered reporter's published website URI without
    /// disturbing their other metadata fields. The reporter authorises the
    /// change; an admin cannot do it on their behalf.
    pub fn update_reporter_uri(env: Env, reporter: Address, new_uri: String) {
        reporter.require_auth();
        require_registered(&env, &reporter);
        if new_uri.is_empty() || new_uri.len() > MAX_WEBSITE_URI_LEN {
            panic_with_error!(&env, RegistryError::InvalidMetadata);
        }
        let key = DataKey::Metadata(reporter.clone());
        let existing = env.storage().persistent().get::<_, ReporterMetadata>(&key);
        let (name, contact_hash) = existing
            .map(|m| (m.name, m.contact_hash))
            .unwrap_or((None, None));
        let metadata = ReporterMetadata {
            name: name.clone(),
            website_uri: Some(new_uri.clone()),
            contact_hash: contact_hash.clone(),
        };
        env.storage().persistent().set(&key, &metadata);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
        ReporterMetadataUpdated {
            reporter,
            name,
            website_uri: Some(new_uri),
            contact_hash,
        }
        .publish(&env);
    }

    /// Replace a registered reporter's self-assigned category tags (e.g.
    /// `bank`, `custodian`, `dex`), capped at `MAX_TAGS`. The reporter
    /// authorises the change. Duplicate tags are collapsed to one entry.
    pub fn update_reporter_tags(env: Env, reporter: Address, tags: Vec<Symbol>) {
        reporter.require_auth();
        require_registered(&env, &reporter);
        if tags.len() > MAX_TAGS {
            panic_with_error!(&env, RegistryError::TooManyTags);
        }
        let mut deduped: Vec<Symbol> = Vec::new(&env);
        for tag in tags.iter() {
            if !deduped.contains(&tag) {
                deduped.push_back(tag);
            }
        }

        let key = DataKey::Tags(reporter.clone());
        let old_tags: Vec<Symbol> = env
            .storage()
            .persistent()
            .get::<_, Vec<Symbol>>(&key)
            .unwrap_or_else(|| Vec::new(&env));

        for tag in old_tags.iter() {
            if !deduped.contains(&tag) {
                remove_from_category(&env, &tag, &reporter);
            }
        }
        for tag in deduped.iter() {
            if !old_tags.contains(&tag) {
                add_to_category(&env, &tag, &reporter);
            }
        }

        if deduped.is_empty() {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &deduped);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        bump_instance(&env);
        ReporterTagsUpdated {
            reporter,
            tags: deduped,
        }
        .publish(&env);
    }

    /// A reporter's self-assigned category tags, or an empty list.
    pub fn get_reporter_tags(env: Env, reporter: Address) -> Vec<Symbol> {
        let key = DataKey::Tags(reporter);
        let tags = env.storage().persistent().get::<_, Vec<Symbol>>(&key);
        if tags.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        tags.unwrap_or_else(|| Vec::new(&env))
    }

    /// Every reporter address currently self-assigned to `tag`.
    pub fn get_reporters_by_category(env: Env, tag: Symbol) -> Vec<Address> {
        let key = DataKey::Category(tag);
        let index = env.storage().persistent().get::<_, Vec<Address>>(&key);
        if index.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        index.unwrap_or_else(|| Vec::new(&env))
    }

    /// Set a registered reporter's on-chain display name. Only the reporter
    /// can set it; a later call replaces it. `Symbol` caps it at 32
    /// characters.
    pub fn set_reporter_name(env: Env, reporter: Address, name: Symbol) {
        reporter.require_auth();
        require_registered(&env, &reporter);
        let key = DataKey::DisplayName(reporter);
        env.storage().persistent().set(&key, &name);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
    }

    /// A reporter's display name, or `None` when they have not set one.
    pub fn get_reporter_name(env: Env, reporter: Address) -> Option<Symbol> {
        let key = DataKey::DisplayName(reporter);
        let name = env.storage().persistent().get::<_, Symbol>(&key);
        if name.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        name
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

    /// Name the coverage-ledger contract trusted to report post/retract
    /// outcomes via `record_report_outcome`. One ledger at a time; setting a
    /// new one replaces it. Admin-gated, since this grants that contract
    /// write access to every reporter's reputation score.
    pub fn set_ledger(env: Env, ledger: Address) {
        admin(&env).require_auth();
        env.storage().instance().set(&DataKey::Ledger, &ledger);
        bump_instance(&env);
        LedgerSet { ledger }.publish(&env);
    }

    /// The coverage-ledger contract trusted for reputation reporting, if set.
    pub fn ledger(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Ledger)
    }

    /// Adjust a reporter's reputation score: `+1` for an accepted report
    /// (a successful `post_reserve`), `-1` for a disputed or retracted one
    /// (`retract_reserve`). Callable only by the trusted coverage-ledger
    /// contract, which authorises itself as the direct caller — a contract
    /// has no keypair to sign with, so this is the host's proof that the
    /// call genuinely came from that contract's own code and not a forged
    /// argument. `ledger` must match the address configured via
    /// `set_ledger`.
    pub fn record_report_outcome(env: Env, ledger: Address, reporter: Address, accepted: bool) {
        let trusted: Option<Address> = env.storage().instance().get(&DataKey::Ledger);
        if trusted != Some(ledger.clone()) {
            panic_with_error!(&env, RegistryError::UntrustedLedger);
        }
        ledger.require_auth();
        require_registered(&env, &reporter);

        let old_score = read_score(&env, &reporter);
        let new_score = if accepted {
            old_score.saturating_add(1)
        } else {
            old_score.saturating_sub(1)
        };
        let key = DataKey::Score(reporter.clone());
        env.storage().persistent().set(&key, &new_score);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
        ReputationChanged {
            reporter,
            old_score,
            new_score,
        }
        .publish(&env);
    }

    /// A reporter's reputation score, or `0` when no outcome has ever been
    /// recorded for them. Stored independently of `ReporterInfo`, so every
    /// existing reporter entry reads as score `0` with no migration needed.
    pub fn get_reporter_score(env: Env, reporter: Address) -> i32 {
        read_score(&env, &reporter)
    }
}

#[cfg(test)]
mod test;
