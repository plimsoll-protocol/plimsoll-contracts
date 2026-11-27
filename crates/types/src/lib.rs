//! Types shared by the Plimsoll contracts, and client traits for the
//! cross-contract calls between them.
#![no_std]

use soroban_sdk::{contractclient, contracttype, Address, Bytes, BytesN, Env, String, Symbol};

/// One day of ledgers at a 5-second close time.
pub const DAY_IN_LEDGERS: u32 = 17_280;
/// Extend a persistent entry once it has less than this many ledgers left.
pub const PERSISTENT_THRESHOLD: u32 = 30 * DAY_IN_LEDGERS;
/// ...back up to this many ledgers.
pub const PERSISTENT_EXTEND_TO: u32 = 90 * DAY_IN_LEDGERS;
/// Same policy for instance storage.
pub const INSTANCE_THRESHOLD: u32 = 30 * DAY_IN_LEDGERS;
pub const INSTANCE_EXTEND_TO: u32 = 90 * DAY_IN_LEDGERS;

/// Minimum ledgers between proposing and executing an upgrade: one day at a
/// 5-second close time. Long enough for watchers to alert and consumers to
/// pause, short enough to ship a fix during an incident.
pub const UPGRADE_DELAY_LEDGERS: u32 = DAY_IN_LEDGERS;

/// Seconds per ledger, used only to render an age in ledgers for display.
/// Freshness itself is always judged in seconds.
pub const SECONDS_PER_LEDGER: u64 = 5;

/// Basis-point denominator: 10_000 bps = 100%.
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Narrowest coverage a reserve report may claim: 1 bps, or 0.01%.
///
/// Amounts are in the asset's smallest unit, so a reporter who types whole
/// units states a figure 10,000,000 times too small. The asset then reads as
/// uncovered, which is indistinguishable from backing that has actually gone.
pub const MIN_PLAUSIBLE_BPS: u32 = 1;

/// Widest coverage a reserve report may claim: 1_000_000 bps, or 10,000%.
///
/// The mirror of the slip above, for a figure typed in units too small, and a
/// ceiling on how over-backed an asset can be claimed to be before the claim
/// says more about the reporter than the reserves.
pub const MAX_PLAUSIBLE_BPS: u32 = 1_000_000;

/// What a registered address is allowed to say.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Role {
    Auditor = 1,
    Transcriber = 2,
    SupplyPoster = 3,
}

/// How much weight a reserve report carries. Ordered: higher is stronger.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Tier {
    /// Copied from a published document by a registered transcriber.
    Transcribed = 1,
    /// Signed by the asset's own issuer account.
    IssuerSigned = 2,
    /// Signed by a registered independent auditor.
    AuditorSigned = 3,
}

impl Tier {
    /// All tiers, strongest first.
    pub const ALL_DESC: [Tier; 3] = [Tier::AuditorSigned, Tier::IssuerSigned, Tier::Transcribed];
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterInfo {
    pub role: Role,
    pub name: String,
    pub added_at: u64,
}

/// Optional public details a reporter publishes about themselves.
///
/// Kept in its own storage key rather than added to `ReporterInfo`: changing
/// that struct's field list would change the encoding of every existing
/// reporter entry. A reporter with no metadata simply has no entry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReporterMetadata {
    /// A short display handle. `Symbol` limits it to 32 bytes of letters,
    /// digits and underscores, so it is not the registered legal name.
    pub name: Option<Symbol>,
    pub website_uri: Option<String>,
    /// Hash of the reporter's preferred contact details. The preimage is
    /// never stored on-chain.
    pub contact_hash: Option<Bytes>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRecord {
    pub sac: Address,
    pub code: String,
    pub issuer: Address,
    pub listed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupplySnapshot {
    pub amount: i128,
    pub ledger: u32,
    pub timestamp: u64,
    pub breakdown_hash: BytesN<32>,
    pub poster: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveReport {
    pub amount: i128,
    pub as_of: u64,
    pub posted_at: u64,
    pub tier: Tier,
    pub reporter: Address,
    pub doc_hash: BytesN<32>,
    pub doc_uri: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Coverage {
    pub bps: u32,
    pub supply: i128,
    pub reserves: i128,
    pub tier: Tier,
    pub supply_ledger: u32,
    pub supply_timestamp: u64,
    pub report_as_of: u64,
}

/// The ratio a `CoverageStatus::Covered` answer carries.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoveredInfo {
    pub bps: u32,
}

/// The last ratio and its age, carried by a `CoverageStatus::Stale` answer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleInfo {
    pub last_bps: u32,
    /// Estimated from the 5-second close time; for display, not consensus.
    pub age_ledgers: u32,
}

/// Why an asset is or is not covered, ready to render without a second call.
/// The precedence is Paused, then Stale, then Uncovered, then Covered.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoverageStatus {
    /// Fresh figures give at least the requested minimum.
    Covered(CoveredInfo),
    /// Figures exist but at least one is older than the requested maximum.
    Stale(StaleInfo),
    /// No figure exists yet, or fresh figures are below the requested minimum.
    Uncovered,
    /// The ledger's incident switch is on.
    Paused,
}

/// A proposed contract upgrade and the first ledger at which it may execute.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub executable_at_ledger: u32,
}

/// `ceil(a * b / 10_000)` for non-negative `a` and `b`, or `None` when the
/// result does not fit in `i128`. Splitting `b` by the denominator keeps the
/// intermediate products small enough to check.
fn mul_div_ceil_bps(a: i128, b: i128) -> Option<i128> {
    let whole = b / BPS_DENOMINATOR;
    let part = b % BPS_DENOMINATOR;
    let high = a.checked_mul(whole)?;
    let low = (a * part + BPS_DENOMINATOR - 1) / BPS_DENOMINATOR;
    high.checked_add(low)
}

/// `floor(remainder * 10_000 / supply)` for `0 <= remainder < supply`.
/// The direct product is tried first; when it would overflow `i128`, a binary
/// search over the 0..=9_999 result uses [`mul_div_ceil_bps`] instead.
fn fractional_bps(remainder: i128, supply: i128) -> u32 {
    if let Some(scaled) = remainder.checked_mul(BPS_DENOMINATOR) {
        return (scaled / supply) as u32;
    }
    let mut low = 0u32;
    let mut high = BPS_DENOMINATOR as u32;
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        let threshold = mul_div_ceil_bps(mid as i128, supply).unwrap_or(i128::MAX);
        if remainder >= threshold {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// `reserves * 10_000 / supply`, rounded down, saturating at `u32::MAX`.
/// Zero supply means nothing is owed, so it is reported as `u32::MAX`.
/// Non-positive reserves report zero.
pub fn coverage_bps(reserves: i128, supply: i128) -> u32 {
    if supply <= 0 {
        return u32::MAX;
    }
    if reserves <= 0 {
        return 0;
    }
    let whole = reserves / supply;
    if whole > (u32::MAX as i128) / BPS_DENOMINATOR {
        return u32::MAX;
    }
    let bps =
        whole as u64 * BPS_DENOMINATOR as u64 + fractional_bps(reserves % supply, supply) as u64;
    if bps > u32::MAX as u64 {
        u32::MAX
    } else {
        bps as u32
    }
}

/// The subset of reporter-registry that coverage-ledger calls.
#[contractclient(name = "RegistryClient")]
pub trait RegistryInterface {
    fn role_of(env: Env, reporter: Address) -> Option<Role>;
}

/// The subset of a SEP-41 token that coverage-ledger calls when an asset opts
/// in to a native supply source.
#[contractclient(name = "SupplyTokenClient")]
pub trait SupplyTokenInterface {
    fn total_supply(env: Env) -> i128;
}

/// The subset of coverage-ledger that consumers call.
#[contractclient(name = "CoverageLedgerClient")]
pub trait CoverageLedgerInterface {
    fn coverage(env: Env, sac: Address, min_tier: Tier) -> Option<Coverage>;
    fn is_covered(env: Env, sac: Address, min_bps: u32, max_age: u64, min_tier: Tier) -> bool;
    fn is_covered_at(
        env: Env,
        sac: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
        as_of_ledger: u32,
    ) -> bool;
    fn coverage_status(
        env: Env,
        sac: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) -> CoverageStatus;
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use num_bigint::BigInt;
    use proptest::prelude::*;

    /// Exact `reserves * 10_000 / supply`, clamped the way the contract's
    /// `i128` implementation must behave.
    fn reference_bps(reserves: i128, supply: i128) -> u32 {
        if supply <= 0 {
            return u32::MAX;
        }
        let scaled = BigInt::from(reserves) * BigInt::from(BPS_DENOMINATOR);
        let bps = scaled / BigInt::from(supply);
        if bps > BigInt::from(u32::MAX) {
            u32::MAX
        } else if bps < BigInt::from(0) {
            0
        } else {
            u32::try_from(bps).unwrap()
        }
    }

    proptest! {
        #[test]
        fn bps_never_panics(reserves: i128, supply: i128) {
            let _ = coverage_bps(reserves, supply);
        }

        #[test]
        fn bps_matches_a_big_integer_reference(reserves: i128, supply: i128) {
            prop_assert_eq!(coverage_bps(reserves, supply), reference_bps(reserves, supply));
        }

        #[test]
        fn bps_is_non_decreasing_in_reserves(a: i128, b: i128, supply: i128) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(coverage_bps(lo, supply) <= coverage_bps(hi, supply));
        }

        #[test]
        fn bps_is_non_increasing_in_positive_supply(
            reserves: i128,
            a: i128,
            b: i128,
        ) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let lo = lo.max(1);
            let hi = hi.max(lo);
            prop_assert!(coverage_bps(reserves, lo) >= coverage_bps(reserves, hi));
        }

        #[test]
        fn bps_non_positive_supply_is_max(reserves: i128, supply: i128) {
            prop_assume!(supply <= 0);
            prop_assert_eq!(coverage_bps(reserves, supply), u32::MAX);
        }
    }

    #[test]
    fn bps_handles_i128_extremes() {
        let values = [i128::MIN, i128::MIN + 1, -1, 0, 1, i128::MAX - 1, i128::MAX];
        for reserves in values {
            for supply in values {
                assert_eq!(
                    coverage_bps(reserves, supply),
                    reference_bps(reserves, supply)
                );
            }
        }
    }

    #[test]
    fn bps_rounds_down() {
        assert_eq!(coverage_bps(1, 3), 3_333);
        assert_eq!(coverage_bps(100, 100), 10_000);
        assert_eq!(coverage_bps(105, 100), 10_500);
    }

    #[test]
    fn bps_zero_supply_is_max() {
        assert_eq!(coverage_bps(0, 0), u32::MAX);
    }

    #[test]
    fn bps_saturates() {
        assert_eq!(coverage_bps(i128::MAX, 1), u32::MAX);
        assert_eq!(coverage_bps(u32::MAX as i128, 1), u32::MAX);
    }

    #[test]
    fn bps_negative_reserves_is_zero() {
        assert_eq!(coverage_bps(-5, 100), 0);
    }

    #[test]
    fn tiers_are_ordered() {
        assert!(Tier::AuditorSigned > Tier::IssuerSigned);
        assert!(Tier::IssuerSigned > Tier::Transcribed);
    }
}
