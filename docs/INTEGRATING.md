# Calling `is_covered` from your own contract

This page shows how to make a Soroban contract refuse an asset while Plimsoll
says it is under-backed or out of date. It covers adding the dependency,
making the call, choosing the three parameters, and the edge cases. The
example is a real crate,
[examples/coverage-gate](../examples/coverage-gate), and the test suite
compiles and tests it. A test also fails if the copy on this page drifts from
the file.

[covered-vault](../contracts/covered-vault/src/lib.rs) is a complete deposit
vault built the same way. [SPEC.md](SPEC.md) is the full specification, and
[auth-model.md](auth-model.md) maps every entry point to the role that may call
it.

Who may call what — including who can post a report and what an unauthorised
call returns — is in [auth-model.md](auth-model.md).

## What the call gives you

```rust
is_covered(sac: Address, min_bps: u32, max_age: u64, min_tier: Tier) -> bool
```

`is_covered` answers `true` when all of these hold:

- the asset has a supply snapshot and a reserve report at `min_tier` or
  stronger;
- both are at most `max_age` seconds old;
- reserves divided by supply is at least `min_bps` basis points.

**It never fails.** Every way an asset can fall short comes back as `false`,
so you can gate on the result without handling errors. A missing figure, a
stale one, a weak signer and an asset nobody listed all read as `false`. If
you need to know *why* the answer is `false`, `coverage(sac, min_tier)`
returns the figures behind it (see [Telling users why](#telling-users-why)).

## 1. Add the dependency

`plimsoll-types` contains the `CoverageLedgerClient`, the `Tier` enum and the
other shared types. It is not published to crates.io, so depend on it from
git and pin a tag or commit:

```toml
[dependencies]
soroban-sdk = "29.0.1"
plimsoll-types = { git = "https://github.com/plimsoll-protocol/plimsoll-contracts", rev = "<commit>" }
```

`plimsoll-types` uses the `soroban-sdk` version of the commit you pin. Use the
same major version in your contract, or Cargo will build two SDKs and the
types will not line up. The `v0.1.0` tag uses `soroban-sdk` 28.0.0, and
`main` currently uses 29.0.1.

**Or declare the interface yourself.** The cross-contract call needs only a
trait and the `Tier` enum, and both have stable on-chain encodings, so you
can copy them instead of taking the dependency:

```rust
use soroban_sdk::{contractclient, contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Tier {
    Transcribed = 1,
    IssuerSigned = 2,
    AuditorSigned = 3,
}

#[contractclient(name = "CoverageLedgerClient")]
pub trait CoverageLedgerInterface {
    fn is_covered(env: Env, sac: Address, min_bps: u32, max_age: u64, min_tier: Tier) -> bool;
}
```

The numbers 1, 2 and 3 are the protocol's tier values and never change.

## 2. Find the two addresses

- **The coverage ledger.** Its contract id for each network is in
  `deployments/<network>.json` under `contracts.coverageLedger`. On testnet it
  is `CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D`.
- **The asset.** Plimsoll identifies an asset by its Stellar Asset Contract
  (SAC) address, not by its code or issuer:

  ```bash
  stellar contract id asset --asset USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5 --network testnet
  ```

  The asset must be listed in the ledger. Listing is permissionless; anyone
  can call `list_asset(sac)`, and `get_asset(sac)` tells you whether it is.
  An unlisted asset is never covered.

Store both addresses when you deploy, as the example does. The ledger is
upgraded in place, so its id does not change between versions.

## 3. Make the call

The whole example contract. `accept` refuses the asset while it is not
covered, and `is_open` lets a front end ask in advance:

```rust
//! A contract that accepts an asset only while Plimsoll says it is covered.
//!
//! This is the example in docs/INTEGRATING.md, kept as a crate so the test
//! suite compiles it and checks every edge case the guide describes. A test
//! also fails if the guide's copy of this file drifts from it.
#![no_std]

use plimsoll_types::{CoverageLedgerClient, Tier, INSTANCE_EXTEND_TO, INSTANCE_THRESHOLD};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env,
};

/// What "covered" has to mean before this contract takes the asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    /// The coverage-ledger contract to ask.
    pub ledger: Address,
    /// The asset's Stellar Asset Contract address (as listed in the ledger).
    pub asset: Address,
    /// Least acceptable reserves / supply, in basis points: 10_000 = 100%.
    pub min_bps: u32,
    /// Oldest acceptable supply snapshot and reserve report, in seconds.
    pub max_age: u64,
    /// Weakest acceptable signer of the reserve report.
    pub min_tier: Tier,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Policy,
    Total,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum GateError {
    InvalidAmount = 1,
    NotCovered = 2,
}

#[contract]
pub struct CoverageGate;

fn policy(env: &Env) -> Policy {
    env.storage()
        .instance()
        .get(&DataKey::Policy)
        .expect("policy is set in the constructor")
}

/// One cross-contract call. `is_covered` never fails: every reason the asset
/// might not qualify (no figures, stale figures, too weak a signer, not
/// enough reserves) comes back as `false`.
fn covered(env: &Env, p: &Policy) -> bool {
    CoverageLedgerClient::new(env, &p.ledger).is_covered(
        &p.asset,
        &p.min_bps,
        &p.max_age,
        &p.min_tier,
    )
}

#[contractimpl]
impl CoverageGate {
    pub fn __constructor(
        env: Env,
        ledger: Address,
        asset: Address,
        min_bps: u32,
        max_age: u64,
        min_tier: Tier,
    ) {
        let p = Policy {
            ledger,
            asset,
            min_bps,
            max_age,
            min_tier,
        };
        env.storage().instance().set(&DataKey::Policy, &p);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
    }

    /// Record `amount` of the asset from `from`, refusing while it is not
    /// covered. A real contract would move the tokens here too.
    pub fn accept(env: Env, from: Address, amount: i128) -> i128 {
        from.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, GateError::InvalidAmount);
        }
        if !covered(&env, &policy(&env)) {
            panic_with_error!(&env, GateError::NotCovered);
        }
        let total: i128 = env.storage().instance().get(&DataKey::Total).unwrap_or(0);
        let total = total
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(&env, GateError::InvalidAmount));
        env.storage().instance().set(&DataKey::Total, &total);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_EXTEND_TO);
        total
    }

    /// Whether `accept` would pass the coverage check right now, so a front
    /// end can say so without simulating a call that fails.
    pub fn is_open(env: Env) -> bool {
        covered(&env, &policy(&env))
    }
}

#[cfg(test)]
mod test;
```

The pattern to copy is the `covered` function plus the `if !covered(...)`
check at the top of the function you are gating. The rest is ordinary
contract code.

Two habits from `covered-vault` worth keeping:

- **Gate the way in, not the way out.** Refuse new deposits, loans against or
  listings of an uncovered asset, but never block users from withdrawing
  what they already hold. A reserve report going stale is not a reason to
  trap funds.
- **Ask at the moment you need the answer.** Do not store the result. It
  changes when a report ages past `max_age`, which can happen without any
  transaction touching the ledger.

## 4. Choose the parameters

### `min_bps`: how much backing

Coverage is `reserves * 10_000 / supply` in basis points, **rounded down**,
so 10_000 means reserves equal supply. Reserves one smallest unit short of
supply give 9_999 and fail `min_bps = 10_000`.

- `10_000` (100%) is the usual choice for a fiat-backed stablecoin.
- Ask for more, such as `10_200`, if the asset's reserves move between
  reports and you want a margin.
- `0` accepts any ratio, so only freshness and tier are checked.

The ledger refuses reports that would put coverage below 1 bps or above
1,000,000 bps against the current supply (`ImplausibleAmount`, see
[ERRORS.md](ERRORS.md#12-implausibleamount)), except for a report of exactly
zero reserves. So you will not see coverage driven to 0 or to 10,000% by a
reporter's unit slip.

### `max_age`: how recent, in seconds

The **same** limit applies to two things, measured against the ledger's
close time:

- the reserve report's `as_of`, the date the statement speaks for;
- the supply snapshot's `timestamp`, the time it was posted.

Pick it from the asset's reporting cadence:

- Monthly attestation: about 35 days (`35 * 86_400`), a month plus a few
  days for the report to be published and posted.
- Weekly statements: about 8 days. The PUSD demo vault uses 7 days.
- Supply is normally posted by the indexer every few minutes, so it is rarely
  what makes `is_covered` false. The reserve report usually is.

The check is inclusive: a figure exactly `max_age` seconds old is still
fresh. `u64::MAX` switches freshness off entirely, which leaves you trusting
a figure of any age.

### `min_tier`: who must have signed

| `Tier` | Value | Reports accepted |
| --- | ---: | --- |
| `Transcribed` | 1 | Every tier: transcriber, issuer or auditor |
| `IssuerSigned` | 2 | The issuer's own account, or an auditor |
| `AuditorSigned` | 3 | Registered auditors only |

The tiers are ordered by how much weight the signature carries. A
transcriber copied a figure from a published document; the issuer's account
signed its own figure; an auditor registered with Plimsoll signed theirs.

**The freshest qualifying report is judged, not the best one.** Among the
reports at `min_tier` or stronger that still count, `is_covered` judges the
one with the latest `as_of`. Ties go to the higher tier, then to the reporter
who posted at that tier first. So with `min_tier = IssuerSigned`, a new issuer
report showing 95% outweighs an older auditor report showing 110%, and the
answer is `false` at `min_bps = 10_000`. If only auditors' figures should
decide, ask for `AuditorSigned`.

## 5. Edge cases

Each case states what the code does and where. Every one of them has a test
in [examples/coverage-gate/src/test.rs](../examples/coverage-gate/src/test.rs).

### Zero supply reads as `u32::MAX` bps, and counts as covered

When the latest snapshot's `amount` is 0, `coverage_bps` returns `u32::MAX`
whatever the reserves, even zero
([crates/types/src/lib.rs](../crates/types/src/lib.rs), `coverage_bps`:
`if supply <= 0 { return u32::MAX; }`). Nothing is owed, so nothing is
uncovered, and `is_covered` is `true` for any `min_bps`, as long as both
figures are fresh and the tier qualifies.

This is correct for the question Plimsoll answers, but it means a brand-new
asset with nothing in circulation passes. If that matters to you, also read
`coverage(sac, min_tier)` and require `supply > 0`.

*Test: `zero_supply_reads_as_max_bps_and_is_covered`.*

### No report or no snapshot is `false`

`coverage` returns `None` when there is no supply snapshot, or no counting
report at `min_tier` or stronger. `is_covered` turns `None` into `false`
([contracts/coverage-ledger/src/lib.rs](../contracts/coverage-ledger/src/lib.rs),
`coverage`, where `read_persistent(...)?` and `best_report(...)?` return
early, and `is_covered`'s `None => return false`). An asset that was never
listed has neither, so it is `false` too.

*Tests: `no_report_is_not_covered`, `no_snapshot_is_not_covered`,
`unlisted_asset_is_not_covered`.*

### Freshness uses the report's `as_of` and the snapshot's post time

`is_covered` computes `now.saturating_sub(t) <= max_age` for
`t = coverage.supply_timestamp` and `t = coverage.report_as_of`, where `now`
is the ledger close time (`is_covered` in
[contracts/coverage-ledger/src/lib.rs](../contracts/coverage-ledger/src/lib.rs)).

- **Report:** `report_as_of` is the report's `as_of`, the date the reserves
  were measured, **not** `posted_at`. A report posted today for reserves as of
  two months ago is two months old.
- **Snapshot:** `supply_timestamp` is the snapshot's `timestamp`, which
  `post_supply` sets to the ledger close time when it was posted. It is
  **not** derived from the snapshot's `ledger` field (the ledger the supply was
  computed at).

Neither can be in the future: the ledger refuses an `as_of` later than the
ledger close time (`FutureTimestamp`) and a `ledger` later than the current
one (`FutureLedger`).

*Tests: `report_freshness_uses_as_of_not_post_time`,
`snapshot_freshness_uses_post_time_not_its_ledger`, `max_age_is_inclusive`.*

### Revoked reporters stop counting at once

An auditor's or transcriber's report counts only while
`reporter-registry.role_of(reporter)` still gives the matching role. Revoking
the reporter removes their figures from `is_covered` in the same transaction,
with no other step (`counts` in
[contracts/coverage-ledger/src/lib.rs](../contracts/coverage-ledger/src/lib.rs)).
The next freshest counting report is judged instead, if there is one.
Issuer-signed reports always count.

*Test: `revoked_auditor_stops_counting_at_once`.*

### Archived entries are restored, then judged by their age

Soroban archives a storage entry whose TTL runs out. Since Protocol 23 the
host restores archived entries in a transaction's footprint automatically.
Simulation adds the restoration and the submitter pays its rent, so
`is_covered` never sees an archived entry and never answers `false` *because*
of archival. It answers from the restored figures
([SPEC.md](SPEC.md#archival-behaviour)).

Reports and snapshots are written with a TTL of `PERSISTENT_EXTEND_TO`,
about 90 days, and reads only extend it. So an entry can only be archived
once at least 90 days have passed since it was posted, and a restored one is
always at least that old. With any `max_age` under 90 days, an archived
report or snapshot fails freshness and `is_covered` is `false`, which is the
right answer for figures nobody has updated in three months. Your users'
transactions may cost slightly more the first time an archived entry is
restored.

*Test: `archived_report_is_restored_and_judged_by_its_age`.*

## Telling users why

`is_covered` gives a yes or no. To explain a no, read the figures it judged:

```rust
coverage(sac: Address, min_tier: Tier) -> Option<Coverage>
```

| Result | Meaning |
| --- | --- |
| `None` | No supply snapshot, or no counting report at `min_tier` or above |
| `bps < min_bps` | Under-backed |
| `now - supply_timestamp > max_age` | Supply snapshot out of date |
| `now - report_as_of > max_age` | Reserve report out of date |

`coverage` is also in `CoverageLedgerClient`. Front ends can call it with a
simulated read and need no transaction.

## Cost

`is_covered` is one cross-contract call. Inside it the ledger reads the
latest snapshot, the stored reports at the tiers you accept, and asks the
registry about the winning report's reporter. An issuer-signed winner costs
no registry call, and an auditor or transcriber winner costs one. CI fails if
`is_covered` grows past the ceiling in
[resource-budgets.json](../resource-budgets.json), currently 155,000 CPU
instructions and 84,000 bytes of memory.

## Testing your integration

Register the real contracts in your tests. Either add the contract crates as
dev-dependencies, as [examples/coverage-gate/Cargo.toml](../examples/coverage-gate/Cargo.toml)
does:

```toml
[dev-dependencies]
soroban-sdk = { version = "29.0.1", features = ["testutils"] }
reporter-registry = { git = "https://github.com/plimsoll-protocol/plimsoll-contracts", rev = "<commit>" }
coverage-ledger = { git = "https://github.com/plimsoll-protocol/plimsoll-contracts", rev = "<commit>" }
```

or load the release wasm with `contractimport!`, as
[tests/e2e](../tests/e2e/tests/scenario.rs) does. Create the asset with
`env.register_stellar_asset_contract_v2(admin)` and use
`handle.issuer().address()` as the issuer. That is the account the ledger
reads from the SAC's name, and the only one whose reports are issuer-signed.

When something fails, [ERRORS.md](ERRORS.md) lists every error code with its
cause and remedy.
