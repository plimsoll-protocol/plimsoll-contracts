extern crate std;

use super::*;
#[allow(unused_imports)]
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    Address, Bytes, BytesN, Env, IntoVal, String, Symbol,
};

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
    env.ledger().set_timestamp(1_700_000_000);
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
}
