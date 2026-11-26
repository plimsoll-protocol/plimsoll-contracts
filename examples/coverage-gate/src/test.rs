//! Every edge case docs/INTEGRATING.md states, checked against the real
//! coverage-ledger and reporter-registry.
extern crate std;

use super::*;
use coverage_ledger::{CoverageLedger, CoverageLedgerClient as LedgerClient};
use plimsoll_types::{CoverageStatus, Role, PERSISTENT_EXTEND_TO};
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Ledger as _},
    Address, BytesN, Env, String, Symbol,
};

const T0: u64 = 1_700_000_000;
const DAY: u64 = 86_400;
const UNIT: i128 = 10_000_000;

struct Fixture {
    env: Env,
    gate: CoverageGateClient<'static>,
    ledger: LedgerClient<'static>,
    registry: ReporterRegistryClient<'static>,
    sac: Address,
    issuer: Address,
    poster: Address,
    auditor: Address,
    transcriber: Address,
}

/// Fully backed, figures at most a day old, issuer-signed or stronger.
fn setup() -> Fixture {
    setup_with(10_000, DAY, Tier::IssuerSigned)
}

fn setup_with(min_bps: u32, max_age: u64, min_tier: Tier) -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    env.ledger().set_sequence_number(1_000);

    let admin = Address::generate(&env);
    let registry_id = env.register(ReporterRegistry, (admin.clone(),));
    let registry = ReporterRegistryClient::new(&env, &registry_id);
    let ledger_id = env.register(CoverageLedger, (admin, registry_id));
    let ledger = LedgerClient::new(&env, &ledger_id);

    let handle = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let sac = handle.address();
    let issuer = handle.issuer().address();
    ledger.list_asset(&sac);

    let poster = Address::generate(&env);
    let auditor = Address::generate(&env);
    let transcriber = Address::generate(&env);
    registry.set_reporter(&poster, &Role::SupplyPoster, &name(&env, "indexer"));
    registry.set_reporter(&auditor, &Role::Auditor, &name(&env, "Acme Audit"));
    registry.set_reporter(&transcriber, &Role::Transcriber, &name(&env, "volunteer"));

    let gate_id = env.register(
        CoverageGate,
        (ledger_id, sac.clone(), min_bps, max_age, min_tier),
    );
    let gate = CoverageGateClient::new(&env, &gate_id);

    Fixture {
        env,
        gate,
        ledger,
        registry,
        sac,
        issuer,
        poster,
        auditor,
        transcriber,
    }
}

fn name(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
        li.sequence_number += (seconds / 5) as u32;
    });
}

impl Fixture {
    /// Supply computed at the current ledger, posted now.
    fn supply(&self, amount: i128) {
        let seq = self.env.ledger().sequence();
        self.supply_at(amount, seq);
    }

    fn supply_at(&self, amount: i128, ledger: u32) {
        self.ledger.post_supply(
            &self.poster,
            &self.sac,
            &amount,
            &ledger,
            &BytesN::from_array(&self.env, &[1; 32]),
        );
    }

    /// A reserve figure from `reporter`, stated as of `as_of`.
    fn reserve(&self, reporter: &Address, amount: i128, as_of: u64) {
        self.ledger.post_reserve(
            reporter,
            &self.sac,
            &amount,
            &as_of,
            &BytesN::from_array(&self.env, &[2; 32]),
            &name(&self.env, "https://issuer.example/reserves.pdf"),
        );
    }

    fn now(&self) -> u64 {
        self.env.ledger().timestamp()
    }
}

// ---- the gate itself ------------------------------------------------------

#[test]
fn accept_when_covered() {
    let f = setup();
    f.supply(1_000 * UNIT);
    f.reserve(&f.issuer, 1_000 * UNIT, f.now());
    assert!(f.gate.is_open());
    assert_eq!(
        f.gate.accept(&Address::generate(&f.env), &(5 * UNIT)),
        5 * UNIT
    );
    assert_eq!(
        f.gate.accept(&Address::generate(&f.env), &(2 * UNIT)),
        7 * UNIT
    );
}

#[test]
fn accept_refused_when_not_covered() {
    let f = setup();
    f.supply(1_000 * UNIT);
    f.reserve(&f.issuer, 999 * UNIT, f.now());
    assert!(!f.gate.is_open());
    assert_eq!(
        f.gate.try_accept(&Address::generate(&f.env), &UNIT),
        Err(Ok(GateError::NotCovered.into()))
    );
}

#[test]
fn accept_rejects_non_positive_amount() {
    let f = setup();
    let from = Address::generate(&f.env);
    assert_eq!(
        f.gate.try_accept(&from, &0),
        Err(Ok(GateError::InvalidAmount.into()))
    );
    assert_eq!(
        f.gate.try_accept(&from, &-1),
        Err(Ok(GateError::InvalidAmount.into()))
    );
}

#[test]
fn constructor_stores_policy() {
    let f = setup();
    assert!(!f.gate.is_open());
    let stored = f.env.as_contract(&f.gate.address, || policy(&f.env));
    assert_eq!(stored.ledger, f.ledger.address);
    assert_eq!(stored.asset, f.sac);
    assert_eq!(stored.min_bps, 10_000);
    assert_eq!(stored.max_age, DAY);
    assert_eq!(stored.min_tier, Tier::IssuerSigned);
}

// ---- min_bps --------------------------------------------------------------

#[test]
fn bps_rounds_down_so_a_hair_under_is_refused() {
    let f = setup();
    // 9_999.99... bps rounds down to 9_999.
    f.supply(1_000 * UNIT);
    f.reserve(&f.issuer, 1_000 * UNIT - 1, f.now());
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap().bps,
        9_999
    );
    assert!(!f.gate.is_open());
}

#[test]
fn zero_supply_reads_as_max_bps_and_is_covered() {
    let f = setup();
    f.supply(0);
    f.reserve(&f.issuer, 0, f.now());
    let c = f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(c.bps, u32::MAX);
    assert!(f.gate.is_open());
}

// ---- no figures -----------------------------------------------------------

#[test]
fn no_report_is_not_covered() {
    let f = setup();
    f.supply(1_000 * UNIT);
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
    assert!(!f.gate.is_open());
}

#[test]
fn no_snapshot_is_not_covered() {
    let f = setup();
    f.reserve(&f.issuer, 1_000 * UNIT, f.now());
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
    assert!(!f.gate.is_open());
}

#[test]
fn unlisted_asset_is_not_covered() {
    let f = setup();
    let other = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    assert_eq!(
        f.ledger
            .is_covered(&other, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
}

// ---- max_age --------------------------------------------------------------

#[test]
fn report_freshness_uses_as_of_not_post_time() {
    let f = setup();
    f.supply(1_000 * UNIT);
    // Posted just now, but stating reserves as of two days ago.
    f.reserve(&f.issuer, 1_000 * UNIT, f.now() - 2 * DAY);
    assert!(!f.gate.is_open());
}

#[test]
fn snapshot_freshness_uses_post_time_not_its_ledger() {
    let f = setup();
    advance(&f.env, 3 * DAY);
    // Computed at a ledger two days back, but posted just now: fresh.
    let two_days_ago = f.env.ledger().sequence() - (2 * DAY / 5) as u32;
    f.supply_at(1_000 * UNIT, two_days_ago);
    f.reserve(&f.issuer, 1_000 * UNIT, f.now());
    assert!(f.gate.is_open());
    // A day and a second later the snapshot is stale, whatever its ledger.
    advance(&f.env, DAY + 1);
    f.reserve(&f.issuer, 1_000 * UNIT, f.now());
    assert!(!f.gate.is_open());
}

#[test]
fn max_age_is_inclusive() {
    let f = setup();
    f.supply(1_000 * UNIT);
    f.reserve(&f.issuer, 1_000 * UNIT, f.now());
    advance(&f.env, DAY);
    assert!(f.gate.is_open());
    advance(&f.env, 1);
    assert!(!f.gate.is_open());
}

// ---- min_tier -------------------------------------------------------------

#[test]
fn weaker_tier_does_not_count() {
    let f = setup();
    f.supply(1_000 * UNIT);
    f.reserve(&f.transcriber, 1_000 * UNIT, f.now());
    assert!(!f.gate.is_open());
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::Transcribed),
        CoverageStatus::Covered
    );
}

#[test]
fn fresher_report_wins_even_if_it_is_worse() {
    let f = setup();
    f.supply(1_000 * UNIT);
    f.reserve(&f.auditor, 1_100 * UNIT, f.now() - 60);
    // The issuer's figure is newer, so it is the one judged, and it is short.
    f.reserve(&f.issuer, 900 * UNIT, f.now());
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap().tier,
        Tier::IssuerSigned
    );
    assert!(!f.gate.is_open());
}

#[test]
fn revoked_auditor_stops_counting_at_once() {
    let f = setup_with(10_000, DAY, Tier::AuditorSigned);
    f.supply(1_000 * UNIT);
    f.reserve(&f.auditor, 1_000 * UNIT, f.now());
    assert!(f.gate.is_open());
    f.registry.revoke(&f.auditor);
    assert!(!f.gate.is_open());
}

// ---- archived entries -----------------------------------------------------

#[test]
fn archived_report_is_restored_and_judged_by_its_age() {
    let f = setup_with(10_000, u64::MAX, Tier::IssuerSigned);
    f.supply(1_000 * UNIT);
    f.reserve(&f.issuer, 1_000 * UNIT, T0);
    let key = (
        Symbol::new(&f.env, "ReportBy"),
        f.sac.clone(),
        Tier::IssuerSigned,
        f.issuer.clone(),
    );
    let ttl = f.env.as_contract(&f.ledger.address, || {
        f.env.storage().persistent().get_ttl(&key)
    });
    assert_eq!(ttl, PERSISTENT_EXTEND_TO);

    // Nobody touches the report until a ledger after its TTL ran out.
    f.env.ledger().with_mut(|li| {
        li.sequence_number += PERSISTENT_EXTEND_TO + 1;
        li.timestamp += (PERSISTENT_EXTEND_TO as u64 + 1) * 5;
    });
    f.supply(1_000 * UNIT);

    // The host restores the entry and the figure still counts...
    assert!(f.gate.is_open());
    // ...but it is now over 90 days old, so any shorter max_age refuses it.
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &(89 * DAY), &Tier::IssuerSigned),
        CoverageStatus::Uncovered
    );
}

// ---- the guide ------------------------------------------------------------

/// docs/INTEGRATING.md quotes this crate's lib.rs. Keep them identical, so the
/// example a reader copies is one that compiles and passes these tests.
#[test]
fn guide_quotes_this_example_verbatim() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let guide = std::fs::read_to_string(root.join("../../docs/INTEGRATING.md"))
        .expect("docs/INTEGRATING.md exists");
    let source = std::fs::read_to_string(root.join("src/lib.rs")).expect("src/lib.rs exists");
    let fenced = std::format!("```rust\n{source}```");
    assert!(
        guide.contains(&fenced),
        "docs/INTEGRATING.md no longer quotes examples/coverage-gate/src/lib.rs \
         verbatim. Paste the current file into the guide's example block."
    );
}
