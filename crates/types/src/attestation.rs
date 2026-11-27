//! Compact coverage attestation encoding and off-chain/on-chain verification.
//!
//! Proof format (v1, all multi-byte integers big-endian):
//! - `0..4`:   magic `b"PLMS"`
//! - `4..5`:   version `0x01`
//! - `5..6`:   covered (`0x01` or `0x00`)
//! - `6..10`:  `min_bps` (`u32`)
//! - `10..18`: `max_age` (`u64`)
//! - `18..22`: `min_tier` (`u32`: `1` = Transcribed, `2` = IssuerSigned, `3` = AuditorSigned)
//! - `22..26`: `ledger_sequence` (`u32`)
//! - `26..34`: `ledger_timestamp` (`u64`)
//! - `34..36`: `asset_xdr_len` (`u16`, `N`)
//! - `36..36+N`: `asset_xdr` (`asset.to_xdr()`)
//! - `36+N..38+N`: `ledger_xdr_len` (`u16`, `M`)
//! - `38+N..38+N+M`: `ledger_xdr` (`ledger_contract.to_xdr()`)
//! - `38+N+M..70+N+M`: `sha256(DOMAIN || body)` (`32` bytes)

use crate::Tier;
use soroban_sdk::{
    xdr::{FromXdr, ToXdr},
    Address, Bytes, BytesN, Env,
};

pub const PROOF_MAGIC: [u8; 4] = *b"PLMS";
pub const PROOF_VERSION: u8 = 1;
pub const DOMAIN_SEPARATOR: &[u8] = b"plimsoll-coverage-attestation-v1";
const HEADER_LEN: usize = 34;
const DIGEST_LEN: usize = 32;
const MIN_PROOF_LEN: usize = HEADER_LEN + 2 + 2 + DIGEST_LEN;

/// Decoded and verified coverage attestation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageAttestation {
    pub asset: Address,
    pub ledger_contract: Address,
    pub min_bps: u32,
    pub max_age: u64,
    pub min_tier: Tier,
    pub covered: bool,
    pub ledger_sequence: u32,
    pub ledger_timestamp: u64,
    pub digest: BytesN<32>,
}

/// Reason a coverage attestation proof failed verification.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AttestationError {
    TooShort,
    InvalidMagic,
    UnsupportedVersion,
    InvalidCoveredFlag,
    InvalidTier,
    MalformedLength,
    InvalidAddressXdr,
    DigestMismatch,
}

fn tier_to_u32(tier: Tier) -> u32 {
    match tier {
        Tier::Transcribed => 1,
        Tier::IssuerSigned => 2,
        Tier::AuditorSigned => 3,
    }
}

fn u32_to_tier(val: u32) -> Result<Tier, AttestationError> {
    match val {
        1 => Ok(Tier::Transcribed),
        2 => Ok(Tier::IssuerSigned),
        3 => Ok(Tier::AuditorSigned),
        _ => Err(AttestationError::InvalidTier),
    }
}

fn compute_digest(env: &Env, body: &Bytes) -> BytesN<32> {
    let mut preimage = Bytes::from_slice(env, DOMAIN_SEPARATOR);
    preimage.append(body);
    env.crypto().sha256(&preimage).into()
}

fn append_u16_prefixed(out: &mut Bytes, data: &Bytes) {
    let len = data.len() as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.append(data);
}

fn read_u16(proof: &Bytes, offset: u32) -> Result<u16, AttestationError> {
    if offset + 2 > proof.len() {
        return Err(AttestationError::MalformedLength);
    }
    let b0 = proof.get(offset).ok_or(AttestationError::MalformedLength)?;
    let b1 = proof
        .get(offset + 1)
        .ok_or(AttestationError::MalformedLength)?;
    Ok(u16::from_be_bytes([b0, b1]))
}

fn read_u32(proof: &Bytes, offset: u32) -> u32 {
    let mut buf = [0u8; 4];
    proof.slice(offset..offset + 4).copy_into_slice(&mut buf);
    u32::from_be_bytes(buf)
}

fn read_u64(proof: &Bytes, offset: u32) -> u64 {
    let mut buf = [0u8; 8];
    proof.slice(offset..offset + 8).copy_into_slice(&mut buf);
    u64::from_be_bytes(buf)
}

/// Encode a coverage attestation into the compact v1 binary proof format.
#[allow(clippy::too_many_arguments)]
pub fn encode_coverage_attestation(
    env: &Env,
    asset: &Address,
    ledger_contract: &Address,
    min_bps: u32,
    max_age: u64,
    min_tier: Tier,
    covered: bool,
    ledger_sequence: u32,
    ledger_timestamp: u64,
) -> Bytes {
    let mut body = Bytes::new(env);
    body.extend_from_slice(&PROOF_MAGIC);
    body.push_back(PROOF_VERSION);
    body.push_back(u8::from(covered));
    body.extend_from_slice(&min_bps.to_be_bytes());
    body.extend_from_slice(&max_age.to_be_bytes());
    body.extend_from_slice(&tier_to_u32(min_tier).to_be_bytes());
    body.extend_from_slice(&ledger_sequence.to_be_bytes());
    body.extend_from_slice(&ledger_timestamp.to_be_bytes());

    append_u16_prefixed(&mut body, &asset.to_xdr(env));
    append_u16_prefixed(&mut body, &ledger_contract.to_xdr(env));

    let digest = compute_digest(env, &body);
    body.append(&Bytes::from_array(env, &digest.to_array()));
    body
}

/// Decode and verify a v1 coverage attestation proof without making any RPC calls.
pub fn verify_coverage_attestation(
    env: &Env,
    proof: &Bytes,
) -> Result<CoverageAttestation, AttestationError> {
    let total_len = proof.len();
    if (total_len as usize) < MIN_PROOF_LEN {
        return Err(AttestationError::TooShort);
    }

    let mut magic = [0u8; 4];
    proof.slice(0..4).copy_into_slice(&mut magic);
    if magic != PROOF_MAGIC {
        return Err(AttestationError::InvalidMagic);
    }

    let version = proof.get(4).ok_or(AttestationError::TooShort)?;
    if version != PROOF_VERSION {
        return Err(AttestationError::UnsupportedVersion);
    }

    let covered = match proof.get(5).ok_or(AttestationError::TooShort)? {
        0 => false,
        1 => true,
        _ => return Err(AttestationError::InvalidCoveredFlag),
    };

    let min_bps = read_u32(proof, 6);
    let max_age = read_u64(proof, 10);
    let min_tier = u32_to_tier(read_u32(proof, 18))?;
    let ledger_sequence = read_u32(proof, 22);
    let ledger_timestamp = read_u64(proof, 26);

    let mut cursor = HEADER_LEN as u32;
    let asset_len = read_u16(proof, cursor)? as u32;
    cursor += 2;
    if cursor + asset_len + 2 + (DIGEST_LEN as u32) > total_len {
        return Err(AttestationError::MalformedLength);
    }
    let asset_xdr = proof.slice(cursor..cursor + asset_len);
    let asset =
        Address::from_xdr(env, &asset_xdr).map_err(|_| AttestationError::InvalidAddressXdr)?;
    cursor += asset_len;

    let ledger_len = read_u16(proof, cursor)? as u32;
    cursor += 2;
    if cursor + ledger_len + (DIGEST_LEN as u32) != total_len {
        return Err(AttestationError::MalformedLength);
    }
    let ledger_xdr = proof.slice(cursor..cursor + ledger_len);
    let ledger_contract =
        Address::from_xdr(env, &ledger_xdr).map_err(|_| AttestationError::InvalidAddressXdr)?;
    cursor += ledger_len;

    let body = proof.slice(0..cursor);
    let expected_digest = compute_digest(env, &body);
    let mut actual_bytes = [0u8; DIGEST_LEN];
    proof
        .slice(cursor..cursor + DIGEST_LEN as u32)
        .copy_into_slice(&mut actual_bytes);
    let digest = BytesN::from_array(env, &actual_bytes);
    if digest != expected_digest {
        return Err(AttestationError::DigestMismatch);
    }

    Ok(CoverageAttestation {
        asset,
        ledger_contract,
        min_bps,
        max_age,
        min_tier,
        covered,
        ledger_sequence,
        ledger_timestamp,
        digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn round_trip_valid_attestation() {
        let env = Env::default();
        let asset = Address::generate(&env);
        let ledger = Address::generate(&env);

        let proof = encode_coverage_attestation(
            &env,
            &asset,
            &ledger,
            10_000,
            604_800,
            Tier::IssuerSigned,
            true,
            12_345,
            1_700_000_123,
        );

        let decoded = verify_coverage_attestation(&env, &proof).expect("valid proof");
        assert_eq!(decoded.asset, asset);
        assert_eq!(decoded.ledger_contract, ledger);
        assert_eq!(decoded.min_bps, 10_000);
        assert_eq!(decoded.max_age, 604_800);
        assert_eq!(decoded.min_tier, Tier::IssuerSigned);
        assert!(decoded.covered);
        assert_eq!(decoded.ledger_sequence, 12_345);
        assert_eq!(decoded.ledger_timestamp, 1_700_000_123);
    }

    #[test]
    fn tampered_proof_fails_digest_check() {
        let env = Env::default();
        let asset = Address::generate(&env);
        let ledger = Address::generate(&env);
        let mut proof = encode_coverage_attestation(
            &env,
            &asset,
            &ledger,
            10_000,
            604_800,
            Tier::AuditorSigned,
            false,
            99,
            1_700_000_000,
        );
        // Flip the `covered` byte from 0 to 1 without updating the digest.
        proof.set(5, 1);
        assert_eq!(
            verify_coverage_attestation(&env, &proof),
            Err(AttestationError::DigestMismatch)
        );
    }

    #[test]
    fn rejects_short_or_bad_header() {
        let env = Env::default();
        let short = Bytes::from_slice(&env, b"PLMS");
        assert_eq!(
            verify_coverage_attestation(&env, &short),
            Err(AttestationError::TooShort)
        );

        let asset = Address::generate(&env);
        let ledger = Address::generate(&env);
        let mut proof = encode_coverage_attestation(
            &env,
            &asset,
            &ledger,
            10_000,
            604_800,
            Tier::Transcribed,
            true,
            1,
            1,
        );

        proof.set(0, b'X');
        assert_eq!(
            verify_coverage_attestation(&env, &proof),
            Err(AttestationError::InvalidMagic)
        );
        proof.set(0, b'P');

        proof.set(4, 99);
        assert_eq!(
            verify_coverage_attestation(&env, &proof),
            Err(AttestationError::UnsupportedVersion)
        );
        proof.set(4, 1);

        proof.set(5, 2);
        assert_eq!(
            verify_coverage_attestation(&env, &proof),
            Err(AttestationError::InvalidCoveredFlag)
        );
        proof.set(5, 1);

        proof.set(21, 9);
        assert_eq!(
            verify_coverage_attestation(&env, &proof),
            Err(AttestationError::InvalidTier)
        );
    }
}
