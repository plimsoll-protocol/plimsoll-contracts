//! The deployment, end to end, against the wasm `stellar contract build`
//! produces: the same three files `scripts/deploy.sh` uploads to testnet.
//!
//! The per-contract tests register their neighbours natively, as Rust. This
//! one loads every contract from its compiled wasm with `contractimport!`, so
//! it exercises what actually ships: the exported interface, the XDR at every
//! cross-contract boundary, and the error codes as a caller decodes them.
//!
//! Build first, then run:
//!
//! ```bash
//! stellar contract build
//! cargo test -p plimsoll-e2e
//! ```

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, Bytes, BytesN, Env, Error, Executable, String,
};

mod registry {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/reporter_registry.wasm"
    );
}

mod ledger {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/coverage_ledger.wasm");
}

mod vault {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/covered_vault.wasm");
}

const T0: u64 = 1_700_000_000;
const SEQ0: u32 = 1_000;
const DAY: u64 = 86_400;
/// One unit of a classic asset: 7 decimals.
const UNIT: i128 = 10_000_000;
/// The vault's policy: fully backed, figures at most a week old, issuer-signed
/// or stronger. The same policy the PUSD demo vault is deployed with.
const MIN_BPS: u32 = 10_000;
const MAX_AGE: u64 = 7 * DAY;

// Error codes, as a front end sees them in `Error(Contract, #n)`. Written out
// as numbers on purpose: if a variant is ever renumbered, this test fails
// before the indexer and every front end silently misread it.
const LEDGER_ALREADY_LISTED: u32 = 2;
const LEDGER_ASSET_NOT_LISTED: u32 = 3;
const LEDGER_NOT_SUPPLY_POSTER: u32 = 4;
const LEDGER_NOT_REPORTER: u32 = 5;
const LEDGER_IMPLAUSIBLE_AMOUNT: u32 = 12;
const VAULT_NOT_COVERED: u32 = 2;
const VAULT_INSUFFICIENT_BALANCE: u32 = 3;
/// The Stellar Asset Contract's own `BalanceError`: the token, not the vault,
/// refuses a transfer larger than the sender holds.
const SAC_BALANCE_ERROR: u32 = 10;

fn contract_error(code: u32) -> Error {
    Error::from_contract_error(code)
}

fn advance(env: &Env, seconds: u64) {
    env.ledger().with_mut(|li| {
        li.timestamp += seconds;
        // 5-second close time, as on the network.
        li.sequence_number += (seconds / 5) as u32;
    });
}

/// The contract at `id` runs the exact bytes in `wasm`, not a native stand-in.
fn assert_runs_wasm(env: &Env, id: &Address, wasm: &[u8]) {
    let hash: BytesN<32> = env.crypto().sha256(&Bytes::from_slice(env, wasm)).into();
    assert_eq!(id.executable(), Some(Executable::Wasm(hash)));
}

#[test]
fn deposit_gated_on_fresh_coverage_and_withdraw_never_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    env.ledger().set_sequence_number(SEQ0);

    // ---- deploy, in dependency order, from the built wasm -----------------
    let admin = Address::generate(&env);
    let registry_id = env.register(registry::WASM, (admin.clone(),));
    let registry = registry::Client::new(&env, &registry_id);
    let ledger_id = env.register(ledger::WASM, (admin.clone(), registry_id.clone()));
    let ledger = ledger::Client::new(&env, &ledger_id);

    let sac_admin = Address::generate(&env);
    let handle = env.register_stellar_asset_contract_v2(sac_admin);
    let sac = handle.address();
    let issuer = handle.issuer().address();
    let token = TokenClient::new(&env, &sac);

    let vault_id = env.register(
        vault::WASM,
        (
            admin.clone(),
            ledger_id.clone(),
            sac.clone(),
            MIN_BPS,
            MAX_AGE,
            vault::Tier::IssuerSigned,
        ),
    );
    let vault = vault::Client::new(&env, &vault_id);

    assert_runs_wasm(&env, &registry_id, registry::WASM);
    assert_runs_wasm(&env, &ledger_id, ledger::WASM);
    assert_runs_wasm(&env, &vault_id, vault::WASM);
    assert_eq!(ledger.registry(), registry_id);
    assert_eq!(vault.config().ledger, ledger_id);

    // ---- 1. register reporters --------------------------------------------
    let poster = Address::generate(&env);
    let auditor = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &registry::Role::SupplyPoster,
        &String::from_str(&env, "indexer"),
    );
    registry.set_reporter(
        &auditor,
        &registry::Role::Auditor,
        &String::from_str(&env, "Acme Audit"),
    );
    assert_eq!(
        registry.role_of(&poster),
        Some(registry::Role::SupplyPoster)
    );
    assert_eq!(registry.role_of(&auditor), Some(registry::Role::Auditor));

    // ---- 2. list the asset ------------------------------------------------
    // Nothing can be posted for an asset that is not listed.
    assert_eq!(
        ledger.try_post_supply(&poster, &sac, &(1_000 * UNIT), &SEQ0, &hash(&env, 1)),
        Err(Ok(contract_error(LEDGER_ASSET_NOT_LISTED)))
    );
    let record = ledger.list_asset(&sac);
    assert_eq!(record.sac, sac);
    assert_eq!(record.issuer, issuer);
    assert_eq!(record.listed_at, T0);
    assert_eq!(
        ledger.try_list_asset(&sac),
        Err(Ok(contract_error(LEDGER_ALREADY_LISTED)))
    );
    // Before any figures exist the vault is closed.
    assert!(!vault.can_deposit());

    // ---- 3. post supply and reserves --------------------------------------
    // Only a registered supply poster may post supply; the ledger asks the
    // registry across the wasm boundary.
    assert_eq!(
        ledger.try_post_supply(&auditor, &sac, &(1_000 * UNIT), &SEQ0, &hash(&env, 1)),
        Err(Ok(contract_error(LEDGER_NOT_SUPPLY_POSTER)))
    );
    ledger.post_supply(&poster, &sac, &(1_000 * UNIT), &SEQ0, &hash(&env, 1));
    let supply = ledger.get_supply(&sac).expect("supply was just posted");
    assert_eq!(supply.amount, 1_000 * UNIT);
    assert_eq!(supply.ledger, SEQ0);
    assert_eq!(supply.timestamp, T0);

    // An address with no role and no claim to be the issuer cannot report.
    let stranger = Address::generate(&env);
    assert_eq!(
        ledger.try_post_reserve(
            &stranger,
            &sac,
            &(1_020 * UNIT),
            &T0,
            &hash(&env, 2),
            &doc_uri(&env),
        ),
        Err(Ok(contract_error(LEDGER_NOT_REPORTER)))
    );
    // A figure typed in whole units is 10^7 times too small: refused.
    assert_eq!(
        ledger.try_post_reserve(&auditor, &sac, &1_020, &T0, &hash(&env, 2), &doc_uri(&env)),
        Err(Ok(contract_error(LEDGER_IMPLAUSIBLE_AMOUNT)))
    );
    // The issuer's own account signs: the tier comes from who signed.
    let tier = ledger.post_reserve(
        &issuer,
        &sac,
        &(1_020 * UNIT),
        &T0,
        &hash(&env, 2),
        &doc_uri(&env),
    );
    assert_eq!(tier, ledger::Tier::IssuerSigned);

    let coverage = ledger
        .coverage(&sac, &ledger::Tier::IssuerSigned)
        .expect("supply and an issuer-signed report exist");
    assert_eq!(coverage.bps, 10_200);
    assert_eq!(coverage.supply, 1_000 * UNIT);
    assert_eq!(coverage.reserves, 1_020 * UNIT);
    assert_eq!(coverage.tier, ledger::Tier::IssuerSigned);
    assert_eq!(coverage.report_as_of, T0);
    assert!(ledger.is_covered(&sac, &MIN_BPS, &MAX_AGE, &ledger::Tier::IssuerSigned));
    // The vault's policy demands no more than the issuer tier, so an auditor
    // requirement is a different question with a different answer.
    assert!(!ledger.is_covered(&sac, &MIN_BPS, &MAX_AGE, &ledger::Tier::AuditorSigned));

    // ---- 4. deposit -------------------------------------------------------
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &sac).mint(&user, &(500 * UNIT));
    assert!(vault.can_deposit());
    assert_eq!(vault.deposit(&user, &(100 * UNIT)), 100 * UNIT);
    assert_eq!(vault.balance(&user), 100 * UNIT);
    assert_eq!(token.balance(&vault_id), 100 * UNIT);
    assert_eq!(token.balance(&user), 400 * UNIT);
    // More than the wallet holds: the asset contract refuses the transfer
    // and its error, not the vault's, is what the caller sees.
    assert_eq!(
        vault.try_deposit(&user, &(401 * UNIT)),
        Err(Ok(contract_error(SAC_BALANCE_ERROR)))
    );
    assert_eq!(vault.balance(&user), 100 * UNIT);

    // ---- 5. let the report go stale ---------------------------------------
    // A week and a second later. The poster keeps supply fresh, so the reserve
    // report is the only thing out of date.
    advance(&env, MAX_AGE + 1);
    let now = env.ledger().timestamp();
    let seq = env.ledger().sequence();
    ledger.post_supply(&poster, &sac, &(1_000 * UNIT), &seq, &hash(&env, 3));
    let coverage = ledger
        .coverage(&sac, &ledger::Tier::IssuerSigned)
        .expect("the stale report is still stored");
    // The ratio has not changed; only its age has.
    assert_eq!(coverage.bps, 10_200);
    assert_eq!(coverage.supply_timestamp, now);
    assert_eq!(now - coverage.report_as_of, MAX_AGE + 1);
    assert!(!ledger.is_covered(&sac, &MIN_BPS, &MAX_AGE, &ledger::Tier::IssuerSigned));
    assert!(!vault.can_deposit());

    // ---- 6. deposit refused -----------------------------------------------
    assert_eq!(
        vault.try_deposit(&user, &(50 * UNIT)),
        Err(Ok(contract_error(VAULT_NOT_COVERED)))
    );
    assert_eq!(vault.balance(&user), 100 * UNIT);
    assert_eq!(token.balance(&vault_id), 100 * UNIT);
    assert_eq!(token.balance(&user), 400 * UNIT);

    // ---- 7. withdrawals remain available with the zero-delay default -----
    vault.request_withdraw(&user, &(40 * UNIT));
    assert_eq!(vault.execute_withdraw(&user), 60 * UNIT);
    assert_eq!(vault.balance(&user), 60 * UNIT);
    assert_eq!(token.balance(&vault_id), 60 * UNIT);
    assert_eq!(token.balance(&user), 440 * UNIT);
    assert_eq!(
        vault.try_request_withdraw(&user, &(61 * UNIT)),
        Err(Ok(contract_error(VAULT_INSUFFICIENT_BALANCE)))
    );
    vault.request_withdraw(&user, &(60 * UNIT));
    assert_eq!(vault.execute_withdraw(&user), 0);
    assert_eq!(token.balance(&vault_id), 0);
    assert_eq!(token.balance(&user), 500 * UNIT);

    // ---- 8. a fresh report reopens the vault ------------------------------
    ledger.post_reserve(
        &issuer,
        &sac,
        &(1_010 * UNIT),
        &now,
        &hash(&env, 4),
        &doc_uri(&env),
    );
    assert!(vault.can_deposit());
    assert_eq!(vault.deposit(&user, &(10 * UNIT)), 10 * UNIT);
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn doc_uri(env: &Env) -> String {
    String::from_str(env, "https://issuer.example/reserves-2023-11.pdf")
}
