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

// ---- delegation & dispute count -------------------------------------------

#[test]
fn post_reserve_accepts_auth_from_delegate_and_rejects_after_revoke() {
    let f = setup();
    f.ledger.list_asset(&f.sac);
    let seq = f.env.ledger().sequence();
    f.ledger
        .post_supply(&f.poster, &f.sac, &(1_000 * UNIT), &seq, &hash(&f.env, 1));

    let delegate = Address::generate(&f.env);
    f.registry.set_delegate(&f.auditor, &delegate);

    let tier = f.ledger.post_reserve(
        &delegate,
        &f.sac,
        &(1_050 * UNIT),
        &T0,
        &hash(&f.env, 2),
        &uri(&f.env),
    );
    assert_eq!(tier, Tier::AuditorSigned);
    let report = f
        .ledger
        .get_report(&f.sac, &Tier::AuditorSigned)
        .expect("report recorded under delegating reporter");
    assert_eq!(report.reporter, f.auditor);

    f.registry.revoke_delegate(&f.auditor);
    assert_eq!(
        f.ledger.try_post_reserve(
            &delegate,
            &f.sac,
            &(1_060 * UNIT),
            &(T0 + 1),
            &hash(&f.env, 3),
            &uri(&f.env),
        ),
        Err(Ok(LedgerError::NotReporter.into()))
    );
}

#[test]
fn get_dispute_count_returns_zero_by_default_and_reads_open_disputes() {
    let f = setup();
    let asset = soroban_sdk::Bytes::from_slice(&f.env, b"USDC:GISSUER");
    assert_eq!(f.ledger.get_dispute_count(&asset), 0);

    f.env.as_contract(&f.ledger.address, || {
        write_persistent(&f.env, &DataKey::DisputeCount(asset.clone()), &3u32);
    });
    assert_eq!(f.ledger.get_dispute_count(&asset), 3);
}
