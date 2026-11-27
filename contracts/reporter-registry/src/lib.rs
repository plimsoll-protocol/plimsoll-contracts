//! Reporter registry: decides which addresses may post reserve reports and
//! supply snapshots to the coverage ledger, and in which role.
#![no_std]

use plimsoll_types::{
    PendingUpgrade, RegistryRole, ReporterInfo, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,

    PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
    PendingUpgrade, ReporterEntry, ReporterInfo, ReporterMetadata, Role,
    DEFAULT_MAX_REPORTERS_PER_ASSET, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO,
    PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
    PendingReporter, PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO,
    INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
    PendingUpgrade, ReporterEntry, ReporterInfo, ReporterMetadata, Role,
    DEFAULT_MAX_REPORTERS_PER_ASSET, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO,
    PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
    PendingReporter, PendingUpgrade, ReporterInfo, ReporterMetadata, Role, INSTANCE_EXTEND_TO,
    INSTANCE_THRESHOLD, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, UPGRADE_DELAY_LEDGERS,
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

    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error,
    token::TokenClient, Address, Bytes, BytesN, ContractExecutable, Env, String, Symbol,
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, Address,
    Bytes, BytesN, ContractExecutable, Env, String, Symbol, Vec,
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error,
    token::TokenClient, Address, Bytes, BytesN, ContractExecutable, Env, String, Symbol,
};

const MAX_NAME_LEN: u32 = 64;
const MAX_WEBSITE_URI_LEN: u32 = 256;
const MAX_CONTACT_HASH_LEN: u32 = 64;
/// Holding period (in seconds) within which distinct watcher disputes accrue
/// toward `dispute_quorum` (24 hours).
pub const DISPUTE_HOLDING_PERIOD_SECS: u64 = 86_400;
/// Most category tags a reporter may self-assign at once.
const MAX_TAGS: u32 = 5;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    StakeAsset,
    MinStakeStroops,
    Reporter(Address),
    ReporterIndex,
    /// Unix-timestamp at which the reporter's registration expires.
    Expiry(Address),
    ReporterDisplayName(Address),
    /// Unix-timestamp at which the reporter's registration expires.
    Expiry(Address),
    ReporterDisplayName(Address),

    Metadata(Address),
    Stake(Address),
    PendingUpgrade,
    /// Explicit access tier assigned to an address via `assign_role`.
    AccessRole(Address),
    MaxReporters,
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
    MaxReporters,
    PendingReporter(Address),
    /// Every reporter ever registered and not since revoked, in registration
    /// order. Append-only, so stored keys keep their discriminants.
    Reporters,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryError {
    ReporterNotFound = 1,
    InvalidName = 2,
    UpgradeNotProposed = 3,
    UpgradeDelayNotElapsed = 4,
    InsufficientStake = 5,
    InvalidStakeAmount = 6,
    ReporterAlreadyRegistered = 7,

    InvalidMetadata = 5,
    InvalidQuorum = 6,
    DuplicateDispute = 7,
    DisputeAlreadyRejected = 8,
    HoldingPeriodElapsed = 9,
    InvalidConfirmWindow = 6,
    PendingRegistrationNotFound = 7,
    RegistrationExpired = 8,
    InvalidStakeAmount = 6,
    InsufficientStake = 7,
    InvalidMaxReporters = 8,
    EmptyBatch = 9,
    TooManyTags = 8,
    UntrustedLedger = 9,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterExpirySet {
    #[topic]
    pub reporter: Address,
    pub expires_at: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterExpirySet {
    #[topic]
    pub reporter: Address,
    pub expires_at: u64,
    InvalidConfirmWindow = 6,
    PendingRegistrationNotFound = 7,
    RegistrationExpired = 8,
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
pub struct StakeRequirementSet {
    #[topic]
    pub stake_asset: Address,
    pub min_stake_stroops: i128,
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
pub struct MaxReportersSet {
    #[topic]
    pub max: u32,
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

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleAssigned {
    #[topic]
    pub assignee: Address,
    pub tier: RegistryRole,
pub struct DisputeQuorumSet {
    pub quorum: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleRevoked {
    #[topic]
    pub assignee: Address,
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
        .unwrap_or_else(|| unreachable!("admin is set in the constructor"))
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
/// Addresses the registry has entries for, in the order they were first
/// registered. Reading an existing index extends its TTL like `get_reporter`.
fn reporters(env: &Env) -> Vec<Address> {
    let key = DataKey::Reporters;
    let index = env.storage().persistent().get::<_, Vec<Address>>(&key);
    if index.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    index.unwrap_or_else(|| Vec::new(env))
}

/// Store the index, or remove it when the last reporter is revoked so an empty
/// registry leaves nothing behind.
fn write_reporters(env: &Env, index: &Vec<Address>) {
    let key = DataKey::Reporters;
    if index.is_empty() {
        env.storage().persistent().remove(&key);
        return;
    }
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
fn meets_minimum_stake(env: &Env, reporter: &Address) -> bool {
    let minimum = env
        .storage()
        .instance()
        .get::<_, i128>(&DataKey::MinStakeStroops)
        .unwrap_or(0);
    if minimum == 0 {
        return true;
    }
    let asset = match env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::StakeAsset)
    {
        Some(asset) => asset,
        None => return false,
    };
    TokenClient::new(env, &asset).balance(reporter) >= minimum
/// The admin's per-asset reporter cap, or `DEFAULT_MAX_REPORTERS_PER_ASSET`
/// while it has not been set. A registry upgraded from v0.1 has no stored
/// value, so it answers the default without a migration.
fn configured_max_reporters(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::MaxReporters)
        .unwrap_or(DEFAULT_MAX_REPORTERS_PER_ASSET)
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

fn require_valid_name(env: &Env, name: &String) {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        panic_with_error!(env, RegistryError::InvalidName);
    }
}

/// Add a reporter, or change an existing reporter's role and name. Used by
/// both the single and the batch entry point, so the two can never drift.
fn write_reporter(env: &Env, reporter: &Address, role: Role, name: &String) {
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
    ReporterSet {
        reporter: reporter.clone(),
        role,
        name: name.clone(),
    }
    .publish(env);
}

#[contractimpl]
impl ReporterRegistry {
    /// Initialises the registry with `admin` as the sole administrator.
    ///
    /// # Arguments
    /// * `admin` - the address that will control this contract
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        bump_instance(&env);
    }

    /// Returns the current administrator address.
    pub fn admin(env: Env) -> Address {
        admin(&env)
    }

    pub fn set_stake_requirement(env: Env, stake_asset: Address, min_stake_stroops: i128) {
        admin(&env).require_auth();
        if min_stake_stroops < 0 {
            panic_with_error!(&env, RegistryError::InvalidStakeAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::StakeAsset, &stake_asset);
        env.storage()
            .instance()
            .set(&DataKey::MinStakeStroops, &min_stake_stroops);
        bump_instance(&env);
        StakeRequirementSet {
            stake_asset,
            min_stake_stroops,
        }
        .publish(&env);
    }

    pub fn min_stake_stroops(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinStakeStroops)
            .unwrap_or(0)
    }

    pub fn has_minimum_stake(env: Env, reporter: Address) -> bool {
        meets_minimum_stake(&env, &reporter)
    }

    pub fn register_reporter(env: Env, reporter: Address, name: String) {
        reporter.require_auth();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            panic_with_error!(&env, RegistryError::InvalidName);
        }
        let key = DataKey::Reporter(reporter.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, RegistryError::ReporterAlreadyRegistered);
        }
        if !meets_minimum_stake(&env, &reporter) {
            panic_with_error!(&env, RegistryError::InsufficientStake);
        }
        let info = ReporterInfo {
            role: Role::SupplyPoster,
            name: name.clone(),
            added_at: env.ledger().timestamp(),
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
            role: Role::SupplyPoster,
            name,
        }
        .publish(&env);
    }

    /// Add a reporter, or change an existing reporter's role and name.
    ///
    /// If `reporter` already exists, `added_at` is preserved; otherwise it is
    /// set to the current ledger timestamp.
    ///
    /// # Arguments
    /// * `reporter` - the address being registered or updated
    /// * `role` - `Auditor`, `Transcriber`, or `SupplyPoster`
    /// * `name` - display name, 1–64 bytes
    ///
    /// # Errors
    /// * [`RegistryError::InvalidName`] when `name` is empty or longer than 64 bytes
    ///
    /// # Events
    /// * [`ReporterSet`] on success
    pub fn set_reporter(env: Env, reporter: Address, role: Role, name: String) {
    pub fn set_stake_requirement(env: Env, stake_asset: Address, min_stake_stroops: i128) {
        admin(&env).require_auth();
        if min_stake_stroops < 0 {
            panic_with_error!(&env, RegistryError::InvalidStakeAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::StakeAsset, &stake_asset);
        env.storage()
            .instance()
            .set(&DataKey::MinStakeStroops, &min_stake_stroops);
        bump_instance(&env);
        StakeRequirementSet {
            stake_asset,
            min_stake_stroops,
        }
        .publish(&env);
    }

    pub fn min_stake_stroops(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinStakeStroops)
            .unwrap_or(0)
    }

    pub fn has_minimum_stake(env: Env, reporter: Address) -> bool {
        meets_minimum_stake(&env, &reporter)
    }

    pub fn register_reporter(env: Env, reporter: Address, name: String) {
        reporter.require_auth();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            panic_with_error!(&env, RegistryError::InvalidName);
        }
        let key = DataKey::Reporter(reporter.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, RegistryError::ReporterAlreadyRegistered);
        }
        if !meets_minimum_stake(&env, &reporter) {
            panic_with_error!(&env, RegistryError::InsufficientStake);
        }
        let info = ReporterInfo {
            role: Role::SupplyPoster,
            name: name.clone(),
            added_at: env.ledger().timestamp(),
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
            role: Role::SupplyPoster,
            name,
        }
        .publish(&env);
    }

    /// Remove a reporter from the registry.
    ///
    /// # Arguments
    /// * `reporter` - the address to revoke
    ///
    /// # Errors
    /// * [`RegistryError::ReporterNotFound`] when `reporter` is not registered
    ///
    /// # Events
    /// * [`ReporterRevoked`] on success
    pub fn revoke(env: Env, reporter: Address) {
    /// Add a reporter, or change an existing reporter's role and name.
    ///
    /// If `reporter` already exists, `added_at` is preserved; otherwise it is
    /// set to the current ledger timestamp.
    ///
    /// # Arguments
    /// * `reporter` - the address being registered or updated
    /// * `role` - `Auditor`, `Transcriber`, or `SupplyPoster`
    /// * `name` - display name, 1–64 bytes
    ///
    /// # Errors
    /// * [`RegistryError::InvalidName`] when `name` is empty or longer than 64 bytes
    ///
    /// # Events
    /// * [`ReporterSet`] on success
    pub fn set_reporter(env: Env, reporter: Address, role: Role, name: String) {
        admin(&env).require_auth();
        require_valid_name(&env, &name);
        write_reporter(&env, &reporter, role, &name);
        bump_instance(&env);
    }

    /// Add or change many reporters in one transaction, so populating the
    /// registry does not take a transaction per reporter.
    ///
    /// Every entry is validated before the first one is written, and a failed
    /// call reverts the whole transaction, so the batch is all or nothing. The
    /// per-asset cap (`max_reporters`) is not a registry rule -- the registry
    /// does not know which assets a reporter will speak for -- so the coverage
    /// ledger applies it at post time and a batch cannot register its way past
    /// it.
    pub fn batch_register(env: Env, reporters: Vec<ReporterEntry>) {
        admin(&env).require_auth();
        if reporters.is_empty() {
            panic_with_error!(&env, RegistryError::EmptyBatch);
        }
        for entry in reporters.iter() {
            require_valid_name(&env, &entry.name);
        }
        for entry in reporters.iter() {
            write_reporter(&env, &entry.reporter, entry.role, &entry.name);
        }
        bump_instance(&env);
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
        // A role or name change for an existing reporter is not a new entry.
        let mut index = reporters(&env);
        if !index.contains(&reporter) {
            index.push_back(reporter.clone());
            write_reporters(&env, &index);
        }
        bump_instance(&env);
        ReporterRegistrationConfirmed {
            reporter,
            role: pending.role,
            name: pending.name,
        require_valid_name(&env, &name);
        write_reporter(&env, &reporter, role, &name);
        bump_instance(&env);
    }

    /// Add or change many reporters in one transaction, so populating the
    /// registry does not take a transaction per reporter.
    ///
    /// Every entry is validated before the first one is written, and a failed
    /// call reverts the whole transaction, so the batch is all or nothing. The
    /// per-asset cap (`max_reporters`) is not a registry rule -- the registry
    /// does not know which assets a reporter will speak for -- so the coverage
    /// ledger applies it at post time and a batch cannot register its way past
    /// it.
    pub fn batch_register(env: Env, reporters: Vec<ReporterEntry>) {
        admin(&env).require_auth();
        if reporters.is_empty() {
            panic_with_error!(&env, RegistryError::EmptyBatch);
        }
        for entry in reporters.iter() {
            require_valid_name(&env, &entry.name);
        }
        for entry in reporters.iter() {
            write_reporter(&env, &entry.reporter, entry.role, &entry.name);
        }
        bump_instance(&env);
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
    /// Remove a reporter from the registry.
    ///
    /// # Arguments
    /// * `reporter` - the address to revoke
    ///
    /// # Errors
    /// * [`RegistryError::ReporterNotFound`] when `reporter` is not registered
    ///
    /// # Events
    /// * [`ReporterRevoked`] on success
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
        let key = DataKey::Reporter(reporter.clone());
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        env.storage().persistent().remove(&key);
        index_remove(&env, &reporter);

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
            .remove(&DataKey::ReporterDisplayName(reporter.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::ReporterDisplayName(reporter.clone()));
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
        let mut index = reporters(&env);
        if let Some(position) = index
            .iter()
            .position(|indexed| indexed == reporter)
            .and_then(|i| u32::try_from(i).ok())
        {
            index.remove(position);
            write_reporters(&env, &index);
        }
        bump_instance(&env);
        ReporterRevoked { reporter }.publish(&env);
    }

    /// How many reporters are currently active: every indexed address whose
    /// `Reporter(Address)` entry is still present. `revoke` drops the address
    /// from the index in the same transaction, so the two normally agree;
    /// checking the entry as well keeps the answer tied to the registrations
    /// if an index entry ever outlives its reporter.
    pub fn reporter_count(env: Env) -> u32 {
        let mut count = 0u32;
        for reporter in reporters(&env).iter() {
            if env
                .storage()
                .persistent()
                .has(&DataKey::Reporter(reporter))
            {
                count += 1;
            }
        }
        count
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

    /// Return the role of `reporter`, or `None` if not registered.
    ///
    /// # Arguments
    /// * `reporter` - the address to query

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

    /// Return the full reporter record for `reporter`, or `None` if not registered.
    ///
    /// # Arguments
    /// * `reporter` - the address to query
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
    /// Set or update the Unix-timestamp after which `reporter` is considered
    /// inactive. The comparison in `is_reporter_active` is always against
    /// `env.ledger().timestamp()` (not `ledger().sequence()`).
    pub fn set_expiry(env: Env, reporter: Address, expires_at: u64) {
        admin(&env).require_auth();
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Reporter(reporter.clone()))
        {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        let key = DataKey::Expiry(reporter.clone());
        env.storage().persistent().set(&key, &expires_at);
    /// Transfer admin rights to `new_admin`. Requires both current and new
    /// admin to authorise.
    ///
    /// # Arguments
    /// * `new_admin` - the address that will become administrator
    ///
    /// # Events
    /// * [`AdminTransferred`] on success
    /// The registry-level cap on how many reporters one asset may hold. The
    /// coverage ledger reads it when a reporter posts for an asset for the
    /// first time.
    pub fn max_reporters(env: Env) -> u32 {
        configured_max_reporters(&env)
    }

    /// Set the per-asset reporter cap. Zero would stop every asset taking a
    /// new reporter, so it is refused; one is the tightest useful setting.
    pub fn set_max_reporters(env: Env, max: u32) {
        admin(&env).require_auth();
        if max == 0 {
            panic_with_error!(&env, RegistryError::InvalidMaxReporters);
        }
        env.storage().instance().set(&DataKey::MaxReporters, &max);
        bump_instance(&env);
        MaxReportersSet { max }.publish(&env);
    /// Set the caller reporter's optional human-readable display name.
    pub fn set_reporter_name(env: Env, reporter: Address, name: Symbol) {
        reporter.require_auth();
        let reporter_key = DataKey::Reporter(reporter.clone());
        if !env.storage().persistent().has(&reporter_key) {
            panic_with_error!(&env, RegistryError::ReporterNotFound);
        }
        // Soroban symbols are limited to 32 bytes, which bounds this name at
        // 32 single-byte characters before the contract receives it.
        env.storage().persistent().extend_ttl(
            &reporter_key,
            PERSISTENT_THRESHOLD,
            PERSISTENT_EXTEND_TO,
        );
        let key = DataKey::ReporterDisplayName(reporter.clone());
        env.storage().persistent().set(&key, &name);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        bump_instance(&env);
        ReporterExpirySet {
            reporter,
            expires_at,
        }
        .publish(&env);
    }

    /// Returns `true` when `reporter` exists in the registry and either has no
    /// expiry or its expiry has not yet been reached on the current ledger
    /// timestamp. The expiry is compared against `env.ledger().timestamp()`
    /// (a Unix second-resolution clock), not against `ledger().sequence()`.
    pub fn is_reporter_active(env: Env, reporter: Address) -> bool {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Reporter(reporter.clone()))
        {
            return false;
        }
        let key = DataKey::Expiry(reporter);
        match env.storage().persistent().get::<_, u64>(&key) {
            None => true,
            Some(expires_at) => env.ledger().timestamp() < expires_at,
        }
    }

    pub fn get_reporter_name(env: Env, reporter: Address) -> Option<Symbol> {
        let key = DataKey::ReporterDisplayName(reporter);
        let name = env.storage().persistent().get::<_, Symbol>(&key);
        if name.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        }
        name
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
    ///
    /// # Arguments
    /// * `wasm_hash` - hash of the new wasm to install
    ///
    /// # Events
    /// * [`UpgradeProposed`] on success
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
    ///
    /// # Errors
    /// * [`RegistryError::UpgradeNotProposed`] when no upgrade has been proposed
    /// * [`RegistryError::UpgradeDelayNotElapsed`] when the delay has not yet passed
    ///
    /// # Events
    /// * [`UpgradeExecuted`] on success
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
    ///
    /// # Errors
    /// * [`RegistryError::UpgradeNotProposed`] when no upgrade has been proposed
    ///
    /// # Events
    /// * [`UpgradeCancelled`] on success
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

    /// Return the pending upgrade, if any.
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
    /// Assign an explicit access tier to `assignee`. Requires admin auth.
    ///
    /// An explicit tier overrides the implicit `Reporter` tier that any address
    /// registered via `set_reporter` would otherwise have. Existing reporter
    /// records are unaffected.
    ///
    /// # Arguments
    /// * `assignee` - the address receiving the tier
    /// * `tier` - the `RegistryRole` tier to assign
    ///
    /// # Events
    /// * [`RoleAssigned`] on success
    pub fn assign_role(env: Env, assignee: Address, tier: RegistryRole) {
        admin(&env).require_auth();
        let key = DataKey::AccessRole(assignee.clone());
        env.storage().persistent().set(&key, &tier);
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
        RoleAssigned { assignee, tier }.publish(&env);
    }

    /// Remove the explicit access tier of `assignee`. Requires admin auth.
    ///
    /// If `assignee` is also a registered reporter, they revert to the implicit
    /// `Reporter` tier returned by `access_tier`. If not registered, their tier
    /// becomes `None`.
    ///
    /// This is a no-op when `assignee` has no explicit tier.
    ///
    /// # Arguments
    /// * `assignee` - the address whose explicit tier is removed
    ///
    /// # Events
    /// * [`RoleRevoked`] on success
    pub fn revoke_role(env: Env, assignee: Address) {
        admin(&env).require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey::AccessRole(assignee.clone()));
        bump_instance(&env);
        RoleRevoked { assignee }.publish(&env);
    }

    /// Return the access tier of `address`.
    ///
    /// An explicit tier set via `assign_role` takes precedence. If none is set,
    /// any address registered via `set_reporter` is treated as `Reporter` tier.
    /// Returns `None` for addresses with neither.
    ///
    /// # Arguments
    /// * `address` - the address to query
    pub fn access_tier(env: Env, address: Address) -> Option<RegistryRole> {
        let key = DataKey::AccessRole(address.clone());
        if let Some(tier) = env
            .storage()
            .persistent()
            .get::<_, RegistryRole>(&key)
        {
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
            return Some(tier);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Reporter(address))
        {
            return Some(RegistryRole::Reporter);
        }
        None

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
