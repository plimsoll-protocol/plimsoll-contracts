extern crate std;

use super::*;
use coverage_ledger::{CoverageLedger, CoverageLedgerClient as LedgerClient};
use plimsoll_types::Role;
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env, String,
};

const T0: u64 = 1_700_000_000;
const DAY: u64 = 86_400;
const UNIT: i128 = 10_000_000;

struct Fixture {
    env: Env,
    vault: CoveredVaultClient<'static>,
    ledger: LedgerClient<'static>,
    token: TokenClient<'static>,
    sac: Address,
    issuer: Address,
    poster: Address,
    user: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    env.ledger().set_sequence_number(1_000);

    let admin = Address::generate(&env);
    let registry_id = env.register(ReporterRegistry, (admin.clone(),));
    let registry = ReporterRegistryClient::new(&env, &registry_id);
    let ledger_id = env.register(CoverageLedger, (admin.clone(), registry_id));
    let ledger = LedgerClient::new(&env, &ledger_id);

    // The SAC admin is a contract address; the asset's issuer is the G-account
    // the test host creates, which is what `name()` reports.
    let sac_admin = Address::generate(&env);
    let handle = env.register_stellar_asset_contract_v2(sac_admin);
    let sac = handle.address();
    let issuer = handle.issuer().address();
    ledger.list_asset(&sac);

    let poster = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &String::from_str(&env, "indexer"),
    );

    let vault_id = env.register(
        CoveredVault,
        (ledger_id, sac.clone(), 10_000u32, DAY, Tier::IssuerSigned),
    );
    let vault = CoveredVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &sac).mint(&user, &(1_000 * UNIT));
    let token = TokenClient::new(&env, &sac);

    Fixture {
        env,
        vault,
        ledger,
        token,
        sac,
        issuer,
        poster,
        user,
    }
}

fn report(f: &Fixture, supply: i128, reserves: i128) {
    let seq = f.env.ledger().sequence();
    f.ledger.post_supply(
        &f.poster,
        &f.sac,
        &supply,
        &seq,
        &BytesN::from_array(&f.env, &[1; 32]),
    );
    f.ledger.post_reserve(
        &f.issuer,
        &f.sac,
        &reserves,
        &f.env.ledger().timestamp(),
        &BytesN::from_array(&f.env, &[2; 32]),
        &String::from_str(&f.env, "https://issuer.example/reserves.pdf"),
    );
}

#[test]
fn deposit_allowed_when_covered() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    let bal = f.vault.deposit(&f.user, &(100 * UNIT));
    assert_eq!(bal, 100 * UNIT);
    assert_eq!(f.vault.balance(&f.user), 100 * UNIT);
    assert_eq!(f.token.balance(&f.vault.address), 100 * UNIT);
    assert_eq!(f.token.balance(&f.user), 900 * UNIT);
}

#[test]
fn deposit_refused_when_under_backed() {
    let f = setup();
    report(&f, 1_000 * UNIT, 999 * UNIT);
    let res = f.vault.try_deposit(&f.user, &(100 * UNIT));
    assert_eq!(res, Err(Ok(VaultError::NotCovered.into())));
    assert_eq!(f.token.balance(&f.user), 1_000 * UNIT);
}

#[test]
fn deposit_refused_without_any_report() {
    let f = setup();
    let res = f.vault.try_deposit(&f.user, &UNIT);
    assert_eq!(res, Err(Ok(VaultError::NotCovered.into())));
}

#[test]
fn deposit_refused_when_stale() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.env.ledger().set_timestamp(T0 + DAY + 1);
    let res = f.vault.try_deposit(&f.user, &UNIT);
    assert_eq!(res, Err(Ok(VaultError::NotCovered.into())));
}

#[test]
fn withdraw_works_even_when_not_covered() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));
    // Coverage lapses; the user can still get out.
    f.env.ledger().set_timestamp(T0 + 10 * DAY);
    let bal = f.vault.withdraw(&f.user, &(40 * UNIT));
    assert_eq!(bal, 60 * UNIT);
    assert_eq!(f.token.balance(&f.user), 940 * UNIT);
}

#[test]
fn withdraw_more_than_balance_fails() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(10 * UNIT));
    let res = f.vault.try_withdraw(&f.user, &(11 * UNIT));
    assert_eq!(res, Err(Ok(VaultError::InsufficientBalance.into())));
}

#[test]
fn zero_amounts_rejected() {
    let f = setup();
    assert_eq!(
        f.vault.try_deposit(&f.user, &0),
        Err(Ok(VaultError::InvalidAmount.into()))
    );
    assert_eq!(
        f.vault.try_withdraw(&f.user, &0),
        Err(Ok(VaultError::InvalidAmount.into()))
    );
}

#[test]
fn config_round_trips() {
    let f = setup();
    let cfg = f.vault.config();
    assert_eq!(cfg.asset, f.sac);
    assert_eq!(cfg.min_bps, 10_000);
    assert_eq!(cfg.max_age, DAY);
    assert_eq!(cfg.min_tier, Tier::IssuerSigned);
}

// ---- can_deposit ----------------------------------------------------------

#[test]
fn can_deposit_true_when_covered() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    assert!(f.vault.can_deposit());
    // ...and a deposit does go through.
    f.vault.deposit(&f.user, &UNIT);
}

#[test]
fn can_deposit_false_when_under_backed() {
    let f = setup();
    report(&f, 1_000 * UNIT, 999 * UNIT);
    assert!(!f.vault.can_deposit());
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(VaultError::NotCovered.into()))
    );
}

#[test]
fn can_deposit_false_when_stale() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    // Exactly max_age old is still fresh.
    f.env.ledger().set_timestamp(T0 + DAY);
    assert!(f.vault.can_deposit());
    f.env.ledger().set_timestamp(T0 + DAY + 1);
    assert!(!f.vault.can_deposit());
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(VaultError::NotCovered.into()))
    );
}

#[test]
fn can_deposit_false_without_any_report() {
    let f = setup();
    assert!(!f.vault.can_deposit());
    // Supply alone is not enough either.
    let seq = f.env.ledger().sequence();
    f.ledger.post_supply(
        &f.poster,
        &f.sac,
        &(1_000 * UNIT),
        &seq,
        &BytesN::from_array(&f.env, &[1; 32]),
    );
    assert!(!f.vault.can_deposit());
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(VaultError::NotCovered.into()))
    );
}

#[test]
fn can_deposit_uses_the_stored_min_tier() {
    let f = setup();
    // A transcriber's figure is below the vault's IssuerSigned floor, so it
    // does not open the vault even though the ratio is fine.
    let transcriber = Address::generate(&f.env);
    ReporterRegistryClient::new(&f.env, &f.ledger.registry()).set_reporter(
        &transcriber,
        &Role::Transcriber,
        &String::from_str(&f.env, "volunteer"),
    );
    let seq = f.env.ledger().sequence();
    f.ledger.post_supply(
        &f.poster,
        &f.sac,
        &(1_000 * UNIT),
        &seq,
        &BytesN::from_array(&f.env, &[1; 32]),
    );
    f.ledger.post_reserve(
        &transcriber,
        &f.sac,
        &(1_000 * UNIT),
        &T0,
        &BytesN::from_array(&f.env, &[2; 32]),
        &String::from_str(&f.env, "https://example.org/report.pdf"),
    );
    assert!(!f.vault.can_deposit());
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(VaultError::NotCovered.into()))
    );
}

// ---- ReserveWarning event snapshot ----------------------------------------

#[test]
fn withdraw_emits_reserve_warning_only_when_crossing_threshold_downward() {
    use soroban_sdk::{
        testutils::Events as _, xdr::ContractEventBody, Map, Symbol, TryFromVal, Val,
    };

    let f = setup();
    // Supply is 1_000 units, vault threshold is 10_000 bps (100%).
    // Mint and deposit 1_200 units so vault starts at 120% (12_000 bps).
    StellarAssetClient::new(&f.env, &f.sac).mint(&f.user, &(500 * UNIT));
    report(&f, 1_000 * UNIT, 1_200 * UNIT);
    f.vault.deposit(&f.user, &(1_200 * UNIT));

    // Withdraw 100 units -> vault holds 1_100 units (11_000 bps >= 10_000 bps): no warning.
    f.vault.withdraw(&f.user, &(100 * UNIT));
    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    assert_eq!(events.events().len(), 1);

    // Withdraw 200 units -> vault drops to 900 units (9_000 bps < 10_000 bps): crosses downward!
    f.vault.withdraw(&f.user, &(200 * UNIT));
    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    assert_eq!(events.events().len(), 2);
    let warning = events.events().last().unwrap();
    let ContractEventBody::V0(body) = &warning.body;
    assert_eq!(
        Symbol::try_from_val(&f.env, body.topics.first().unwrap()).unwrap(),
        Symbol::new(&f.env, "reserve_warning")
    );
    let data =
        Map::<Symbol, Val>::try_from_val(&f.env, &Val::try_from_val(&f.env, &body.data).unwrap())
            .unwrap();
    assert_eq!(
        i128::try_from_val(&f.env, &data.get(Symbol::new(&f.env, "balance")).unwrap()).unwrap(),
        900 * UNIT
    );
    assert_eq!(
        u32::try_from_val(
            &f.env,
            &data.get(Symbol::new(&f.env, "threshold_bps")).unwrap()
        )
        .unwrap(),
        10_000
    );

    // Withdraw another 100 units while already below threshold -> no second warning.
    f.vault.withdraw(&f.user, &(100 * UNIT));
    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    assert_eq!(events.events().len(), 1);
}
