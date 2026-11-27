//! Coverage ledger: records circulating-supply snapshots and reserve reports
//! for Stellar-issued assets, and answers "is this asset covered?" for any
//! contract that asks.
#![no_std]

use plimsoll_types::{
    coverage_bps as compute_bps, AssetRecord, Coverage, CoverageStatus, CoveredInfo,
    PendingUpgrade, RegistryClient, ReserveReport, Role, StaleInfo, SupplySnapshot,
    SupplyTokenClient, Tier, DAY_IN_LEDGERS, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    MAX_PLAUSIBLE_BPS, MIN_PLAUSIBLE_BPS, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD,
    SECONDS_PER_LEDGER, UPGRADE_DELAY_LEDGERS,
    coverage_bps, AssetRecord, Coverage, CoverageSnapshot, PendingUpgrade, RegistryClient,
    attestation::encode_coverage_attestation, coverage_bps, AssetRecord, Coverage, CoverageStatus,
    CoveredInfo, PendingUpgrade, RegistryClient, ReserveReport, Role, StaleInfo, SupplySnapshot,
    SupplyTokenClient, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, MAX_PLAUSIBLE_BPS,
    MIN_PLAUSIBLE_BPS, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, SECONDS_PER_LEDGER,
    UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contractmeta, contracttype,
    panic_with_error, token, Address, BytesN, ContractExecutable, Env, Executable, String, Vec,
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, Bytes, BytesN, ContractExecutable, Env, Executable, String, Vec,
    coverage_bps, AssetRecord, Coverage, CoverageStatus, PendingUpgrade, RegistryClient,
    coverage_bps, AssetRecord, Coverage, CoverageSnapshot, PendingUpgrade, RegistryClient,
    ReserveReport, Role, SupplySnapshot, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    MAX_PLAUSIBLE_BPS, MIN_PLAUSIBLE_BPS, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD,
    UPGRADE_DELAY_LEDGERS,
    coverage_bps, AssetRecord, Coverage, CoverageStatus, CoveredInfo, PendingUpgrade,
    RegistryClient, ReserveReport, Role, StaleInfo, SupplySnapshot, SupplyTokenClient, Tier,
    INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, MAX_PLAUSIBLE_BPS, MIN_PLAUSIBLE_BPS,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, SECONDS_PER_LEDGER, UPGRADE_DELAY_LEDGERS,};
    coverage_bps, AssetRecord, Coverage, CoverageStatus, PendingUpgrade, RegistryClient,
    ReserveReport, Role, SupplySnapshot, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD,
    MAX_PLAUSIBLE_BPS, MIN_PLAUSIBLE_BPS, PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD,
    UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, BytesN, ContractExecutable, Env, Executable, String, Symbol, Vec,
    contract, contracterror, contractevent, contractimpl, contractmeta, contracttype,
    panic_with_error, token, Address, BytesN, ContractExecutable, Env, Executable, String, Vec,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, Bytes, BytesN, ContractExecutable, Env, Executable, String, Vec,
};

const MAX_URI_LEN: u32 = 256;
/// Longest SAC name: 12-char code + ':' + 56-char G-address.
const MAX_SAC_NAME_LEN: usize = 69;
const STRKEY_LEN: usize = 56;
const MAX_CODE_LEN: usize = 12;

/// Most reporters whose figures one asset keeps at one tier. Bounds what
/// `coverage` reads, and so what every `is_covered` caller pays.
pub const MAX_REPORTERS_PER_TIER: u32 = 4;
/// At Stellar's usual five-second ledger cadence, bounds supply snapshots to
/// approximately one hour of age.
pub const MAX_SUPPLY_LEDGER_AGE: u32 = 720;

/// Most asset-reporter pairs one `asset_reporter_pairs` call returns.
pub const MAX_PAIRS_PER_CALL: u32 = 50;

/// Maximum number of live reports returned by [`CoverageLedger::get_all_reports`].
pub const MAX_ALL_REPORTS: u32 = 20;

/// Maximum report age in seconds (90 days, matching the persistent TTL window)
/// for [`CoverageLedger::get_all_reports`] to treat a report as non-expired.
pub const MAX_REPORT_AGE_SECS: u64 = 90 * 86_400;

/// Maximum coverage snapshots kept per asset in the history ring buffer.
pub const HISTORY_CAPACITY: u32 = 10;

/// Largest N accepted by `get_recent_reports`.
pub const MAX_RECENT_REPORTS: u32 = 50;

/// Default number of ledgers a disputed report is frozen before it is
/// considered delisted. One day at 5-second close time.
pub const DEFAULT_DISPUTE_HOLD_LEDGERS: u32 = DAY_IN_LEDGERS;

/// A single entry in the cross-asset recent-reports index.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RecentReport {
    pub sac: Address,
    pub report: ReserveReport,
}

/// An active dispute on a reserve report identified by `(sac, as_of)`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeRecord {
    pub disputer: Address,
    pub hold_until_ledger: u32,
}

/// Contract version. Increment on every behavioural change.
const VERSION: u32 = 1;

// Contract metadata for `stellar contract info meta`
contractmeta!(key = "description", val = "Plimsoll Coverage Ledger");
contractmeta!(
    key = "repository",
    val = "https://github.com/plimsoll-protocol/plimsoll-contracts"
);

/// Largest N accepted by `get_recent_reports`.
pub const MAX_RECENT_REPORTS: u32 = 50;

/// Default number of ledgers a disputed report is frozen before it is
/// considered delisted. One day at 5-second close time.
pub const DEFAULT_DISPUTE_HOLD_LEDGERS: u32 = DAY_IN_LEDGERS;

/// A single entry in the cross-asset recent-reports index.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RecentReport {
    pub sac: Address,
    pub report: ReserveReport,
}

/// An active dispute on a reserve report identified by `(sac, as_of)`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeRecord {
    pub disputer: Address,
    pub hold_until_ledger: u32,
}

/// Contract version. Increment on every behavioural change.
const VERSION: u32 = 1;

// Contract metadata for `stellar contract info meta`
contractmeta!(key = "description", val = "Plimsoll Coverage Ledger");
contractmeta!(
    key = "repository",
    val = "https://github.com/plimsoll-protocol/plimsoll-contracts"
);

/// Maximum coverage snapshots kept per asset in the history ring buffer.
pub const HISTORY_CAPACITY: u32 = 10;
/// Shortest `doc_uri` `post_reserve` accepts until an admin sets
/// [`CoverageLedger::set_min_doc_uri_len`]. Ten characters rejects placeholder
/// values that carry no information (`n/a`, `TODO`, `x`) without refusing a
/// real link such as `ipfs://Qm...`.
pub const DEFAULT_MIN_DOC_URI_LEN: u32 = 10;

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Registry,
    PendingUpgrade,
    TtlGrace,
    Asset(Address),
    Supply(Address),
    /// v0.1 layout, one report per tier. Read until migrated, never written.
    Report(Address, Tier),
    /// The latest report from one reporter at one tier: (sac, tier, reporter).
    ReportBy(Address, Tier, Address),
    /// Reporters with a `ReportBy` entry at (sac, tier), in first-post order.
    Reporters(Address, Tier),
    /// The account the asset's issuer appointed to post `IssuerSigned` for it.
    IssuerDelegate(Address),
    /// The N most recently posted reports across all assets (bounded at MAX_RECENT_REPORTS).
    RecentReports,
    /// An active dispute on a report: keyed by (sac, as_of).
    Dispute(Address, u64),
    /// Configurable freeze period (in ledgers) for disputed reports.
    DisputeHold,
    /// One slot in the coverage-history ring buffer for `sac`.
    HistorySlot(Address, u32),
    /// Next write index for `sac`'s history ring buffer (wraps on overflow).
    HistoryHead(Address),
    /// Number of entries in `sac`'s ring buffer, capped at `HISTORY_CAPACITY`.
    HistoryCount(Address),
    /// Incident switch. Absent means not paused.
    Paused,
    /// The SEP-41 token whose `total_supply()` replaces the posted snapshot.
    NativeSupply(Address),
    /// Maps a deprecated asset SAC to its successor SAC (at most one hop).
    Alias(Address),}
    Alias(Address),
    /// Index of all listed asset SACs, in listing order.
    Assets,
    SupplyPostedLedger(Address),
    ReportPostedLedger(Address, Tier, Address),
    /// Shortest `doc_uri` `post_reserve` accepts. Absent means
    /// `DEFAULT_MIN_DOC_URI_LEN`.
    MinDocUriLen,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LedgerError {
    NotAStellarAsset = 1,
    AlreadyListed = 2,
    AssetNotListed = 3,
    NotSupplyPoster = 4,
    NotReporter = 5,
    InvalidAmount = 6,
    FutureLedger = 7,
    StaleLedger = 8,
    FutureTimestamp = 9,
    StaleReport = 10,
    InvalidUri = 11,
    ImplausibleAmount = 12,
    TooManyReporters = 13,
    UpgradeNotProposed = 14,
    UpgradeDelayNotElapsed = 15,
    NotAWatcher = 16,
    AlreadyDisputed = 17,
    DisputeNotFound = 18,
    SelfAlias = 16,
    Paused = 17,}
    InsufficientStake = 17,
    Paused = 18,
    Paused = 17,
    LedgerTooOld = 18,
    NoReportToRetract = 19,
    TooManyReportersPerAsset = 20,
    UriTooShort = 20,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetListed {
    #[topic]
    pub sac: Address,
    pub code: String,
    pub issuer: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetDelisted {
    #[topic]
    pub sac: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupplyPosted {
    #[topic]
    pub sac: Address,
    pub amount: i128,
    pub ledger: u32,
    pub breakdown_hash: BytesN<32>,
    pub poster: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReservePosted {
    #[topic]
    pub sac: Address,
    #[topic]
    pub tier: Tier,
    pub amount: i128,
    pub as_of: u64,
    pub reporter: Address,
    pub doc_hash: BytesN<32>,
    pub doc_uri: String,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveRetracted {
    #[topic]
    pub sac: Address,
    #[topic]
    pub tier: Tier,
    pub reporter: Address,
    pub as_of: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferred {
    #[topic]
    pub new_admin: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuerDelegateSet {
    #[topic]
    pub sac: Address,
    pub delegate: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuerDelegateCleared {
    #[topic]
    pub sac: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSupplyEnabled {
    #[topic]
    pub sac: Address,
    pub token: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paused {
    pub by: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unpaused {
    pub by: Address,
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
pub struct ReportDisputed {
    #[topic]
    pub sac: Address,
    #[topic]
    pub as_of: u64,
    pub disputer: Address,
    pub hold_until_ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeResolved {
    #[topic]
    pub sac: Address,
    #[topic]
    pub as_of: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TtlGraceUpdated {
    pub grace_ledgers: u32,
pub struct AssetAliased {
    #[topic]
    pub old: Address,
    #[topic]
    pub new: Address,
pub struct TtlGraceUpdated {
    pub grace_ledgers: u32,
}

#[contract]
pub struct CoverageLedger;

fn admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap_or_else(|| unreachable!("admin is set in the constructor"))
}

fn registry(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Registry)
        .unwrap_or_else(|| unreachable!("registry is set in the constructor"))
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
}

fn pending(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

fn dispute_hold_ledgers(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::DisputeHold)
        .unwrap_or(DEFAULT_DISPUTE_HOLD_LEDGERS)
}

/// Returns `true` if `report` is under an active dispute or its hold has
/// expired (making it effectively delisted). The check is against the current
/// ledger sequence number.
fn is_disputed(env: &Env, sac: &Address, report: &ReserveReport) -> bool {
    let key = DataKey::Dispute(sac.clone(), report.as_of);
    match read_persistent::<DisputeRecord>(env, &key) {
        None => false,
        Some(_) => true,
    }
}

fn paused_flag(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Stop a write at the door while the incident switch is on. Checked after
/// authentication and before every other write error, so the pause is the
/// reason callers see.
fn require_not_paused(env: &Env) {
    if paused_flag(env) {
        panic_with_error!(env, LedgerError::Paused);
    }
}

fn read_persistent<V: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(
    env: &Env,
    key: &DataKey,
) -> Option<V> {
    let value = env.storage().persistent().get::<_, V>(key);
    if value.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
    }
    value
}

fn write_persistent<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(
    env: &Env,
    key: &DataKey,
    value: &V,
) {
    env.storage().persistent().set(key, value);
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
}

/// Read a persistent entry without extending its TTL. Views that promise to
/// write nothing use this instead of [`read_persistent`].
fn peek<V: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(
    env: &Env,
    key: &DataKey,
) -> Option<V> {
    env.storage().persistent().get::<_, V>(key)
}

/// [`read_persistent`] or [`peek`], chosen per call, so one implementation of
/// a query serves both the TTL-maintaining and the side-effect-free paths.
fn read_or_peek<V: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(
    env: &Env,
    key: &DataKey,
    peek_only: bool,
) -> Option<V> {
    if peek_only {
        peek(env, key)
    } else {
        read_persistent(env, key)
    }
}

fn require_listed(env: &Env, sac: &Address) -> AssetRecord {
    match read_persistent::<AssetRecord>(env, &DataKey::Asset(sac.clone())) {
        Some(asset) => asset,
        None => panic_with_error!(env, LedgerError::AssetNotListed),
    }
}

/// Check that `sac` is listed without deserialising its `AssetRecord`.
fn ensure_listed(env: &Env, sac: &Address) {
    let key = DataKey::Asset(sac.clone());
    if !env.storage().persistent().has(&key) {
        panic_with_error!(env, LedgerError::AssetNotListed);
    }
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
}

/// Split a Stellar Asset Contract name (`CODE:GISSUER...`) into code and
/// issuer. Returns `None` for `native` or anything malformed.
fn parse_sac_name(env: &Env, name: &String) -> Option<(String, Address)> {
    let len = name.len() as usize;
    if !(STRKEY_LEN + 2..=MAX_SAC_NAME_LEN).contains(&len) {
        return None;
    }
    let mut buf = [0u8; MAX_SAC_NAME_LEN];
    name.copy_into_slice(&mut buf[..len]);
    let colon = buf[..len].iter().position(|b| *b == b':')?;
    if colon == 0 || colon > MAX_CODE_LEN || len - colon - 1 != STRKEY_LEN {
        return None;
    }
    let code = String::from_bytes(env, &buf[..colon]);
    let issuer = String::from_bytes(env, &buf[colon + 1..len]);
    Some((code, Address::from_string(&issuer)))
}

fn reporters(env: &Env, sac: &Address, tier: Tier) -> Vec<Address> {
    read_persistent(env, &DataKey::Reporters(sac.clone(), tier)).unwrap_or_else(|| Vec::new(env))
}

fn assets(env: &Env) -> Vec<Address> {
    read_persistent(env, &DataKey::Assets).unwrap_or_else(|| Vec::new(env))
}

/// Every stored report at `tier`: a not-yet-migrated v0.1 entry first, then
/// the indexed reporters' in first-post order.
fn reports_at(env: &Env, sac: &Address, tier: Tier) -> Vec<ReserveReport> {
    gather_reports_at(env, sac, tier, false)
}

/// [`reports_at`], reading through [`read_or_peek`] so a view can gather the
/// same candidates without extending any TTL.
fn gather_reports_at(
    env: &Env,
    sac: &Address,
    tier: Tier,
    peek_only: bool,
) -> Vec<ReserveReport> {
    let mut out = Vec::new(env);
    let legacy_key = DataKey::Report(sac.clone(), tier);
    if let Some(legacy) = read_or_peek::<ReserveReport>(env, &legacy_key, peek_only) {
        out.push_back(legacy);
    }
    // Read the index directly rather than through `reporters`, which always
    // extends its TTL.
    let index_key = DataKey::Reporters(sac.clone(), tier);
    let stored: Option<Vec<Address>> = read_or_peek(env, &index_key, peek_only);
    let index = stored.unwrap_or_else(|| Vec::new(env));
    for reporter in index.iter() {
        let report_key = DataKey::ReportBy(sac.clone(), tier, reporter);
        if let Some(report) = read_or_peek::<ReserveReport>(env, &report_key, peek_only) {
            out.push_back(report);
        }
    }
    out
}

/// Every stored report at `tier` whose current version was posted at or
/// before `as_of_ledger`, with the TTL behaviour chosen by `peek_only`.
fn gather_reports_at_ledger(
    env: &Env,
    sac: &Address,
    tier: Tier,
    as_of_ledger: u32,
    peek_only: bool,
) -> Vec<ReserveReport> {
    let mut out = Vec::new(env);
    for report in gather_reports_at(env, sac, tier, peek_only).iter() {
        let key = DataKey::ReportPostedLedger(sac.clone(), tier, report.reporter.clone());
        let posted_ledger = match read_or_peek::<u32>(env, &key, peek_only) {
            Some(ledger) => ledger,
            None => env.ledger().sequence(),
        };
        if posted_ledger <= as_of_ledger {
            out.push_back(report);
        }
    }
    out
}

/// Move a v0.1 `Report(sac, tier)` entry into the per-reporter layout. Runs
/// before every write at a tier, so a legacy key only ever exists while that
/// tier's index is empty: the legacy reporter becomes its first entry, and no
/// report is ever read twice.
fn migrate_legacy(env: &Env, sac: &Address, tier: Tier) {
    let legacy_key = DataKey::Report(sac.clone(), tier);
    if let Some(legacy) = env
        .storage()
        .persistent()
        .get::<_, ReserveReport>(&legacy_key)
    {
        let reporter = legacy.reporter.clone();
        write_persistent(
            env,
            &DataKey::ReportBy(sac.clone(), tier, reporter.clone()),
            &legacy,
        );
        write_persistent(
            env,
            &DataKey::Reporters(sac.clone(), tier),
            &Vec::from_array(env, [reporter]),
        );
        env.storage().persistent().remove(&legacy_key);
    }
}

/// Whether `reporter` may sign at `IssuerSigned` for `sac`: the asset's issuer
/// account, or the delegate it currently appoints.
fn speaks_for_issuer(env: &Env, sac: &Address, issuer: &Address, reporter: &Address) -> bool {
    reporter == issuer
        || read_persistent::<Address>(env, &DataKey::IssuerDelegate(sac.clone()))
            .is_some_and(|delegate| delegate == *reporter)
}

/// The supply figure coverage is judged against, from either the native source
/// or the posted snapshot.
struct SupplyFigure {
    amount: i128,
    ledger: u32,
    timestamp: u64,
}

/// The token opted in to as this asset's native supply source, if any.
fn native_token(env: &Env, sac: &Address) -> Option<Address> {
    read_persistent(env, &DataKey::NativeSupply(sac.clone()))
}

/// The native figure, or `None` when the opt-in is absent or the call fails.
/// A failure is not an error: callers fall back to the posted snapshot.
fn native_supply(env: &Env, sac: &Address) -> Option<SupplyFigure> {
    let token = native_token(env, sac)?;
    match SupplyTokenClient::new(env, &token).try_total_supply() {
        Ok(Ok(amount)) if amount >= 0 => Some(SupplyFigure {
            amount,
            ledger: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        }),
        _ => None,
    }
}

/// Native supply when opted in and answering, otherwise the latest snapshot.
fn effective_supply(env: &Env, sac: &Address) -> Option<SupplyFigure> {
    if let Some(native) = native_supply(env, sac) {
        return Some(native);
    }
    read_persistent::<SupplySnapshot>(env, &DataKey::Supply(sac.clone())).map(|snapshot| {
        SupplyFigure {
            amount: snapshot.amount,
            ledger: snapshot.ledger,
            timestamp: snapshot.timestamp,
        }
    })
}

fn posted_supply_at_ledger(
    env: &Env,
    sac: &Address,
    as_of_ledger: u32,
    peek_only: bool,
) -> Option<SupplyFigure> {
    let snapshot = read_or_peek::<SupplySnapshot>(env, &DataKey::Supply(sac.clone()), peek_only)?;
    let posted_key = DataKey::SupplyPostedLedger(sac.clone());
    let posted_ledger = match read_or_peek::<u32>(env, &posted_key, peek_only) {
        Some(ledger) => ledger,
        None => env.ledger().sequence(),
    };
    if posted_ledger > as_of_ledger {
        return None;
    }
    Some(SupplyFigure {
        amount: snapshot.amount,
        ledger: snapshot.ledger,
        timestamp: snapshot.timestamp,
    })
}

/// Whether a reporter at `tier` still counts: issuer-signed reports always
/// count, while auditor and transcriber reports require the matching role.
fn counts_for_tier(registry: &RegistryClient, tier: Tier, reporter: &Address) -> bool {
    let needed = match tier {
        Tier::IssuerSigned => return true,
        Tier::AuditorSigned => Role::Auditor,
        Tier::Transcribed => Role::Transcriber,
    };
    registry.role_of(reporter) == Some(needed)
/// The `Coverage` a supply figure and a report make together.
fn coverage_of(supply: &SupplyFigure, report: &ReserveReport) -> Coverage {
    Coverage {
        bps: coverage_bps(report.amount, supply.amount),
        supply: supply.amount,
        reserves: report.amount,
        tier: report.tier,
        supply_ledger: supply.ledger,
        supply_timestamp: supply.timestamp,
        report_as_of: report.as_of,
    }
}

/// The shortest `doc_uri` `post_reserve` accepts. `MinDocUriLen` is absent on
/// any deployment that has never set it, including a ledger upgraded from
/// v0.1, so the default lives in code rather than in the constructor.
fn configured_min_doc_uri_len(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::MinDocUriLen)
        .unwrap_or(DEFAULT_MIN_DOC_URI_LEN)
}

/// Whether a stored report still counts: its reporter must still be entitled
/// to its tier. An issuer-signed report counts while its reporter is the
/// issuer, which the asset record fixes, or the current delegate. A revoked
/// auditor or transcriber, one whose role changed, or a delegate the issuer
/// cleared or replaced, no longer speaks for the figures they posted.
fn counts(env: &Env, registry: &RegistryClient, sac: &Address, report: &ReserveReport) -> bool {
    let needed = match report.tier {
        Tier::IssuerSigned => {
            return read_persistent::<AssetRecord>(env, &DataKey::Asset(sac.clone()))
                .is_some_and(|asset| speaks_for_issuer(env, sac, &asset.issuer, &report.reporter))
        }
        Tier::AuditorSigned => Role::Auditor,
        Tier::Transcribed => Role::Transcriber,
    };
    registry.role_of(reporter) == Some(needed)
}

/// Whether a stored report still counts: its reporter must still be entitled
/// to its tier. An issuer-signed report counts while its reporter is the
/// issuer, which the asset record fixes, or the current delegate. A revoked
/// auditor or transcriber, one whose role changed, or a delegate the issuer
/// cleared or replaced, no longer speaks for the figures they posted.
fn counts(env: &Env, registry: &RegistryClient, sac: &Address, report: &ReserveReport) -> bool {
    let needed = match report.tier {
        Tier::IssuerSigned => {
            return read_persistent::<AssetRecord>(env, &DataKey::Asset(sac.clone()))
                .is_some_and(|asset| speaks_for_issuer(env, sac, &asset.issuer, &report.reporter))
        }
        Tier::AuditorSigned => Role::Auditor,
        Tier::Transcribed => Role::Transcriber,
    };
    registry.role_of(&report.reporter) == Some(needed)
/// to its tier. Issuer-signed reports always count; the issuer is fixed by the
/// asset record. A revoked auditor or transcriber, or one whose role changed,
/// no longer speaks for the figures they posted.
fn counts(registry: &RegistryClient, report: &ReserveReport) -> bool {
    counts_for_tier(registry, report.tier, &report.reporter)
}

/// The freshest report in `candidates` that still counts and is not disputed.
/// Candidates come in priority order (strongest tier first, then first-post
/// order), so on equal `as_of` the earlier one wins. The registry is asked
/// only about the report that would win, and again only if that one no longer
/// counts, so a fresh issuer-signed report costs no cross-contract call at all.
///
/// Two-phase per iteration: (1) a single O(n) index scan that finds the
/// globally freshest non-rejected candidate without touching the registry,
/// then (2) one `counts` call. If that report is revoked the index is marked
/// and the loop repeats. This avoids Vec::remove (O(n) element shift) and
/// keeps registry calls at most equal to the number of revoked reporters.
/// A bitmask tracks rejected positions; `candidates` is never mutated.
/// We have at most 3 tiers × (1 legacy + MAX_REPORTERS_PER_TIER) = 15
/// entries, so a u32 bitmask is always sufficient.
fn pick_counting(env: &Env, sac: &Address, candidates: Vec<ReserveReport>) -> Option<ReserveReport> {
fn pick_counting(
    env: &Env,
    sac: &Address,
    candidates: Vec<ReserveReport>,
) -> Option<ReserveReport> {
    let registry = RegistryClient::new(env, &registry(env));
    let n = candidates.len();
    let mut skip: u32 = 0;
    loop {
        // Phase 1: find the freshest non-skipped candidate (no registry call).
        let mut best_i: Option<u32> = None;
        for i in 0..n {
            if skip & (1u32 << i) != 0 {
                continue;
            }
            let r = candidates.get(i).unwrap_or_else(|| unreachable!());
            best_i = Some(match best_i {
                None => i,
                Some(bi) => {
                    let b = candidates.get(bi).unwrap_or_else(|| unreachable!());
                    if r.as_of > b.as_of || (r.as_of == b.as_of && r.tier > b.tier) {
                        i
                    } else {
                        bi
                    }
                }
            });
        }
        // Phase 2: check only the winner.
        let i = best_i?;
        let report = candidates.get(i).unwrap();
        if counts(env, &registry, sac, &report) {
        if counts(&registry, &report) && !is_disputed(env, sac, &report) {
        let report = candidates.get(i).unwrap_or_else(|| unreachable!());
        if counts(&registry, &report) {
        if counts(&registry, &report) && !is_disputed(env, sac, &report) {
            return Some(report);
        }
        skip |= 1u32 << i;
    }
}

/// The freshest counting report at `tier`.
fn freshest_at(env: &Env, sac: &Address, tier: Tier) -> Option<ReserveReport> {
    pick_counting(env, sac, reports_at(env, sac, tier))
}

fn best_report(env: &Env, sac: &Address, min_tier: Tier) -> Option<ReserveReport> {
    let mut candidates = Vec::new(env);
    for tier in Tier::ALL_DESC {
        if tier >= min_tier {
            candidates.append(&reports_at(env, sac, tier));
        }
    }
    pick_counting(env, sac, candidates)
}

/// The distinct reporters that hold a stored report for `sac`, strongest tier
/// first and first-post order within a tier. A v0.1 `Report(sac, tier)` entry
/// counts until its tier is migrated, because it is still a reporter of this
/// asset to anyone reading it.
fn asset_reporters(env: &Env, sac: &Address) -> Vec<Address> {
    let mut out = Vec::new(env);
    for tier in Tier::ALL_DESC {
        for reporter in reporters(env, sac, tier).iter() {
            if !out.contains(&reporter) {
                out.push_back(reporter);
            }
        }
        let legacy_key = DataKey::Report(sac.clone(), tier);
        if let Some(legacy) = read_persistent::<ReserveReport>(env, &legacy_key) {
            if !out.contains(&legacy.reporter) {
                out.push_back(legacy.reporter);
            }
        }
    }
    out
}

/// Drop tier-index entries, and their reports, whose reporter no longer counts
/// at every tier of `sac`. Called only when the asset is at the registry's
/// reporter cap, so a revoked or promoted reporter cannot hold a slot against a
/// working one.
fn reclaim_asset(env: &Env, registry: &RegistryClient, sac: &Address) {
    for tier in Tier::ALL_DESC {
        let index = reporters(env, sac, tier);
        let kept = reclaim_slots(env, registry, sac, tier, &index);
        if kept.len() != index.len() {
            write_persistent(env, &DataKey::Reporters(sac.clone(), tier), &kept);
        }
    }
fn best_report_at_ledger(
    env: &Env,
    sac: &Address,
    min_tier: Tier,
    as_of_ledger: u32,
) -> Option<ReserveReport> {
    best_report_as_of(env, sac, min_tier, as_of_ledger, false)
}

/// [`best_report_at_ledger`], with the TTL behaviour chosen by `peek_only`.
fn best_report_as_of(
    env: &Env,
    sac: &Address,
    min_tier: Tier,
    as_of_ledger: u32,
    peek_only: bool,
) -> Option<ReserveReport> {
    let mut candidates = Vec::new(env);
    for tier in Tier::ALL_DESC {
        if tier >= min_tier {
            let at_tier = gather_reports_at_ledger(env, sac, tier, as_of_ledger, peek_only);
            candidates.append(&at_tier);
        }
    }
    pick_counting(env, candidates)
}

/// The distinct reporters that hold a stored report for `sac`, strongest tier
/// first and first-post order within a tier. A v0.1 `Report(sac, tier)` entry
/// counts until its tier is migrated, because it is still a reporter of this
/// asset to anyone reading it.
fn asset_reporters(env: &Env, sac: &Address) -> Vec<Address> {
    let mut out = Vec::new(env);
    for tier in Tier::ALL_DESC {
        for reporter in reporters(env, sac, tier).iter() {
            if !out.contains(&reporter) {
                out.push_back(reporter);
            }
        }
        let legacy_key = DataKey::Report(sac.clone(), tier);
        if let Some(legacy) = read_persistent::<ReserveReport>(env, &legacy_key) {
            if !out.contains(&legacy.reporter) {
                out.push_back(legacy.reporter);
            }
        }
    }
    out
}

/// Drop tier-index entries, and their reports, whose reporter no longer counts
/// at every tier of `sac`. Called only when the asset is at the registry's
/// reporter cap, so a revoked or promoted reporter cannot hold a slot against a
/// working one.
fn reclaim_asset(env: &Env, registry: &RegistryClient, sac: &Address) {
    for tier in Tier::ALL_DESC {
        let index = reporters(env, sac, tier);
        let kept = reclaim_slots(env, registry, sac, tier, &index);
        if kept.len() != index.len() {
            write_persistent(env, &DataKey::Reporters(sac.clone(), tier), &kept);
        }
    }
}

/// Drop index entries, and their reports, whose reporter no longer counts.
/// Called only when a tier is full, so a revoked reporter cannot hold a slot
/// against a working one.
fn reclaim_slots(
    env: &Env,
    registry: &RegistryClient,
    sac: &Address,
    tier: Tier,
    index: &Vec<Address>,
) -> Vec<Address> {
    let mut kept = Vec::new(env);
    for reporter in index.iter() {
        let key = DataKey::ReportBy(sac.clone(), tier, reporter.clone());
        let keep = env
            .storage()
            .persistent()
            .get::<_, ReserveReport>(&key)
            .is_some_and(|report| counts(env, registry, sac, &report));
        let keep =
            env.storage().persistent().has(&key) && counts_for_tier(registry, tier, &reporter);
        if keep {
            kept.push_back(reporter);
        } else {
            env.storage().persistent().remove(&key);
            env.storage()
                .persistent()
                .remove(&DataKey::ReportPostedLedger(sac.clone(), tier, reporter));
        }
    }
    kept
}

/// Prepend `entry` to the cross-asset recent-reports ring (capped at
/// `MAX_RECENT_REPORTS`). Called on every successful `post_reserve`.
fn push_recent_report(env: &Env, entry: RecentReport) {
    let key = DataKey::RecentReports;
    let existing: Vec<RecentReport> = read_persistent(env, &key).unwrap_or_else(|| Vec::new(env));
    let mut updated = Vec::new(env);
    updated.push_back(entry);
    for item in existing.iter() {
        if updated.len() < MAX_RECENT_REPORTS {
            updated.push_back(item);
        }
    }
    write_persistent(env, &key, &updated);
/// Append `snapshot` to the ring buffer for `sac`. The oldest entry is
/// overwritten once the buffer reaches `HISTORY_CAPACITY`.
fn history_push(env: &Env, sac: &Address, snapshot: CoverageSnapshot) {
    let head_key = DataKey::HistoryHead(sac.clone());
    let count_key = DataKey::HistoryCount(sac.clone());
    let head: u32 = read_persistent::<u32>(env, &head_key).unwrap_or(0);
    let count: u32 = read_persistent::<u32>(env, &count_key).unwrap_or(0);
    let slot = head % HISTORY_CAPACITY;
    write_persistent(env, &DataKey::HistorySlot(sac.clone(), slot), &snapshot);
    write_persistent(env, &head_key, &head.wrapping_add(1));
    if count < HISTORY_CAPACITY {
        write_persistent(env, &count_key, &(count + 1));
    }
fn validate_post_reserve(
    env: &Env,
    reporter: &Address,
    sac: &Address,
    amount: i128,
    as_of: u64,
    doc_uri: &String,
) -> Result<Tier, LedgerError> {
    if paused_flag(env) {
        return Err(LedgerError::Paused);
    }
    let asset = match read_persistent::<AssetRecord>(env, &DataKey::Asset(sac.clone())) {
        Some(asset) => asset,
        None => return Err(LedgerError::AssetNotListed),
    };
    let tier = if reporter == &asset.issuer {
        Tier::IssuerSigned
    } else {
        match RegistryClient::new(env, &registry(env)).role_of(reporter) {
            Some(Role::Auditor) => Tier::AuditorSigned,
            Some(Role::Transcriber) => Tier::Transcribed,
            _ => return Err(LedgerError::NotReporter),
        }
    };
    if amount < 0 {
        return Err(LedgerError::InvalidAmount);
    }
    if doc_uri.is_empty() || doc_uri.len() > MAX_URI_LEN {
        return Err(LedgerError::InvalidUri);
    }
    if as_of > env.ledger().timestamp() {
        return Err(LedgerError::FutureTimestamp);
    }
    if amount != 0 {
        if let Some(supply) = read_persistent::<SupplySnapshot>(env, &DataKey::Supply(sac.clone()))
        {
            if supply.amount > 0 {
                let bps = coverage_bps(amount, supply.amount);
                if !(MIN_PLAUSIBLE_BPS..=MAX_PLAUSIBLE_BPS).contains(&bps) {
                    return Err(LedgerError::ImplausibleAmount);
/// One page of the (asset, reporter) pairs in bootstrap order: assets in
/// listing order, tiers strongest first within an asset, first-post order
/// within a tier, each reporter once per asset. `start` is the index of the
/// first pair and `page` is the number of pairs to return.
fn pair_page(env: &Env, start: u32, page: u32) -> Vec<(Address, Address)> {
    let mut out = Vec::new(env);
    if page == 0 {
        return out;
    }
    let mut index = 0u32;
    for asset in assets(env).iter() {
        let mut reported = Vec::new(env);
        for tier in Tier::ALL_DESC {
            for reporter in reporters(env, &asset, tier).iter() {
                if reported.contains(&reporter) {
                    continue;
                }
                reported.push_back(reporter.clone());
                if index < start {
                    index += 1;
                    continue;
                }
                out.push_back((asset.clone(), reporter));
                if out.len() == page {
                    return out;
                }
            }
        }
    }

    let legacy = read_persistent::<ReserveReport>(env, &DataKey::Report(sac.clone(), tier));
    let report_key = DataKey::ReportBy(sac.clone(), tier, reporter.clone());
    if let Some(previous) = read_persistent::<ReserveReport>(env, &report_key) {
        if as_of <= previous.as_of {
            return Err(LedgerError::StaleReport);
        }
    } else if let Some(previous) = legacy.as_ref() {
        if &previous.reporter == reporter && as_of <= previous.as_of {
            return Err(LedgerError::StaleReport);
        }
    }

    let mut index = reporters(env, sac, tier);
    if let Some(previous) = legacy.as_ref() {
        index = Vec::from_array(env, [previous.reporter.clone()]);
    }
    if !index.contains(reporter) && index.len() >= MAX_REPORTERS_PER_TIER {
        let registry = RegistryClient::new(env, &registry(env));
        let mut active_reporters = 0u32;
        for indexed_reporter in index.iter() {
            let report = match legacy.as_ref() {
                Some(previous) if previous.reporter == indexed_reporter => Some(previous.clone()),
                _ => read_persistent::<ReserveReport>(
                    env,
                    &DataKey::ReportBy(sac.clone(), tier, indexed_reporter.clone()),
                ),
            };
            if report
                .as_ref()
                .is_some_and(|report| counts(&registry, report))
            {
                active_reporters = match active_reporters.checked_add(1) {
                    Some(count) => count,
                    None => return Err(LedgerError::TooManyReporters),
                };
            }
        }
        if active_reporters >= MAX_REPORTERS_PER_TIER {
            return Err(LedgerError::TooManyReporters);
        }
    }
    Ok(tier)
    out
}

/// The freshest figure `reporter` has for `sac` across every tier: latest
/// `as_of`, ties to the stronger tier — the rule `coverage` uses too.
fn latest_for(env: &Env, sac: &Address, reporter: &Address) -> Option<ReserveReport> {
    let mut best: Option<ReserveReport> = None;
    for tier in Tier::ALL_DESC {
        for report in reports_at(env, sac, tier).iter() {
            if &report.reporter != reporter {
                continue;
            }
            best = Some(match best {
                None => report,
                Some(current) => {
                    if report.as_of > current.as_of
                        || (report.as_of == current.as_of && report.tier > current.tier)
                    {
                        report
                    } else {
                        current
                    }
                }
            });
        }
    }
    best
}

#[contractimpl]
impl CoverageLedger {
    /// Initialises the ledger with `admin` as administrator and `registry` as
    /// the reporter-registry used to validate reporter roles.
    ///
    /// # Arguments
    /// * `admin` - address that controls upgrades and admin transfer
    /// * `registry` - address of the reporter-registry contract
    pub fn __constructor(env: Env, admin: Address, registry: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Registry, &registry);
        bump_instance(&env);
    }

    /// Returns the current administrator address.
    pub fn admin(env: Env) -> Address {
        admin(&env)
    }

    /// Returns the reporter-registry address used to validate roles.
    pub fn registry(env: Env) -> Address {
        registry(&env)
    }

    /// The total number of listed assets.
    pub fn asset_count(env: Env) -> u32 {
        assets(&env).len()
    }

    /// List a Stellar-issued asset by its Stellar Asset Contract address.
    ///
    /// Permissionless: the code and issuer are read from the SAC itself.
    ///
    /// # Arguments
    /// * `sac` - address of the Stellar Asset Contract to list
    ///
    /// # Errors
    /// * [`LedgerError::NotAStellarAsset`] when `sac` is not a Stellar Asset Contract or its name is malformed
    /// * [`LedgerError::AlreadyListed`] when `sac` is already listed
    ///
    /// # Events
    /// * [`AssetListed`] on success
    pub fn list_asset(env: Env, sac: Address) -> AssetRecord {
    /// The optional display_name is set by the admin and returned by
    /// `asset_metadata`.
    pub fn list_asset(env: Env, sac: Address, display_name: Option<Symbol>) -> AssetRecord {
        if sac.executable() != Some(Executable::StellarAsset) {
            panic_with_error!(&env, LedgerError::NotAStellarAsset);
        }
        let key = DataKey::Asset(sac.clone());
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, LedgerError::AlreadyListed);
        }
        let name = token::TokenClient::new(&env, &sac).name();
        let (code, issuer) = match parse_sac_name(&env, &name) {
            Some(parts) => parts,
            None => panic_with_error!(&env, LedgerError::NotAStellarAsset),
        };
        let record = AssetRecord {
            sac: sac.clone(),
            code: code.clone(),
            issuer: issuer.clone(),
            listed_at: env.ledger().timestamp(),
            display_name,
        };
        write_persistent(&env, &key, &record);
        let mut index = assets(&env);
        index.push_back(sac.clone());
        write_persistent(&env, &DataKey::Assets, &index);
        bump_instance(&env);
        AssetListed { sac, code, issuer }.publish(&env);
        record
    }

    /// Remove multiple listed assets in a single transaction.
    /// Non-listed assets are silently skipped; an `AssetDelisted` event is
    /// emitted for each asset actually removed.
    pub fn batch_delist(env: Env, assets_to_remove: Vec<Address>) {
        admin(&env).require_auth();
        let mut removed_any = false;
        for sac in assets_to_remove.iter() {
            let asset_key = DataKey::Asset(sac.clone());
            if !env.storage().persistent().has(&asset_key) {
                continue;
            }
            removed_any = true;
            env.storage().persistent().remove(&asset_key);
            env.storage()
                .persistent()
                .remove(&DataKey::Supply(sac.clone()));
            env.storage()
                .persistent()
                .remove(&DataKey::NativeSupply(sac.clone()));
            env.storage()
                .persistent()
                .remove(&DataKey::Alias(sac.clone()));
            for tier in Tier::ALL_DESC {
                env.storage()
                    .persistent()
                    .remove(&DataKey::Report(sac.clone(), tier));
                let reporters_key = DataKey::Reporters(sac.clone(), tier);
                if let Some(reporters) = env
                    .storage()
                    .persistent()
                    .get::<_, Vec<Address>>(&reporters_key)
                {
                    for reporter in reporters.iter() {
                        env.storage().persistent().remove(&DataKey::ReportBy(
                            sac.clone(),
                            tier,
                            reporter,
                        ));
                    }
                    env.storage().persistent().remove(&reporters_key);
                }
            }
            AssetDelisted { sac }.publish(&env);
        }
        if removed_any {
            let current = assets(&env);
            let mut remaining = Vec::new(&env);
            for sac in current.iter() {
                if env.storage().persistent().has(&DataKey::Asset(sac.clone())) {
                    remaining.push_back(sac);
                }
            }
            write_persistent(&env, &DataKey::Assets, &remaining);
            bump_instance(&env);
        }
    }

    /// Record the circulating supply of `sac` as computed at `ledger`.
    ///
    /// # Arguments
    /// * `poster` - registered supply-poster; must authorise this call
    /// * `sac` - the listed asset
    /// * `amount` - circulating supply in the asset's smallest unit; must be non-negative
    /// * `ledger` - ledger sequence at which this supply was computed; must not be in the future or stale
    /// * `breakdown_hash` - SHA-256 of the off-chain supply breakdown document
    ///
    /// # Errors
    /// * [`LedgerError::NotSupplyPoster`] when `poster` is not a registered supply-poster
    /// * [`LedgerError::AssetNotListed`] when `sac` has not been listed
    /// * [`LedgerError::InvalidAmount`] when `amount` is negative
    /// * [`LedgerError::FutureLedger`] when `ledger` is ahead of the current sequence
    /// * [`LedgerError::StaleLedger`] when `ledger` is not strictly after the previous snapshot
    ///
    /// # Events
    /// * [`SupplyPosted`] on success
    pub fn post_supply(
        env: Env,
        poster: Address,
        sac: Address,
        amount: i128,
        ledger: u32,
        breakdown_hash: BytesN<32>,
    ) {
        poster.require_auth();
        require_not_paused(&env);
        let role = RegistryClient::new(&env, &registry(&env)).role_of(&poster);
        if role != Some(Role::SupplyPoster) {
            panic_with_error!(&env, LedgerError::NotSupplyPoster);
        }
        require_listed(&env, &sac);
        if !RegistryClient::new(&env, &registry(&env)).has_minimum_stake(&poster) {
            panic_with_error!(&env, LedgerError::InsufficientStake);
        }
        ensure_listed(&env, &sac);
        if amount < 0 {
            panic_with_error!(&env, LedgerError::InvalidAmount);
        }
        let current_ledger = env.ledger().sequence();
        if ledger > current_ledger {
            panic_with_error!(&env, LedgerError::FutureLedger);
        }
        if current_ledger - ledger > MAX_SUPPLY_LEDGER_AGE {
            panic_with_error!(&env, LedgerError::LedgerTooOld);
        }
        let key = DataKey::Supply(sac.clone());
        if let Some(previous) = read_persistent::<SupplySnapshot>(&env, &key) {
            if ledger <= previous.ledger {
                panic_with_error!(&env, LedgerError::StaleLedger);
            }
        }
        let snapshot = SupplySnapshot {
            amount,
            ledger,
            timestamp: env.ledger().timestamp(),
            breakdown_hash: breakdown_hash.clone(),
            poster: poster.clone(),
        };
        write_persistent(&env, &key, &snapshot);
        write_persistent(
            &env,
            &DataKey::SupplyPostedLedger(sac.clone()),
            &current_ledger,
        );
        bump_instance(&env);
        SupplyPosted {
            sac: sac.clone(),
            amount,
            ledger,
            breakdown_hash,
            poster,
        }
        .publish(&env);
        // History: push a coverage snapshot if any counting report exists.
        if let Some(bst) = best_report(&env, &sac, Tier::Transcribed) {
            history_push(
                &env,
                &sac,
                CoverageSnapshot {
                    bps: coverage_bps(bst.amount, snapshot.amount),
                    supply: snapshot.amount,
                    reserves: bst.amount,
                    tier: bst.tier,
                    ledger: env.ledger().sequence(),
                    timestamp: env.ledger().timestamp(),
                },
            );
        }
    }

    /// Check whether the reporter's complete reserve post would pass validation.
    pub fn can_post_report(
        env: Env,
        reporter: Address,
        sac: Address,
        amount: i128,
        as_of: u64,
        doc_hash: BytesN<32>,
        doc_uri: String,
    ) -> Result<(), LedgerError> {
        let _ = doc_hash;
        validate_post_reserve(&env, &reporter, &sac, amount, as_of, &doc_uri).map(|_| ())
    }

    /// Check whether the reporter's complete reserve post would pass validation.
    pub fn can_post_report(
        env: Env,
        reporter: Address,
        sac: Address,
        amount: i128,
        as_of: u64,
        doc_hash: BytesN<32>,
        doc_uri: String,
    ) -> Result<(), LedgerError> {
        let _ = doc_hash;
        validate_post_reserve(&env, &reporter, &sac, amount, as_of, &doc_uri).map(|_| ())
    }

    /// Record a reserve figure for `sac`. The reporter's identity sets the
    /// tier: the asset's issuer account, an issuer-appointed delegate, a
    /// registered auditor, or a registered transcriber.
    /// tier: the asset's issuer account, a registered auditor, or a
    /// registered transcriber.
    ///
    /// # Arguments
    /// * `reporter` - must authorise this call; determines the report's tier
    /// * `sac` - the listed asset
    /// * `amount` - reserves in the asset's smallest unit; must be non-negative
    /// * `as_of` - Unix timestamp the reserves were measured at; must not be in the future or stale for this reporter
    /// * `doc_hash` - SHA-256 of the attestation document
    /// * `doc_uri` - URI of the attestation document; 1–256 bytes
    ///
    /// # Errors
    /// * [`LedgerError::AssetNotListed`] when `sac` has not been listed
    /// * [`LedgerError::NotReporter`] when `reporter` has no applicable role
    /// * [`LedgerError::InvalidAmount`] when `amount` is negative
    /// * [`LedgerError::InvalidUri`] when `doc_uri` is empty or longer than 256 bytes
    /// * [`LedgerError::FutureTimestamp`] when `as_of` is ahead of the current ledger timestamp
    /// * [`LedgerError::ImplausibleAmount`] when the reserve-to-supply ratio is outside the plausible range
    /// * [`LedgerError::StaleReport`] when `as_of` is not strictly after this reporter's previous `as_of`
    /// * [`LedgerError::TooManyReporters`] when the tier is full and no revoked slots can be reclaimed
    ///
    /// # Events
    /// * [`ReservePosted`] on success
    pub fn post_reserve(
        env: Env,
        reporter: Address,
        sac: Address,
        amount: i128,
        as_of: u64,
        doc_hash: BytesN<32>,
        doc_uri: String,
    ) -> Tier {
        reporter.require_auth();
        require_not_paused(&env);
        let asset = require_listed(&env, &sac);
        let tier = if speaks_for_issuer(&env, &sac, &asset.issuer, &reporter) {
            Tier::IssuerSigned
        } else {
            match RegistryClient::new(&env, &registry(&env)).role_of(&reporter) {
                Some(Role::Auditor) => Tier::AuditorSigned,
                Some(Role::Transcriber) => Tier::Transcribed,
                _ => panic_with_error!(&env, LedgerError::NotReporter),
            }
        let tier = match validate_post_reserve(&env, &reporter, &sac, amount, as_of, &doc_uri) {
            Ok(tier) => tier,
            Err(error) => panic_with_error!(&env, error),
        };
        if amount < 0 {
            panic_with_error!(&env, LedgerError::InvalidAmount);
        }
        if doc_uri.is_empty() || doc_uri.len() > MAX_URI_LEN {
            panic_with_error!(&env, LedgerError::InvalidUri);
        }
        // A placeholder is not a source document. The floor is admin-settable
        // because the length that carries information depends on the scheme
        // a reporter uses (`ipfs://` needs fewer bytes than `https://`).
        if doc_uri.len() < configured_min_doc_uri_len(&env) {
            panic_with_error!(&env, LedgerError::UriTooShort);
        }
        let now = env.ledger().timestamp();
        if as_of > now {
            panic_with_error!(&env, LedgerError::FutureTimestamp);
        }
        // A figure typed in whole units instead of the asset's smallest unit is
        // 10,000,000 times too small, and the asset then reads as uncovered --
        // the same reading as backing that has genuinely gone. Refuse rather
        // than publish an ambiguity like that. Zero is exempt: no reserves at
        // all is a real statement and cannot come from a unit slip.
        if amount != 0 {
            if let Some(supply) =
                read_persistent::<SupplySnapshot>(&env, &DataKey::Supply(sac.clone()))
            {
                // With no supply there is nothing to judge the figure against,
                // and coverage_bps reports u32::MAX for it.
                if supply.amount > 0 {
                    let bps = compute_bps(amount, supply.amount);
                    if !(MIN_PLAUSIBLE_BPS..=MAX_PLAUSIBLE_BPS).contains(&bps) {
                        panic_with_error!(&env, LedgerError::ImplausibleAmount);
                    }
                }
            }
        }

        migrate_legacy(&env, &sac, tier);
        let key = DataKey::ReportBy(sac.clone(), tier, reporter.clone());
        let mut index = reporters(&env, &sac, tier);
        if !index.contains(&reporter) {
            let registry_client = RegistryClient::new(&env, &registry(&env));
            if index.len() >= MAX_REPORTERS_PER_TIER {
                index = reclaim_slots(&env, &registry_client, &sac, tier, &index);
            }
            if index.len() >= MAX_REPORTERS_PER_TIER {
                panic_with_error!(&env, LedgerError::TooManyReporters);
            }
            // The registry's cap on how many reporters one asset may hold. A
            // reporter who already speaks for this asset at another tier keeps
            // their slot: the cap counts reporters, not entries.
            let mut asset = asset_reporters(&env, &sac);
            if !asset.contains(&reporter) {
                let max = registry_client.max_reporters();
                if asset.len() >= max {
                    reclaim_asset(&env, &registry_client, &sac);
                    asset = asset_reporters(&env, &sac);
                }
                if asset.len() >= max {
                    panic_with_error!(&env, LedgerError::TooManyReportersPerAsset);
                }
            }
            index.push_back(reporter.clone());
            write_persistent(&env, &DataKey::Reporters(sac.clone(), tier), &index);
        }
        let report = ReserveReport {
            amount,
            as_of,
            posted_at: now,
            tier,
            reporter: reporter.clone(),
            doc_hash: doc_hash.clone(),
            doc_uri: doc_uri.clone(),
        };
        write_persistent(&env, &key, &report);
        push_recent_report(
            &env,
            RecentReport {
                sac: sac.clone(),
                report: report.clone(),
            },
        write_persistent(
            &env,
            &DataKey::ReportPostedLedger(sac.clone(), tier, reporter.clone()),
            &env.ledger().sequence(),
        );
        bump_instance(&env);
        // Best-effort: a registry with no trusted ledger configured (or any
        // other failure) must never block a reserve report from landing, so
        // this uses `try_` and swallows the result.
        let _ = RegistryClient::new(&env, &registry(&env)).try_record_report_outcome(
            &env.current_contract_address(),
            &reporter,
            &true,
        );
        ReservePosted {
            sac: sac.clone(),
            tier,
            amount,
            as_of,
            reporter,
            doc_hash,
            doc_uri,
        }
        .publish(&env);
        // History: push a coverage snapshot if supply exists.
        if let Some(sup) = read_persistent::<SupplySnapshot>(&env, &DataKey::Supply(sac.clone())) {
            if let Some(bst) = best_report(&env, &sac, Tier::Transcribed) {
                history_push(
                    &env,
                    &sac,
                    CoverageSnapshot {
                        bps: coverage_bps(bst.amount, sup.amount),
                        supply: sup.amount,
                        reserves: bst.amount,
                        tier: bst.tier,
                        ledger: env.ledger().sequence(),
                        timestamp: now,
                    },
                );
            }
        }
        tier
    }

    /// Return the asset record for `sac`, or `None` if not listed.
    ///
    /// # Arguments
    /// * `sac` - the asset to look up
    /// Pre-flight for `post_reserve`: the error it would raise for this
    /// reporter and asset, checked in the same order, without writing any
    /// storage or extending any TTL. Checks that depend on the figure itself
    /// (amount, URI, timestamps, plausibility) are not covered.
    pub fn can_post_report(env: Env, reporter: Address, asset: Address) -> Result<(), LedgerError> {
        if paused_flag(&env) {
            return Err(LedgerError::Paused);
        }
        let record = env
            .storage()
            .persistent()
            .get::<_, AssetRecord>(&DataKey::Asset(asset.clone()))
            .ok_or(LedgerError::AssetNotListed)?;
        let registry = RegistryClient::new(&env, &registry(&env));
        let tier = if reporter == record.issuer {
            Tier::IssuerSigned
        } else {
            match registry.role_of(&reporter) {
                Some(Role::Auditor) => Tier::AuditorSigned,
                Some(Role::Transcriber) => Tier::Transcribed,
                _ => return Err(LedgerError::NotReporter),
            }
        };
        // A legacy entry only exists while the tier's index is empty, so it
        // can never fill the tier; only the index needs counting.
        let index: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Reporters(asset.clone(), tier))
            .unwrap_or_else(|| Vec::new(&env));
        if !index.contains(&reporter) && index.len() >= MAX_REPORTERS_PER_TIER {
            // Count the slots `post_reserve` would keep after reclaiming.
            let mut kept: u32 = 0;
            for held_by in index.iter() {
                let key = DataKey::ReportBy(asset.clone(), tier, held_by.clone());
                if env.storage().persistent().has(&key)
                    && counts_for_tier(&registry, tier, &held_by)
                {
                    kept += 1;
                }
            }
            if kept >= MAX_REPORTERS_PER_TIER {
                return Err(LedgerError::TooManyReporters);
            }
        }
        Ok(())
    }

    /// Remove the caller's latest reserve report at the tier their identity
    /// resolves to, returning coverage to the next report if one exists.
    pub fn retract_reserve(env: Env, reporter: Address, sac: Address) {
        reporter.require_auth();
        let asset = require_listed(&env, &sac);
        let tier = if reporter == asset.issuer {
            Tier::IssuerSigned
        } else {
            match RegistryClient::new(&env, &registry(&env)).role_of(&reporter) {
                Some(Role::Auditor) => Tier::AuditorSigned,
                Some(Role::Transcriber) => Tier::Transcribed,
                _ => panic_with_error!(&env, LedgerError::NotReporter),
            }
        };

        migrate_legacy(&env, &sac, tier);
        let report_key = DataKey::ReportBy(sac.clone(), tier, reporter.clone());
        let report = match read_persistent::<ReserveReport>(&env, &report_key) {
            Some(report) if report.reporter == reporter => report,
            _ => panic_with_error!(&env, LedgerError::NoReportToRetract),
        };
        let mut index = reporters(&env, &sac, tier);
        if let Some(i) = index
            .iter()
            .position(|indexed| indexed == reporter)
            .and_then(|i| u32::try_from(i).ok())
        {
            index.remove(i);
        } else {
            panic_with_error!(&env, LedgerError::NoReportToRetract);
        }

        env.storage().persistent().remove(&report_key);
        env.storage()
            .persistent()
            .remove(&DataKey::ReportPostedLedger(
                sac.clone(),
                tier,
                reporter.clone(),
            ));
        let index_key = DataKey::Reporters(sac.clone(), tier);
        if index.is_empty() {
            env.storage().persistent().remove(&index_key);
        } else {
            write_persistent(&env, &index_key, &index);
        }
        bump_instance(&env);
        let _ = RegistryClient::new(&env, &registry(&env)).try_record_report_outcome(
            &env.current_contract_address(),
            &reporter,
            &false,
        );
        ReserveRetracted {
            sac,
            tier,
            reporter,
            as_of: report.as_of,
        }
        .publish(&env);
    }

    pub fn get_asset(env: Env, sac: Address) -> Option<AssetRecord> {
        read_persistent(&env, &DataKey::Asset(sac))
    }

    /// Return the most recent supply snapshot for `sac`, or `None` if none has been posted.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    pub fn get_supply(env: Env, sac: Address) -> Option<SupplySnapshot> {
        read_persistent(&env, &DataKey::Supply(sac))
    }

    /// Return the freshest counting report at `tier`, or `None` if none exists.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    /// * `tier` - the report tier to query
    /// Register `new` as the successor of the deprecated asset `old`.
    /// Both must already be listed. At most one alias hop is ever followed:
    /// `is_covered(old, ...)` and `coverage(old, ...)` will resolve `old` to
    /// `new`, but `new`'s own alias (if any) is ignored.
    pub fn alias_asset(env: Env, old: Address, new: Address) {
        admin(&env).require_auth();
        if old == new {
            panic_with_error!(&env, LedgerError::SelfAlias);
        }
        ensure_listed(&env, &old);
        ensure_listed(&env, &new);
        write_persistent(&env, &DataKey::Alias(old.clone()), &new);
        bump_instance(&env);
        AssetAliased { old, new }.publish(&env);
    }

    /// The registered successor of `asset`, if one exists.
    pub fn get_alias(env: Env, asset: Address) -> Option<Address> {
        read_persistent(&env, &DataKey::Alias(asset))
    }

    /// The freshest report at `tier` that still counts.    pub fn get_report(env: Env, sac: Address, tier: Tier) -> Option<ReserveReport> {
        freshest_at(&env, &sac, tier)
    }

    /// Return every stored report that still counts, strongest tier first,
    /// then first-post order within a tier.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    pub fn get_reports(env: Env, sac: Address) -> Vec<ReserveReport> {
        let registry = RegistryClient::new(&env, &registry(&env));
        let mut out = Vec::new(&env);
        for tier in Tier::ALL_DESC {
            for report in reports_at(&env, &sac, tier).iter() {
                if counts(&env, &registry, &sac, &report) {
                    out.push_back(report);
                }
            }
        }
        out
    }

    /// Return the latest supply against the freshest report at `min_tier` or stronger.
    ///
    /// Returns `None` when no supply snapshot or no counting report at or above
    /// `min_tier` has been posted.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    /// * `min_tier` - weakest report tier that satisfies the query
    /// All live (non-expired and still counting) reports for `sac`, sorted by
    /// tier descending (`AuditorSigned`, `IssuerSigned`, `Transcribed`), capped
    /// at [`MAX_ALL_REPORTS`].
    pub fn get_all_reports(env: Env, sac: Address) -> Vec<ReserveReport> {
        let registry = RegistryClient::new(&env, &registry(&env));
        let now = env.ledger().timestamp();
        let mut out = Vec::new(&env);
        for tier in Tier::ALL_DESC {
            for report in reports_at(&env, &sac, tier).iter() {
                if now.saturating_sub(report.as_of) <= MAX_REPORT_AGE_SECS
                    && counts(&registry, &report)
                {
                    out.push_back(report);
                    if out.len() >= MAX_ALL_REPORTS {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// When `reporter` most recently posted a reserve figure for `asset`: the
    /// ledger timestamp of that post (`posted_at`), not the figure's `as_of`.
    ///
    /// Every tier is searched, so a reporter whose role changed answers with
    /// whichever of their posts is later, and a not-yet-migrated v0.1 entry
    /// counts as one of their posts too. The registry is not consulted: a
    /// revoked reporter's posts remain history, and `get_reports` already says
    /// which of them still count. `None` when `reporter` has never posted for
    /// `asset`.
    pub fn reporter_last_post(env: Env, reporter: Address, asset: Address) -> Option<u64> {
        let mut last: Option<u64> = None;
        let mut note = |posted_at: u64| {
            if last.is_none_or(|seen| posted_at > seen) {
                last = Some(posted_at);
            }
        };
        for tier in Tier::ALL_DESC {
            if let Some(legacy) =
                read_persistent::<ReserveReport>(&env, &DataKey::Report(asset.clone(), tier))
            {
                if legacy.reporter == reporter {
                    note(legacy.posted_at);
                }
            }
            if let Some(report) = read_persistent::<ReserveReport>(
                &env,
                &DataKey::ReportBy(asset.clone(), tier, reporter.clone()),
            ) {
                note(report.posted_at);
            }
        }
        last
    }

    /// A page of the (asset, reporter) pairs a bootstrap walks: at most
    /// [`MAX_PAIRS_PER_CALL`] pairs per call.
    ///
    /// Pairs come from the stored per-tier reporter indexes, so no registry
    /// call is made: a reporter whose figure no longer counts still appears,
    /// and `get_reports` / `reporter_last_post` say what they posted. A
    /// reporter with an entry at more than one tier appears once per asset.
    ///
    /// Order: assets in listing order, and within an asset the tiers strongest
    /// first (`AuditorSigned`, `IssuerSigned`, `Transcribed`), first-post
    /// order within a tier. `cursor` is the index of the first pair to return
    /// and defaults to `0`; `limit` is capped at `MAX_PAIRS_PER_CALL = 50`.
    /// An empty page means the cursor is at or past the last pair. Listings
    /// and first posts only ever grow the sequence, so a bootstrap that must
    /// be exact should re-read from `0` or dedupe by pair.
    pub fn asset_reporter_pairs(
        env: Env,
        cursor: Option<u32>,
        limit: u32,
    ) -> Vec<(Address, Address)> {
        pair_page(&env, cursor.unwrap_or(0), limit.min(MAX_PAIRS_PER_CALL))
    }

    /// The ledger timestamp of the asset's most recent reserve post: the
    /// post's `posted_at`, not the figure's `as_of`.
    ///
    /// Every tier is searched, including a not-yet-migrated v0.1 entry, and the
    /// registry is not consulted: a figure whose reporter was later revoked
    /// still counts as a post. `get_reports` answers which figures count now.
    /// `None` for an unlisted asset and for a listed asset that has never been
    /// reported on — "never reported" and "reported long ago" are different
    /// answers and must not collapse into each other. If `asset` has a
    /// registered alias its successor is used, as `coverage` does.
    pub fn last_updated(env: Env, asset: Address) -> Option<u64> {
        let resolved = read_persistent::<Address>(&env, &DataKey::Alias(asset.clone()))
            .unwrap_or_else(|| asset.clone());
        let key = DataKey::Asset(resolved.clone());
        // Existence only: the answer does not depend on the asset record, so
        // the record is not deserialised.
        if !env.storage().persistent().has(&key) {
            return None;
        }
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_THRESHOLD, PERSISTENT_EXTEND_TO);
        let mut latest: Option<u64> = None;
        for tier in Tier::ALL_DESC {
            for report in reports_at(&env, &resolved, tier).iter() {
                latest = Some(match latest {
                    None => report.posted_at,
                    Some(seen) => seen.max(report.posted_at),
                });
            }
        }
        latest
    }

    /// One figure per (asset, reporter) pair across every listed asset, for an
    /// indexer bootstrap: at most [`MAX_PAIRS_PER_CALL`] pairs per call.
    ///
    /// `cursor` is the index of the first pair and defaults to `0`, so a
    /// bootstrap walks the ledger with `cursor = Some(seen)` until a page comes
    /// back empty. Order mirrors the pair listing: assets in listing order,
    /// tiers strongest first (`AuditorSigned`, `IssuerSigned`, `Transcribed`),
    /// first-post order within a tier.
    ///
    /// A reporter with figures at more than one tier appears once, carrying the
    /// freshest of them: latest `as_of`, ties to the stronger tier. The registry
    /// is not consulted, so a figure whose reporter was since revoked is still
    /// listed and the walk costs one read per pair; `get_reports` answers which
    /// figures count now. A reporter whose figure exists only as a
    /// not-yet-migrated v0.1 entry is listed by neither this function nor
    /// `asset_reporter_pairs`: the pair index is the per-tier reporter index.
    pub fn get_all_latest_reports(env: Env, cursor: Option<u32>, limit: u32) -> Vec<ReserveReport> {
        let mut out = Vec::new(&env);
        for (asset, reporter) in
            pair_page(&env, cursor.unwrap_or(0), limit.min(MAX_PAIRS_PER_CALL)).iter()
        {
            if let Some(report) = latest_for(&env, &asset, &reporter) {
                out.push_back(report);
            }
        }
        out
    }

    /// Latest supply against the freshest report at `min_tier` or stronger.
    /// If `sac` has a registered alias its successor is used transparently
    /// (at most one hop — the successor's own alias, if any, is not followed).    pub fn coverage(env: Env, sac: Address, min_tier: Tier) -> Option<Coverage> {
        let resolved = read_persistent::<Address>(&env, &DataKey::Alias(sac.clone()))
            .unwrap_or_else(|| sac.clone());
        let supply = effective_supply(&env, &resolved)?;
        let report = best_report(&env, &resolved, min_tier)?;
        Some(Coverage {
            bps: compute_bps(report.amount, supply.amount),
            supply: supply.amount,
            reserves: report.amount,
            tier: report.tier,
            supply_ledger: supply.ledger,
            supply_timestamp: supply.timestamp,
            report_as_of: report.as_of,
        })
        Some(coverage_of(&supply, &report))
    }

    /// Return `true` when both the supply snapshot and the chosen report are at
    /// most `max_age` seconds old and coverage is at least `min_bps`.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    /// * `min_bps` - minimum coverage ratio in basis points
    /// * `max_age` - maximum age of supply and reserve data in seconds
    /// * `min_tier` - weakest report tier that satisfies the query
    /// Returns `Covered` when both the supply snapshot and the chosen report
    /// are at most `max_age` seconds old and coverage meets `min_bps`.
    /// Returns `Stale` when bps is met but age falls in the grace window
    /// `(max_age, max_age + ttl_grace_ledgers * 5]`. Returns `Uncovered`
    /// otherwise, including when there is no data.
    pub fn is_covered(
    /// True when both the supply snapshot and the chosen report are at most
    /// `max_age` seconds old and coverage is at least `min_bps` (inclusive).
    ///
    /// If `sac` has a registered alias its successor is used transparently
    /// (at most one hop). See `alias_asset` and `get_alias`.
    ///
    /// Performance note: the supply snapshot is loaded first. When it is
    /// absent or already older than `max_age` the function returns false
    /// immediately, before reading any reserve reports. This is the common
    /// early-exit path: a stale supply avoids all subsequent storage reads.    pub fn is_covered(env: Env, sac: Address, min_bps: u32, max_age: u64, min_tier: Tier) -> bool {
        match Self::coverage_status(env.clone(), sac, min_bps, max_age, min_tier) {
            CoverageStatus::Paused => panic_with_error!(&env, LedgerError::Paused),
            CoverageStatus::Covered(_) => true,
            CoverageStatus::Stale(_) | CoverageStatus::Uncovered => false,
        }
    }

    /// Evaluate currently retained figures at a historical ledger sequence.
    ///
    /// Replaced figures are not retained, so a value whose latest stored
    /// version was posted after `as_of_ledger` is unavailable. The target
    /// timestamp is estimated from the five-second ledger cadence.
    pub fn is_covered_at(
        env: Env,
        sac: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) -> CoverageStatus {
        let coverage = match Self::coverage(env.clone(), sac, min_tier) {
            Some(c) => c,
            None => return CoverageStatus::Uncovered,
        };
        if coverage.bps < min_bps {
            return CoverageStatus::Uncovered;
        }
        let now = env.ledger().timestamp();
        let supply_age = now.saturating_sub(coverage.supply_timestamp);
        let report_age = now.saturating_sub(coverage.report_as_of);
        let age = supply_age.max(report_age);
        if age <= max_age {
            return CoverageStatus::Covered;
        }
        let grace_ledgers: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TtlGrace)
            .unwrap_or(0);
        let grace_secs = (grace_ledgers as u64).saturating_mul(5);
        if age <= max_age.saturating_add(grace_secs) {
            return CoverageStatus::Stale;
        }
        CoverageStatus::Uncovered
    }

    /// Set the TTL grace window in ledgers (1 ledger ≈ 5 seconds). Callers
    /// receive `Stale` during this window instead of `Uncovered`. Zero disables
    /// the grace window.
    pub fn set_ttl_grace(env: Env, grace_ledgers: u32) {
        admin(&env).require_auth();
        env.storage()
            .instance()
            .set(&DataKey::TtlGrace, &grace_ledgers);
        bump_instance(&env);
        TtlGraceUpdated { grace_ledgers }.publish(&env);
    }

    /// The current TTL grace window in ledgers. Zero means no grace period.
    pub fn ttl_grace(env: Env) -> u32 {
        let grace = env
            .storage()
            .instance()
            .get(&DataKey::TtlGrace)
            .unwrap_or(0u32);
        if grace > 0 {
            bump_instance(&env);
        }
        grace
        as_of_ledger: u32,
    ) -> bool {
        let current_ledger = env.ledger().sequence();
        if as_of_ledger > current_ledger {
            panic_with_error!(&env, LedgerError::FutureLedger);
        }
        if paused_flag(&env) {
            panic_with_error!(&env, LedgerError::Paused);
        }
        let resolved =
            read_persistent::<Address>(&env, &DataKey::Alias(sac.clone())).unwrap_or(sac);
        let supply = match posted_supply_at_ledger(&env, &resolved, as_of_ledger, false) {
            Some(supply) => supply,
            None => return false,
        };
        let report = match best_report_at_ledger(&env, &resolved, min_tier, as_of_ledger) {
            Some(report) => report,
            None => return false,
        };
        let elapsed = u64::from(current_ledger - as_of_ledger).saturating_mul(SECONDS_PER_LEDGER);
        let as_of_timestamp = env.ledger().timestamp().saturating_sub(elapsed);
        let age = as_of_timestamp
            .saturating_sub(supply.timestamp)
            .max(as_of_timestamp.saturating_sub(report.as_of));
        age <= max_age && coverage_bps(report.amount, supply.amount) >= min_bps
    }

    /// Coverage in basis points as the figures posted no later than
    /// `as_of_ledger` gave it, for backtesting and reports.
    ///
    /// `max_age` counts **ledgers**: both the supply snapshot and the chosen
    /// report must have been posted at most `max_age` ledgers before
    /// `as_of_ledger` (inclusive), the ledger equivalent of `is_covered`'s
    /// seconds. `None` when `as_of_ledger` is in the future, when nothing is
    /// stored for both sides as of that ledger, or when either figure is older
    /// than that.
    ///
    /// Like `is_covered_at`, this reads the figures that were posted no later
    /// than `as_of_ledger`: the ledger does not retain figures that have since
    /// been replaced or retracted, so a ratio those superseded figures gave
    /// cannot be reconstructed here. The events are the exact source for that.
    pub fn historical_coverage_bps(
        env: Env,
        asset: Address,
        max_age: u32,
        as_of_ledger: u32,
    ) -> Option<u128> {
        let current_ledger = env.ledger().sequence();
        if as_of_ledger > current_ledger {
            return None;
        }
        let resolved = read_persistent::<Address>(&env, &DataKey::Alias(asset.clone()))
            .unwrap_or_else(|| asset.clone());
        let supply = posted_supply_at_ledger(&env, &resolved, as_of_ledger)?;
        let report = best_report_at_ledger(&env, &resolved, Tier::Transcribed, as_of_ledger)?;
        let supply_posted =
            read_persistent::<u32>(&env, &DataKey::SupplyPostedLedger(resolved.clone()))
                .unwrap_or(current_ledger);
        let report_posted = read_persistent::<u32>(
            &env,
            &DataKey::ReportPostedLedger(resolved.clone(), report.tier, report.reporter.clone()),
        )
        .unwrap_or(current_ledger);
        // Both figures are read at the same ledger, so the age of each is the
        // distance from that ledger to when it was posted. A figure posted
        // after `as_of_ledger` was already excluded above.
        if as_of_ledger.saturating_sub(supply_posted) > max_age
            || as_of_ledger.saturating_sub(report_posted) > max_age
        {
            return None;
        }
        Some(u128::from(coverage_bps(report.amount, supply.amount)))
    /// The coverage `coverage` reported at ledger `as_of_ledger`, from the
    /// figures that were on chain by then: the posted supply snapshot and the
    /// freshest counting report posted at or before that ledger. Any tier
    /// qualifies, exactly as `coverage(sac, Tier::Transcribed)` does.
    ///
    /// `None` is a clean miss, not an error: it means no supply snapshot had
    /// been posted by that ledger, or no report had. Replaced figures are not
    /// retained, so a newer version of a figure that was live at
    /// `as_of_ledger` hides the older one; the events carry that history.
    ///
    /// Read-only: it writes no state and extends no TTL, so it can be
    /// simulated freely and costs no rent. Native supply is not consulted —
    /// `total_supply()` can only answer for the current ledger, so a snapshot
    /// is always judged against the posted figure.
    ///
    /// A registered alias is followed, at most one hop, like `coverage`.
    pub fn get_coverage_snapshot(env: Env, sac: Address, as_of_ledger: u32) -> Option<Coverage> {
        if as_of_ledger > env.ledger().sequence() {
            panic_with_error!(&env, LedgerError::FutureLedger);
        }
        let resolved = peek::<Address>(&env, &DataKey::Alias(sac.clone())).unwrap_or(sac);
        let supply = posted_supply_at_ledger(&env, &resolved, as_of_ledger, true)?;
        let report = best_report_as_of(&env, &resolved, Tier::Transcribed, as_of_ledger, true)?;
        Some(coverage_of(&supply, &report))
    }

    /// `coverage` with the reason attached, in one call. Precedence: paused,
    /// then stale (the last ratio is not a verdict on backing), then
    /// under-backed, then covered.
    pub fn coverage_status(
        env: Env,
        sac: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) -> CoverageStatus {
        if paused_flag(&env) {
            return CoverageStatus::Paused;
        }
        let coverage = match Self::coverage(env.clone(), sac, min_tier) {
            Some(c) => c,
            None => return CoverageStatus::Uncovered,
        };
        let now = env.ledger().timestamp();
        let age = now
            .saturating_sub(coverage.supply_timestamp)
            .max(now.saturating_sub(coverage.report_as_of));
        if age > max_age {
            return CoverageStatus::Stale(StaleInfo {
                last_bps: coverage.bps,
                age_ledgers: (age / SECONDS_PER_LEDGER).min(u32::MAX as u64) as u32,
            });
        }
        if coverage.bps < min_bps {
            return CoverageStatus::Uncovered;
        }
        CoverageStatus::Covered(CoveredInfo { bps: coverage.bps })
    }

    /// Stop recording figures and answering `is_covered` during an incident.
    /// Idempotent, and the event names the admin that asserted the state.
    pub fn pause(env: Env) {
        let by = admin(&env);
        by.require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        bump_instance(&env);
        Paused { by }.publish(&env);
    }

    /// Clear the incident switch.
    pub fn unpause(env: Env) {
        let by = admin(&env);
        by.require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        bump_instance(&env);
        Unpaused { by }.publish(&env);
    }

    pub fn is_paused(env: Env) -> bool {
        paused_flag(&env)
    }

    /// Opt `sac` in to reading supply from `token.total_supply()` instead of
    /// the posted snapshot. The snapshot is kept as the fallback.
    pub fn enable_native_supply(env: Env, sac: Address, token: Address) {
        admin(&env).require_auth();
        ensure_listed(&env, &sac);
        write_persistent(&env, &DataKey::NativeSupply(sac.clone()), &token);
        bump_instance(&env);
        NativeSupplyEnabled { sac, token }.publish(&env);
    }

    /// The opted-in token, or `None` when the asset reads the snapshot.
    pub fn native_supply_token(env: Env, sac: Address) -> Option<Address> {
        native_token(&env, &sac)
    }

    /// The min(n, 50) most recently posted reports across all listed assets,
    /// in newest-first order.
    pub fn get_recent_reports(env: Env, n: u32) -> Vec<RecentReport> {
        let cap = n.min(MAX_RECENT_REPORTS);
        let all: Vec<RecentReport> =
            read_persistent(&env, &DataKey::RecentReports).unwrap_or_else(|| Vec::new(&env));
        let len = all.len().min(cap);
        let mut out = Vec::new(&env);
        for i in 0..len {
            out.push_back(all.get(i).unwrap());
    /// Return up to `limit` coverage snapshots for `sac`, newest first.
    ///
    /// Each snapshot is recorded automatically whenever a supply or reserve
    /// post results in both supply and a counting report being present.
    ///
    /// # Arguments
    /// * `sac` - the listed asset
    /// * `limit` - maximum number of entries to return; clamped to `HISTORY_CAPACITY`
    pub fn get_coverage_history(env: Env, sac: Address, limit: u32) -> Vec<CoverageSnapshot> {
        let head: u32 =
            read_persistent::<u32>(&env, &DataKey::HistoryHead(sac.clone())).unwrap_or(0);
        let count: u32 =
            read_persistent::<u32>(&env, &DataKey::HistoryCount(sac.clone())).unwrap_or(0);
        let take = limit.min(count);
        let mut out = Vec::new(&env);
        for i in 0..take {
            let slot = head.wrapping_sub(i + 1) % HISTORY_CAPACITY;
            if let Some(snap) = read_persistent::<CoverageSnapshot>(
                &env,
                &DataKey::HistorySlot(sac.clone(), slot),
            ) {
                out.push_back(snap);
            }
        }
        out
    }

    /// The coverage of `sac` as basis points (`10_000 == 100 %`), returned as
    /// `Option<u128>`. Returns `None` when no supply snapshot or report is
    /// present, or when either is older than `max_age` seconds. The value is
    /// the same scale as `Coverage::bps` but widened to `u128`.
    pub fn coverage_bps(env: Env, sac: Address, max_age: u64) -> Option<u128> {
        let supply = read_persistent::<SupplySnapshot>(&env, &DataKey::Supply(sac.clone()))?;
        let report = best_report(&env, &sac, Tier::Transcribed)?;
        let now = env.ledger().timestamp();
        let fresh = |t: u64| now.saturating_sub(t) <= max_age;
        if !fresh(supply.timestamp) || !fresh(report.as_of) {
            return None;
        }
        Some(compute_bps(report.amount, supply.amount) as u128)
    }

    /// Freeze the report identified by `(sac, as_of)` for the configured
    /// dispute hold period. Only a registered `Role::Watcher` may call this.
    pub fn dispute_report(env: Env, watcher: Address, sac: Address, as_of: u64) {
        watcher.require_auth();
        require_listed(&env, &sac);
        let role = RegistryClient::new(&env, &registry(&env)).role_of(&watcher);
        if role != Some(Role::Watcher) {
            panic_with_error!(&env, LedgerError::NotAWatcher);
        }
        let key = DataKey::Dispute(sac.clone(), as_of);
        if read_persistent::<DisputeRecord>(&env, &key).is_some() {
            panic_with_error!(&env, LedgerError::AlreadyDisputed);
        }
        let hold = dispute_hold_ledgers(&env);
        let hold_until_ledger = env.ledger().sequence().saturating_add(hold);
        let record = DisputeRecord {
            disputer: watcher.clone(),
            hold_until_ledger,
        };
        write_persistent(&env, &key, &record);
        bump_instance(&env);
        ReportDisputed {
            sac,
            as_of,
            disputer: watcher,
            hold_until_ledger,
        }
        .publish(&env);
    }

    /// The dispute record for `(sac, as_of)`, if one exists.
    pub fn get_dispute_status(env: Env, sac: Address, as_of: u64) -> Option<DisputeRecord> {
        read_persistent(&env, &DataKey::Dispute(sac, as_of))
    }

    /// Admin resolves a dispute early: removes the freeze so the report
    /// counts again (or removes the delist marker once the hold has expired).
    pub fn resolve_dispute(env: Env, sac: Address, as_of: u64) {
        admin(&env).require_auth();
        let key = DataKey::Dispute(sac.clone(), as_of);
        if read_persistent::<DisputeRecord>(&env, &key).is_none() {
            panic_with_error!(&env, LedgerError::DisputeNotFound);
        }
        env.storage().persistent().remove(&key);
        bump_instance(&env);
        DisputeResolved { sac, as_of }.publish(&env);
    }

    /// Set the number of ledgers a disputed report is frozen before it is
    /// treated as delisted.
    pub fn set_dispute_hold(env: Env, n: u32) {
        admin(&env).require_auth();
        env.storage().instance().set(&DataKey::DisputeHold, &n);
        bump_instance(&env);
    }

    /// The current dispute hold period in ledgers.
    pub fn get_dispute_hold(env: Env) -> u32 {
        dispute_hold_ledgers(&env)
    /// Transfer admin rights to `new_admin`. Requires both current and new
    /// admin to authorise.
    ///
    /// # Arguments
    /// * `new_admin` - the address that will become administrator
    ///
    /// # Events
    /// * [`AdminTransferred`] on success
    /// Return a compact authenticated attestation of `is_covered` for off-chain
    /// verification without an RPC call.
    pub fn sign_coverage(
        env: Env,
        asset: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) -> Bytes {
        let covered = Self::is_covered(env.clone(), asset.clone(), min_bps, max_age, min_tier);
        let ledger_contract = env.current_contract_address();
        let ledger_sequence = env.ledger().sequence();
        let ledger_timestamp = env.ledger().timestamp();
        encode_coverage_attestation(
            &env,
            &asset,
            &ledger_contract,
            min_bps,
            max_age,
            min_tier,
            covered,
            ledger_sequence,
            ledger_timestamp,
        )
        };
        if coverage.bps < min_bps {
            return CoverageStatus::Uncovered;
        }
        let now = env.ledger().timestamp();
        let supply_age = now.saturating_sub(coverage.supply_timestamp);
        let report_age = now.saturating_sub(coverage.report_as_of);
        let age = supply_age.max(report_age);
        if age <= max_age {
            return CoverageStatus::Covered;
        }
        let grace_ledgers: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TtlGrace)
            .unwrap_or(0);
        let grace_secs = (grace_ledgers as u64).saturating_mul(5);
        if age <= max_age.saturating_add(grace_secs) {
            return CoverageStatus::Stale;
        }
        CoverageStatus::Uncovered
    }

    /// Set the TTL grace window in ledgers (1 ledger ≈ 5 seconds). Callers
    /// receive `Stale` during this window instead of `Uncovered`. Zero disables
    /// the grace window.
    pub fn set_ttl_grace(env: Env, grace_ledgers: u32) {
        admin(&env).require_auth();
        env.storage()
            .instance()
            .set(&DataKey::TtlGrace, &grace_ledgers);
        bump_instance(&env);
        TtlGraceUpdated { grace_ledgers }.publish(&env);
    }

    /// The current TTL grace window in ledgers. Zero means no grace period.
    pub fn ttl_grace(env: Env) -> u32 {
        let grace = env
            .storage()
            .instance()
            .get(&DataKey::TtlGrace)
            .unwrap_or(0u32);
        if grace > 0 {
            bump_instance(&env);
        }
        grace
    /// Set the shortest `doc_uri` `post_reserve` accepts. `0` disables the
    /// floor; an empty URI is still `InvalidUri`. Values above `MAX_URI_LEN`
    /// are rejected with `InvalidUri`, since they would refuse every URI the
    /// contract can accept. Absent means [`DEFAULT_MIN_DOC_URI_LEN`], so an
    /// upgraded deployment keeps the documented default until an admin acts.
    pub fn set_min_doc_uri_len(env: Env, min_len: u32) {
        admin(&env).require_auth();
        if min_len > MAX_URI_LEN {
            panic_with_error!(&env, LedgerError::InvalidUri);
        }
        env.storage()
            .instance()
            .set(&DataKey::MinDocUriLen, &min_len);
        bump_instance(&env);
    }

    /// The shortest `doc_uri` `post_reserve` accepts, `0` when the floor is
    /// disabled.
    pub fn min_doc_uri_len(env: Env) -> u32 {
        configured_min_doc_uri_len(&env)
    }

    pub fn transfer_admin(env: Env, new_admin: Address) {
        admin(&env).require_auth();
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        bump_instance(&env);
        AdminTransferred { new_admin }.publish(&env);
    }

    /// Appoint, replace or clear the account that posts `IssuerSigned`
    /// reports for `sac` on the issuer's behalf. Only the issuer may call it.
    pub fn set_issuer_delegate(env: Env, sac: Address, delegate: Option<Address>) {
        let asset = require_listed(&env, &sac);
        asset.issuer.require_auth();
        let key = DataKey::IssuerDelegate(sac.clone());
        match delegate {
            Some(delegate) => {
                write_persistent(&env, &key, &delegate);
                bump_instance(&env);
                IssuerDelegateSet { sac, delegate }.publish(&env);
            }
            None => {
                env.storage().persistent().remove(&key);
                bump_instance(&env);
                IssuerDelegateCleared { sac }.publish(&env);
            }
        }
    }

    /// The account the issuer of `sac` appointed to post for it, if any.
    pub fn get_issuer_delegate(env: Env, sac: Address) -> Option<Address> {
        read_persistent(&env, &DataKey::IssuerDelegate(sac))
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
    /// * [`LedgerError::UpgradeNotProposed`] when no upgrade has been proposed
    /// * [`LedgerError::UpgradeDelayNotElapsed`] when the delay has not yet passed
    ///
    /// # Events
    /// * [`UpgradeExecuted`] on success
    pub fn execute_upgrade(env: Env) {
        admin(&env).require_auth();
        let proposed = match pending(&env) {
            Some(proposed) => proposed,
            None => panic_with_error!(&env, LedgerError::UpgradeNotProposed),
        };
        if env.ledger().sequence() < proposed.executable_at_ledger {
            panic_with_error!(&env, LedgerError::UpgradeDelayNotElapsed);
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
    /// * [`LedgerError::UpgradeNotProposed`] when no upgrade has been proposed
    ///
    /// # Events
    /// * [`UpgradeCancelled`] on success
    pub fn cancel_upgrade(env: Env) {
        admin(&env).require_auth();
        let proposed = match pending(&env) {
            Some(proposed) => proposed,
            None => panic_with_error!(&env, LedgerError::UpgradeNotProposed),
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
    }
}

#[cfg(test)]
mod test;
