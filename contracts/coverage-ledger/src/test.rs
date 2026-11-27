extern crate std;

use super::*;
use plimsoll_types::CoverageStatus;
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Events, Ledger as _, MockAuth, MockAuthInvoke},
    xdr::{ContractEventBody, ScErrorCode, ScErrorType},
    Address, Bytes, BytesN, Env, Error, IntoVal, Map, String, Symbol, TryFromVal, Val,
};

const T0: u64 = 1_700_000_000;
const UNIT: i128 = 10_000_000;
const UPGRADE_WASM: &[u8] = include_bytes!("../../../testdata/upgrade.wasm");

#[allow(dead_code)]
fn upgrade_fixture_wasm() -> std::vec::Vec<u8> {
    const VENDORED: &[u8] =
        include_bytes!("../../upgrade-fixture-ledger/testdata/upgrade_fixture_ledger.wasm");
    let built = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/wasm32v1-none/release/upgrade_fixture_ledger.wasm");
    std::fs::read(built).unwrap_or_else(|_| VENDORED.to_vec())
}

#[allow(dead_code)]
fn upload_upgrade_fixture(env: &Env) -> BytesN<32> {
    env.deployer()
        .upload_contract_wasm(Bytes::from_slice(env, &upgrade_fixture_wasm()))
}

#[allow(dead_code)]
struct Fixture {
    env: Env,
    admin: Address,
    ledger: CoverageLedgerClient<'static>,
    registry: ReporterRegistryClient<'static>,
    sac: Address,
    issuer: Address,
    poster: Address,
    auditor: Address,
    transcriber: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    env.ledger().set_sequence_number(1_000);

    let admin = Address::generate(&env);
    let registry_id = env.register(ReporterRegistry, (admin.clone(),));
    let registry = ReporterRegistryClient::new(&env, &registry_id);
    let ledger_id = env.register(CoverageLedger, (admin.clone(), registry_id.clone()));
    let ledger = CoverageLedgerClient::new(&env, &ledger_id);

    // The SAC admin is a contract address; the asset's issuer is the G-account
    // the test host creates, which is what `name()` reports.
    let sac_admin = Address::generate(&env);
    let handle = env.register_stellar_asset_contract_v2(sac_admin);
    let sac = handle.address();
    let issuer = handle.issuer().address();

    let poster = Address::generate(&env);
    let auditor = Address::generate(&env);
    let transcriber = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &String::from_str(&env, "indexer"),
    );
    registry.set_reporter(
        &auditor,
        &Role::Auditor,
        &String::from_str(&env, "Acme Audit"),
    );
    registry.set_reporter(
        &transcriber,
        &Role::Transcriber,
        &String::from_str(&env, "volunteer"),
    );

    Fixture {
        env,
        admin,
        ledger,
        registry,
        sac,
        issuer,
        poster,
        auditor,
        transcriber,
    }
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn uri(env: &Env) -> String {
    String::from_str(env, "https://example.org/attestation-2026-09.pdf")
}

/// The most recent event emitted by the ledger, decoded into the same
/// `Vec<Val>` topics and `Map<Symbol, Val>` data the indexer sees.
fn last_ledger_event(f: &Fixture) -> (Vec<Val>, Map<Symbol, Val>) {
    let events = f.env.events().all().filter_by_contract(&f.ledger.address);
    let event = events.events().last().expect("an event was emitted");
    let ContractEventBody::V0(body) = &event.body;
    let mut topics = Vec::new(&f.env);
    for topic in body.topics.iter() {
        topics.push_back(Val::try_from_val(&f.env, topic).unwrap());
    }
    let data = Val::try_from_val(&f.env, &body.data).unwrap();
    (topics, Map::try_from_val(&f.env, &data).unwrap())
}

fn last_ledger_topics(f: &Fixture) -> Vec<Val> {
    let events = f.env.events().all().filter_by_contract(&f.ledger.address);
    let event = events.events().last().expect("an event was emitted");
    let ContractEventBody::V0(body) = &event.body;
    let mut topics = Vec::new(&f.env);
    for topic in body.topics.iter() {
        topics.push_back(Val::try_from_val(&f.env, topic).unwrap());
    }
    topics
}

fn listed() -> Fixture {
    let f = setup();
    f.ledger.list_asset(&f.sac);
    f
}

// ---- list_asset -----------------------------------------------------------

#[test]
fn list_asset_reads_code_and_issuer_from_sac() {
    let f = setup();
    let record = f.ledger.list_asset(&f.sac);
    assert_eq!(record.issuer, f.issuer);
    assert_eq!(record.sac, f.sac);
    assert_eq!(record.listed_at, T0);
    assert!(!record.code.is_empty());
    assert_eq!(f.ledger.get_asset(&f.sac), Some(record));
}

#[test]
fn list_asset_twice_fails() {
    let f = listed();
    let res = f.ledger.try_list_asset(&f.sac);
    assert_eq!(res, Err(Ok(LedgerError::AlreadyListed.into())));
}

#[test]
fn list_asset_rejects_wasm_contract() {
    let f = setup();
    // The registry is a Wasm-style contract, not a Stellar Asset Contract.
    let res = f.ledger.try_list_asset(&f.registry.address);
    assert_eq!(res, Err(Ok(LedgerError::NotAStellarAsset.into())));
}

#[test]
fn list_asset_rejects_account_address() {
    let f = setup();
    let res = f.ledger.try_list_asset(&Address::generate(&f.env));
    assert!(res.is_err());
}

#[test]
fn asset_count_is_zero_on_a_fresh_ledger() {
    let f = setup();
    assert_eq!(f.ledger.asset_count(), 0);
}

#[test]
fn asset_count_increments_per_listed_asset() {
    let f = setup();
    f.ledger.list_asset(&f.sac);
    assert_eq!(f.ledger.asset_count(), 1);

    let second_admin = Address::generate(&f.env);
    let second_sac = f
        .env
        .register_stellar_asset_contract_v2(second_admin)
        .address();
    f.ledger.list_asset(&second_sac);
    assert_eq!(f.ledger.asset_count(), 2);
}

#[test]
fn parse_rejects_native_and_malformed() {
    let env = Env::default();
    assert!(parse_sac_name(&env, &String::from_str(&env, "native")).is_none());
    assert!(parse_sac_name(&env, &String::from_str(&env, "USDC")).is_none());
    let no_code = std::format!(":{}", "G".repeat(56));
    assert!(parse_sac_name(&env, &String::from_str(&env, &no_code)).is_none());
    let long_code = std::format!("ABCDEFGHIJKLM:{}", "G".repeat(56));
    assert!(parse_sac_name(&env, &String::from_str(&env, &long_code)).is_none());
}

#[test]
fn parse_accepts_real_name() {
    let env = Env::default();
    let name = "USDC:GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN";
    let (code, issuer) = parse_sac_name(&env, &String::from_str(&env, name)).unwrap();
    assert_eq!(code, String::from_str(&env, "USDC"));
    assert_eq!(
        issuer.to_string(),
        String::from_str(
            &env,
            "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN"
        )
    );
}

// ---- events ---------------------------------------------------------------

#[test]
fn asset_listed_event_matches_the_documented_layout() {
    let f = setup();
    let record = f.ledger.list_asset(&f.sac);

    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> =
        (Symbol::new(&f.env, "asset_listed"), f.sac.clone()).into_val(&f.env);
    assert_eq!(topics, expected_topics);
    let expected_data = Map::from_array(
        &f.env,
        [
            (
                Symbol::new(&f.env, "code"),
                record.code.clone().into_val(&f.env),
            ),
            (
                Symbol::new(&f.env, "issuer"),
                record.issuer.clone().into_val(&f.env),
            ),
        ],
    );
    assert_eq!(data, expected_data);
}

#[test]
fn supply_posted_event_matches_the_documented_layout() {
    let f = listed();
    let amount = 1_000 * UNIT;
    let ledger = 990u32;
    let breakdown_hash = hash(&f.env, 1);
    f.ledger
        .post_supply(&f.poster, &f.sac, &amount, &ledger, &breakdown_hash);

    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> =
        (Symbol::new(&f.env, "supply_posted"), f.sac.clone()).into_val(&f.env);
    assert_eq!(topics, expected_topics);
    let expected_data = Map::from_array(
        &f.env,
        [
            (Symbol::new(&f.env, "amount"), amount.into_val(&f.env)),
            (Symbol::new(&f.env, "ledger"), ledger.into_val(&f.env)),
            (
                Symbol::new(&f.env, "breakdown_hash"),
                breakdown_hash.into_val(&f.env),
            ),
            (
                Symbol::new(&f.env, "poster"),
                f.poster.clone().into_val(&f.env),
            ),
        ],
    );
    assert_eq!(data, expected_data);
}

#[test]
fn reserve_posted_event_matches_the_documented_layout() {
    let f = listed();
    let amount = UNIT;
    let as_of = T0 - 60;
    let doc_hash = hash(&f.env, 2);
    let doc_uri = uri(&f.env);
    f.ledger
        .post_reserve(&f.auditor, &f.sac, &amount, &as_of, &doc_hash, &doc_uri);

    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> = (
        Symbol::new(&f.env, "reserve_posted"),
        f.sac.clone(),
        Tier::AuditorSigned,
    )
        .into_val(&f.env);
    assert_eq!(topics, expected_topics);
    let expected_data = Map::from_array(
        &f.env,
        [
            (Symbol::new(&f.env, "amount"), amount.into_val(&f.env)),
            (Symbol::new(&f.env, "as_of"), as_of.into_val(&f.env)),
            (
                Symbol::new(&f.env, "reporter"),
                f.auditor.clone().into_val(&f.env),
            ),
            (Symbol::new(&f.env, "doc_hash"), doc_hash.into_val(&f.env)),
            (Symbol::new(&f.env, "doc_uri"), doc_uri.into_val(&f.env)),
        ],
    );
    assert_eq!(data, expected_data);
}

#[test]
fn admin_transferred_event_matches_the_documented_layout() {
    let f = setup();
    let next = Address::generate(&f.env);
    f.ledger.transfer_admin(&next);

    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> =
        (Symbol::new(&f.env, "admin_transferred"), next).into_val(&f.env);
    assert_eq!(topics, expected_topics);
    assert_eq!(data, Map::<Symbol, Val>::new(&f.env));
}

// ---- post_supply ----------------------------------------------------------

#[test]
fn post_supply_stores_snapshot() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(1_000 * UNIT), &990, &hash(&f.env, 1));
    let s = f.ledger.get_supply(&f.sac).unwrap();
    assert_eq!(s.amount, 1_000 * UNIT);
    assert_eq!(s.ledger, 990);
    assert_eq!(s.timestamp, T0);
    assert_eq!(s.poster, f.poster);
}

#[test]
fn post_supply_requires_poster_role() {
    let f = listed();
    let res = f
        .ledger
        .try_post_supply(&f.auditor, &f.sac, &UNIT, &990, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::NotSupplyPoster.into())));
}

#[test]
fn post_supply_requires_listed_asset() {
    let f = setup();
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::AssetNotListed.into())));
}

#[test]
fn post_supply_rejects_negative() {
    let f = listed();
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &-1, &990, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::InvalidAmount.into())));
}

#[test]
fn post_supply_rejects_future_ledger() {
    let f = listed();
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &UNIT, &1_001, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::FutureLedger.into())));
}

#[test]
fn post_supply_accepts_snapshot_at_maximum_age() {
    let f = listed();
    let ledger = 1_000 - MAX_SUPPLY_LEDGER_AGE;
    f.ledger
        .post_supply(&f.poster, &f.sac, &UNIT, &ledger, &hash(&f.env, 1));
    assert_eq!(f.ledger.get_supply(&f.sac).unwrap().ledger, ledger);
}

#[test]
fn post_supply_rejects_snapshot_older_than_maximum_age() {
    let f = listed();
    let ledger = 1_000 - MAX_SUPPLY_LEDGER_AGE - 1;
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &UNIT, &ledger, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::LedgerTooOld.into())));
}

#[test]
fn post_supply_must_move_forward() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1));
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 2));
    assert_eq!(res, Err(Ok(LedgerError::StaleLedger.into())));
}

#[test]
fn revoked_poster_cannot_post() {
    let f = listed();
    f.registry.revoke(&f.poster);
    let res = f
        .ledger
        .try_post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1));
    assert_eq!(res, Err(Ok(LedgerError::NotSupplyPoster.into())));
}

// ---- post_reserve ---------------------------------------------------------

#[test]
fn issuer_report_is_issuer_signed() {
    let f = listed();
    let tier = f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(1_000 * UNIT),
        &(T0 - 60),
        &hash(&f.env, 9),
        &uri(&f.env),
    );
    assert_eq!(tier, Tier::IssuerSigned);
    let r = f.ledger.get_report(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(r.amount, 1_000 * UNIT);
    assert_eq!(r.reporter, f.issuer);
    assert_eq!(r.posted_at, T0);
}

#[test]
fn auditor_and_transcriber_tiers() {
    let f = listed();
    let a = f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    let t = f.ledger.post_reserve(
        &f.transcriber,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    assert_eq!(a, Tier::AuditorSigned);
    assert_eq!(t, Tier::Transcribed);
}

#[test]
fn stranger_cannot_report() {
    let f = listed();
    let res = f.ledger.try_post_reserve(
        &Address::generate(&f.env),
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::NotReporter.into())));
}

#[test]
fn supply_poster_cannot_report_reserves() {
    let f = listed();
    let res = f.ledger.try_post_reserve(
        &f.poster,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::NotReporter.into())));
}

#[test]
fn report_rejects_future_as_of() {
    let f = listed();
    let res = f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &(T0 + 1),
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::FutureTimestamp.into())));
}

#[test]
fn report_must_move_forward_per_tier() {
    let f = listed();
    f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    let res = f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::StaleReport.into())));
    // A different tier at the same timestamp is fine.
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 3),
        &uri(&f.env),
    );
}

#[test]
fn report_rejects_empty_uri() {
    let f = listed();
    let res = f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &String::from_str(&f.env, ""),
    );
    assert_eq!(res, Err(Ok(LedgerError::InvalidUri.into())));
}

#[test]
fn report_rejects_negative_amount() {
    let f = listed();
    let res =
        f.ledger
            .try_post_reserve(&f.auditor, &f.sac, &-5, &T0, &hash(&f.env, 1), &uri(&f.env));
    assert_eq!(res, Err(Ok(LedgerError::InvalidAmount.into())));
}

/// Posts `amount` as an auditor report against a supply of `supply`, returning
/// what `try_post_reserve` gave back so a caller can assert on the error.
type TryReserve = Result<
    Result<Tier, soroban_sdk::ConversionError>,
    Result<soroban_sdk::Error, soroban_sdk::InvokeError>,
>;

fn report_against_supply(f: &Fixture, supply: i128, amount: i128) -> TryReserve {
    f.ledger
        .post_supply(&f.poster, &f.sac, &supply, &990, &hash(&f.env, 1));
    f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &amount,
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    )
}

// The slip this guards against: 1,020 reserves typed in whole units rather
// than the 7-decimal units the contract counts in. The asset would read as
// 0.00% covered, which is what a collapse of backing looks like too.
#[test]
fn report_rejects_a_figure_typed_in_whole_units() {
    let f = listed();
    let res = report_against_supply(&f, 1_000 * UNIT, 1_020);
    assert_eq!(res, Err(Ok(LedgerError::ImplausibleAmount.into())));
}

#[test]
fn report_accepts_the_narrowest_plausible_coverage() {
    let f = listed();
    // 1 unit of reserves against 10,000 of supply is exactly 1 bps.
    assert!(report_against_supply(&f, 10_000 * UNIT, UNIT).is_ok());
}

#[test]
fn report_rejects_coverage_below_the_narrowest() {
    let f = listed();
    // One stroop less rounds to 0 bps.
    let res = report_against_supply(&f, 10_000 * UNIT, UNIT - 1);
    assert_eq!(res, Err(Ok(LedgerError::ImplausibleAmount.into())));
}

#[test]
fn report_accepts_the_widest_plausible_coverage() {
    let f = listed();
    // 100 units against 1 is 1,000,000 bps, or 10,000%.
    assert!(report_against_supply(&f, UNIT, 100 * UNIT).is_ok());
}

#[test]
fn report_rejects_coverage_above_the_widest() {
    let f = listed();
    // 1,000,001 bps: the smallest figure over the ceiling.
    let res = report_against_supply(&f, UNIT, 100 * UNIT + 1_000);
    assert_eq!(res, Err(Ok(LedgerError::ImplausibleAmount.into())));
}

// With no snapshot there is nothing to judge a figure against, and refusing
// reports until someone posts supply would stop an asset being described at
// all.
#[test]
fn report_skips_the_plausibility_check_when_no_supply_is_posted() {
    let f = listed();
    f.ledger
        .post_reserve(&f.auditor, &f.sac, &1, &T0, &hash(&f.env, 1), &uri(&f.env));
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .amount,
        1
    );
}

// No reserves at all is a statement the protocol exists to carry, and unlike a
// small non-zero figure it cannot be a unit slip.
#[test]
fn report_accepts_zero_reserves() {
    let f = listed();
    assert!(report_against_supply(&f, 1_000 * UNIT, 0).is_ok());
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap().bps,
        0
    );
}

// ---- get_reports ----------------------------------------------------------

#[test]
fn get_reports_is_empty_when_nothing_is_posted() {
    let f = listed();
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 0);
}

#[test]
fn get_reports_returns_the_one_posted_tier() {
    let f = listed();
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(1_000 * UNIT),
        &(T0 - 60),
        &hash(&f.env, 9),
        &uri(&f.env),
    );
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 1);
    let only = reports.get(0).unwrap();
    assert_eq!(only.tier, Tier::IssuerSigned);
    assert_eq!(only.amount, 1_000 * UNIT);
}

#[test]
fn get_reports_returns_every_tier_strongest_first() {
    let f = listed();
    f.ledger.post_reserve(
        &f.transcriber,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(2 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &(3 * UNIT),
        &T0,
        &hash(&f.env, 3),
        &uri(&f.env),
    );
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 3);
    assert_eq!(reports.get(0).unwrap().tier, Tier::AuditorSigned);
    assert_eq!(reports.get(1).unwrap().tier, Tier::IssuerSigned);
    assert_eq!(reports.get(2).unwrap().tier, Tier::Transcribed);
    // Same order as calling get_report once per tier.
    for report in reports.iter() {
        assert_eq!(f.ledger.get_report(&f.sac, &report.tier), Some(report));
    }
}

#[test]
fn get_reports_keeps_the_latest_per_tier() {
    let f = listed();
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &UNIT,
        &(T0 - 600),
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(5 * UNIT),
        &(T0 - 60),
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports.get(0).unwrap().amount, 5 * UNIT);
}

#[test]
fn get_reports_on_an_unlisted_asset_is_empty() {
    let f = setup();
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 0);
}

// ---- coverage / is_covered ------------------------------------------------

#[test]
fn coverage_none_without_data() {
    let f = listed();
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
    f.ledger
        .post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1));
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
}

#[test]
fn coverage_computes_bps() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(1_000 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &(1_020 * UNIT),
        &(T0 - 100),
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.bps, 10_200);
    assert_eq!(c.tier, Tier::AuditorSigned);
    assert_eq!(c.supply, 1_000 * UNIT);
    assert_eq!(c.reserves, 1_020 * UNIT);
    assert_eq!(c.supply_ledger, 990);
    assert_eq!(c.report_as_of, T0 - 100);
}

#[test]
fn coverage_prefers_freshest_report_at_or_above_min_tier() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    // Older audit says 90%, newer transcription says 101%.
    f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &(90 * UNIT),
        &(T0 - 1_000),
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.transcriber,
        &f.sac,
        &(101 * UNIT),
        &(T0 - 10),
        &hash(&f.env, 3),
        &uri(&f.env),
    );
    let any = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(any.tier, Tier::Transcribed);
    assert_eq!(any.bps, 10_100);
    let audited = f.ledger.coverage(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(audited.tier, Tier::AuditorSigned);
    assert_eq!(audited.bps, 9_000);
}

#[test]
fn coverage_tie_goes_to_stronger_tier() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.transcriber,
        &f.sac,
        &(50 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 3),
        &uri(&f.env),
    );
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.tier, Tier::IssuerSigned);
}

#[test]
fn is_covered_checks_ratio_age_and_tier() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    let day = 86_400;
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Covered
    );
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_001, &day, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::AuditorSigned),
        CoverageStatus::Uncovered
    );

    // Two days later both inputs are stale (no grace set → Uncovered).
    f.env.ledger().set_timestamp(T0 + 2 * day);
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &(3 * day), &Tier::Transcribed),
        CoverageStatus::Covered
    );
}

/// is_covered uses `bps >= min_bps` (inclusive boundary).
/// An asset whose coverage ratio equals min_bps exactly must return true,
/// and one bps below must return false. See SPEC.md "is_covered" rules.
#[test]
fn is_covered_at_filters_figures_by_posting_ledger() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.env.ledger().set_sequence_number(1_010);
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );

    assert!(!f
        .ledger
        .is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_000,));
    assert!(f
        .ledger
        .is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_010,));
}

#[test]
fn is_covered_at_cannot_reconstruct_replaced_reports() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    assert!(f
        .ledger
        .is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_000,));

    f.env.ledger().set_sequence_number(1_010);
    f.env.ledger().set_timestamp(T0 + 50);
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &(T0 + 50),
        &hash(&f.env, 3),
        &uri(&f.env),
    );
    assert!(!f
        .ledger
        .is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_005,));
}

#[test]
fn is_covered_at_rejects_a_future_ledger() {
    let f = listed();
    assert_eq!(
        f.ledger
            .try_is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_001,),
        Err(Ok(LedgerError::FutureLedger.into()))
    );
}

#[test]
fn is_covered_at_propagates_pause() {
    let f = listed();
    f.ledger.pause();
    assert_eq!(
        f.ledger
            .try_is_covered_at(&f.sac, &10_000, &86_400, &Tier::Transcribed, &1_000,),
        Err(Ok(LedgerError::Paused.into()))
    );
}

#[test]
fn is_covered_exact_bps_boundary_is_inclusive() {
    let f = listed();
    // 100 reserves against 100 supply → exactly 10_000 bps (100 %).
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    let day = 86_400u64;
    // Exactly at the boundary → covered.
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));
    // One bps above the actual coverage → not covered.
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_001, &day, &Tier::Transcribed));
}

#[test]
fn is_covered_false_for_unlisted() {
    let f = setup();
    assert_eq!(
        f.ledger.is_covered(&f.sac, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
}

#[test]
fn is_covered_stale_within_grace_window() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    // Set a 1-day grace window (17_280 ledgers × 5 s = 86_400 s).
    f.ledger.set_ttl_grace(&f.admin, &17_280u32);

    let day = 86_400u64;
    // Exactly at max_age → still Covered.
    f.env.ledger().set_timestamp(T0 + day);
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Covered
    );
    // One second past max_age, within grace → Stale.
    f.env.ledger().set_timestamp(T0 + day + 1);
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Stale
    );
    // Exactly at max_age + grace_secs → still Stale.
    f.env.ledger().set_timestamp(T0 + 2 * day);
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Stale
    );
    // One second beyond grace → Uncovered.
    f.env.ledger().set_timestamp(T0 + 2 * day + 1);
    assert_eq!(
        f.ledger.is_covered(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
}

#[test]
fn set_ttl_grace_stores_and_ttl_grace_reads_back() {
    let f = setup();
    assert_eq!(f.ledger.ttl_grace(), 0);
    f.ledger.set_ttl_grace(&f.admin, &1_000u32);
    assert_eq!(f.ledger.ttl_grace(), 1_000);
    f.ledger.set_ttl_grace(&f.admin, &0u32);
    assert_eq!(f.ledger.ttl_grace(), 0);
}

#[test]
fn zero_supply_counts_as_covered() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &0, &990, &hash(&f.env, 1));
    f.ledger
        .post_reserve(&f.issuer, &f.sac, &0, &T0, &hash(&f.env, 2), &uri(&f.env));
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.bps, u32::MAX);
}

// ---- asset aliasing (#80) -------------------------------------------------

fn second_sac(f: &Fixture) -> Address {
    let handle = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env));
    let sac = handle.address();
    f.ledger.list_asset(&sac);
    sac
}

#[test]
fn alias_asset_redirects_coverage_queries() {
    let f = listed();
    let new_sac = second_sac(&f);
    supply(&f, 100 * UNIT);

    // Post supply and reserve for the new asset (use auditor; f.issuer is new_sac's issuer only
    // when the SAC admin happens to match, but here we use the registered auditor to be safe).
    f.ledger
        .post_supply(&f.poster, &new_sac, &(100 * UNIT), &990, &hash(&f.env, 10));
    f.ledger.post_reserve(
        &f.auditor,
        &new_sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 11),
        &uri(&f.env),
    );

    // Before aliasing, old SAC has no data.
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);

    f.ledger.alias_asset(&f.sac, &new_sac);
    assert_eq!(f.ledger.get_alias(&f.sac), Some(new_sac.clone()));

    // After aliasing, coverage and is_covered transparently use new_sac.
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.bps, 10_000);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));
}

#[test]
fn alias_asset_requires_both_assets_listed() {
    let f = listed();
    let unlisted = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    let res = f.ledger.try_alias_asset(&f.sac, &unlisted);
    assert_eq!(res, Err(Ok(LedgerError::AssetNotListed.into())));
}

#[test]
fn alias_asset_rejects_self_alias() {
    let f = listed();
    let res = f.ledger.try_alias_asset(&f.sac, &f.sac);
    assert_eq!(res, Err(Ok(LedgerError::SelfAlias.into())));
}

#[test]
fn alias_asset_requires_admin_auth() {
    let f = listed();
    let new_sac = second_sac(&f);
    let non_admin = Address::generate(&f.env);
    authorise_only(
        &f,
        &[(
            &non_admin,
            "alias_asset",
            (f.sac.clone(), new_sac.clone()).into_val(&f.env),
        )],
    );
    assert_eq!(
        f.ledger.try_alias_asset(&f.sac, &new_sac),
        Err(Ok(auth_failure()))
    );
}

#[test]
fn alias_follows_at_most_one_hop() {
    let f = listed();
    let mid = second_sac(&f);
    let handle = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env));
    let end = handle.address();
    f.ledger.list_asset(&end);

    // Supply + reserve only on `end` (use auditor since we don't have end's issuer).
    f.ledger
        .post_supply(&f.poster, &end, &(100 * UNIT), &990, &hash(&f.env, 20));
    f.ledger.post_reserve(
        &f.auditor,
        &end,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 21),
        &uri(&f.env),
    );

    // Chain: sac → mid → end (but only one hop is followed).
    f.ledger.alias_asset(&f.sac, &mid);
    f.ledger.alias_asset(&mid, &end);

    // is_covered(sac) follows sac→mid but NOT mid→end, so it sees no data.
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));
    // is_covered(mid) follows mid→end and correctly sees the data.
    assert!(f
        .ledger
        .is_covered(&mid, &10_000, &86_400, &Tier::Transcribed));
}

// ---- one report per reporter (#26) ---------------------------------------

fn add_reporter(f: &Fixture, role: Role) -> Address {
    let who = Address::generate(&f.env);
    f.registry
        .set_reporter(&who, &role, &String::from_str(&f.env, "extra"));
    who
}

fn post(f: &Fixture, who: &Address, amount: i128, as_of: u64) -> Tier {
    f.ledger
        .post_reserve(who, &f.sac, &amount, &as_of, &hash(&f.env, 4), &uri(&f.env))
}

fn supply(f: &Fixture, amount: i128) {
    f.ledger
        .post_supply(&f.poster, &f.sac, &amount, &990, &hash(&f.env, 1));
}

#[test]
fn retract_reserve_removes_only_the_callers_report_and_falls_back() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, 90 * UNIT, T0 - 100);
    post(&f, &second, 95 * UNIT, T0 - 10);

    f.ledger.retract_reserve(&second, &f.sac);

    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> = (
        Symbol::new(&f.env, "reserve_retracted"),
        f.sac.clone(),
        Tier::AuditorSigned,
    )
        .into_val(&f.env);
    assert_eq!(topics, expected_topics);
    let expected_data = Map::from_array(
        &f.env,
        [
            (
                Symbol::new(&f.env, "reporter"),
                second.clone().into_val(&f.env),
            ),
            (Symbol::new(&f.env, "as_of"), (T0 - 10).into_val(&f.env)),
        ],
    );
    assert_eq!(data, expected_data);

    let remaining = f.ledger.get_report(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(remaining.reporter, f.auditor);
    assert_eq!(remaining.amount, 90 * UNIT);
    assert_eq!(
        f.ledger
            .coverage(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .reserves,
        90 * UNIT
    );

    f.ledger.retract_reserve(&f.auditor, &f.sac);
    assert!(f.ledger.coverage(&f.sac, &Tier::AuditorSigned).is_none());
}

#[test]
fn retract_reserve_rejects_a_reporter_who_did_not_post_it() {
    let f = listed();
    let other = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, UNIT, T0);

    let res = f.ledger.try_retract_reserve(&other, &f.sac);
    assert_eq!(res, Err(Ok(LedgerError::NoReportToRetract.into())));
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 1);
}

#[test]
fn retract_reserve_rejects_when_the_reporter_has_nothing_to_retract() {
    let f = listed();
    let res = f.ledger.try_retract_reserve(&f.auditor, &f.sac);
    assert_eq!(res, Err(Ok(LedgerError::NoReportToRetract.into())));
}

#[test]
fn retract_reserve_rejects_an_unregistered_reporter() {
    let f = listed();
    let res = f.ledger.try_retract_reserve(&f.poster, &f.sac);
    assert_eq!(res, Err(Ok(LedgerError::NotReporter.into())));
}

#[test]
fn retract_reserve_requires_the_reporters_authorisation() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0);
    authorise_only(&f, &[]);

    assert_eq!(
        f.ledger.try_retract_reserve(&f.auditor, &f.sac),
        Err(Ok(auth_failure()))
    );
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 1);
}

#[test]
fn retract_reserve_requires_a_listed_asset() {
    let f = setup();
    let res = f.ledger.try_retract_reserve(&f.auditor, &f.sac);
    assert_eq!(res, Err(Ok(LedgerError::AssetNotListed.into())));
}

#[test]
fn a_reporter_can_repost_after_retraction() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 10);
    f.ledger.retract_reserve(&f.auditor, &f.sac);

    post(&f, &f.auditor, 2 * UNIT, T0 - 10);

    let report = f.ledger.get_report(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(report.amount, 2 * UNIT);
    assert_eq!(report.as_of, T0 - 10);
}

#[test]
fn two_auditors_reports_coexist_and_coverage_picks_the_freshest() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, 90 * UNIT, T0 - 100);
    post(&f, &second, 105 * UNIT, T0 - 10);

    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 2);
    assert_eq!(reports.get(0).unwrap().reporter, f.auditor);
    assert_eq!(reports.get(0).unwrap().amount, 90 * UNIT);
    assert_eq!(reports.get(1).unwrap().reporter, second);

    let c = f.ledger.coverage(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(c.reserves, 105 * UNIT);
    assert_eq!(c.report_as_of, T0 - 10);
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .reporter,
        second
    );

    // The first auditor's next attestation is fresher again and wins, and the
    // second auditor's figure is still there.
    post(&f, &f.auditor, 95 * UNIT, T0);
    let c = f.ledger.coverage(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(c.reserves, 95 * UNIT);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 2);
}

#[test]
fn an_older_figure_from_another_reporter_is_recorded_not_stale() {
    let f = listed();
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, UNIT, T0);
    // Under one-report-per-tier this was StaleReport and the figure was lost.
    post(&f, &second, 2 * UNIT, T0 - 1_000);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 2);
    // The fresher figure still decides coverage.
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .reporter,
        f.auditor
    );
}

#[test]
fn a_later_transcription_does_not_erase_another_transcribers_figure() {
    let f = listed();
    let second = add_reporter(&f, Role::Transcriber);
    post(&f, &f.transcriber, UNIT, T0 - 100);
    post(&f, &second, 3 * UNIT, T0);
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 2);
    assert!(reports
        .iter()
        .any(|r| r.reporter == f.transcriber && r.amount == UNIT));
}

#[test]
fn staleness_is_per_reporter() {
    let f = listed();
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, UNIT, T0 - 10);
    post(&f, &second, UNIT, T0 - 10);
    let res = f.ledger.try_post_reserve(
        &second,
        &f.sac,
        &UNIT,
        &(T0 - 10),
        &hash(&f.env, 5),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::StaleReport.into())));
}

#[test]
fn a_tie_within_a_tier_goes_to_the_first_reporter_to_post() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, 100 * UNIT, T0);
    post(&f, &second, 50 * UNIT, T0);
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.reserves, 100 * UNIT);
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .reporter,
        f.auditor
    );
}

#[test]
fn a_tie_across_tiers_still_goes_to_the_stronger_tier() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let second = add_reporter(&f, Role::Transcriber);
    post(&f, &f.transcriber, 10 * UNIT, T0);
    post(&f, &second, 20 * UNIT, T0);
    post(&f, &f.issuer, 100 * UNIT, T0);
    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.tier, Tier::IssuerSigned);
}

#[test]
fn reporters_per_tier_are_capped() {
    let f = listed();
    let mut auditors = std::vec![f.auditor.clone()];
    for _ in 1..MAX_REPORTERS_PER_TIER {
        auditors.push(add_reporter(&f, Role::Auditor));
    }
    for (i, a) in auditors.iter().enumerate() {
        post(&f, a, UNIT, T0 - 100 + i as u64);
    }
    let one_too_many = add_reporter(&f, Role::Auditor);
    let res = f.ledger.try_post_reserve(
        &one_too_many,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 5),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::TooManyReporters.into())));

    // A reporter already holding a slot can still post its next figure.
    post(&f, &auditors[0], 2 * UNIT, T0);
    // And the cap is per tier: other tiers are unaffected.
    post(&f, &f.transcriber, UNIT, T0);
    assert_eq!(
        f.ledger.get_reports(&f.sac).len(),
        MAX_REPORTERS_PER_TIER + 1
    );
}

#[test]
fn reporters_per_tier_are_capped_per_asset() {
    let f = listed();
    for _ in 1..MAX_REPORTERS_PER_TIER {
        let a = add_reporter(&f, Role::Auditor);
        post(&f, &a, UNIT, T0);
    }
    post(&f, &f.auditor, UNIT, T0);
    // A second asset has its own slots.
    let other = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    f.ledger.list_asset(&other);
    let late = add_reporter(&f, Role::Auditor);
    f.ledger
        .post_reserve(&late, &other, &UNIT, &T0, &hash(&f.env, 6), &uri(&f.env));
}

#[test]
fn a_reporter_whose_role_changes_starts_a_new_entry_at_the_new_tier() {
    let f = listed();
    post(&f, &f.transcriber, UNIT, T0 - 100);
    f.registry.set_reporter(
        &f.transcriber,
        &Role::Auditor,
        &String::from_str(&f.env, "promoted"),
    );
    // Not stale: the new figure is at a different tier.
    let tier = post(&f, &f.transcriber, 2 * UNIT, T0 - 100);
    assert_eq!(tier, Tier::AuditorSigned);
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .amount,
        2 * UNIT
    );
}

// ---- perf: lazy coverage selection with 20-report-equivalent asset (#78) ---

/// Fills all three tiers to their per-tier cap, posts one legacy entry per
/// tier, then verifies that is_covered returns the correct answer. Ensures the
/// lazy cursor path handles the maximum number of stored reports correctly, and
/// that the early stale-supply exit fires before any report reads.
#[test]
fn is_covered_with_fully_loaded_tiers_returns_correct_result() {
    let f = listed();
    supply(&f, 100 * UNIT);

    // Fill the auditor tier (4 reporters).
    let mut auditors = std::vec![f.auditor.clone()];
    for _ in 1..MAX_REPORTERS_PER_TIER {
        auditors.push(add_reporter(&f, Role::Auditor));
    }
    for (i, a) in auditors.iter().enumerate() {
        post(&f, a, 100 * UNIT, T0 - 400 + i as u64);
    }
    // Fill the transcriber tier (4 reporters).
    let mut transcribers = std::vec![f.transcriber.clone()];
    for _ in 1..MAX_REPORTERS_PER_TIER {
        transcribers.push(add_reporter(&f, Role::Transcriber));
    }
    for (i, t) in transcribers.iter().enumerate() {
        post(&f, t, 100 * UNIT, T0 - 300 + i as u64);
    }
    // Issuer also posts.
    post(&f, &f.issuer, 100 * UNIT, T0 - 200);

    let total = f.ledger.get_reports(&f.sac).len();
    // 4 auditors + 4 transcribers + 1 issuer = 9 counting reports.
    assert_eq!(total, MAX_REPORTERS_PER_TIER * 2 + 1);

    let day = 86_400u64;
    // Fully covered asset with many reporters: should be true.
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));

    // Early-exit on stale supply: advance time beyond max_age for the supply;
    // is_covered must return false without loading reports.
    f.env.ledger().set_timestamp(T0 + 2 * day);
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));
}

// ---- v0.1 storage migration ----------------------------------------------

/// Write a report the way the v0.1 ledger stored it: one per tier.
fn write_legacy(f: &Fixture, report: &ReserveReport) {
    f.env.as_contract(&f.ledger.address, || {
        f.env
            .storage()
            .persistent()
            .set(&DataKey::Report(f.sac.clone(), report.tier), report);
    });
}

fn has_legacy(f: &Fixture, tier: Tier) -> bool {
    f.env.as_contract(&f.ledger.address, || {
        f.env
            .storage()
            .persistent()
            .has(&DataKey::Report(f.sac.clone(), tier))
    })
}

fn legacy_report(
    f: &Fixture,
    reporter: &Address,
    tier: Tier,
    amount: i128,
    as_of: u64,
) -> ReserveReport {
    ReserveReport {
        amount,
        as_of,
        posted_at: as_of,
        tier,
        reporter: reporter.clone(),
        doc_hash: hash(&f.env, 8),
        doc_uri: uri(&f.env),
    }
}

#[test]
fn a_legacy_report_is_read_before_migration() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let legacy = legacy_report(&f, &f.auditor, Tier::AuditorSigned, 101 * UNIT, T0 - 50);
    write_legacy(&f, &legacy);
    assert_eq!(
        f.ledger.get_report(&f.sac, &Tier::AuditorSigned),
        Some(legacy.clone())
    );
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 1);
    let c = f.ledger.coverage(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(c.bps, 10_100);
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &86_400, &Tier::AuditorSigned),
        CoverageStatus::Covered
    );
}

#[test]
fn the_first_post_at_a_tier_migrates_its_legacy_report() {
    let f = listed();
    let legacy = legacy_report(&f, &f.auditor, Tier::AuditorSigned, UNIT, T0 - 50);
    write_legacy(&f, &legacy);
    let issuer_legacy = legacy_report(&f, &f.issuer, Tier::IssuerSigned, UNIT, T0 - 50);
    write_legacy(&f, &issuer_legacy);

    let second = add_reporter(&f, Role::Auditor);
    post(&f, &second, 2 * UNIT, T0 - 60);

    assert!(!has_legacy(&f, Tier::AuditorSigned));
    // Another tier's legacy entry is left until that tier is written.
    assert!(has_legacy(&f, Tier::IssuerSigned));
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 3);
    // Legacy auditor keeps first place in its tier; nothing is counted twice.
    assert_eq!(reports.get(0).unwrap(), legacy);
    assert_eq!(reports.get(1).unwrap().reporter, second);
    assert_eq!(reports.get(2).unwrap(), issuer_legacy);
    assert_eq!(
        f.ledger.get_report(&f.sac, &Tier::AuditorSigned),
        Some(legacy)
    );
}

#[test]
fn a_migrated_reporter_must_still_move_forward() {
    let f = listed();
    write_legacy(
        &f,
        &legacy_report(&f, &f.auditor, Tier::AuditorSigned, UNIT, T0 - 50),
    );
    let res = f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &(T0 - 50),
        &hash(&f.env, 5),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::StaleReport.into())));
    post(&f, &f.auditor, 3 * UNIT, T0 - 49);
    assert!(!has_legacy(&f, Tier::AuditorSigned));
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports.get(0).unwrap().amount, 3 * UNIT);
}

// ---- revoked reporters stop counting (#27) ---------------------------------

const DAY: u64 = 86_400;

/// One fully-covering report per tier, freshest at the weakest tier so that
/// each tier is what decides coverage at its own `min_tier`.
fn one_report_per_tier(f: &Fixture) {
    supply(f, 100 * UNIT);
    post(f, &f.auditor, 100 * UNIT, T0 - 300);
    post(f, &f.issuer, 100 * UNIT, T0 - 200);
    post(f, &f.transcriber, 100 * UNIT, T0 - 100);
}

#[test]
fn revoking_the_auditor_drops_its_report_at_every_tier() {
    let f = listed();
    one_report_per_tier(&f);
    f.registry.revoke(&f.auditor);

    assert_eq!(f.ledger.coverage(&f.sac, &Tier::AuditorSigned), None);
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::AuditorSigned),
        CoverageStatus::Uncovered
    );
    // Weaker floors are still met by the reports that count.
    let issuer = f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(issuer.tier, Tier::IssuerSigned);
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::IssuerSigned),
        CoverageStatus::Covered
    );
    let any = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(any.tier, Tier::Transcribed);

    assert_eq!(f.ledger.get_report(&f.sac, &Tier::AuditorSigned), None);
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.reporter != f.auditor));
}

#[test]
fn revoking_the_transcriber_drops_its_report_at_every_tier() {
    let f = listed();
    one_report_per_tier(&f);
    f.registry.revoke(&f.transcriber);

    // The freshest report overall was the transcription; coverage now falls
    // back to the issuer's older one.
    let any = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(any.tier, Tier::IssuerSigned);
    assert_eq!(any.report_as_of, T0 - 200);
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap().tier,
        Tier::IssuerSigned
    );
    assert_eq!(
        f.ledger
            .coverage(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .tier,
        Tier::AuditorSigned
    );
    assert_eq!(f.ledger.get_report(&f.sac, &Tier::Transcribed), None);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 2);
}

#[test]
fn a_revoked_transcribers_bad_figure_stops_satisfying_is_covered() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 80 * UNIT, T0 - 1_000);
    // A bad actor posts a fresher, flattering figure.
    post(&f, &f.transcriber, 120 * UNIT, T0);
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::Transcribed),
        CoverageStatus::Covered
    );
    f.registry.revoke(&f.transcriber);
    // Immediately, with no action on the ledger.
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap().bps,
        8_000
    );
}

#[test]
fn issuer_signed_reports_are_unaffected_by_the_registry() {
    let f = listed();
    one_report_per_tier(&f);
    f.registry.revoke(&f.auditor);
    f.registry.revoke(&f.transcriber);
    // Even an issuer that was registered and then revoked keeps signing as
    // the issuer.
    f.registry.set_reporter(
        &f.issuer,
        &Role::Transcriber,
        &String::from_str(&f.env, "issuer"),
    );
    f.registry.revoke(&f.issuer);

    for min_tier in [Tier::Transcribed, Tier::IssuerSigned] {
        let c = f.ledger.coverage(&f.sac, &min_tier).unwrap();
        assert_eq!(c.tier, Tier::IssuerSigned);
        assert_eq!(
            f.ledger.is_covered(&f.sac, &10_000, &DAY, &min_tier),
            CoverageStatus::Covered
        );
    }
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::AuditorSigned), None);
    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports.get(0).unwrap().tier, Tier::IssuerSigned);
}

#[test]
fn revoking_the_freshest_auditor_falls_back_to_another_auditor() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);
    post(&f, &second, 150 * UNIT, T0);
    f.registry.revoke(&second);
    let c = f.ledger.coverage(&f.sac, &Tier::AuditorSigned).unwrap();
    assert_eq!(c.reserves, 100 * UNIT);
    assert_eq!(
        f.ledger
            .get_report(&f.sac, &Tier::AuditorSigned)
            .unwrap()
            .reporter,
        f.auditor
    );
}

#[test]
fn a_report_stops_counting_when_its_reporter_changes_role() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0);
    f.registry.set_reporter(
        &f.auditor,
        &Role::Transcriber,
        &String::from_str(&f.env, "demoted"),
    );
    // The auditor-tier figure is no longer backed by an auditor.
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 0);
}

#[test]
fn reinstating_a_reporter_restores_its_report() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0);
    f.registry.revoke(&f.auditor);
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::AuditorSigned), None);
    f.registry.set_reporter(
        &f.auditor,
        &Role::Auditor,
        &String::from_str(&f.env, "Acme Audit"),
    );
    assert_eq!(
        f.ledger
            .is_covered(&f.sac, &10_000, &DAY, &Tier::AuditorSigned),
        CoverageStatus::Covered
    );
}

#[test]
fn a_revoked_reporters_slot_is_reclaimed_when_the_tier_is_full() {
    let f = listed();
    let mut auditors = std::vec![f.auditor.clone()];
    for _ in 1..MAX_REPORTERS_PER_TIER {
        auditors.push(add_reporter(&f, Role::Auditor));
    }
    for a in auditors.iter() {
        post(&f, a, UNIT, T0 - 100);
    }
    f.registry.revoke(&auditors[1]);
    let newcomer = add_reporter(&f, Role::Auditor);
    post(&f, &newcomer, 2 * UNIT, T0);

    let reports = f.ledger.get_reports(&f.sac);
    assert_eq!(reports.len(), MAX_REPORTERS_PER_TIER);
    assert!(reports.iter().any(|r| r.reporter == newcomer));
    // The reclaimed report is gone, not just hidden: reinstating its reporter
    // does not bring it back.
    f.registry.set_reporter(
        &auditors[1],
        &Role::Auditor,
        &String::from_str(&f.env, "back"),
    );
    assert!(f
        .ledger
        .get_reports(&f.sac)
        .iter()
        .all(|r| r.reporter != auditors[1]));
    // ...and with every slot held by a reporter who counts, the tier is full.
    let res = f.ledger.try_post_reserve(
        &auditors[1],
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 5),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::TooManyReporters.into())));
}

#[test]
fn a_revoked_reporters_slot_is_kept_while_the_tier_has_room() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 100);
    f.registry.revoke(&f.auditor);
    let second = add_reporter(&f, Role::Auditor);
    post(&f, &second, UNIT, T0);
    // Reinstated before anyone needed the slot: the old report is still there.
    f.registry.set_reporter(
        &f.auditor,
        &Role::Auditor,
        &String::from_str(&f.env, "back"),
    );
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 2);
}

#[test]
fn a_legacy_report_from_a_revoked_reporter_does_not_count() {
    let f = listed();
    supply(&f, 100 * UNIT);
    write_legacy(
        &f,
        &legacy_report(&f, &f.auditor, Tier::AuditorSigned, 100 * UNIT, T0),
    );
    f.registry.revoke(&f.auditor);
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::AuditorSigned), None);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 0);
}

#[test]
fn a_revoked_auditor_cannot_post_again() {
    let f = listed();
    f.registry.revoke(&f.auditor);
    let res = f.ledger.try_post_reserve(
        &f.auditor,
        &f.sac,
        &UNIT,
        &T0,
        &hash(&f.env, 1),
        &uri(&f.env),
    );
    assert_eq!(res, Err(Ok(LedgerError::NotReporter.into())));
}

// ---- admin ----------------------------------------------------------------

#[test]
fn transfer_admin_changes_admin() {
    let f = setup();
    let next = Address::generate(&f.env);
    f.ledger.transfer_admin(&next);
    assert_eq!(f.ledger.admin(), next);
    assert_eq!(f.ledger.registry(), f.registry.address);
}

/// Authorise exactly these `(address, function, args)` calls on the ledger and
/// nothing else, replacing the fixture's `mock_all_auths`.
fn authorise_only(f: &Fixture, entries: &[(&Address, &str, soroban_sdk::Vec<soroban_sdk::Val>)]) {
    let invokes: std::vec::Vec<MockAuthInvoke> = entries
        .iter()
        .map(|(_, fn_name, args)| MockAuthInvoke {
            contract: &f.ledger.address,
            fn_name,
            args: args.clone(),
            sub_invokes: &[],
        })
        .collect();
    let mocks: std::vec::Vec<MockAuth> = entries
        .iter()
        .zip(invokes.iter())
        .map(|((address, _, _), invoke)| MockAuth { address, invoke })
        .collect();
    f.env.mock_auths(&mocks);
}

/// What a client sees when a `require_auth` inside the ledger finds no
/// matching authorisation: the host reports it against the failing frame.
fn auth_failure() -> Error {
    Error::from_type_and_code(ScErrorType::Context, ScErrorCode::InvalidAction)
}

#[test]
fn transfer_admin_requires_both() {
    let f = setup();
    let admin = f.ledger.admin();
    let next = Address::generate(&f.env);
    f.ledger.transfer_admin(&next);

    // Both the outgoing and the incoming admin signed this exact call.
    let call = AuthorizedFunction::Contract((
        f.ledger.address.clone(),
        Symbol::new(&f.env, "transfer_admin"),
        (next.clone(),).into_val(&f.env),
    ));
    let auths = f.env.auths();
    assert_eq!(auths.len(), 2);
    for signer in [&admin, &next] {
        assert!(auths
            .iter()
            .any(|(a, inv)| a == signer && inv.function == call));
    }
    assert_eq!(f.ledger.admin(), next);
}

#[test]
fn transfer_admin_fails_without_the_new_admins_signature() {
    let f = setup();
    let admin = f.ledger.admin();
    let next = Address::generate(&f.env);
    authorise_only(
        &f,
        &[(&admin, "transfer_admin", (next.clone(),).into_val(&f.env))],
    );
    assert_eq!(f.ledger.try_transfer_admin(&next), Err(Ok(auth_failure())));
    assert_eq!(f.ledger.admin(), admin);
}

#[test]
fn transfer_admin_fails_without_the_current_admins_signature() {
    let f = setup();
    let admin = f.ledger.admin();
    let next = Address::generate(&f.env);
    authorise_only(
        &f,
        &[(&next, "transfer_admin", (next.clone(),).into_val(&f.env))],
    );
    assert_eq!(f.ledger.try_transfer_admin(&next), Err(Ok(auth_failure())));
    assert_eq!(f.ledger.admin(), admin);
}

#[test]
fn old_admin_loses_upgrade_and_transfer_after_transfer() {
    let f = setup();
    let old = f.ledger.admin();
    let next = Address::generate(&f.env);
    let third = Address::generate(&f.env);
    let wasm_hash = BytesN::from_array(&f.env, &[7; 32]);
    authorise_only(
        &f,
        &[
            (&old, "transfer_admin", (next.clone(),).into_val(&f.env)),
            (&next, "transfer_admin", (next.clone(),).into_val(&f.env)),
        ],
    );
    f.ledger.transfer_admin(&next);
    assert_eq!(f.ledger.admin(), next);

    // The old admin's signature on any upgrade step no longer carries weight.
    authorise_only(
        &f,
        &[(
            &old,
            "propose_upgrade",
            (wasm_hash.clone(),).into_val(&f.env),
        )],
    );
    assert_eq!(
        f.ledger.try_propose_upgrade(&wasm_hash),
        Err(Ok(auth_failure()))
    );
    authorise_only(&f, &[(&old, "execute_upgrade", ().into_val(&f.env))]);
    assert_eq!(f.ledger.try_execute_upgrade(), Err(Ok(auth_failure())));
    authorise_only(&f, &[(&old, "cancel_upgrade", ().into_val(&f.env))]);
    assert_eq!(f.ledger.try_cancel_upgrade(), Err(Ok(auth_failure())));

    // Nor can the old admin hand the role on, even with the recipient's
    // signature.
    authorise_only(
        &f,
        &[
            (&old, "transfer_admin", (third.clone(),).into_val(&f.env)),
            (&third, "transfer_admin", (third.clone(),).into_val(&f.env)),
        ],
    );
    assert_eq!(f.ledger.try_transfer_admin(&third), Err(Ok(auth_failure())));
    assert_eq!(f.ledger.admin(), next);

    // The new admin can.
    authorise_only(
        &f,
        &[
            (&next, "transfer_admin", (third.clone(),).into_val(&f.env)),
            (&third, "transfer_admin", (third.clone(),).into_val(&f.env)),
        ],
    );
    f.ledger.transfer_admin(&third);
    assert_eq!(f.ledger.admin(), third);
}

// ---- time-locked upgrades -------------------------------------------------

fn upgrade_hash(env: &Env) -> BytesN<32> {
    env.deployer().upload_contract_wasm(UPGRADE_WASM)
}

fn pending_storage(f: &Fixture) -> Option<PendingUpgrade> {
    f.env.as_contract(&f.ledger.address, || {
        f.env.storage().instance().get(&DataKey::PendingUpgrade)
    })
}

#[test]
fn propose_upgrade_stores_the_hash_and_execution_ledger() {
    let f = setup();
    let wasm_hash = upgrade_hash(&f.env);
    f.ledger.propose_upgrade(&wasm_hash);

    let (topics, data) = last_ledger_event(&f);
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "upgrade_proposed")
    );
    assert_eq!(
        BytesN::<32>::try_from_val(&f.env, &topics.get(1).unwrap()).unwrap(),
        wasm_hash
    );
    assert_eq!(
        u32::try_from_val(
            &f.env,
            &data
                .get(Symbol::new(&f.env, "executable_at_ledger"))
                .unwrap()
        )
        .unwrap(),
        1_000 + UPGRADE_DELAY_LEDGERS
    );
    assert_eq!(
        f.ledger.pending_upgrade(),
        Some(PendingUpgrade {
            wasm_hash,
            executable_at_ledger: 1_000 + UPGRADE_DELAY_LEDGERS,
        })
    );
}

#[test]
fn execute_upgrade_before_the_delay_fails() {
    let f = setup();
    f.ledger.propose_upgrade(&upgrade_hash(&f.env));
    f.env
        .ledger()
        .set_sequence_number(1_000 + UPGRADE_DELAY_LEDGERS - 1);

    assert_eq!(
        f.ledger.try_execute_upgrade(),
        Err(Ok(LedgerError::UpgradeDelayNotElapsed.into()))
    );
    assert!(f.ledger.pending_upgrade().is_some());
}

#[test]
fn execute_upgrade_without_a_proposal_fails() {
    let f = setup();
    assert_eq!(
        f.ledger.try_execute_upgrade(),
        Err(Ok(LedgerError::UpgradeNotProposed.into()))
    );
}

#[test]
fn execute_upgrade_after_the_delay_installs_the_wasm() {
    let f = setup();
    let wasm_hash = upgrade_hash(&f.env);
    f.ledger.propose_upgrade(&wasm_hash);
    f.env
        .ledger()
        .set_sequence_number(1_000 + UPGRADE_DELAY_LEDGERS);

    f.ledger.execute_upgrade();

    let topics = last_ledger_topics(&f);
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "upgrade_executed")
    );
    assert_eq!(
        BytesN::<32>::try_from_val(&f.env, &topics.get(1).unwrap()).unwrap(),
        wasm_hash
    );
    assert_eq!(pending_storage(&f), None);
}

#[test]
fn cancel_upgrade_clears_the_proposal() {
    let f = setup();
    let wasm_hash = upgrade_hash(&f.env);
    f.ledger.propose_upgrade(&wasm_hash);
    f.ledger.cancel_upgrade();

    let topics = last_ledger_topics(&f);
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "upgrade_cancelled")
    );
    assert_eq!(
        BytesN::<32>::try_from_val(&f.env, &topics.get(1).unwrap()).unwrap(),
        wasm_hash
    );
    assert_eq!(f.ledger.pending_upgrade(), None);
    assert_eq!(
        f.ledger.try_execute_upgrade(),
        Err(Ok(LedgerError::UpgradeNotProposed.into()))
    );
}

#[test]
fn cancel_upgrade_without_a_proposal_fails() {
    let f = setup();
    assert_eq!(
        f.ledger.try_cancel_upgrade(),
        Err(Ok(LedgerError::UpgradeNotProposed.into()))
    );
}

#[test]
fn a_new_proposal_restarts_the_clock() {
    let f = setup();
    let first = upgrade_hash(&f.env);
    f.ledger.propose_upgrade(&first);

    f.env.ledger().set_sequence_number(1_500);
    let second = f.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    f.ledger.propose_upgrade(&second);

    assert_eq!(
        f.ledger.pending_upgrade(),
        Some(PendingUpgrade {
            wasm_hash: second,
            executable_at_ledger: 1_500 + UPGRADE_DELAY_LEDGERS,
        })
    );
    f.env
        .ledger()
        .set_sequence_number(1_000 + UPGRADE_DELAY_LEDGERS);
    assert_eq!(
        f.ledger.try_execute_upgrade(),
        Err(Ok(LedgerError::UpgradeDelayNotElapsed.into()))
    );
}

// ---- reporter_last_post (#138) --------------------------------------------

fn another_asset(f: &Fixture) -> Address {
    f.env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address()
}

#[test]
fn reporter_last_post_is_none_before_the_reporter_posts() {
    let f = listed();
    assert_eq!(f.ledger.reporter_last_post(&f.auditor, &f.sac), None);
    assert_eq!(f.ledger.reporter_last_post(&f.issuer, &f.sac), None);
}

#[test]
fn reporter_last_post_returns_the_post_timestamp_not_the_as_of() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 500);
    assert_eq!(f.ledger.reporter_last_post(&f.auditor, &f.sac), Some(T0));

    f.env.ledger().set_timestamp(T0 + 60);
    post(&f, &f.auditor, UNIT, T0 - 100);
    assert_eq!(
        f.ledger.reporter_last_post(&f.auditor, &f.sac),
        Some(T0 + 60)
    );
}

#[test]
fn reporter_last_post_is_per_reporter_and_per_asset() {
    let f = listed();
    let other = another_asset(&f);
    f.ledger.list_asset(&other);
    post(&f, &f.issuer, UNIT, T0 - 10);

    assert_eq!(f.ledger.reporter_last_post(&f.issuer, &f.sac), Some(T0));
    // The transcriber never posted for this asset, and the issuer's post is
    // for this asset only.
    assert_eq!(f.ledger.reporter_last_post(&f.transcriber, &f.sac), None);
    assert_eq!(f.ledger.reporter_last_post(&f.issuer, &other), None);
}

#[test]
fn reporter_last_post_spans_every_tier_the_reporter_posted_at() {
    let f = listed();
    post(&f, &f.transcriber, UNIT, T0 - 300);
    assert_eq!(
        f.ledger.reporter_last_post(&f.transcriber, &f.sac),
        Some(T0)
    );

    // Promoted to auditor, the same reporter keeps the transcription and
    // starts a second entry at the stronger tier.
    f.registry.set_reporter(
        &f.transcriber,
        &Role::Auditor,
        &String::from_str(&f.env, "promoted"),
    );
    f.env.ledger().set_timestamp(T0 + 120);
    assert_eq!(post(&f, &f.transcriber, UNIT, T0 - 60), Tier::AuditorSigned);
    assert_eq!(
        f.ledger.reporter_last_post(&f.transcriber, &f.sac),
        Some(T0 + 120)
    );
}

#[test]
fn reporter_last_post_reads_a_legacy_report() {
    let f = listed();
    let legacy = legacy_report(&f, &f.auditor, Tier::AuditorSigned, UNIT, T0 - 50);
    write_legacy(&f, &legacy);
    assert_eq!(
        f.ledger.reporter_last_post(&f.auditor, &f.sac),
        Some(T0 - 50)
    );
}

// ---- asset_reporter_pairs (#159) ------------------------------------------

#[test]
fn asset_reporter_pairs_is_empty_before_anything_is_listed() {
    let f = setup();
    assert!(f.ledger.asset_reporter_pairs(&None, &10).is_empty());
}

#[test]
fn asset_reporter_pairs_lists_each_reporter_once_strongest_tier_first() {
    let f = listed();
    one_report_per_tier(&f);

    let pairs = f.ledger.asset_reporter_pairs(&None, &10);
    assert_eq!(pairs.len(), 3);
    assert_eq!(pairs.get(0).unwrap(), (f.sac.clone(), f.auditor.clone()));
    assert_eq!(pairs.get(1).unwrap(), (f.sac.clone(), f.issuer.clone()));
    assert_eq!(
        pairs.get(2).unwrap(),
        (f.sac.clone(), f.transcriber.clone())
    );
}

#[test]
fn asset_reporter_pairs_lists_a_reporter_once_after_a_role_change() {
    let f = listed();
    post(&f, &f.transcriber, UNIT, T0 - 300);
    f.registry.set_reporter(
        &f.transcriber,
        &Role::Auditor,
        &String::from_str(&f.env, "promoted"),
    );
    post(&f, &f.transcriber, UNIT, T0 - 200);

    let pairs = f.ledger.asset_reporter_pairs(&None, &10);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs.get(0).unwrap(),
        (f.sac.clone(), f.transcriber.clone())
    );
}

#[test]
fn asset_reporter_pairs_keeps_a_report_whose_reporter_no_longer_counts() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 10);
    f.registry.revoke(&f.auditor);

    assert!(f.ledger.get_reports(&f.sac).is_empty());
    let pairs = f.ledger.asset_reporter_pairs(&None, &10);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs.get(0).unwrap(), (f.sac.clone(), f.auditor.clone()));
}

#[test]
fn asset_reporter_pairs_pages_with_a_cursor() {
    let f = listed();
    let other = another_asset(&f);
    f.ledger.list_asset(&other);
    post(&f, &f.auditor, UNIT, T0 - 300);
    post(&f, &f.issuer, UNIT, T0 - 200);
    f.ledger.post_reserve(
        &f.transcriber,
        &other,
        &UNIT,
        &(T0 - 100),
        &hash(&f.env, 4),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.auditor,
        &other,
        &UNIT,
        &(T0 - 50),
        &hash(&f.env, 4),
        &uri(&f.env),
    );

    let all = f.ledger.asset_reporter_pairs(&None, &10);
    assert_eq!(all.len(), 4);
    assert_eq!(all.get(0).unwrap(), (f.sac.clone(), f.auditor.clone()));
    assert_eq!(all.get(1).unwrap(), (f.sac.clone(), f.issuer.clone()));
    assert_eq!(all.get(2).unwrap(), (other.clone(), f.auditor.clone()));
    assert_eq!(all.get(3).unwrap(), (other.clone(), f.transcriber.clone()));

    let first = f.ledger.asset_reporter_pairs(&None, &3);
    let second = f.ledger.asset_reporter_pairs(&Some(3), &3);
    assert_eq!(first.len(), 3);
    assert_eq!(second.len(), 1);
    for i in 0..3 {
        assert_eq!(first.get(i).unwrap(), all.get(i).unwrap());
    }
    assert_eq!(second.get(0).unwrap(), all.get(3).unwrap());

    // The explicit zero cursor is the default, and a zero limit is empty.
    assert_eq!(f.ledger.asset_reporter_pairs(&Some(0), &3), first);
    assert!(f.ledger.asset_reporter_pairs(&None, &0).is_empty());
    assert!(f.ledger.asset_reporter_pairs(&Some(4), &3).is_empty());
    assert!(f.ledger.asset_reporter_pairs(&Some(99), &3).is_empty());
}

#[test]
fn asset_reporter_pairs_caps_a_page_at_fifty() {
    let f = setup();
    let auditors: std::vec::Vec<Address> =
        (0..4).map(|_| add_reporter(&f, Role::Auditor)).collect();
    let transcribers: std::vec::Vec<Address> = (0..4)
        .map(|_| add_reporter(&f, Role::Transcriber))
        .collect();

    // Six assets, each with four auditors, the issuer and four transcribers:
    // 54 pairs, more than one page may carry.
    for _ in 0..6 {
        let handle = f
            .env
            .register_stellar_asset_contract_v2(Address::generate(&f.env));
        let sac = handle.address();
        let issuer = handle.issuer().address();
        f.ledger.list_asset(&sac);
        for (i, who) in auditors.iter().enumerate() {
            f.ledger.post_reserve(
                who,
                &sac,
                &UNIT,
                &(T0 - 1_000 + i as u64),
                &hash(&f.env, 4),
                &uri(&f.env),
            );
        }
        f.ledger.post_reserve(
            &issuer,
            &sac,
            &UNIT,
            &(T0 - 900),
            &hash(&f.env, 4),
            &uri(&f.env),
        );
        for (i, who) in transcribers.iter().enumerate() {
            f.ledger.post_reserve(
                who,
                &sac,
                &UNIT,
                &(T0 - 800 + i as u64),
                &hash(&f.env, 4),
                &uri(&f.env),
            );
        }
    }

    let first = f.ledger.asset_reporter_pairs(&None, &1_000);
    assert_eq!(first.len(), MAX_PAIRS_PER_CALL);

    let mut total = 0u32;
    let mut cursor: Option<u32> = None;
    loop {
        let page = f.ledger.asset_reporter_pairs(&cursor, &1_000);
        assert!(page.len() <= MAX_PAIRS_PER_CALL);
        if page.is_empty() {
            break;
        }
        total += page.len();
        cursor = Some(total);
    }
    assert_eq!(total, 54);
}

// ---- emergency pause (#77) ------------------------------------------------

#[test]
fn pause_sets_the_flag_and_emits_the_admin() {
    let f = listed();
    assert!(!f.ledger.is_paused());

    f.ledger.pause();

    let (topics, data) = last_ledger_event(&f);
    assert!(f.ledger.is_paused());
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "paused")
    );
    assert_eq!(
        Address::try_from_val(&f.env, &data.get(Symbol::new(&f.env, "by")).unwrap()).unwrap(),
        f.admin
    );
}

// ---- resource budget ceilings (#103) --------------------------------------
// Mirrors resource-budgets.json, which carries the prose for why each number
// exists. A failure here names the call that grew; raise the ceiling in the
// commit that needs the room, never to make a red run green.

const IS_COVERED_MAX_CPU_INSTRUCTIONS: u64 = 300_000;
const IS_COVERED_MAX_MEMORY_BYTES: u64 = 134_000;
const POST_SUPPLY_MAX_CPU_INSTRUCTIONS: u64 = 230_000;
const POST_SUPPLY_MAX_MEMORY_BYTES: u64 = 116_000;
const POST_RESERVE_MAX_CPU_INSTRUCTIONS: u64 = 372_000;
const POST_RESERVE_MAX_MEMORY_BYTES: u64 = 179_000;

/// Run `call` and fail when it costs more than the ceiling documented for it.
fn assert_within_budget(
    f: &Fixture,
    label: &str,
    max_cpu: u64,
    max_memory: u64,
    call: impl FnOnce(),
) {
    let cpu_before = f.env.cost_estimate().budget().cpu_instruction_cost();
    let memory_before = f.env.cost_estimate().budget().memory_bytes_cost();
    call();
    let cpu = f
        .env
        .cost_estimate()
        .budget()
        .cpu_instruction_cost()
        .saturating_sub(cpu_before);
    let memory = f
        .env
        .cost_estimate()
        .budget()
        .memory_bytes_cost()
        .saturating_sub(memory_before);
    std::println!("budget {label}: {cpu} CPU / {memory} bytes");
    assert!(cpu <= max_cpu, "{label} used {cpu} CPU, ceiling {max_cpu}");
    assert!(
        memory <= max_memory,
        "{label} used {memory} bytes, ceiling {max_memory}"
    );
}

#[test]
fn is_covered_stays_within_its_resource_budget() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(100 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    assert_within_budget(
        &f,
        "is_covered",
        IS_COVERED_MAX_CPU_INSTRUCTIONS,
        IS_COVERED_MAX_MEMORY_BYTES,
        || {
            assert!(f
                .ledger
                .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));
        },
    );
}

#[test]
fn unpause_clears_the_flag_and_emits_the_admin() {
    let f = listed();
    f.ledger.pause();

    f.ledger.unpause();

    let (topics, data) = last_ledger_event(&f);
    assert!(!f.ledger.is_paused());
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "unpaused")
    );
    assert_eq!(
        Address::try_from_val(&f.env, &data.get(Symbol::new(&f.env, "by")).unwrap()).unwrap(),
        f.admin
    );
}

#[test]
fn post_supply_stays_within_its_resource_budget() {
    let f = listed();
    assert_within_budget(
        &f,
        "post_supply",
        POST_SUPPLY_MAX_CPU_INSTRUCTIONS,
        POST_SUPPLY_MAX_MEMORY_BYTES,
        || {
            f.ledger
                .post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1));
        },
    );
}

#[test]
fn pause_is_idempotent() {
    let f = listed();
    f.ledger.pause();
    f.ledger.pause();
    assert!(f.ledger.is_paused());
}

#[test]
fn pause_requires_the_admin() {
    let f = listed();
    let stranger = Address::generate(&f.env);
    authorise_only(&f, &[(&stranger, "pause", ().into_val(&f.env))]);
    assert_eq!(f.ledger.try_pause(), Err(Ok(auth_failure())));
}

#[test]
fn unpause_requires_the_admin() {
    let f = listed();
    f.ledger.pause();
    let stranger = Address::generate(&f.env);
    authorise_only(&f, &[(&stranger, "unpause", ().into_val(&f.env))]);
    assert_eq!(f.ledger.try_unpause(), Err(Ok(auth_failure())));
}

#[test]
fn a_paused_ledger_records_neither_supply_nor_reserves() {
    let f = listed();
    f.ledger.pause();

    assert_eq!(
        f.ledger
            .try_post_supply(&f.poster, &f.sac, &UNIT, &990, &hash(&f.env, 1)),
        Err(Ok(LedgerError::Paused.into()))
    );
    assert_eq!(
        f.ledger.try_post_reserve(
            &f.issuer,
            &f.sac,
            &UNIT,
            &T0,
            &hash(&f.env, 2),
            &uri(&f.env)
        ),
        Err(Ok(LedgerError::Paused.into()))
    );
    assert_eq!(f.ledger.get_supply(&f.sac), None);
}

#[test]
fn a_paused_ledger_refuses_is_covered_but_still_answers_coverage() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 100 * UNIT, T0);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));

    f.ledger.pause();

    assert_eq!(
        f.ledger
            .try_is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed),
        Err(Ok(LedgerError::Paused.into()))
    );
    // Views keep working so a dashboard can still show the recorded state.
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap().bps,
        10_000
    );
}

#[test]
fn unpausing_restores_posting_and_is_covered() {
    let f = listed();
    f.ledger.pause();
    f.ledger.unpause();

    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 100 * UNIT, T0);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));
}

// ---- coverage_status (#98) ------------------------------------------------

fn covered() -> Fixture {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 100 * UNIT, T0);
    f
}

#[test]
fn coverage_status_covers_fresh_figures_with_the_ratio() {
    let f = covered();
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Covered(CoveredInfo { bps: 10_000 })
    );
}

#[test]
fn coverage_status_reports_fresh_shortfall_as_uncovered() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 90 * UNIT, T0);
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
    // The ratio is still a pass when the caller asks for less.
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &9_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Covered(CoveredInfo { bps: 9_000 })
    );
}

#[test]
fn coverage_status_reports_missing_figures_as_uncovered() {
    let f = listed();
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
    // Supply alone is not a coverage answer.
    supply(&f, 100 * UNIT);
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
}

#[test]
fn coverage_status_reports_stale_with_the_last_ratio_and_age() {
    let f = covered();
    let day = 86_400;
    f.env.ledger().set_timestamp(T0 + 2 * day);

    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_000, &day, &Tier::Transcribed),
        CoverageStatus::Stale(StaleInfo {
            last_bps: 10_000,
            age_ledgers: (2 * day / SECONDS_PER_LEDGER) as u32
        })
    );
}

#[test]
fn stale_wins_over_under_backed() {
    let f = covered();
    let day = 86_400;
    f.env.ledger().set_timestamp(T0 + 2 * day);

    // Asking for more than the last ratio still reports Stale, because a
    // stale ratio is not a verdict on backing.
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_001, &day, &Tier::Transcribed),
        CoverageStatus::Stale(StaleInfo {
            last_bps: 10_000,
            age_ledgers: (2 * day / SECONDS_PER_LEDGER) as u32
        })
    );
}

#[test]
fn coverage_status_reports_the_oldest_of_supply_and_report() {
    let f = listed();
    // Supply posted now, the report describes a figure two days old.
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 100 * UNIT, T0 - 2 * 86_400);

    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Stale(StaleInfo {
            last_bps: 10_000,
            age_ledgers: (2 * 86_400 / SECONDS_PER_LEDGER) as u32
        })
    );
}

#[test]
fn coverage_status_reports_paused_before_anything_else() {
    let f = listed();
    f.ledger.pause();
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &10_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Paused
    );

    let f = covered();
    f.ledger.pause();
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Paused
    );
}

#[test]
fn coverage_status_uses_min_tier_and_bps_like_is_covered() {
    let f = covered();
    let day = 86_400;
    for (min_bps, max_age, min_tier) in [
        (10_000u32, day, Tier::Transcribed),
        (10_001, day, Tier::Transcribed),
        (10_000, day - 1, Tier::Transcribed),
        (10_000, day, Tier::AuditorSigned),
        (0, u64::MAX, Tier::Transcribed),
    ] {
        assert_eq!(
            f.ledger
                .coverage_status(&f.sac, &min_bps, &max_age, &min_tier)
                == CoverageStatus::Covered(CoveredInfo { bps: 10_000 }),
            f.ledger.is_covered(&f.sac, &min_bps, &max_age, &min_tier),
            "disagreed for ({min_bps}, {max_age}, {min_tier:?})"
        );
    }
}

// ---- native supply source (#99) -------------------------------------------

/// A minimal SEP-41 supply source: the test host's Stellar Asset Contract
/// does not expose `total_supply`, but a Soroban token can.
#[contract]
pub struct TestSupplyToken;

#[contractimpl]
impl TestSupplyToken {
    pub fn __constructor(env: Env, supply: i128) {
        env.storage().instance().set(&0u32, &supply);
    }

    pub fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&0u32).unwrap_or(0)
    }
}

fn native_token(f: &Fixture, supply: i128) -> Address {
    f.env.register(TestSupplyToken, (supply,))
}

#[test]
fn enable_native_supply_stores_the_token_and_emits() {
    let f = listed();
    assert_eq!(f.ledger.native_supply_token(&f.sac), None);

    f.ledger.enable_native_supply(&f.sac, &f.sac);

    let (topics, data) = last_ledger_event(&f);
    assert_eq!(
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap(),
        Symbol::new(&f.env, "native_supply_enabled")
    );
    assert_eq!(
        Address::try_from_val(&f.env, &topics.get(1).unwrap()).unwrap(),
        f.sac
    );
    assert_eq!(
        Address::try_from_val(&f.env, &data.get(Symbol::new(&f.env, "token")).unwrap()).unwrap(),
        f.sac
    );
    assert_eq!(f.ledger.native_supply_token(&f.sac), Some(f.sac));
}

#[test]
fn enable_native_supply_requires_a_listed_asset() {
    let f = setup();
    assert_eq!(
        f.ledger.try_enable_native_supply(&f.sac, &f.sac),
        Err(Ok(LedgerError::AssetNotListed.into()))
    );
}

#[test]
fn post_report_stays_within_its_resource_budget() {
    let f = listed();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &990, &hash(&f.env, 1));
    assert_within_budget(
        &f,
        "post_reserve",
        POST_RESERVE_MAX_CPU_INSTRUCTIONS,
        POST_RESERVE_MAX_MEMORY_BYTES,
        || {
            f.ledger.post_reserve(
                &f.auditor,
                &f.sac,
                &(100 * UNIT),
                &T0,
                &hash(&f.env, 2),
                &uri(&f.env),
            );
        },
    );
}

#[test]
fn enable_native_supply_requires_the_admin() {
    let f = listed();
    let stranger = Address::generate(&f.env);
    authorise_only(
        &f,
        &[(
            &stranger,
            "enable_native_supply",
            (f.sac.clone(), f.sac.clone()).into_val(&f.env),
        )],
    );
    assert_eq!(
        f.ledger.try_enable_native_supply(&f.sac, &f.sac),
        Err(Ok(auth_failure()))
    );
}

#[test]
fn coverage_prefers_the_native_supply_over_the_snapshot() {
    let f = covered();
    let token = native_token(&f, 50 * UNIT);
    f.ledger.enable_native_supply(&f.sac, &token);

    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.supply, 50 * UNIT);
    assert_eq!(c.bps, 20_000);
    // The snapshot is still readable and is the fallback.
    assert_eq!(f.ledger.get_supply(&f.sac).unwrap().amount, 100 * UNIT);
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &20_000, &86_400, &Tier::Transcribed),
        CoverageStatus::Covered(CoveredInfo { bps: 20_000 })
    );
}

#[test]
fn native_supply_is_always_as_fresh_as_the_current_ledger() {
    let f = covered();
    let day = 86_400;
    let later = T0 + 10 * day;
    f.env.ledger().set_timestamp(later);
    post(&f, &f.issuer, 100 * UNIT, later);

    // The snapshot timestamp is T0, so it is stale for a one-day window...
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));

    // ...but the native figure is stamped at the current ledger.
    let token = native_token(&f, 100 * UNIT);
    f.ledger.enable_native_supply(&f.sac, &token);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));
}

#[test]
fn a_failing_native_query_falls_back_to_the_snapshot() {
    let f = covered();
    // The registry has no sep41 total_supply entry point.
    f.ledger.enable_native_supply(&f.sac, &f.registry.address);

    let c = f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap();
    assert_eq!(c.supply, 100 * UNIT);
    assert_eq!(c.bps, 10_000);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::Transcribed));
}

#[test]
fn a_failing_native_query_without_a_snapshot_is_uncovered() {
    let f = listed();
    f.ledger.enable_native_supply(&f.sac, &f.registry.address);

    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
    assert_eq!(
        f.ledger
            .coverage_status(&f.sac, &0, &u64::MAX, &Tier::Transcribed),
        CoverageStatus::Uncovered
    );
}

fn try_post_now(f: &Fixture, who: &Address) -> Result<(), LedgerError> {
    match f
        .ledger
        .try_post_reserve(who, &f.sac, &UNIT, &T0, &hash(&f.env, 6), &uri(&f.env))
    {
        Ok(_) => Ok(()),
        Err(Ok(error)) => Err(LedgerError::try_from(error).expect("a ledger error")),
        Err(Err(error)) => panic!("post_reserve failed outside the contract: {error:?}"),
    }
}

fn can_post(f: &Fixture, who: &Address) -> Result<(), LedgerError> {
    match f.ledger.try_can_post_report(who, &f.sac) {
        Ok(Ok(())) => Ok(()),
        Err(Ok(error)) => Err(error),
        other => panic!("can_post_report failed outside the contract: {other:?}"),
    }
}

#[test]
fn can_post_report_allows_every_reporting_identity() {
    let f = listed();
    for who in [&f.issuer, &f.auditor, &f.transcriber] {
        assert_eq!(can_post(&f, who), Ok(()));
        f.ledger.can_post_report(who, &f.sac);
        // And post_reserve agrees.
        assert_eq!(try_post_now(&f, who), Ok(()));
    }
}

#[test]
fn can_post_report_rejects_an_unlisted_asset() {
    let f = setup();
    assert_eq!(can_post(&f, &f.auditor), Err(LedgerError::AssetNotListed));
    assert_eq!(
        try_post_now(&f, &f.auditor),
        Err(LedgerError::AssetNotListed)
    );
}

#[test]
fn can_post_report_rejects_a_non_reporter() {
    let f = listed();
    let stranger = Address::generate(&f.env);
    for who in [&f.poster, &stranger] {
        assert_eq!(can_post(&f, who), Err(LedgerError::NotReporter));
        assert_eq!(try_post_now(&f, who), Err(LedgerError::NotReporter));
    }
}

#[test]
fn can_post_report_reports_the_pause_first() {
    let f = setup();
    f.ledger.pause();
    // Paused wins over the unlisted asset, as it does in post_reserve.
    assert_eq!(can_post(&f, &f.auditor), Err(LedgerError::Paused));
    assert_eq!(try_post_now(&f, &f.auditor), Err(LedgerError::Paused));
}

#[test]
fn can_post_report_rejects_a_full_tier_unless_a_slot_can_be_reclaimed() {
    let f = listed();
    let mut auditors = std::vec![f.auditor.clone()];
    for _ in 1..MAX_REPORTERS_PER_TIER {
        auditors.push(add_reporter(&f, Role::Auditor));
    }
    for (i, a) in auditors.iter().enumerate() {
        post(&f, a, UNIT, T0 - 100 + i as u64);
    }
    let one_too_many = add_reporter(&f, Role::Auditor);
    assert_eq!(
        can_post(&f, &one_too_many),
        Err(LedgerError::TooManyReporters)
    );
    assert_eq!(
        try_post_now(&f, &one_too_many),
        Err(LedgerError::TooManyReporters)
    );
    // A reporter already holding a slot is not blocked by the cap.
    assert_eq!(can_post(&f, &auditors[0]), Ok(()));

    // Revoking a holder frees a slot post_reserve would reclaim.
    f.registry.revoke(&auditors[1]);
    assert_eq!(can_post(&f, &one_too_many), Ok(()));
    assert_eq!(try_post_now(&f, &one_too_many), Ok(()));
}

#[test]
fn can_post_report_writes_no_storage() {
    use soroban_sdk::testutils::storage::Persistent as _;

    let f = listed();
    let key = DataKey::Asset(f.sac.clone());
    let ttl = |f: &Fixture| {
        f.env.as_contract(&f.ledger.address, || {
            f.env.storage().persistent().get_ttl(&key)
        })
    };
    // Move close enough to expiry that any read that extends TTL would.
    let remaining = ttl(&f);
    f.env
        .ledger()
        .set_sequence_number(f.env.ledger().sequence() + remaining - 10);
    let before = ttl(&f);
    assert!(before < PERSISTENT_THRESHOLD);

    assert_eq!(can_post(&f, &f.auditor), Ok(()));
    assert_eq!(ttl(&f), before, "can_post_report must not extend TTLs");
    assert!(f.ledger.get_reports(&f.sac).is_empty());

    // get_asset, by contrast, extends it.
    f.ledger.get_asset(&f.sac);
    assert!(ttl(&f) > before);
}

// ---- last_updated (#123) --------------------------------------------------

#[test]
fn last_updated_is_none_for_an_asset_that_is_not_listed() {
    let f = listed();
    let other = another_asset(&f);
    post(&f, &f.auditor, UNIT, T0 - 10);

    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0));
    // Unlisted, so not the asset the report was posted for, and not askable.
    assert_eq!(f.ledger.last_updated(&other), None);
}

#[test]
fn last_updated_is_none_while_only_a_supply_figure_exists() {
    let f = listed();
    supply(&f, 100 * UNIT);
    // Never reported on is a different answer from reported long ago.
    assert_eq!(f.ledger.last_updated(&f.sac), None);
}

#[test]
fn last_updated_is_the_post_timestamp_not_the_as_of() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 500);
    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0));

    f.env.ledger().set_timestamp(T0 + 60);
    post(&f, &f.auditor, UNIT, T0 - 400);
    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0 + 60));
}

#[test]
fn last_updated_spans_every_tier() {
    let f = listed();
    post(&f, &f.transcriber, UNIT, T0 - 300);
    f.env.ledger().set_timestamp(T0 + 10);
    post(&f, &f.issuer, UNIT, T0 - 200);
    f.env.ledger().set_timestamp(T0 + 20);
    post(&f, &f.auditor, UNIT, T0 - 100);

    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0 + 20));
}

#[test]
fn last_updated_includes_a_report_that_stopped_counting() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 10);
    f.env.ledger().set_timestamp(T0 + 30);
    post(&f, &f.transcriber, UNIT, T0 - 5);
    f.registry.revoke(&f.transcriber);

    // Only the auditor's figure still counts, but the revoked transcriber's
    // later post is still the most recent one.
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 1);
    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0 + 30));
}

#[test]
fn last_updated_reads_a_legacy_report() {
    let f = listed();
    let legacy = legacy_report(&f, &f.auditor, Tier::AuditorSigned, UNIT, T0 - 50);
    write_legacy(&f, &legacy);

    assert_eq!(f.ledger.last_updated(&f.sac), Some(T0 - 50));
}

// ---- get_all_latest_reports (#166) ----------------------------------------

#[test]
fn get_all_latest_reports_is_empty_before_anything_is_posted() {
    let f = listed();
    assert!(f.ledger.get_all_latest_reports(&None, &10).is_empty());
}

#[test]
fn get_all_latest_reports_returns_only_the_latest_figure_per_pair() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 50 * UNIT, T0 - 300);
    f.env.ledger().set_timestamp(T0 + 10);
    post(&f, &f.auditor, 100 * UNIT, T0 - 200);

    let reports = f.ledger.get_all_latest_reports(&None, &10);
    assert_eq!(reports.len(), 1);
    let report = reports.get(0).unwrap();
    assert_eq!(report.amount, 100 * UNIT);
    assert_eq!(report.as_of, T0 - 200);
    assert_eq!(report.posted_at, T0 + 10);
}

#[test]
fn get_all_latest_reports_orders_like_asset_reporter_pairs() {
    let f = listed();
    one_report_per_tier(&f);

    let pairs = f.ledger.asset_reporter_pairs(&None, &10);
    let reports = f.ledger.get_all_latest_reports(&None, &10);
    assert_eq!(reports.len(), pairs.len());
    for i in 0..pairs.len() {
        let (asset, reporter) = pairs.get(i).unwrap();
        let report = reports.get(i).unwrap();
        assert_eq!(asset, f.sac);
        assert_eq!(report.reporter, reporter);
    }
    assert_eq!(reports.get(0).unwrap().tier, Tier::AuditorSigned);
    assert_eq!(reports.get(1).unwrap().tier, Tier::IssuerSigned);
    assert_eq!(reports.get(2).unwrap().tier, Tier::Transcribed);
}

#[test]
fn get_all_latest_reports_picks_a_reporters_freshest_tier() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.transcriber, 100 * UNIT, T0 - 300);
    f.registry.set_reporter(
        &f.transcriber,
        &Role::Auditor,
        &String::from_str(&f.env, "promoted"),
    );
    f.env.ledger().set_timestamp(T0 + 10);
    post(&f, &f.transcriber, 90 * UNIT, T0 - 200);

    let reports = f.ledger.get_all_latest_reports(&None, &10);
    assert_eq!(reports.len(), 1);
    // The transcriber has an entry at two tiers; the fresher one wins.
    let report = reports.get(0).unwrap();
    assert_eq!(report.tier, Tier::AuditorSigned);
    assert_eq!(report.as_of, T0 - 200);
}

#[test]
fn get_all_latest_reports_keeps_a_report_whose_reporter_no_longer_counts() {
    let f = listed();
    post(&f, &f.auditor, UNIT, T0 - 10);
    f.registry.revoke(&f.auditor);

    assert!(f.ledger.get_reports(&f.sac).is_empty());
    let reports = f.ledger.get_all_latest_reports(&None, &10);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports.get(0).unwrap().reporter, f.auditor);
}

#[test]
fn get_all_latest_reports_pages_with_a_cursor() {
    let f = listed();
    let other = another_asset(&f);
    f.ledger.list_asset(&other);
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 300);
    post(&f, &f.issuer, 100 * UNIT, T0 - 200);
    f.ledger.post_reserve(
        &f.transcriber,
        &other,
        &UNIT,
        &(T0 - 100),
        &hash(&f.env, 4),
        &uri(&f.env),
    );
    f.ledger.post_reserve(
        &f.auditor,
        &other,
        &UNIT,
        &(T0 - 50),
        &hash(&f.env, 4),
        &uri(&f.env),
    );

    let all = f.ledger.get_all_latest_reports(&None, &10);
    assert_eq!(all.len(), 4);

    let first = f.ledger.get_all_latest_reports(&None, &3);
    let second = f.ledger.get_all_latest_reports(&Some(3), &3);
    assert_eq!(first.len(), 3);
    assert_eq!(second.len(), 1);
    for i in 0..3 {
        assert_eq!(first.get(i).unwrap(), all.get(i).unwrap());
    }
    assert_eq!(second.get(0).unwrap(), all.get(3).unwrap());

    // The explicit zero cursor is the default, and a zero limit is empty.
    assert_eq!(f.ledger.get_all_latest_reports(&Some(0), &3), first);
    assert!(f.ledger.get_all_latest_reports(&None, &0).is_empty());
    assert!(f.ledger.get_all_latest_reports(&Some(4), &3).is_empty());
    assert!(f.ledger.get_all_latest_reports(&Some(99), &3).is_empty());
}

#[test]
fn get_all_latest_reports_caps_a_page_at_fifty() {
    let f = setup();
    let auditors: std::vec::Vec<Address> =
        (0..4).map(|_| add_reporter(&f, Role::Auditor)).collect();
    let transcribers: std::vec::Vec<Address> = (0..4)
        .map(|_| add_reporter(&f, Role::Transcriber))
        .collect();

    // Six assets, each with four auditors, the issuer and four transcribers:
    // 54 pairs, more than one page may carry.
    for _ in 0..6 {
        let handle = f
            .env
            .register_stellar_asset_contract_v2(Address::generate(&f.env));
        let sac = handle.address();
        let issuer = handle.issuer().address();
        f.ledger.list_asset(&sac);
        for (i, who) in auditors.iter().enumerate() {
            f.ledger.post_reserve(
                who,
                &sac,
                &UNIT,
                &(T0 - 1_000 + i as u64),
                &hash(&f.env, 4),
                &uri(&f.env),
            );
        }
        f.ledger.post_reserve(
            &issuer,
            &sac,
            &UNIT,
            &(T0 - 900),
            &hash(&f.env, 4),
            &uri(&f.env),
        );
        for (i, who) in transcribers.iter().enumerate() {
            f.ledger.post_reserve(
                who,
                &sac,
                &UNIT,
                &(T0 - 800 + i as u64),
                &hash(&f.env, 4),
                &uri(&f.env),
            );
        }
    }

    let first = f.ledger.get_all_latest_reports(&None, &1_000);
    assert_eq!(first.len(), MAX_PAIRS_PER_CALL);

    let mut total = 0u32;
    let mut cursor: Option<u32> = None;
    loop {
        let page = f.ledger.get_all_latest_reports(&cursor, &1_000);
        assert!(page.len() <= MAX_PAIRS_PER_CALL);
        if page.is_empty() {
            break;
        }
        total += page.len();
        cursor = Some(total);
    }
    assert_eq!(total, 54);
}

// ---- historical_coverage_bps (#164) ---------------------------------------

/// The ratio the ledger reports for `f.sac` at `as_of_ledger`.
fn hist(f: &Fixture, max_age: u32, as_of_ledger: u32) -> Option<u128> {
    f.ledger
        .historical_coverage_bps(&f.sac, &max_age, &as_of_ledger)
}

#[test]
fn historical_coverage_bps_is_none_until_both_sides_are_posted() {
    let f = listed();
    assert_eq!(hist(&f, u32::MAX, 1_000), None);
    supply(&f, 100 * UNIT);
    // A supply figure on its own is not coverage.
    assert_eq!(hist(&f, u32::MAX, 1_000), None);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);
    assert_eq!(hist(&f, u32::MAX, 1_000), Some(10_000));
}

#[test]
fn historical_coverage_bps_answers_a_past_ledger() {
    let f = listed();
    supply(&f, 400 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 300);
    assert_eq!(hist(&f, u32::MAX, 1_000), Some(2_500));

    f.env.ledger().set_sequence_number(1_050);
    f.env.ledger().set_timestamp(T0 + 50);
    f.ledger
        .post_supply(&f.poster, &f.sac, &(100 * UNIT), &1_040, &hash(&f.env, 1));

    // The 400 * UNIT snapshot was replaced, and the ledger keeps only the
    // latest, so a ledger between the two posts has no supply figure to read.
    assert_eq!(hist(&f, u32::MAX, 1_049), None);
    assert_eq!(hist(&f, u32::MAX, 1_050), Some(10_000));
    // At the newest figures the replay agrees with the live answer.
    assert_eq!(
        f.ledger.coverage(&f.sac, &Tier::Transcribed).unwrap().bps,
        10_000
    );
}

#[test]
fn historical_coverage_bps_counts_age_in_ledgers_inclusively() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);
    f.env.ledger().set_sequence_number(1_010);

    // Both figures were posted at 1_000: ten ledgers old at 1_010 is accepted,
    // nine is not, so the comparison is inclusive as in `is_covered`.
    assert_eq!(hist(&f, 10, 1_010), Some(10_000));
    assert_eq!(hist(&f, 9, 1_010), None);
    assert_eq!(hist(&f, 0, 1_000), Some(10_000));
    assert_eq!(hist(&f, 0, 1_001), None);
}

#[test]
fn historical_coverage_bps_is_none_for_a_future_ledger() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);

    // Answering about a ledger that has not happened would be a guess.
    assert_eq!(hist(&f, u32::MAX, 1_001), None);
}

#[test]
fn historical_coverage_bps_cannot_reconstruct_a_replaced_report() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);
    assert_eq!(hist(&f, u32::MAX, 1_000), Some(10_000));

    f.env.ledger().set_sequence_number(1_010);
    f.env.ledger().set_timestamp(T0 + 50);
    post(&f, &f.auditor, 90 * UNIT, T0 + 50);

    // The figure posted at 1_000 was replaced, so a query at 1_005 has no
    // report posted by then; the events carry the ratio it gave.
    assert_eq!(hist(&f, u32::MAX, 1_005), None);
    assert_eq!(hist(&f, u32::MAX, 1_010), Some(9_000));
}

#[test]
fn historical_coverage_bps_is_per_asset() {
    let f = listed();
    let other = another_asset(&f);
    supply(&f, 100 * UNIT);
    post(&f, &f.auditor, 100 * UNIT, T0 - 100);

    assert_eq!(hist(&f, u32::MAX, 1_000), Some(10_000));
    // `other` is not listed, so it has no figures to replay.
    assert_eq!(
        f.ledger.historical_coverage_bps(&other, &u32::MAX, &1_000),
        None
    );
}
