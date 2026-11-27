#![no_main]

use coverage_ledger::{CoverageLedger, CoverageLedgerClient, LedgerError};
use libfuzzer_sys::fuzz_target;
use plimsoll_types::{Role, Tier};
use reporter_registry::{ReporterRegistry, ReporterRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, String,
};

const T0: u64 = 1_700_000_000;
const UNIT: i128 = 10_000_000;

fn bytes<const N: usize>(input: &[u8], offset: usize) -> [u8; N] {
    let mut value = [0; N];
    for (index, byte) in value.iter_mut().enumerate() {
        if let Some(input_byte) = input.get(offset + index) {
            *byte = *input_byte;
        }
    }
    value
}

fuzz_target!(|input: &[u8]| {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(T0);
    env.ledger().set_sequence_number(1_000);

    let admin = Address::generate(&env);
    let registry_id = env.register(ReporterRegistry, (admin.clone(),));
    let registry = ReporterRegistryClient::new(&env, &registry_id);
    let ledger_id = env.register(CoverageLedger, (admin, registry_id.clone()));
    let ledger = CoverageLedgerClient::new(&env, &ledger_id);

    let sac_handle = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let sac = sac_handle.address();
    let issuer = sac_handle.issuer().address();
    ledger.list_asset(&sac);

    let poster = Address::generate(&env);
    registry.set_reporter(
        &poster,
        &Role::SupplyPoster,
        &String::from_str(&env, "fuzz"),
    );
    let supply = (u32::from_le_bytes(bytes::<4>(input, 9)) as i128 + 1) * UNIT;
    ledger.post_supply(
        &poster,
        &sac,
        &supply,
        &1_000,
        &BytesN::from_array(&env, &[1; 32]),
    );
    ledger.post_reserve(
        &issuer,
        &sac,
        &supply,
        &T0,
        &BytesN::from_array(&env, &[2; 32]),
        &String::from_str(&env, "https://example.org/report"),
    );

    let min_bps = u32::from_le_bytes(bytes::<4>(input, 0));
    let max_age = u64::from_le_bytes(bytes::<8>(input, 4));
    let min_tier = match input.first().copied().unwrap_or_default() % 3 {
        0 => Tier::Transcribed,
        1 => Tier::IssuerSigned,
        _ => Tier::AuditorSigned,
    };
    if input.get(13).copied().unwrap_or_default() & 1 == 1 {
        ledger.pause();
    }
    let queried_sac = if input.get(14).copied().unwrap_or_default() & 1 == 1 {
        sac
    } else {
        Address::generate(&env)
    };

    match ledger.try_is_covered(&queried_sac, &min_bps, &max_age, &min_tier) {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => panic!("unexpected conversion failure: {error:?}"),
        Err(Ok(error)) if error == LedgerError::Paused.into() => {}
        Err(Ok(error)) => panic!("unexpected ledger error: {error:?}"),
        Err(Err(error)) => panic!("unexpected invocation failure: {error:?}"),
    }
});
