extern crate std;

use super::*;
#[allow(unused_imports)]
use plimsoll_types::RegistryRole;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::StellarAssetClient,
    Address, Bytes, BytesN, Env, IntoVal, String,

    Address, Bytes, BytesN, Env, IntoVal, String, Symbol,
    xdr::ContractEventBody,
    Address, Bytes, BytesN, Env, IntoVal, Map, String, Symbol, TryFromVal, Val, Vec,
};

const T0: u64 = 1_700_000_000;

#[allow(dead_code)]
fn upgrade_fixture_wasm() -> std::vec::Vec<u8> {
    const VENDORED: &[u8] =
        include_bytes!("../../upgrade-fixture-registry/testdata/upgrade_fixture_registry.wasm");
    let built = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/wasm32v1-none/release/upgrade_fixture_registry.wasm");
    std::fs::read(built).unwrap_or_else(|_| VENDORED.to_vec())
}

#[allow(dead_code)]
fn upload_upgrade_fixture(env: &Env) -> BytesN<32> {
    env.deployer()
        .upload_contract_wasm(Bytes::from_slice(env, &upgrade_fixture_wasm()))
}

fn setup() -> (Env, Address, ReporterRegistryClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin.clone(),));
    let client = ReporterRegistryClient::new(&env, &id);
    (env, admin, client)
}

#[test]
fn constructor_sets_admin() {
    let (_env, admin, client) = setup();
    assert_eq!(client.admin(), admin);
}

#[test]
fn set_reporter_stores_role_and_name() {
    let (env, admin, client) = setup();
    let auditor = Address::generate(&env);
    client.set_reporter(
        &auditor,
        &Role::Auditor,
        &String::from_str(&env, "Acme Audit LLP"),
    );

    assert_eq!(env.auths()[0].0, admin);
    let info = client.get_reporter(&auditor).unwrap();
    assert_eq!(info.role, Role::Auditor);
    assert_eq!(info.name, String::from_str(&env, "Acme Audit LLP"));
    assert_eq!(info.added_at, 1_700_000_000);
    assert_eq!(client.role_of(&auditor), Some(Role::Auditor));
}

#[test]
fn set_reporter_emits_event() {
    let (env, _admin, client) = setup();
    let poster = Address::generate(&env);
    client.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &String::from_str(&env, "indexer"),
    );
    assert_eq!(env.events().all().events().len(), 1);
}

#[test]
fn stake_requirement_is_configurable() {
    let (env, _admin, client) = setup();
    let stake_asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    client.set_stake_requirement(&stake_asset, &100);
    assert_eq!(client.min_stake_stroops(), 100);
}

#[test]
fn negative_stake_requirement_is_rejected() {
    let (env, _admin, client) = setup();
    let stake_asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    assert_eq!(
        client.try_set_stake_requirement(&stake_asset, &-1),
        Err(Ok(RegistryError::InvalidStakeAmount.into()))
    );
}

#[test]
fn register_reporter_rejects_insufficient_stake() {
    let (env, _admin, client) = setup();
    let stake = env.register_stellar_asset_contract_v2(Address::generate(&env));
    client.set_stake_requirement(&stake.address(), &100);
    let reporter = Address::generate(&env);
    StellarAssetClient::new(&env, &stake.address()).mint(&reporter, &99);

    assert_eq!(
        client.try_register_reporter(&reporter, &String::from_str(&env, "indexer")),
        Err(Ok(RegistryError::InsufficientStake.into()))
    );
    assert_eq!(client.role_of(&reporter), None);
}

#[test]
fn register_reporter_adds_a_supply_poster_with_sufficient_stake() {
    let (env, _admin, client) = setup();
    let stake = env.register_stellar_asset_contract_v2(Address::generate(&env));
    client.set_stake_requirement(&stake.address(), &100);
    let reporter = Address::generate(&env);
    StellarAssetClient::new(&env, &stake.address()).mint(&reporter, &100);

    client.register_reporter(&reporter, &String::from_str(&env, "indexer"));

    assert_eq!(client.role_of(&reporter), Some(Role::SupplyPoster));
    assert!(client.has_minimum_stake(&reporter));
}

#[test]
fn registered_reporter_must_keep_the_minimum_balance() {
    let (env, _admin, client) = setup();
    let stake = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let stake_asset = stake.address();
    client.set_stake_requirement(&stake_asset, &100);
    let reporter = Address::generate(&env);
    let token = StellarAssetClient::new(&env, &stake_asset);
    token.mint(&reporter, &100);
    client.register_reporter(&reporter, &String::from_str(&env, "indexer"));
    token.burn(&reporter, &1);

    assert!(!client.has_minimum_stake(&reporter));
}

#[test]
fn reporter_cannot_self_register_twice() {
    let (env, _admin, client) = setup();
    let reporter = Address::generate(&env);
    client.register_reporter(&reporter, &String::from_str(&env, "indexer"));

    assert_eq!(
        client.try_register_reporter(&reporter, &String::from_str(&env, "again")),
        Err(Ok(RegistryError::ReporterAlreadyRegistered.into()))
    );
}

#[test]
fn changing_role_keeps_added_at() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "vol"));
    env.ledger().set_timestamp(1_800_000_000);
    client.set_reporter(
        &who,
        &Role::Auditor,
        &String::from_str(&env, "vol, now audit"),
    );
    let info = client.get_reporter(&who).unwrap();
    assert_eq!(info.role, Role::Auditor);
    assert_eq!(info.added_at, 1_700_000_000);
}

#[test]
fn empty_name_rejected() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let res = client.try_set_reporter(&who, &Role::Auditor, &String::from_str(&env, ""));
    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));
}

#[test]
fn long_name_rejected() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let long = String::from_str(&env, &"x".repeat(65));
    let res = client.try_set_reporter(&who, &Role::Auditor, &long);
    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));
}

#[test]
fn revoke_removes_reporter() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));
    client.revoke(&who);
    assert_eq!(client.role_of(&who), None);
}

#[test]
fn revoke_unknown_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let res = client.try_revoke(&who);
    assert_eq!(res, Err(Ok(RegistryError::ReporterNotFound.into())));
}

#[test]
fn batch_deregister_removes_registered_and_skips_unknown() {
    let (env, _admin, client) = setup();
    let known = Address::generate(&env);
    let unknown = Address::generate(&env);
    client.set_reporter(&known, &Role::Auditor, &String::from_str(&env, "a"));

    let mut addresses = Vec::new(&env);
    addresses.push_back(known.clone());
    addresses.push_back(unknown.clone());
    client.batch_deregister(&addresses);

    assert_eq!(client.role_of(&known), None);
    assert_eq!(client.role_of(&unknown), None);
}

#[test]
fn batch_deregister_emits_event_per_removed_reporter() {
    let (env, _admin, client) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.set_reporter(&first, &Role::Auditor, &String::from_str(&env, "a"));
    client.set_reporter(&second, &Role::Transcriber, &String::from_str(&env, "b"));

    let mut addresses = Vec::new(&env);
    addresses.push_back(first);
    addresses.push_back(second);
    addresses.push_back(Address::generate(&env));
    client.batch_deregister(&addresses);

    assert_eq!(env.events().all().events().len(), 2);
}

#[test]
fn batch_deregister_without_admin_auth_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin.clone(),));
    let client = ReporterRegistryClient::new(&env, &id);
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));

    env.mock_auths(&[]);
    let mut addresses = Vec::new(&env);
    addresses.push_back(who.clone());
    let res = client.try_batch_deregister(&addresses);
    assert!(res.is_err());
    assert_eq!(client.role_of(&who), Some(Role::Auditor));
}

#[test]
fn unknown_reporter_has_no_role() {
    let (env, _admin, client) = setup();
    assert_eq!(client.role_of(&Address::generate(&env)), None);
}

#[test]
fn transfer_admin_requires_both() {
    let (env, admin, client) = setup();
    let next = Address::generate(&env);
    client.transfer_admin(&next);
    let auths = env.auths();
    assert!(auths.iter().any(|(a, _)| *a == admin));
    assert!(auths.iter().any(|(a, _)| *a == next));
    assert_eq!(client.admin(), next);
}

#[test]
#[should_panic]
fn set_reporter_without_auth_panics() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);
    client.set_reporter(
        &Address::generate(&env),
        &Role::Auditor,
        &String::from_str(&env, "x"),
    );
}

#[test]
fn revoke_then_re_add_resets_added_at() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "first"));
    let first_added = client.get_reporter(&who).unwrap().added_at;
    env.ledger().set_timestamp(1_800_000_000);
    client.revoke(&who);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "second"));
    let second_added = client.get_reporter(&who).unwrap().added_at;
    assert_eq!(second_added, 1_800_000_000);
    assert_ne!(second_added, first_added);
}

#[test]
fn name_exactly_64_bytes_accepted() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let name_64 = String::from_str(&env, &"x".repeat(64));
    client.set_reporter(&who, &Role::Auditor, &name_64);
    assert_eq!(client.get_reporter(&who).unwrap().name, name_64);
}

#[test]
fn name_65_bytes_rejected() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let name_65 = String::from_str(&env, &"x".repeat(65));
    let res = client.try_set_reporter(&who, &Role::Auditor, &name_65);
    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));
}

#[test]
fn revoke_without_admin_auth_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin.clone(),));
    let client = ReporterRegistryClient::new(&env, &id);
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));
    // Clear mocked auths for the next call
    env.mock_auths(&[]);
    let res = client.try_revoke(&who);
    assert!(res.is_err());
    // Reporter should still exist
    assert_eq!(client.role_of(&who), Some(Role::Auditor));
}

#[test]
fn update_metadata_stores_every_field_and_requires_reporter_auth() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(
        &who,
        &Role::Auditor,
        &String::from_str(&env, "Acme Audit LLP"),
    );

    let name = Symbol::new(&env, "acme_audit");
    let uri = String::from_str(&env, "https://acme.example/plimsoll");
    let contact = Bytes::from_slice(&env, &[7u8; 32]);
    client.update_reporter_metadata(
        &who,
        &Some(name.clone()),
        &Some(uri.clone()),
        &Some(contact.clone()),
    );

    assert_eq!(env.auths()[0].0, who);
    assert_eq!(
        client.get_reporter_metadata(&who).unwrap(),
        ReporterMetadata {
            name: Some(name),
            website_uri: Some(uri),
            contact_hash: Some(contact),
        }
    );
}

#[test]
fn update_metadata_emits_event() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "vol"));
    client.update_reporter_metadata(&who, &Some(Symbol::new(&env, "vol")), &None, &None);
    // The latest invocation published exactly one event.
    assert_eq!(env.events().all().events().len(), 1);
}

#[test]
fn metadata_is_optional_per_field() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    let uri = String::from_str(&env, "https://acme.example");
    client.update_reporter_metadata(&who, &None, &Some(uri.clone()), &None);
    let stored = client.get_reporter_metadata(&who).unwrap();
    assert_eq!(stored.name, None);
    assert_eq!(stored.website_uri, Some(uri));
    assert_eq!(stored.contact_hash, None);

    client.update_reporter_metadata(&who, &None, &None, &None);
    assert_eq!(client.get_reporter_metadata(&who), None);
}

#[test]
fn metadata_kept_when_role_changes() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "vol"));
    client.update_reporter_metadata(&who, &Some(Symbol::new(&env, "vol")), &None, &None);

    client.set_reporter(
        &who,
        &Role::Auditor,
        &String::from_str(&env, "vol, now audit"),
    );
    assert_eq!(
        client.get_reporter_metadata(&who).unwrap().name,
        Some(Symbol::new(&env, "vol"))
    );
}

#[test]
fn metadata_on_unregistered_reporter_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let res = client.try_update_reporter_metadata(&who, &None, &None, &None);
    assert_eq!(res, Err(Ok(RegistryError::ReporterNotFound.into())));
}

#[test]
fn empty_or_long_website_rejected() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    let empty = String::from_str(&env, "");
    let res = client.try_update_reporter_metadata(&who, &None, &Some(empty), &None);
    assert_eq!(res, Err(Ok(RegistryError::InvalidMetadata.into())));

    let long = String::from_str(&env, &"x".repeat(257));
    let res = client.try_update_reporter_metadata(&who, &None, &Some(long), &None);
    assert_eq!(res, Err(Ok(RegistryError::InvalidMetadata.into())));
}

#[test]
fn website_at_limit_and_contact_hash_at_limit_accepted() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    let uri = String::from_str(&env, &"x".repeat(256));
    let hash = Bytes::from_slice(&env, &[1u8; 64]);
    client.update_reporter_metadata(&who, &None, &Some(uri), &Some(hash));
    assert!(client.get_reporter_metadata(&who).is_some());
}

#[test]
fn empty_or_long_contact_hash_rejected() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    let empty = Bytes::from_slice(&env, &[]);
    let res = client.try_update_reporter_metadata(&who, &None, &None, &Some(empty));
    assert_eq!(res, Err(Ok(RegistryError::InvalidMetadata.into())));

    let long = Bytes::from_slice(&env, &[1u8; 65]);
    let res = client.try_update_reporter_metadata(&who, &None, &None, &Some(long));
    assert_eq!(res, Err(Ok(RegistryError::InvalidMetadata.into())));
}

#[test]
fn revoke_removes_metadata() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    client.update_reporter_metadata(&who, &Some(Symbol::new(&env, "acme")), &None, &None);

    client.revoke(&who);
    assert_eq!(client.get_reporter_metadata(&who), None);
}

#[test]
fn update_metadata_without_reporter_auth_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    env.mock_auths(&[]);

    let res = client.try_update_reporter_metadata(&who, &None, &None, &None);
    assert!(res.is_err());
}

#[test]
fn transfer_admin_without_new_admin_auth_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin.clone(),));
    let client = ReporterRegistryClient::new(&env, &id);
    let next = Address::generate(&env);
    let res = client.try_transfer_admin(&next);
    assert!(res.is_err());
    assert_eq!(client.admin(), admin);
}

// ---- reporter index -------------------------------------------------------

fn add(env: &Env, client: &ReporterRegistryClient, role: Role) -> Address {
    let who = Address::generate(env);
    client.set_reporter(&who, &role, &String::from_str(env, "reporter"));
// ---- is_reporter_active / set_expiry (issue #92) ---------------------------

#[test]
fn unknown_reporter_is_not_active() {
    let (env, _admin, client) = setup();
    assert!(!client.is_reporter_active(&Address::generate(&env)));
}

#[test]
fn reporter_with_no_expiry_is_active() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));
    assert!(client.is_reporter_active(&who));
}

#[test]
fn reporter_becomes_inactive_at_expiry_timestamp() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));

    let expires_at = T0 + 1_000;
    client.set_expiry(&who, &expires_at);

    // Before expiry.
    assert!(client.is_reporter_active(&who));

    // At the exact expiry second the reporter is no longer active.
    env.ledger().set_timestamp(expires_at);
    assert!(!client.is_reporter_active(&who));

    // After expiry.
    env.ledger().set_timestamp(expires_at + 1);
    assert!(!client.is_reporter_active(&who));
}

#[test]
fn expiry_one_ledger_in_the_past_makes_reporter_inactive() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "t"));

    // Set expiry to one ledger-worth of seconds (5 s) before the current timestamp.
    let past_expiry = T0 - 5;
    client.set_expiry(&who, &past_expiry);

    // The reporter's expiry is in the past: they must be inactive.
    assert!(!client.is_reporter_active(&who));
}

#[test]
fn set_expiry_on_unknown_reporter_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    let res = client.try_set_expiry(&who, &(T0 + 1_000));
    assert_eq!(res, Err(Ok(RegistryError::ReporterNotFound.into())));
}

#[test]
fn set_expiry_emits_event() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));
    let event_count_before = env.events().all().events().len();
    client.set_expiry(&who, &(T0 + 500));
    assert_eq!(env.events().all().events().len(), event_count_before + 1);
}

#[test]
fn revoked_reporter_is_not_active_even_with_future_expiry() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "a"));
    client.set_expiry(&who, &(T0 + 9_999));
    client.revoke(&who);
    assert!(!client.is_reporter_active(&who));
// ---- access tier / RBAC (#84) ---------------------------------------------

#[test]
fn access_tier_none_for_unknown_address() {
    let (env, _admin, client) = setup();
    assert_eq!(client.access_tier(&Address::generate(&env)), None);
}

#[test]
fn access_tier_reporter_for_set_reporter_address() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "acme"));
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Reporter));
}

#[test]
fn assign_role_sets_tier() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.assign_role(&who, &RegistryRole::Observer);
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Observer));
}

#[test]
fn assign_role_overrides_reporter_fallback() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "acme"));
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Reporter));
    client.assign_role(&who, &RegistryRole::Admin);
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Admin));
}

#[test]
fn revoke_role_clears_assigned_tier() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.assign_role(&who, &RegistryRole::Observer);
    client.revoke_role(&who);
    assert_eq!(client.access_tier(&who), None);
}

#[test]
fn revoke_role_falls_back_to_reporter_tier_when_registered() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "acme"));
    client.assign_role(&who, &RegistryRole::Admin);
    client.revoke_role(&who);
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Reporter));
}

#[test]
fn revoke_role_on_unset_address_is_a_noop() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.revoke_role(&who);
    assert_eq!(client.access_tier(&who), None);
}

#[test]
fn assign_role_requires_admin_auth() {
// ---- the per-asset reporter cap (#109) -----------------------------------

#[test]
fn max_reporters_defaults_to_twenty() {
    let (_env, _admin, client) = setup();
    assert_eq!(client.max_reporters(), DEFAULT_MAX_REPORTERS_PER_ASSET);
    assert_eq!(client.max_reporters(), 20);
}

#[test]
fn set_max_reporters_changes_the_cap() {
    let (env, admin, client) = setup();
    client.set_max_reporters(&3);

    assert_eq!(env.auths()[0].0, admin);
    assert_eq!(client.max_reporters(), 3);
}

#[test]
fn set_max_reporters_emits_event() {
    let (env, _admin, client) = setup();
    client.set_max_reporters(&7);
    assert_eq!(env.events().all().events().len(), 1);
}

#[test]
fn set_max_reporters_rejects_zero() {
    let (_env, _admin, client) = setup();
    let res = client.try_set_max_reporters(&0);
    assert_eq!(res, Err(Ok(RegistryError::InvalidMaxReporters.into())));
    // The refused call left the default in force.
    assert_eq!(client.max_reporters(), DEFAULT_MAX_REPORTERS_PER_ASSET);
}

#[test]
fn set_max_reporters_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);

    assert!(client.try_set_max_reporters(&5).is_err());
    assert_eq!(client.max_reporters(), DEFAULT_MAX_REPORTERS_PER_ASSET);
}

// ---- batch_register (#111) -----------------------------------------------

fn entry(env: &Env, reporter: &Address, role: Role, name: &str) -> ReporterEntry {
    ReporterEntry {
        reporter: reporter.clone(),
        role,
        name: String::from_str(env, name),
    }
}

#[test]
fn batch_register_registers_every_entry() {
    let (env, admin, client) = setup();
    let auditor = Address::generate(&env);
    let poster = Address::generate(&env);
    client.batch_register(&Vec::from_array(
        &env,
        [
            entry(&env, &auditor, Role::Auditor, "Acme Audit"),
            entry(&env, &poster, Role::SupplyPoster, "indexer"),
        ],
    ));

    assert_eq!(env.auths()[0].0, admin);
    let info = client.get_reporter(&auditor).unwrap();
    assert_eq!(info.role, Role::Auditor);
    assert_eq!(info.name, String::from_str(&env, "Acme Audit"));
    assert_eq!(info.added_at, 1_700_000_000);
    assert_eq!(client.role_of(&poster), Some(Role::SupplyPoster));
}

#[test]
fn batch_register_changes_an_existing_reporter_and_keeps_added_at() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Transcriber, &String::from_str(&env, "vol"));
    env.ledger().set_timestamp(1_800_000_000);
    client.batch_register(&Vec::from_array(
        &env,
        [entry(&env, &who, Role::Auditor, "vol, now audit")],
    ));

    let info = client.get_reporter(&who).unwrap();
    assert_eq!(info.role, Role::Auditor);
    assert_eq!(info.added_at, 1_700_000_000);
}

#[test]
fn batch_register_emits_one_event_per_entry() {
    let (env, _admin, client) = setup();
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.batch_register(&Vec::from_array(
        &env,
        [
            entry(&env, &first, Role::Auditor, "first"),
            entry(&env, &second, Role::Transcriber, "second"),
        ],
    ));

    assert_eq!(env.events().all().events().len(), 2);
}

#[test]
fn batch_register_is_atomic() {
    let (env, _admin, client) = setup();
    let good = Address::generate(&env);
    let bad = Address::generate(&env);
    let res = client.try_batch_register(&Vec::from_array(
        &env,
        [
            entry(&env, &good, Role::Auditor, "good"),
            entry(&env, &bad, Role::Auditor, ""),
        ],
    ));

    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));
    // Nothing from the batch was written, not even the valid first entry.
    assert_eq!(client.get_reporter(&good), None);
    assert_eq!(client.get_reporter(&bad), None);
}

#[test]
fn batch_register_rejects_an_empty_batch() {
    let (env, _admin, client) = setup();
    let res = client.try_batch_register(&Vec::new(&env));
    assert_eq!(res, Err(Ok(RegistryError::EmptyBatch.into())));
}

#[test]
fn batch_register_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);
    let who = Address::generate(&env);
    let res = client.try_assign_role(&who, &RegistryRole::Observer);
    assert!(res.is_err());
}

#[test]
fn assign_role_emits_event() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.assign_role(&who, &RegistryRole::Admin);
    assert_eq!(env.events().all().events().len(), 1);
}

#[test]
fn revoke_role_emits_event() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.assign_role(&who, &RegistryRole::Observer);
    client.revoke_role(&who);
    assert_eq!(env.events().all().events().len(), 1);
}

#[test]
fn access_tier_independent_of_reporter_revocation() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "acme"));
    client.assign_role(&who, &RegistryRole::Admin);
    client.revoke(&who);
    // Reporter removed but explicit tier remains.
    assert_eq!(client.access_tier(&who), Some(RegistryRole::Admin));
    assert_eq!(client.role_of(&who), None);

    let res = client.try_batch_register(&Vec::from_array(
        &env,
        [entry(&env, &who, Role::Auditor, "nobody")],
    ));
    assert!(res.is_err());
    assert_eq!(client.get_reporter(&who), None);
}

#[test]
fn a_duplicate_in_a_batch_applies_in_order() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.batch_register(&Vec::from_array(
        &env,
        [
            entry(&env, &who, Role::Auditor, "first"),
            entry(&env, &who, Role::Transcriber, "second"),
        ],
    ));

    let info = client.get_reporter(&who).unwrap();
    assert_eq!(info.role, Role::Transcriber);
    assert_eq!(info.name, String::from_str(&env, "second"));
}

fn decode_stake_events(env: &Env, contract: &Address) -> std::vec::Vec<(Address, i128, i128)> {
    let events = env.events().all().filter_by_contract(contract);
    let mut out = std::vec::Vec::new();
    for event in events.events().iter() {
        let ContractEventBody::V0(body) = &event.body;
        let topics: std::vec::Vec<Val> = body
            .topics
            .iter()
            .map(|t| Val::try_from_val(env, t).unwrap())
            .collect();
        let name = Symbol::try_from_val(env, &topics[0]).unwrap();
        if name != Symbol::new(env, "stake_changed") {
            continue;
        }
        let reporter = Address::try_from_val(env, &topics[1]).unwrap();
        let data_val = Val::try_from_val(env, &body.data).unwrap();
        let data = Map::<Symbol, Val>::try_from_val(env, &data_val).unwrap();
        let old_stake =
            i128::try_from_val(env, &data.get(Symbol::new(env, "old_stake")).unwrap()).unwrap();
        let new_stake =
            i128::try_from_val(env, &data.get(Symbol::new(env, "new_stake")).unwrap()).unwrap();
        out.push((reporter, old_stake, new_stake));
    }
    out
}

#[test]
fn reporter_can_set_and_read_display_name() {
    let (env, _admin, client) = setup();
    let reporter = Address::generate(&env);
    client.set_reporter(&reporter, &Role::Auditor, &String::from_str(&env, "audit"));
    let name = soroban_sdk::Symbol::new(&env, "Tobi");

    client.set_reporter_name(&reporter, &name);

    assert_eq!(client.get_reporter_name(&reporter), Some(name));
}

#[test]
fn reporter_display_name_is_optional_for_existing_entries() {
    let (env, _admin, client) = setup();
    let reporter = Address::generate(&env);
    client.set_reporter(&reporter, &Role::Auditor, &String::from_str(&env, "audit"));

    assert_eq!(client.get_reporter_name(&reporter), None);
    assert_eq!(client.get_reporter(&reporter).unwrap().role, Role::Auditor);
}

#[test]
fn reporter_display_name_accepts_thirty_two_characters() {
    let (env, _admin, client) = setup();
    let reporter = Address::generate(&env);
    client.set_reporter(&reporter, &Role::Auditor, &String::from_str(&env, "audit"));
    let name = soroban_sdk::Symbol::new(&env, &"x".repeat(32));

    client.set_reporter_name(&reporter, &name);

    assert_eq!(client.get_reporter_name(&reporter), Some(name));
}

#[test]
fn setting_display_name_for_unknown_reporter_fails() {
    let (env, _admin, client) = setup();
    let reporter = Address::generate(&env);
    let name = soroban_sdk::Symbol::new(&env, "Tobi");

    assert_eq!(
        client.try_set_reporter_name(&reporter, &name),
        Err(Ok(RegistryError::ReporterNotFound.into()))
    );
}

#[test]
fn setting_display_name_requires_reporter_auth() {
fn dispute_quorum_defaults_to_1_and_is_admin_settable() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);
    let reporter = Address::generate(&env);
    client.set_reporter(&reporter, &Role::Auditor, &String::from_str(&env, "audit"));
    env.mock_auths(&[]);
    let name = soroban_sdk::Symbol::new(&env, "Tobi");

    let res = client.try_set_reporter_name(&reporter, &name);

    assert!(res.is_err());
    assert_eq!(client.get_reporter_name(&reporter), None);

    assert_eq!(client.dispute_quorum(), 1);
    client.set_dispute_quorum(&3);
    assert_eq!(client.dispute_quorum(), 3);
}

#[test]
fn set_dispute_quorum_rejects_zero_and_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);

    let res = client.try_set_dispute_quorum(&0);
    assert_eq!(res, Err(Ok(RegistryError::InvalidQuorum.into())));

    env.mock_auths(&[]);
    assert!(client.try_set_dispute_quorum(&2).is_err());
}

#[test]
fn dispute_report_auto_rejects_at_default_quorum_1_and_fires_event() {
    use soroban_sdk::{testutils::Events, xdr::ContractEventBody, Symbol, TryFromVal};

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);

    let watcher = Address::generate(&env);
    let report_id = BytesN::from_array(&env, &[1u8; 32]);
    let status = client.dispute_report(&watcher, &report_id);
    assert_eq!(status, DisputeStatus::Rejected);

    let rejected_events = env
        .events()
        .all()
        .events()
        .iter()
        .filter(|e| {
            let ContractEventBody::V0(body) = &e.body;
            body.topics
                .first()
                .and_then(|t| Symbol::try_from_val(&env, t).ok())
                .is_some_and(|s| s == Symbol::new(&env, "report_rejected"))
        })
        .count();
    assert_eq!(rejected_events, 1);

    let record = client.get_dispute(&report_id).unwrap();
    assert_eq!(record.status, DisputeStatus::Rejected);
    assert_eq!(record.watchers.len(), 1);
}

#[test]
fn dispute_report_holds_until_quorum_met_by_distinct_watchers() {
    use soroban_sdk::{testutils::Events, xdr::ContractEventBody, Symbol, TryFromVal};

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);

    client.set_dispute_quorum(&3);
    let report_id = BytesN::from_array(&env, &[2u8; 32]);
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);

    assert_eq!(client.dispute_report(&w1, &report_id), DisputeStatus::Held);
    assert_eq!(client.dispute_report(&w2, &report_id), DisputeStatus::Held);
    // Not rejected yet.
    let rejected_before = env
        .events()
        .all()
        .events()
        .iter()
        .filter(|e| {
            let ContractEventBody::V0(body) = &e.body;
            body.topics
                .first()
                .and_then(|t| Symbol::try_from_val(&env, t).ok())
                .is_some_and(|s| s == Symbol::new(&env, "report_rejected"))
        })
        .count();
    assert_eq!(rejected_before, 0);
    assert_eq!(
        client.get_dispute(&report_id).unwrap().status,
        DisputeStatus::Held
    );

    // Third distinct watcher meets quorum -> auto-rejected + event.
    assert_eq!(
        client.dispute_report(&w3, &report_id),
        DisputeStatus::Rejected
    );
    let rejected_after = env
        .events()
        .all()
        .events()
        .iter()
        .filter(|e| {
            let ContractEventBody::V0(body) = &e.body;
            body.topics
                .first()
                .and_then(|t| Symbol::try_from_val(&env, t).ok())
                .is_some_and(|s| s == Symbol::new(&env, "report_rejected"))
        })
        .count();
    assert_eq!(rejected_after, 1);
}

#[test]
fn duplicate_dispute_and_post_rejection_and_expired_holding_period_fail() {
    use soroban_sdk::testutils::Ledger;

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ReporterRegistry, (admin,));
    let client = ReporterRegistryClient::new(&env, &id);

    client.set_dispute_quorum(&2);
    let report_id = BytesN::from_array(&env, &[3u8; 32]);
    let w1 = Address::generate(&env);
    let w2 = Address::generate(&env);
    let w3 = Address::generate(&env);

    assert_eq!(client.dispute_report(&w1, &report_id), DisputeStatus::Held);
    // Same watcher cannot dispute twice.
    assert_eq!(
        client.try_dispute_report(&w1, &report_id),
        Err(Ok(RegistryError::DuplicateDispute.into()))
    );

    // Second distinct watcher triggers rejection.
    assert_eq!(
        client.dispute_report(&w2, &report_id),
        DisputeStatus::Rejected
    );
    // Further disputes after rejection fail.
    assert_eq!(
        client.try_dispute_report(&w3, &report_id),
        Err(Ok(RegistryError::DisputeAlreadyRejected.into()))
    );

    // Expired holding period fails for a new report.
    let report_id_2 = BytesN::from_array(&env, &[4u8; 32]);
    assert_eq!(
        client.dispute_report(&w1, &report_id_2),
        DisputeStatus::Held
    );
    env.ledger()
        .with_mut(|li| li.timestamp += DISPUTE_HOLDING_PERIOD_SECS + 1);
    assert_eq!(
        client.try_dispute_report(&w2, &report_id_2),
        Err(Ok(RegistryError::HoldingPeriodElapsed.into()))
    );
}
fn register_and_confirm_reporter_activates_within_window() {
    let (env, admin, client) = setup();
    env.ledger().set_sequence_number(100);
    let who = Address::generate(&env);
    let name = String::from_str(&env, "TwoFactorAudit");

    client.register_reporter(&who, &Role::Auditor, &name, &50);
    assert_eq!(env.auths()[0].0, admin);
    assert_eq!(env.events().all().events().len(), 1);
    // Pending entry is not active yet.
    assert_eq!(client.role_of(&who), None);
    assert_eq!(client.get_reporter(&who), None);
    let pending = client.get_pending_reporter(&who).unwrap();
    assert_eq!(pending.role, Role::Auditor);
    assert_eq!(pending.name, name);
    assert_eq!(pending.expires_at_ledger, 150);

    // Confirm on the exact boundary ledger (150).
    env.ledger().set_sequence_number(150);
    client.confirm_registration(&who);
    assert_eq!(env.auths()[0].0, who);
    assert_eq!(env.events().all().events().len(), 1);
    assert_eq!(client.get_pending_reporter(&who), None);
    assert_eq!(client.role_of(&who), Some(Role::Auditor));
    let info = client.get_reporter(&who).unwrap();
    assert_eq!(info.name, name);
}

#[test]
fn register_reporter_rejects_invalid_name_or_zero_window() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);

    let res = client.try_register_reporter(&who, &Role::Auditor, &String::from_str(&env, ""), &10);
    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));

    let res = client.try_register_reporter(
        &who,
        &Role::Auditor,
        &String::from_str(&env, "ValidName"),
        &0,
    );
    assert_eq!(res, Err(Ok(RegistryError::InvalidConfirmWindow.into())));
}

#[test]
fn confirm_registration_fails_when_missing_or_expired() {
    let (env, _admin, client) = setup();
    env.ledger().set_sequence_number(100);
    let who = Address::generate(&env);

    let res = client.try_confirm_registration(&who);
    assert_eq!(
        res,
        Err(Ok(RegistryError::PendingRegistrationNotFound.into()))
    );

    client.register_reporter(
        &who,
        &Role::Transcriber,
        &String::from_str(&env, "Vol"),
        &10,
    );
    env.ledger().set_sequence_number(111);
    let res = client.try_confirm_registration(&who);
    assert_eq!(res, Err(Ok(RegistryError::RegistrationExpired.into())));
    assert_eq!(client.role_of(&who), None);
    assert_eq!(client.get_pending_reporter(&who), None);
}

#[test]
fn cleanup_expired_registration_removes_expired_entry_and_emits_event() {
    let (env, _admin, client) = setup();
    env.ledger().set_sequence_number(100);
    let who = Address::generate(&env);

    assert!(!client.cleanup_expired_registration(&who));

    client.register_reporter(&who, &Role::Auditor, &String::from_str(&env, "Exp"), &10);
    // Not expired yet at ledger 110.
    env.ledger().set_sequence_number(110);
    assert!(!client.cleanup_expired_registration(&who));
    assert!(client.get_pending_reporter(&who).is_some());

    // Expired at ledger 111.
    env.ledger().set_sequence_number(111);
    assert!(client.cleanup_expired_registration(&who));
    assert_eq!(env.events().all().events().len(), 1);
    // Second cleanup is a no-op.
    assert!(!client.cleanup_expired_registration(&who));
fn execute_upgrade_without_proposal_fails() {
    let (_env, _admin, client) = setup();
    let res = client.try_execute_upgrade();
    assert_eq!(res, Err(Ok(RegistryError::UpgradeNotProposed.into())));
}

#[test]
fn cancel_upgrade_without_proposal_fails() {
    let (_env, _admin, client) = setup();
    let res = client.try_cancel_upgrade();
    assert_eq!(res, Err(Ok(RegistryError::UpgradeNotProposed.into())));
}

#[test]
fn top_up_stake_accumulates_and_is_readable() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    assert_eq!(client.stake_of(&who), 0);
    assert_eq!(client.top_up_stake(&who, &1_000), 1_000);
    assert_eq!(client.top_up_stake(&who, &500), 1_500);
    assert_eq!(client.stake_of(&who), 1_500);
}

#[test]
fn top_up_stake_with_non_positive_amount_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));

    let res = client.try_top_up_stake(&who, &0);
    assert_eq!(res, Err(Ok(RegistryError::InvalidStakeAmount.into())));
    assert_eq!(client.stake_of(&who), 0);
}

#[test]
fn top_up_stake_past_i128_max_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    client.top_up_stake(&who, &i128::MAX);

    let res = client.try_top_up_stake(&who, &1);
    assert_eq!(res, Err(Ok(RegistryError::InvalidStakeAmount.into())));
    assert_eq!(client.stake_of(&who), i128::MAX);
}

#[test]
fn reduce_stake_lowers_and_clears_the_balance() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    client.top_up_stake(&who, &1_000);

    assert_eq!(client.reduce_stake(&who, &400), 600);
    assert_eq!(client.stake_of(&who), 600);
    assert_eq!(client.reduce_stake(&who, &600), 0);
    assert_eq!(client.stake_of(&who), 0);
}

#[test]
fn reduce_stake_with_non_positive_amount_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    client.top_up_stake(&who, &500);

    let res = client.try_reduce_stake(&who, &0);
    assert_eq!(res, Err(Ok(RegistryError::InvalidStakeAmount.into())));
    assert_eq!(client.stake_of(&who), 500);
}

#[test]
fn reduce_stake_above_the_tracked_balance_fails() {
    let (env, _admin, client) = setup();
    let who = Address::generate(&env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme"));
    client.top_up_stake(&who, &250);

    let res = client.try_reduce_stake(&who, &251);
    assert_eq!(res, Err(Ok(RegistryError::InsufficientStake.into())));
    assert_eq!(client.stake_of(&who), 250);
}

#[test]
fn execute_upgrade_before_delay_fails() {
    let (env, _admin, client) = setup();
    let hash = BytesN::from_array(&env, &[7; 32]);
    client.propose_upgrade(&hash);

    // A proposal is executable only once UPGRADE_DELAY_LEDGERS have closed.
    let res = client.try_execute_upgrade();
    assert_eq!(res, Err(Ok(RegistryError::UpgradeDelayNotElapsed.into())));
}

fn registered(env: &Env, client: &ReporterRegistryClient<'static>) -> Address {
    let who = Address::generate(env);
    client.set_reporter(&who, &Role::Auditor, &String::from_str(env, "Acme Audit"));
    who
}

#[test]
fn index_is_empty_initially() {
    let (_env, _admin, client) = setup();
    assert_eq!(client.reporter_count(), 0);
    assert_eq!(client.reporters(&0, &10).len(), 0);
}

#[test]
fn reporters_lists_in_first_indexed_order() {
    let (env, _admin, client) = setup();
    let a = add(&env, &client, Role::Auditor);
    let b = add(&env, &client, Role::Transcriber);
    let c = add(&env, &client, Role::SupplyPoster);
    assert_eq!(client.reporter_count(), 3);
    assert_eq!(client.reporters(&0, &10), Vec::from_array(&env, [a, b, c]));
}

#[test]
fn reporters_pages_through_the_index() {
    let (env, _admin, client) = setup();
    let all: std::vec::Vec<Address> = (0..5).map(|_| add(&env, &client, Role::Auditor)).collect();
    assert_eq!(
        client.reporters(&0, &2),
        Vec::from_array(&env, [all[0].clone(), all[1].clone()])
    );
    assert_eq!(
        client.reporters(&2, &2),
        Vec::from_array(&env, [all[2].clone(), all[3].clone()])
    );
    assert_eq!(
        client.reporters(&4, &2),
        Vec::from_array(&env, [all[4].clone()])
    );
    assert_eq!(client.reporters(&5, &2).len(), 0);
    assert_eq!(client.reporters(&u32::MAX, &u32::MAX).len(), 0);
    assert_eq!(client.reporters(&0, &0).len(), 0);
}

#[test]
fn reporters_caps_a_page_at_100() {
    let (env, _admin, client) = setup();
    env.cost_estimate().budget().reset_unlimited();
    for _ in 0..101 {
        add(&env, &client, Role::Auditor);
    }
    assert_eq!(client.reporter_count(), 101);
    assert_eq!(client.reporters(&0, &u32::MAX).len(), 100);
    assert_eq!(client.reporters(&100, &u32::MAX).len(), 1);
}

#[test]
fn changing_role_keeps_the_index_position() {
    let (env, _admin, client) = setup();
    let a = add(&env, &client, Role::Transcriber);
    let b = add(&env, &client, Role::Auditor);
    client.set_reporter(&a, &Role::Auditor, &String::from_str(&env, "promoted"));
    assert_eq!(client.reporter_count(), 2);
    assert_eq!(client.reporters(&0, &10), Vec::from_array(&env, [a, b]));
}

#[test]
fn revoke_removes_from_the_index_and_keeps_order() {
    let (env, _admin, client) = setup();
    let a = add(&env, &client, Role::Auditor);
    let b = add(&env, &client, Role::Transcriber);
    let c = add(&env, &client, Role::SupplyPoster);
    client.revoke(&a);
    assert_eq!(client.reporter_count(), 2);
    assert_eq!(client.reporters(&0, &10), Vec::from_array(&env, [b, c]));
}

#[test]
fn re_adding_after_revoke_appends_to_the_end() {
    let (env, _admin, client) = setup();
    let a = add(&env, &client, Role::Auditor);
    let b = add(&env, &client, Role::Auditor);
    client.revoke(&a);
    client.set_reporter(&a, &Role::Transcriber, &String::from_str(&env, "back"));
    assert_eq!(client.reporters(&0, &10), Vec::from_array(&env, [b, a]));
}

#[test]
fn revoke_unknown_leaves_the_index_alone() {
    let (env, _admin, client) = setup();
    let a = add(&env, &client, Role::Auditor);
    let res = client.try_revoke(&Address::generate(&env));
    assert_eq!(res, Err(Ok(RegistryError::ReporterNotFound.into())));
    assert_eq!(client.reporters(&0, &10), Vec::from_array(&env, [a]));
}

#[test]
fn reporters_from_before_the_index_join_it_when_re_set() {
    let (env, _admin, client) = setup();
    let old = add(&env, &client, Role::Auditor);
    // A deployment upgraded from a version without the index has the
    // reporter's entry but no index.
    env.as_contract(&client.address, || {
        env.storage().persistent().remove(&DataKey::ReporterIndex);
    });
    assert_eq!(client.reporter_count(), 0);
    let new = add(&env, &client, Role::Transcriber);
    assert_eq!(
        client.reporters(&0, &10),
        Vec::from_array(&env, [new.clone()])
    );

    client.set_reporter(&old, &Role::Auditor, &String::from_str(&env, "reporter"));
    assert_eq!(
        client.reporters(&0, &10),
        Vec::from_array(&env, [new, old.clone()])
    );
    assert_eq!(client.role_of(&old), Some(Role::Auditor));
}

// ---- version --------------------------------------------------------------

#[test]
fn version_is_the_constant() {
    let (_env, _admin, client) = setup();
    assert_eq!(client.version(), VERSION);
    assert_eq!(client.version(), 1);
fn set_reporter_name_stores_the_name_under_the_reporters_authority() {
    let (env, _admin, client) = setup();
    let who = registered(&env, &client);
    assert_eq!(client.get_reporter_name(&who), None);

    client.set_reporter_name(&who, &Symbol::new(&env, "AcmeAudit"));
    assert_eq!(env.auths()[0].0, who);
    assert_eq!(
        client.get_reporter_name(&who),
        Some(Symbol::new(&env, "AcmeAudit"))
    );

    // A later call replaces the name.
    client.set_reporter_name(&who, &Symbol::new(&env, "AcmeAuditLLP"));
    assert_eq!(
        client.get_reporter_name(&who),
        Some(Symbol::new(&env, "AcmeAuditLLP"))
    );
}

#[test]
fn set_reporter_name_accepts_the_32_character_maximum() {
    let (env, _admin, client) = setup();
    let who = registered(&env, &client);
    let longest = "abcdefghijklmnopqrstuvwxyz_01234";
    assert_eq!(longest.len(), 32);
    client.set_reporter_name(&who, &Symbol::new(&env, longest));
    assert_eq!(
        client.get_reporter_name(&who),
        Some(Symbol::new(&env, longest))
    );
}

/// The 32-character cap is the `Symbol` type's own: a 33-character name
/// cannot be built, so it can never reach `set_reporter_name`.
#[test]
#[should_panic]
fn a_display_name_over_32_characters_cannot_be_constructed() {
    let env = Env::default();
    let _ = Symbol::new(&env, "abcdefghijklmnopqrstuvwxyz_012345");
}

#[test]
fn set_reporter_name_rejects_an_unregistered_reporter() {
    let (env, _admin, client) = setup();
    let unknown = Address::generate(&env);
    assert_eq!(
        client.try_set_reporter_name(&unknown, &Symbol::new(&env, "nobody")),
        Err(Ok(RegistryError::ReporterNotFound.into()))
    );
    assert_eq!(client.get_reporter_name(&unknown), None);
}

#[test]
fn reporter_name_leaves_the_existing_entry_unchanged_and_is_removed_on_revoke() {
    let (env, _admin, client) = setup();
    let who = registered(&env, &client);
    let before = client.get_reporter(&who).unwrap();

    client.set_reporter_name(&who, &Symbol::new(&env, "AcmeAudit"));
    assert_eq!(client.get_reporter(&who).unwrap(), before);
    assert_eq!(client.get_reporter_metadata(&who), None);

    client.revoke(&who);
    assert_eq!(client.get_reporter_name(&who), None);

    // Re-adding the reporter does not bring the old name back.
    client.set_reporter(&who, &Role::Auditor, &String::from_str(&env, "Acme Audit"));
    assert_eq!(client.get_reporter_name(&who), None);
}
}
