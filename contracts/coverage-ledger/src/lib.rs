//! Coverage ledger: records circulating-supply snapshots and reserve reports
//! for Stellar-issued assets, and answers "is this asset covered?" for any
//! contract that asks.
#![no_std]

use plimsoll_types::{
    coverage_bps, AssetRecord, Coverage, CoverageStatus, CoveredInfo, PendingUpgrade,
    RegistryClient, ReserveReport, Role, StaleInfo, SupplySnapshot, SupplyTokenClient, Tier,
    INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD, MAX_PLAUSIBLE_BPS, MIN_PLAUSIBLE_BPS,
    PERSISTENT_EXTEND_TO, PERSISTENT_THRESHOLD, SECONDS_PER_LEDGER, UPGRADE_DELAY_LEDGERS,
};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, BytesN, ContractExecutable, Env, Executable, String, Vec,
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

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Registry,
    PendingUpgrade,
    Asset(Address),
    Supply(Address),
    /// v0.1 layout, one report per tier. Read until migrated, never written.
    Report(Address, Tier),
    /// The latest report from one reporter at one tier: (sac, tier, reporter).
    ReportBy(Address, Tier, Address),
    /// Reporters with a `ReportBy` entry at (sac, tier), in first-post order.
    Reporters(Address, Tier),
    /// Incident switch. Absent means not paused.
    Paused,
    /// The SEP-41 token whose `total_supply()` replaces the posted snapshot.
    NativeSupply(Address),
    /// Maps a deprecated asset SAC to its successor SAC (at most one hop).
    Alias(Address),
    /// Index of all listed asset SACs, in listing order.
    Assets,
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
    SelfAlias = 16,
    Paused = 17,
    LedgerTooOld = 18,
    NoReportToRetract = 19,
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
pub struct AssetAliased {
    #[topic]
    pub old: Address,
    #[topic]
    pub new: Address,
}

#[contract]
pub struct CoverageLedger;

fn admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("admin is set in the constructor")
}

fn registry(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Registry)
        .expect("registry is set in the constructor")
}

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
}

fn pending(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
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
    let mut out = Vec::new(env);
    if let Some(legacy) = read_persistent::<ReserveReport>(env, &DataKey::Report(sac.clone(), tier))
    {
        out.push_back(legacy);
    }
    for reporter in reporters(env, sac, tier).iter() {
        if let Some(report) =
            read_persistent::<ReserveReport>(env, &DataKey::ReportBy(sac.clone(), tier, reporter))
        {
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

/// Whether a reporter at `tier` still counts: issuer-signed reports always
/// count, while auditor and transcriber reports require the matching role.
fn counts_for_tier(registry: &RegistryClient, tier: Tier, reporter: &Address) -> bool {
    let needed = match tier {
        Tier::IssuerSigned => return true,
        Tier::AuditorSigned => Role::Auditor,
        Tier::Transcribed => Role::Transcriber,
    };
    registry.role_of(reporter) == Some(needed)
}

/// Whether a stored report still counts: its reporter must still be entitled
/// to its tier. Issuer-signed reports always count; the issuer is fixed by the
/// asset record. A revoked auditor or transcriber, or one whose role changed,
/// no longer speaks for the figures they posted.
fn counts(registry: &RegistryClient, report: &ReserveReport) -> bool {
    counts_for_tier(registry, report.tier, &report.reporter)
}

/// The freshest counting report in `candidates`.
///
/// Two-phase per iteration: (1) a single O(n) index scan that finds the
/// globally freshest non-rejected candidate without touching the registry,
/// then (2) one `counts` call. If that report is revoked the index is marked
/// and the loop repeats. This avoids Vec::remove (O(n) element shift) and
/// keeps registry calls at most equal to the number of revoked reporters.
/// A bitmask tracks rejected positions; `candidates` is never mutated.
/// We have at most 3 tiers × (1 legacy + MAX_REPORTERS_PER_TIER) = 15
/// entries, so a u32 bitmask is always sufficient.
fn pick_counting(env: &Env, candidates: Vec<ReserveReport>) -> Option<ReserveReport> {
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
            let r = candidates.get(i).unwrap();
            best_i = Some(match best_i {
                None => i,
                Some(bi) => {
                    let b = candidates.get(bi).unwrap();
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
        if counts(&registry, &report) {
            return Some(report);
        }
        skip |= 1u32 << i;
    }
}

/// The freshest counting report at `tier`.
fn freshest_at(env: &Env, sac: &Address, tier: Tier) -> Option<ReserveReport> {
    pick_counting(env, reports_at(env, sac, tier))
}

fn best_report(env: &Env, sac: &Address, min_tier: Tier) -> Option<ReserveReport> {
    let mut candidates = Vec::new(env);
    for tier in Tier::ALL_DESC {
        if tier >= min_tier {
            candidates.append(&reports_at(env, sac, tier));
        }
    }
    pick_counting(env, candidates)
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
        let keep =
            env.storage().persistent().has(&key) && counts_for_tier(registry, tier, &reporter);
        if keep {
            kept.push_back(reporter);
        } else {
            env.storage().persistent().remove(&key);
        }
    }
    kept
}

#[contractimpl]
impl CoverageLedger {
    pub fn __constructor(env: Env, admin: Address, registry: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Registry, &registry);
        bump_instance(&env);
    }

    pub fn admin(env: Env) -> Address {
        admin(&env)
    }

    pub fn registry(env: Env) -> Address {
        registry(&env)
    }

    /// List a Stellar-issued asset by its Stellar Asset Contract address.
    /// Permissionless: the code and issuer are read from the SAC itself.
    pub fn list_asset(env: Env, sac: Address) -> AssetRecord {
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
        };
        write_persistent(&env, &key, &record);
        let mut index = assets(&env);
        index.push_back(sac.clone());
        write_persistent(&env, &DataKey::Assets, &index);
        bump_instance(&env);
        AssetListed { sac, code, issuer }.publish(&env);
        record
    }

    /// Record the circulating supply of `sac` as computed at `ledger`.
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
        bump_instance(&env);
        SupplyPosted {
            sac,
            amount,
            ledger,
            breakdown_hash,
            poster,
        }
        .publish(&env);
    }

    /// Record a reserve figure for `sac`. The reporter's identity sets the
    /// tier: the asset's issuer account, a registered auditor, or a
    /// registered transcriber.
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
        let tier = if reporter == asset.issuer {
            Tier::IssuerSigned
        } else {
            match RegistryClient::new(&env, &registry(&env)).role_of(&reporter) {
                Some(Role::Auditor) => Tier::AuditorSigned,
                Some(Role::Transcriber) => Tier::Transcribed,
                _ => panic_with_error!(&env, LedgerError::NotReporter),
            }
        };
        if amount < 0 {
            panic_with_error!(&env, LedgerError::InvalidAmount);
        }
        if doc_uri.is_empty() || doc_uri.len() > MAX_URI_LEN {
            panic_with_error!(&env, LedgerError::InvalidUri);
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
                    let bps = coverage_bps(amount, supply.amount);
                    if !(MIN_PLAUSIBLE_BPS..=MAX_PLAUSIBLE_BPS).contains(&bps) {
                        panic_with_error!(&env, LedgerError::ImplausibleAmount);
                    }
                }
            }
        }

        migrate_legacy(&env, &sac, tier);
        // Staleness is per reporter: another reporter's figure at this tier,
        // however recent, does not stop this one being recorded.
        let key = DataKey::ReportBy(sac.clone(), tier, reporter.clone());
        if let Some(previous) = read_persistent::<ReserveReport>(&env, &key) {
            if as_of <= previous.as_of {
                panic_with_error!(&env, LedgerError::StaleReport);
            }
        }
        let mut index = reporters(&env, &sac, tier);
        if !index.contains(&reporter) {
            if index.len() >= MAX_REPORTERS_PER_TIER {
                let registry = RegistryClient::new(&env, &registry(&env));
                index = reclaim_slots(&env, &registry, &sac, tier, &index);
            }
            if index.len() >= MAX_REPORTERS_PER_TIER {
                panic_with_error!(&env, LedgerError::TooManyReporters);
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
        bump_instance(&env);
        ReservePosted {
            sac,
            tier,
            amount,
            as_of,
            reporter,
            doc_hash,
            doc_uri,
        }
        .publish(&env);
        tier
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
        let index_key = DataKey::Reporters(sac.clone(), tier);
        if index.is_empty() {
            env.storage().persistent().remove(&index_key);
        } else {
            write_persistent(&env, &index_key, &index);
        }
        bump_instance(&env);
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

    pub fn get_supply(env: Env, sac: Address) -> Option<SupplySnapshot> {
        read_persistent(&env, &DataKey::Supply(sac))
    }

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

    /// The freshest report at `tier` that still counts.
    pub fn get_report(env: Env, sac: Address, tier: Tier) -> Option<ReserveReport> {
        freshest_at(&env, &sac, tier)
    }

    /// Every stored report that still counts, strongest tier first, then
    /// first-post order within a tier.
    pub fn get_reports(env: Env, sac: Address) -> Vec<ReserveReport> {
        let registry = RegistryClient::new(&env, &registry(&env));
        let mut out = Vec::new(&env);
        for tier in Tier::ALL_DESC {
            for report in reports_at(&env, &sac, tier).iter() {
                if counts(&registry, &report) {
                    out.push_back(report);
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

    /// A page of the (asset, reporter) pairs the ledger holds figures for, so
    /// an indexer can bootstrap its dataset without walking every asset.
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
        let start = cursor.unwrap_or(0);
        let page = limit.min(MAX_PAIRS_PER_CALL);
        let mut out = Vec::new(&env);
        if page == 0 {
            return out;
        }
        let mut index = 0u32;
        for asset in assets(&env).iter() {
            let mut reported = Vec::new(&env);
            for tier in Tier::ALL_DESC {
                for reporter in reporters(&env, &asset, tier).iter() {
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
        out
    }

    /// Latest supply against the freshest report at `min_tier` or stronger.
    /// If `sac` has a registered alias its successor is used transparently
    /// (at most one hop — the successor's own alias, if any, is not followed).
    pub fn coverage(env: Env, sac: Address, min_tier: Tier) -> Option<Coverage> {
        let resolved = read_persistent::<Address>(&env, &DataKey::Alias(sac.clone()))
            .unwrap_or_else(|| sac.clone());
        let supply = effective_supply(&env, &resolved)?;
        let report = best_report(&env, &resolved, min_tier)?;
        Some(Coverage {
            bps: coverage_bps(report.amount, supply.amount),
            supply: supply.amount,
            reserves: report.amount,
            tier: report.tier,
            supply_ledger: supply.ledger,
            supply_timestamp: supply.timestamp,
            report_as_of: report.as_of,
        })
    }

    /// True when both the supply snapshot and the chosen report are at most
    /// `max_age` seconds old and coverage is at least `min_bps` (inclusive).
    ///
    /// If `sac` has a registered alias its successor is used transparently
    /// (at most one hop). See `alias_asset` and `get_alias`.
    ///
    /// Performance note: the supply snapshot is loaded first. When it is
    /// absent or already older than `max_age` the function returns false
    /// immediately, before reading any reserve reports. This is the common
    /// early-exit path: a stale supply avoids all subsequent storage reads.
    pub fn is_covered(env: Env, sac: Address, min_bps: u32, max_age: u64, min_tier: Tier) -> bool {
        match Self::coverage_status(env.clone(), sac, min_bps, max_age, min_tier) {
            CoverageStatus::Paused => panic_with_error!(&env, LedgerError::Paused),
            CoverageStatus::Covered(_) => true,
            CoverageStatus::Stale(_) | CoverageStatus::Uncovered => false,
        }
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
