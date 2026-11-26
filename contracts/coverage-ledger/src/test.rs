extern crate std;

use super::*;
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
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_001, &day, &Tier::Transcribed));
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::AuditorSigned));

    // Two days later both inputs are stale.
    f.env.ledger().set_timestamp(T0 + 2 * day);
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &day, &Tier::Transcribed));
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &(3 * day), &Tier::Transcribed));
}

/// is_covered uses `bps >= min_bps` (inclusive boundary).
/// An asset whose coverage ratio equals min_bps exactly must return true,
/// and one bps below must return false. See SPEC.md "is_covered" rules.
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
    assert!(!f
        .ledger
        .is_covered(&f.sac, &0, &u64::MAX, &Tier::Transcribed));
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
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &86_400, &Tier::AuditorSigned));
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
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &DAY, &Tier::AuditorSigned));
    // Weaker floors are still met by the reports that count.
    let issuer = f.ledger.coverage(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(issuer.tier, Tier::IssuerSigned);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &DAY, &Tier::IssuerSigned));
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
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &DAY, &Tier::Transcribed));
    f.registry.revoke(&f.transcriber);
    // Immediately, with no action on the ledger.
    assert!(!f
        .ledger
        .is_covered(&f.sac, &10_000, &DAY, &Tier::Transcribed));
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
        assert!(f.ledger.is_covered(&f.sac, &10_000, &DAY, &min_tier));
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
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &DAY, &Tier::AuditorSigned));
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

// ---- issuer delegate ------------------------------------------------------

fn appoint(f: &Fixture) -> Address {
    let delegate = Address::generate(&f.env);
    f.ledger
        .set_issuer_delegate(&f.sac, &Some(delegate.clone()));
    delegate
}

#[test]
fn set_issuer_delegate_appoints_replaces_and_clears() {
    let f = listed();
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), None);
    let first = appoint(&f);
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), Some(first));
    let second = appoint(&f);
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), Some(second));
    f.ledger.set_issuer_delegate(&f.sac, &None);
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), None);
}

#[test]
fn set_issuer_delegate_is_signed_by_the_issuer() {
    let f = listed();
    let delegate = appoint(&f);
    let call = AuthorizedFunction::Contract((
        f.ledger.address.clone(),
        Symbol::new(&f.env, "set_issuer_delegate"),
        (f.sac.clone(), Some(delegate)).into_val(&f.env),
    ));
    let auths = f.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, f.issuer);
    assert_eq!(auths[0].1.function, call);
}

#[test]
fn set_issuer_delegate_fails_without_the_issuers_signature() {
    let f = listed();
    let delegate = Address::generate(&f.env);
    let args: soroban_sdk::Vec<Val> = (f.sac.clone(), Some(delegate.clone())).into_val(&f.env);
    // Neither the ledger admin nor the would-be delegate can appoint it.
    for signer in [&f.admin, &delegate] {
        authorise_only(&f, &[(signer, "set_issuer_delegate", args.clone())]);
        assert_eq!(
            f.ledger
                .try_set_issuer_delegate(&f.sac, &Some(delegate.clone())),
            Err(Ok(auth_failure()))
        );
    }
    f.env.mock_all_auths();
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), None);
}

#[test]
fn set_issuer_delegate_requires_a_listed_asset() {
    let f = setup();
    let delegate = Address::generate(&f.env);
    assert_eq!(
        f.ledger.try_set_issuer_delegate(&f.sac, &Some(delegate)),
        Err(Ok(LedgerError::AssetNotListed.into()))
    );
    assert_eq!(
        f.ledger.try_set_issuer_delegate(&f.sac, &None),
        Err(Ok(LedgerError::AssetNotListed.into()))
    );
}

#[test]
fn get_issuer_delegate_on_an_unlisted_asset_is_none() {
    let f = setup();
    assert_eq!(f.ledger.get_issuer_delegate(&f.sac), None);
}

#[test]
fn issuer_delegate_set_event_matches_the_documented_layout() {
    let f = listed();
    let delegate = appoint(&f);
    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> =
        (Symbol::new(&f.env, "issuer_delegate_set"), f.sac.clone()).into_val(&f.env);
    assert_eq!(topics, expected_topics);
    let expected_data = Map::from_array(
        &f.env,
        [(Symbol::new(&f.env, "delegate"), delegate.into_val(&f.env))],
    );
    assert_eq!(data, expected_data);
}

#[test]
fn issuer_delegate_cleared_event_matches_the_documented_layout() {
    let f = listed();
    appoint(&f);
    f.ledger.set_issuer_delegate(&f.sac, &None);
    let (topics, data) = last_ledger_event(&f);
    let expected_topics: Vec<Val> = (
        Symbol::new(&f.env, "issuer_delegate_cleared"),
        f.sac.clone(),
    )
        .into_val(&f.env);
    assert_eq!(topics, expected_topics);
    assert_eq!(data, Map::<Symbol, Val>::new(&f.env));
}

#[test]
fn a_delegate_posts_issuer_signed() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let delegate = appoint(&f);
    assert_eq!(post(&f, &delegate, 101 * UNIT, T0 - 60), Tier::IssuerSigned);
    let r = f.ledger.get_report(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(r.reporter, delegate);
    assert_eq!(r.amount, 101 * UNIT);
    assert!(f
        .ledger
        .is_covered(&f.sac, &10_000, &3_600, &Tier::IssuerSigned));
}

#[test]
fn a_delegate_who_is_also_an_auditor_posts_issuer_signed() {
    let f = listed();
    f.ledger
        .set_issuer_delegate(&f.sac, &Some(f.auditor.clone()));
    assert_eq!(post(&f, &f.auditor, UNIT, T0 - 60), Tier::IssuerSigned);
}

#[test]
fn a_delegate_speaks_only_for_its_own_asset() {
    let f = listed();
    let other = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    f.ledger.list_asset(&other);
    let delegate = appoint(&f);
    assert_eq!(post(&f, &delegate, UNIT, T0 - 60), Tier::IssuerSigned);
    assert_eq!(
        f.ledger.try_post_reserve(
            &delegate,
            &other,
            &UNIT,
            &(T0 - 60),
            &hash(&f.env, 4),
            &uri(&f.env)
        ),
        Err(Ok(LedgerError::NotReporter.into()))
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
        f.admin    );
}

#[test]
fn a_cleared_delegate_cannot_post_and_its_figures_stop_counting() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let delegate = appoint(&f);
    post(&f, &delegate, 100 * UNIT, T0 - 60);
    f.ledger.set_issuer_delegate(&f.sac, &None);

    assert_eq!(
        f.ledger.try_post_reserve(
            &delegate,
            &f.sac,
            &UNIT,
            &(T0 - 30),
            &hash(&f.env, 4),
            &uri(&f.env)
        ),
        Err(Ok(LedgerError::NotReporter.into()))
    );
    assert_eq!(f.ledger.get_report(&f.sac, &Tier::IssuerSigned), None);
    assert_eq!(f.ledger.get_reports(&f.sac).len(), 0);
    assert_eq!(f.ledger.coverage(&f.sac, &Tier::Transcribed), None);
}

#[test]
fn a_replaced_delegates_figures_stop_counting_until_reappointed() {
    let f = listed();
    supply(&f, 100 * UNIT);
    let first = appoint(&f);
    post(&f, &first, 100 * UNIT, T0 - 10);
    let second = appoint(&f);
    post(&f, &second, 90 * UNIT, T0 - 60);

    // The first delegate's fresher figure no longer counts.
    let r = f.ledger.get_report(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(r.reporter, second);
    assert_eq!(
        f.ledger
            .coverage(&f.sac, &Tier::IssuerSigned)
            .unwrap()
            .reserves,
        90 * UNIT
    );

    f.ledger.set_issuer_delegate(&f.sac, &Some(first.clone()));
    let r = f.ledger.get_report(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(r.reporter, first);
}

#[test]
fn the_issuers_own_figures_count_whatever_the_delegate() {
    let f = listed();
    supply(&f, 100 * UNIT);
    post(&f, &f.issuer, 100 * UNIT, T0 - 60);
    appoint(&f);
    f.ledger.set_issuer_delegate(&f.sac, &None);
    let r = f.ledger.get_report(&f.sac, &Tier::IssuerSigned).unwrap();
    assert_eq!(r.reporter, f.issuer);
}

#[test]
fn a_former_delegates_slot_is_reclaimed_when_the_tier_is_full() {
    let f = listed();
    post(&f, &f.issuer, UNIT, T0 - 60);
    for _ in 1..MAX_REPORTERS_PER_TIER {
        let delegate = appoint(&f);
        post(&f, &delegate, UNIT, T0 - 60);
    }
    // The tier is full: the issuer and three delegates, all now former.
    let current = appoint(&f);
    assert_eq!(post(&f, &current, UNIT, T0 - 30), Tier::IssuerSigned);
    let reporters: std::vec::Vec<Address> = f
        .ledger
        .get_reports(&f.sac)
        .iter()
        .map(|r| r.reporter)
        .collect();
    assert_eq!(reporters, std::vec![f.issuer.clone(), current]);
}

// ---- version --------------------------------------------------------------

#[test]
fn version_is_the_constant() {
    let f = setup();
    assert_eq!(f.ledger.version(), VERSION);
    assert_eq!(f.ledger.version(), 1);
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
    );}
