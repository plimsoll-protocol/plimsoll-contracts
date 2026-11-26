//! Snapshot tests for the `coverage-ledger` and `covered-vault` event schemas.
//!
//! Each test drives its contract through every event it can emit, decodes the
//! raw topics and data map the way the indexer sees them, and compares the
//! normalised result with a checked-in file under `tests/snapshots/`.
//!
//! Addresses, byte arrays and strings are rendered as type placeholders: their
//! values are random in the test host and are not part of the event schema.
//! Numbers and symbols are rendered as themselves, so a changed tier, amount
//! key or event name is visible in the diff.
//!
//! Regenerate intentionally after updating `docs/SPEC.md` and
//! `docs/EVENTS.md`:
//!
//! ```bash
//! UPDATE_SNAPSHOTS=1 cargo test -p plimsoll-event-snapshots
//! ```
extern crate std;

use coverage_ledger::{CoverageLedger, CoverageLedgerClient};
use covered_vault::{CoveredVault, CoveredVaultClient};
use plimsoll_types::{Role, Tier};
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    xdr::ContractEventBody,
    Address, Bytes, BytesN, Env, Executable, Map, Symbol, TryFromVal, Val, Vec,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    string::String as StdString,
};

const T0: u64 = 1_700_000_000;
const DAY: u64 = 86_400;
const UNIT: i128 = 10_000_000;

/// The vendored fixture the upgrade tests already use, so executing an upgrade
/// in the harness does not need `stellar contract build` first.
const UPGRADE_WASM: &[u8] =
    include_bytes!("../../../contracts/coverage-ledger/testdata/upgrade_fixture_ledger.wasm");

#[derive(Clone, Debug, Eq, PartialEq)]
struct EventRecord {
    name: StdString,
    topics: std::vec::Vec<StdString>,
    /// Rendered JSON for the data map, or `null` when the event has no data.
    data: StdString,
}

struct Fixture {
    env: Env,
    ledger_id: Address,
    ledger: CoverageLedgerClient<'static>,
    registry_id: Address,
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
    let ledger_id = env.register(CoverageLedger, (admin.clone(), registry_id.clone()));
    let ledger = CoverageLedgerClient::new(&env, &ledger_id);

    // The SAC issuer is a G-account the test host creates; the snapshot renders
    // it as an account placeholder, so its random value does not leak in.
    let sac_admin = Address::generate(&env);
    let handle = env.register_stellar_asset_contract_v2(sac_admin);
    let sac = handle.address();
    let issuer = handle.issuer().address();

    let registry = ReporterRegistryClient::new(&env, &registry_id);
    let poster = Address::generate(&env);
    let auditor = Address::generate(&env);
    let transcriber = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &soroban_sdk::String::from_str(&env, "indexer"),
    );
    registry.set_reporter(
        &auditor,
        &Role::Auditor,
        &soroban_sdk::String::from_str(&env, "Acme Audit"),
    );
    registry.set_reporter(
        &transcriber,
        &Role::Transcriber,
        &soroban_sdk::String::from_str(&env, "volunteer"),
    );

    Fixture {
        env,
        ledger_id,
        ledger,
        registry_id,
        sac,
        issuer,
        poster,
        auditor,
        transcriber,
    }
}

/// Take the events the last top-level invocation emitted from `contract`.
fn take(env: &Env, contract: &Address) -> std::vec::Vec<EventRecord> {
    let all = env.events().all().filter_by_contract(contract);
    all.events()
        .iter()
        .map(|event| {
            let ContractEventBody::V0(body) = &event.body;
            let topics: std::vec::Vec<Val> = body
                .topics
                .iter()
                .map(|topic| Val::try_from_val(env, topic).expect("a decodable topic"))
                .collect();
            let data = Val::try_from_val(env, &body.data).expect("decodable event data");
            let name = topics
                .first()
                .and_then(|topic| Symbol::try_from_val(env, topic).ok())
                .map(|symbol| symbol.to_string())
                .unwrap_or_else(|| "<unnamed>".to_string());
            EventRecord {
                name,
                topics: topics.iter().map(|topic| render(env, topic)).collect(),
                data: render_data(env, &data),
            }
        })
        .collect()
}

fn escape(value: &str) -> StdString {
    let mut out = StdString::new();
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// A stable rendering of one value: literals that are part of the schema are
/// kept, caller-specific values become typed placeholders.
fn render(env: &Env, val: &Val) -> StdString {
    if let Ok(addr) = Address::try_from_val(env, val) {
        let kind = match addr.executable() {
            Some(Executable::Account) | None => "<account>",
            Some(Executable::StellarAsset) | Some(Executable::Wasm(_)) => "<contract>",
        };
        return std::format!("\"{kind}\"");
    }
    if BytesN::<32>::try_from_val(env, val).is_ok() {
        return "\"<bytes32>\"".to_string();
    }
    if let Ok(symbol) = Symbol::try_from_val(env, val) {
        return std::format!("\"{}\"", escape(&symbol.to_string()));
    }
    if soroban_sdk::String::try_from_val(env, val).is_ok() {
        return "\"<string>\"".to_string();
    }
    if let Ok(amount) = i128::try_from_val(env, val) {
        return std::format!("{amount}");
    }
    if let Ok(value) = u64::try_from_val(env, val) {
        return std::format!("{value}");
    }
    if let Ok(value) = u32::try_from_val(env, val) {
        return std::format!("{value}");
    }
    if Map::<Symbol, Val>::try_from_val(env, val).is_ok() {
        return render_data(env, val);
    }
    if let Ok(items) = Vec::<Val>::try_from_val(env, val) {
        let rendered: std::vec::Vec<StdString> =
            items.iter().map(|item| render(env, &item)).collect();
        return std::format!("[{}]", rendered.join(", "));
    }
    std::format!("\"{}\"", escape(&std::format!("{val:?}")))
}

fn render_data(env: &Env, data: &Val) -> StdString {
    if <()>::try_from_val(env, data).is_ok() {
        return "null".to_string();
    }
    let Ok(map) = Map::<Symbol, Val>::try_from_val(env, data) else {
        return render(env, data);
    };
    // The encoded map is keyed by symbol and Soroban sorts those keys; sorting
    // again here makes that explicit and independent of the host's iteration.
    let mut entries: BTreeMap<StdString, Val> = BTreeMap::new();
    for (key, value) in map.iter() {
        entries.insert(key.to_string(), value);
    }
    if entries.is_empty() {
        return "{}".to_string();
    }
    let mut out = StdString::from("{\n");
    let last = entries.len();
    for (i, (key, value)) in entries.iter().enumerate() {
        out.push_str(&std::format!(
            "        \"{}\": {}",
            escape(key),
            render(env, value)
        ));
        if i + 1 < last {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("      }");
    out
}

fn render_snapshot(contract: &str, events: &[EventRecord]) -> StdString {
    let mut out = StdString::from("{\n");
    out.push_str(&std::format!("  \"contract\": \"{contract}\",\n"));
    out.push_str("  \"events\": [\n");
    for (i, event) in events.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&std::format!("      \"name\": \"{}\",\n", event.name));
        out.push_str("      \"topics\": [\n");
        for (j, topic) in event.topics.iter().enumerate() {
            out.push_str(&std::format!("        {topic}"));
            if j + 1 < event.topics.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("      ],\n");
        out.push_str(&std::format!("      \"data\": {}\n", event.data));
        out.push_str("    }");
        if i + 1 < events.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

fn snapshot_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/snapshots")
        .join(name)
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = snapshot_path(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual)
            .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing event snapshot {}: {error}\nRun `UPDATE_SNAPSHOTS=1 cargo test -p plimsoll-event-snapshots` and review the result.",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "\nevent schema changed: {name}\nIf this is intentional, update docs/SPEC.md and docs/EVENTS.md, then regenerate with `UPDATE_SNAPSHOTS=1 cargo test -p plimsoll-event-snapshots` and review the diff.\n"
    );
}

#[test]
fn coverage_ledger_event_schema_matches_the_snapshot() {
    let f = setup();
    let mut events: std::vec::Vec<EventRecord> = std::vec::Vec::new();

    f.ledger.list_asset(&f.sac);
    events.extend(take(&f.env, &f.ledger_id));

    let sequence = f.env.ledger().sequence();
    f.ledger.post_supply(
        &f.poster,
        &f.sac,
        &(1_000 * UNIT),
        &sequence,
        &BytesN::from_array(&f.env, &[1; 32]),
    );
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(1_000 * UNIT),
        &T0,
        &BytesN::from_array(&f.env, &[2; 32]),
        &soroban_sdk::String::from_str(&f.env, "https://issuer.example/reserves.pdf"),
    );
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.post_reserve(
        &f.auditor,
        &f.sac,
        &(990 * UNIT),
        &T0,
        &BytesN::from_array(&f.env, &[3; 32]),
        &soroban_sdk::String::from_str(&f.env, "https://auditor.example/report.pdf"),
    );
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.post_reserve(
        &f.transcriber,
        &f.sac,
        &(980 * UNIT),
        &T0,
        &BytesN::from_array(&f.env, &[4; 32]),
        &soroban_sdk::String::from_str(&f.env, "https://example.org/published.pdf"),
    );
    events.extend(take(&f.env, &f.ledger_id));

    // The registry address stands in as the token: enabling does not call it,
    // and the event only records the opt-in.
    f.ledger.enable_native_supply(&f.sac, &f.registry_id);
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.pause();
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.unpause();
    events.extend(take(&f.env, &f.ledger_id));

    let next_admin = Address::generate(&f.env);
    f.ledger.transfer_admin(&next_admin);
    events.extend(take(&f.env, &f.ledger_id));

    let wasm_hash = f
        .env
        .deployer()
        .upload_contract_wasm(Bytes::from_slice(&f.env, UPGRADE_WASM));
    f.ledger.propose_upgrade(&wasm_hash);
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.cancel_upgrade();
    events.extend(take(&f.env, &f.ledger_id));

    f.ledger.propose_upgrade(&wasm_hash);
    events.extend(take(&f.env, &f.ledger_id));

    f.env
        .ledger()
        .set_sequence_number(1_000 + plimsoll_types::UPGRADE_DELAY_LEDGERS);
    f.ledger.execute_upgrade();
    events.extend(take(&f.env, &f.ledger_id));

    // Sanity: every event type in docs/EVENTS.md is represented.
    let names: std::vec::Vec<&str> = events.iter().map(|event| event.name.as_str()).collect();
    for expected in [
        "asset_listed",
        "supply_posted",
        "reserve_posted",
        "native_supply_enabled",
        "paused",
        "unpaused",
        "admin_transferred",
        "upgrade_proposed",
        "upgrade_cancelled",
        "upgrade_executed",
    ] {
        assert!(
            names.contains(&expected),
            "the harness did not emit {expected}; events: {names:?}"
        );
    }

    assert_snapshot(
        "coverage-ledger.events.json",
        &render_snapshot("coverage-ledger", &events),
    );
}

#[test]
fn covered_vault_event_schema_matches_the_snapshot() {
    let f = setup();
    f.ledger.list_asset(&f.sac);

    let sequence = f.env.ledger().sequence();
    f.ledger.post_supply(
        &f.poster,
        &f.sac,
        &(1_000 * UNIT),
        &sequence,
        &BytesN::from_array(&f.env, &[1; 32]),
    );
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &(1_000 * UNIT),
        &T0,
        &BytesN::from_array(&f.env, &[2; 32]),
        &soroban_sdk::String::from_str(&f.env, "https://issuer.example/reserves.pdf"),
    );

    let vault_id = f.env.register(
        CoveredVault,
        (
            f.ledger_id.clone(),
            f.sac.clone(),
            10_000u32,
            DAY,
            Tier::IssuerSigned,
        ),
    );
    let vault = CoveredVaultClient::new(&f.env, &vault_id);
    let user = Address::generate(&f.env);
    soroban_sdk::token::StellarAssetClient::new(&f.env, &f.sac).mint(&user, &(100 * UNIT));

    let mut events: std::vec::Vec<EventRecord> = std::vec::Vec::new();

    vault.deposit(&user, &(40 * UNIT));
    events.extend(take(&f.env, &vault_id));

    vault.withdraw(&user, &(10 * UNIT));
    events.extend(take(&f.env, &vault_id));

    let names: std::vec::Vec<&str> = events.iter().map(|event| event.name.as_str()).collect();
    for expected in ["deposited", "withdrawn"] {
        assert!(
            names.contains(&expected),
            "the harness did not emit {expected}; events: {names:?}"
        );
    }

    assert_snapshot(
        "covered-vault.events.json",
        &render_snapshot("covered-vault", &events),
    );
}
