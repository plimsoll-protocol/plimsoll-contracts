extern crate std;

use super::*;
use coverage_ledger::{CoverageLedger, CoverageLedgerClient as LedgerClient, LedgerError};
use plimsoll_types::Role;
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEvent, ContractEventBody},
    Address, BytesN, Env, IntoVal, Map, String, Symbol, TryFromVal, Val, Vec,
};

const T0: u64 = 1_700_000_000;
const DAY: u64 = 86_400;
const UNIT: i128 = 10_000_000;

struct Fixture {
    env: Env,
    vault: CoveredVaultClient<'static>,
    ledger: LedgerClient<'static>,
    token: TokenClient<'static>,
    admin: Address,
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
        (
            admin.clone(),
            ledger_id,
            sac.clone(),
            10_000u32,
            DAY,
            Tier::IssuerSigned,
        ),
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
        admin,
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

// ---- coverage_dropped -----------------------------------------------------

/// The topics and the data of one event, as an off-chain reader decodes them.
fn decode_event(env: &Env, event: &ContractEvent) -> (Vec<Val>, Val) {
    let ContractEventBody::V0(body) = &event.body;
    let mut topics = Vec::new(env);
    for topic in body.topics.iter() {
        topics.push_back(Val::try_from_val(env, topic).unwrap());
    }
    (topics, Val::try_from_val(env, &body.data).unwrap())
}

/// A ledger stand-in whose `is_covered` answer changes from one call to the
/// next. The real ledger answers from the stored supply snapshot and reports,
/// and a deposit writes neither, so this is the only way to make the
/// post-deposit read disagree with the gate: the case the event is for.
#[contract]
pub struct FlakyLedger;

#[contractimpl]
impl FlakyLedger {
    pub fn is_covered(
        env: Env,
        _sac: Address,
        _min_bps: u32,
        _max_age: u64,
        _min_tier: Tier,
    ) -> bool {
        let key = Symbol::new(&env, "calls");
        let calls: u32 = env.storage().instance().get(&key).unwrap_or(0);
        env.storage().instance().set(&key, &(calls + 1));
        calls == 0
    }
}

#[test]
fn deposit_emits_coverage_dropped_when_post_deposit_coverage_is_false() {
    let env = Env::default();
    env.mock_all_auths();
    let sac = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let admin = Address::generate(&env);
    let ledger = env.register(FlakyLedger, ());
    let vault_id = env.register(
        CoveredVault,
        (
            admin,
            ledger,
            sac.clone(),
            10_000u32,
            DAY,
            Tier::IssuerSigned,
        ),
    );
    let vault = CoveredVaultClient::new(&env, &vault_id);
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &sac).mint(&user, &(100 * UNIT));

    // The deposit commits: the gate said yes and the balance is recorded.
    assert_eq!(vault.deposit(&user, &(40 * UNIT)), 40 * UNIT);

    // `all()` reports the events of the last invocation, so read them before
    // any other contract call.
    let events = env.events().all().filter_by_contract(&vault_id);
    let events = events.events();
    assert_eq!(events.len(), 2);
    let (topics, data) = decode_event(&env, events.last().unwrap());
    let expected: Vec<Val> = (Symbol::new(&env, "coverage_dropped"), sac.clone()).into_val(&env);
    assert_eq!(topics, expected);
    let data = Map::<Symbol, Val>::try_from_val(&env, &data).unwrap();
    assert!(data.is_empty());

    assert_eq!(vault.balance(&user), 40 * UNIT);
    assert_eq!(TokenClient::new(&env, &sac).balance(&vault_id), 40 * UNIT);
}

#[test]
fn a_covered_deposit_publishes_only_deposited() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));

    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    let events = events.events();
    assert_eq!(events.len(), 1);
    let (topics, _) = decode_event(&f.env, events.last().unwrap());
    let expected: Vec<Val> = (Symbol::new(&f.env, "deposited"), f.user.clone()).into_val(&f.env);
    assert_eq!(topics, expected);
}

#[test]
fn a_refused_deposit_publishes_no_event() {
    let f = setup();
    // Not covered. The error rolls the transaction back, events included, so a
    // keeper cannot learn about a drop from the refusal itself.
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(VaultError::NotCovered.into()))
    );
    assert_eq!(f.env.events().all().events().len(), 0);
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

#[test]
fn deposit_propagates_a_paused_ledger() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.ledger.pause();

    // A pause is not an under-backed asset: the ledger's Paused error reaches
    // the depositor unchanged.
    assert_eq!(
        f.vault.try_deposit(&f.user, &UNIT),
        Err(Ok(LedgerError::Paused.into()))
    );
}

#[test]
fn propose_and_accept_vault_admin_transfers_the_role() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.vault.propose_vault_admin(&new_admin);
    assert_eq!(f.vault.pending_vault_admin(), Some(new_admin.clone()));

    f.vault.accept_vault_admin();
    assert_eq!(f.vault.admin(), new_admin);
    assert_eq!(f.vault.pending_vault_admin(), None);
}

#[test]
fn only_current_admin_can_propose_vault_admin() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    f.vault.propose_vault_admin(&new_admin);

    let auths = f.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, f.admin);
}

#[test]
fn propose_vault_admin_without_admin_auth_fails() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    // Clear mocked auths so require_auth() has nothing to satisfy.
    f.env.mock_auths(&[]);
    let res = f.vault.try_propose_vault_admin(&new_admin);
    assert!(res.is_err());
    assert_eq!(f.vault.pending_vault_admin(), None);
}

#[test]
fn a_new_proposal_cancels_the_pending_one() {
    let f = setup();
    let first = Address::generate(&f.env);
    let second = Address::generate(&f.env);

    f.vault.propose_vault_admin(&first);
    f.vault.propose_vault_admin(&second);
    assert_eq!(f.vault.pending_vault_admin(), Some(second.clone()));

    f.vault.accept_vault_admin();
    assert_eq!(f.vault.admin(), second);
}

#[test]
fn accept_vault_admin_without_a_pending_proposal_fails() {
    let f = setup();
    assert_eq!(
        f.vault.try_accept_vault_admin(),
        Err(Ok(VaultError::NoPendingAdmin.into()))
    );
}
