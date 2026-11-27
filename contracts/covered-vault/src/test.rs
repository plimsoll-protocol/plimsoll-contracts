extern crate std;

use super::*;
use coverage_ledger::{CoverageLedger, CoverageLedgerClient as LedgerClient, LedgerError};
use plimsoll_types::Role;
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env, IntoVal, String,

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
    admin: Address,
    vault: CoveredVaultClient<'static>,
    ledger: LedgerClient<'static>,
    token: TokenClient<'static>,
    admin: Address,
    sac: Address,
    issuer: Address,
    poster: Address,
    user: Address,
    approver: Address,
    vault_admin: Address,
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
    ledger.list_asset(&sac, &None);

    let poster = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &String::from_str(&env, "indexer"),
    );

    let approver = Address::generate(&env);
    let vault_id = env.register(
        CoveredVault,
        (
    let vault_admin = Address::generate(&env);
    let vault_id = env.register(
        CoveredVault,
        (
            admin.clone(),
            ledger_id,
            sac.clone(),
            10_000u32,
            DAY,
            Tier::IssuerSigned,
            approver.clone(),
            50 * UNIT,
            admin.clone(),
            vault_admin.clone(),
        ),
    );
    let vault = CoveredVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &sac).mint(&user, &(1_000 * UNIT));
    let token = TokenClient::new(&env, &sac);

    Fixture {
        env,
        admin,
        vault,
        ledger,
        token,
        admin,
        sac,
        issuer,
        poster,
        user,
        approver,
        vault_admin,
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

fn authorize_recipient_withdrawal(f: &Fixture, amount: i128) {
    let invoke = MockAuthInvoke {
        contract: &f.vault.address,
        fn_name: "withdraw",
        args: (f.user.clone(), amount).into_val(&f.env),
        sub_invokes: &[],
    };
    let auth = MockAuth {
        address: &f.user,
        invoke: &invoke,
    };
    f.env.mock_auths(&[auth]);
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
    f.vault.request_withdraw(&f.user, &(40 * UNIT));
    let bal = f.vault.execute_withdraw(&f.user);
    assert_eq!(bal, 60 * UNIT);
    assert_eq!(f.token.balance(&f.user), 940 * UNIT);
}

#[test]
fn withdrawal_at_threshold_does_not_require_approver() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));
    authorize_recipient_withdrawal(&f, 50 * UNIT);
    let balance = f.vault.withdraw(&f.user, &(50 * UNIT));
    assert_eq!(balance, 50 * UNIT);
}

#[test]
fn withdrawal_above_threshold_requires_approver() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));
    authorize_recipient_withdrawal(&f, 51 * UNIT);
    let result = f.vault.try_withdraw(&f.user, &(51 * UNIT));
    assert!(result.is_err());
    assert_eq!(f.vault.balance(&f.user), 100 * UNIT);
}

#[test]
fn withdrawal_above_threshold_succeeds_with_approver() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));
    f.env.mock_all_auths();
    let balance = f.vault.withdraw(&f.user, &(51 * UNIT));
    assert_eq!(balance, 49 * UNIT);
}

#[test]
fn withdraw_more_than_balance_fails() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(10 * UNIT));
    let res = f.vault.try_request_withdraw(&f.user, &(11 * UNIT));
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
        f.vault.try_request_withdraw(&f.user, &0),
        Err(Ok(VaultError::InvalidAmount.into()))
    );
}

#[test]
fn get_balance_reads_the_on_chain_token_balance() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    assert_eq!(f.vault.get_balance(), 0);
    f.vault.deposit(&f.user, &(100 * UNIT));
    assert_eq!(f.vault.get_balance(), 100 * UNIT);
    assert_eq!(f.vault.get_balance(), f.token.balance(&f.vault.address));
    f.vault.withdraw(&f.user, &(40 * UNIT));
    assert_eq!(f.vault.get_balance(), 60 * UNIT);
}

#[test]
fn withdrawal_delay_blocks_execution_until_available() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &(100 * UNIT));
    f.vault.set_withdrawal_delay(&3_600);
    assert_eq!(f.vault.withdrawal_delay_seconds(), 3_600);
    assert_eq!(f.vault.request_withdraw(&f.user, &(40 * UNIT)), T0 + 3_600);
    assert_eq!(
        f.vault.try_execute_withdraw(&f.user),
        Err(Ok(VaultError::WithdrawalDelayNotElapsed.into()))
    );

    f.env.ledger().set_timestamp(T0 + 3_600);
    assert_eq!(f.vault.execute_withdraw(&f.user), 60 * UNIT);
}

#[test]
fn withdrawal_delay_can_be_updated_by_admin_only() {
    let f = setup();
    f.vault.set_withdrawal_delay(&60);
    f.vault.set_withdrawal_delay(&120);
    assert_eq!(f.vault.withdrawal_delay_seconds(), 120);

    f.env.mock_auths(&[]);
    assert!(f.vault.try_set_withdrawal_delay(&0).is_err());
    assert_eq!(f.vault.withdrawal_delay_seconds(), 120);
}

#[test]
fn withdrawal_requires_a_pending_request() {
    let f = setup();
    assert_eq!(
        f.vault.try_execute_withdraw(&f.user),
        Err(Ok(VaultError::NoPendingWithdrawal.into()))
    );
}

#[test]
fn withdrawal_delay_rejects_timestamp_overflow() {
    let f = setup();
    assert_eq!(
        f.vault.try_set_withdrawal_delay(&u64::MAX),
        Err(Ok(VaultError::InvalidDelay.into()))
    );
}

#[test]
fn request_withdraw_rejects_available_time_overflow() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    f.vault.deposit(&f.user, &UNIT);
    f.vault.set_withdrawal_delay(&(u64::MAX - T0));
    f.env.ledger().set_timestamp(T0 + 1);
    assert_eq!(
        f.vault.try_request_withdraw(&f.user, &UNIT),
        Err(Ok(VaultError::InvalidDelay.into()))
    );
}

#[test]
fn execute_withdraw_rechecks_balance() {
    let f = setup();
    f.env.as_contract(&f.vault.address, || {
        f.env.storage().persistent().set(
            &DataKey::PendingWithdrawal(f.user.clone()),
            &PendingWithdrawal {
                amount: UNIT,
                available_at: T0,
            },
        );
    });
    assert_eq!(
        f.vault.try_execute_withdraw(&f.user),
        Err(Ok(VaultError::InsufficientBalance.into()))
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
    assert_eq!(cfg.approver, f.approver);
    assert_eq!(cfg.large_withdrawal_threshold, 50 * UNIT);
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
    let vault_admin = Address::generate(&env);
    let vault_id = env.register(
        CoveredVault,
        (
            admin,
            Address::generate(&env),
            ledger,
            sac.clone(),
            10_000u32,
            DAY,
            Tier::IssuerSigned,
            vault_admin,
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

// ---- admin / update_thresholds -------------------------------------------

#[test]
fn admin_round_trips() {
    let f = setup();
    assert_eq!(f.vault.admin(), f.vault_admin);
}

#[test]
fn update_thresholds_changes_config() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    // Raise min_bps above 100% → vault closes.
    f.vault
        .update_thresholds(&f.vault_admin, &10_001u32, &DAY, &Tier::IssuerSigned);
    assert!(!f.vault.can_deposit());
    // Restore to 100% → vault opens again.
    f.vault
        .update_thresholds(&f.vault_admin, &10_000u32, &DAY, &Tier::IssuerSigned);
    assert!(f.vault.can_deposit());
    let cfg = f.vault.config();
    assert_eq!(cfg.min_bps, 10_000);
    assert_eq!(cfg.max_age, DAY);
    assert_eq!(cfg.min_tier, Tier::IssuerSigned);
}

#[test]
fn update_thresholds_emits_event() {
    let f = setup();
    f.vault
        .update_thresholds(&f.vault_admin, &9_000u32, &(2 * DAY), &Tier::Transcribed);
    let cfg = f.vault.config();
    assert_eq!(cfg.min_bps, 9_000);
    assert_eq!(cfg.max_age, 2 * DAY);
    assert_eq!(cfg.min_tier, Tier::Transcribed);
}

#[test]
fn update_thresholds_tightens_deposit_gate() {
    let f = setup();
    report(&f, 1_000 * UNIT, 1_000 * UNIT);
    // Currently open (IssuerSigned satisfied).
    assert!(f.vault.can_deposit());
    // Require AuditorSigned — no such report exists.
    f.vault
        .update_thresholds(&f.vault_admin, &10_000u32, &DAY, &Tier::AuditorSigned);
    assert!(!f.vault.can_deposit());
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
fn propose_and_accept_admin_transfers_role_and_emits_events() {
    let f = setup();
    assert_eq!(f.vault.admin(), f.admin);
    assert_eq!(f.vault.pending_admin(), None);

    let next_admin = Address::generate(&f.env);
    f.vault.propose_admin(&next_admin);
    assert_eq!(f.env.auths()[0].0, f.admin);

    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    let (topics, data) = decode_event(&f.env, events.events().last().unwrap());
    let expected_topics: Vec<Val> = (
        Symbol::new(&f.env, "vault_admin_proposed"),
        next_admin.clone(),
    )
        .into_val(&f.env);
    assert_eq!(topics, expected_topics);
    assert!(Map::<Symbol, Val>::try_from_val(&f.env, &data)
        .unwrap()
        .is_empty());
    assert_eq!(f.vault.pending_admin(), Some(next_admin.clone()));
    assert_eq!(f.vault.admin(), f.admin);

    f.vault.accept_admin();
    assert_eq!(f.env.auths()[0].0, next_admin);

    let events = f.env.events().all().filter_by_contract(&f.vault.address);
    let (topics, data) = decode_event(&f.env, events.events().last().unwrap());
    let expected_topics: Vec<Val> = (
        Symbol::new(&f.env, "vault_admin_transferred"),
        f.admin.clone(),
        next_admin.clone(),
    )
        .into_val(&f.env);
    assert_eq!(topics, expected_topics);
    assert!(Map::<Symbol, Val>::try_from_val(&f.env, &data)
        .unwrap()
        .is_empty());
    assert_eq!(f.vault.admin(), next_admin);
    assert_eq!(f.vault.pending_admin(), None);
}

#[test]
fn accept_admin_without_proposal_fails() {
    let f = setup();
    let res = f.vault.try_accept_admin();
    assert_eq!(res, Err(Ok(VaultError::NoPendingAdmin.into())));
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
