# Plimsoll contract specification (v0.1)

This document is the source of truth for the contract layer. Code that
disagrees with it is a bug in one or the other; fix whichever is wrong and
update both in the same pull request.

What every error code means and what a caller should do about it is in
[ERRORS.md](ERRORS.md). Error numbers are never reused or renumbered.

## What the contracts do

Plimsoll answers one question about a Stellar-issued asset: *how much of what
is circulating is backed, according to whom, and how recently?*

- The **liability side** (circulating supply) is computed off-chain from public
  ledger data by a supply poster and recorded on-chain with a hash of the
  per-bucket breakdown, so anyone can recompute and compare.
- The **reserve side** is a signed report. Who signed it decides its tier:
  the asset's own issuer account, a registered auditor, or a registered
  transcriber who copied a figure from a published report.
- Any contract can ask `is_covered(asset, min_bps, max_age, min_tier)` and get
  a yes or no, then refuse the asset when the answer is no.

## Contracts and dependency order

```
plimsoll-types   (library crate: shared types, cross-contract client traits)
      │
      ├── reporter-registry   (no contract dependencies)
      │
      ├── coverage-ledger     ──calls──▶ reporter-registry.role_of
      │                       ──calls──▶ <SAC>.name
      │
      └── covered-vault       ──calls──▶ coverage-ledger.is_covered
                              ──calls──▶ <SAC>.transfer
```

Build and deploy order: `reporter-registry` → `coverage-ledger` →
`covered-vault`. The vault is a reference consumer, not part of the protocol;
it exists to show the integration in about 100 lines.

## Shared types (`crates/types`)

| Type | Kind | Fields / variants |
| --- | --- | --- |
| `Role` | `u32` enum | `Auditor = 1`, `Transcriber = 2`, `SupplyPoster = 3` |
| `Tier` | `u32` enum | `Transcribed = 1`, `IssuerSigned = 2`, `AuditorSigned = 3` (ordered: higher is stronger) |
| `ReporterInfo` | struct | `role: Role`, `name: String`, `added_at: u64` |
| `ReporterMetadata` | struct | `name: Option<Symbol>`, `website_uri: Option<String>`, `contact_hash: Option<Bytes>` |
| `AssetRecord` | struct | `sac: Address`, `code: String`, `issuer: Address`, `listed_at: u64` |
| `SupplySnapshot` | struct | `amount: i128`, `ledger: u32`, `timestamp: u64`, `breakdown_hash: BytesN<32>`, `poster: Address` |
| `ReserveReport` | struct | `amount: i128`, `as_of: u64`, `posted_at: u64`, `tier: Tier`, `reporter: Address`, `doc_hash: BytesN<32>`, `doc_uri: String` |
| `Coverage` | struct | `bps: u32`, `supply: i128`, `reserves: i128`, `tier: Tier`, `supply_ledger: u32`, `supply_timestamp: u64`, `report_as_of: u64` |
| `PendingUpgrade` | struct | `wasm_hash: BytesN<32>`, `executable_at_ledger: u32` |
| `CoveredInfo` | struct | `bps: u32` |
| `StaleInfo` | struct | `last_bps: u32`, `age_ledgers: u32` |
| `CoverageStatus` | enum | `Covered(CoveredInfo)`, `Stale(StaleInfo)`, `Uncovered`, `Paused` |

Amounts are in the asset's smallest unit (Stellar classic assets: 7 decimals,
so `1_0000000` is one unit). No floats anywhere. Coverage is basis points:
`bps = reserves * 10_000 / supply`, rounded down. Non-positive reserves report
`0` bps. Positive results saturate at `u32::MAX`, including when
`reserves * 10_000` would overflow `i128`. When supply is zero or negative,
`bps = u32::MAX` (nothing is owed, so nothing is uncovered).

## Time-locked upgrades

`reporter-registry` and `coverage-ledger` replace the old immediate `upgrade`
with a two-step process. A compromised or careless admin key can still propose
code, but cannot install it until `UPGRADE_DELAY_LEDGERS` ledgers have passed.
That window lets watchers notice and alert, and lets consumers pause or exit
before the logic they depend on changes.

| Constant | Value | Meaning |
| --- | --- | --- |
| `UPGRADE_DELAY_LEDGERS` | `17_280` (one day at 5-second ledgers) | Minimum ledgers between proposal and execution. |

The delay is measured in ledgers rather than seconds so it does not depend on
ledger close-time drift. One day is deliberately short enough to ship a fix
during an incident, but long enough for an always-on watcher to raise an alarm.

**Storage (each contract)**

| Key | Storage | Value |
| --- | --- | --- |
| `PendingUpgrade` | instance | `PendingUpgrade` |

**Functions (each contract)**

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `propose_upgrade(wasm_hash: BytesN<32>)` | admin | `()` | — | `upgrade_proposed {wasm_hash} executable_at_ledger` |
| `execute_upgrade()` | admin | `()` | `UpgradeNotProposed`, `UpgradeDelayNotElapsed` | `upgrade_executed {wasm_hash}` |
| `cancel_upgrade()` | admin | `()` | `UpgradeNotProposed` | `upgrade_cancelled {wasm_hash}` |
| `pending_upgrade()` | — | `Option<PendingUpgrade>` | — | — |

Rules:

- `propose_upgrade` stores the hash and
  `executable_at_ledger = current ledger + UPGRADE_DELAY_LEDGERS`. It replaces
  any existing proposal, so a new proposal restarts the clock.
- `execute_upgrade` fails with `UpgradeNotProposed` when nothing is pending and
  with `UpgradeDelayNotElapsed` before `executable_at_ledger`. On success it
  clears the proposal, publishes `upgrade_executed`, and installs the wasm. If
  installation fails the transaction rolls back and the proposal remains.
- `cancel_upgrade` clears the proposal and publishes `upgrade_cancelled`.
  Calling it with no proposal fails with `UpgradeNotProposed`.
- The admin authorises every step. `pending_upgrade` is a public view so a
  watcher or front end can show what is scheduled and when it may execute.
- The old immediate `upgrade(wasm_hash)` is removed from both contracts.

## Storage and TTL

All three contracts use the same TTL policy, defined in `plimsoll-types`:

| Constant | Value | Meaning |
| --- | --- | --- |
| `DAY_IN_LEDGERS` | 17 280 | One day at 5-second close time. |
| `PERSISTENT_THRESHOLD` | 30 days (518 400 ledgers) | Extend a persistent entry when its remaining TTL drops below this. |
| `PERSISTENT_EXTEND_TO` | 90 days (1 555 200 ledgers) | Extend persistent entries up to this TTL. |
| `INSTANCE_THRESHOLD` | 30 days (518 400 ledgers) | Extend instance storage when its remaining TTL drops below this. |
| `INSTANCE_EXTEND_TO` | 90 days (1 555 200 ledgers) | Extend instance storage up to this TTL. |

### Which operations extend which entries

**reporter-registry**

| Entry | Extended by |
| --- | --- |
| `Admin` (instance) | `__constructor`, `transfer_admin`, `upgrade`, `set_reporter`, `revoke`, `top_up_stake`, `reduce_stake` |
| `Reporter(Address)` (persistent) | `set_reporter` (write), `get_reporter` / `role_of` (read) |
| `Metadata(Address)` (persistent) | `update_reporter_metadata` (write), `get_reporter_metadata` (read) |
| `Stake(Address)` (persistent) | `top_up_stake`, `reduce_stake` (write), `stake_of` (read) |

**coverage-ledger**

| Entry | Extended by |
| --- | --- |
| `Admin`, `Registry`, `Paused` (instance) | `__constructor`, `transfer_admin`, `list_asset`, `post_supply`, `post_reserve`, `pause`, `unpause`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` |
| `Asset(Address)` (persistent) | `list_asset` (write), `get_asset` (read) |
| `Supply(Address)` (persistent) | `post_supply` (write), `get_supply`, `coverage`, `is_covered`, `coverage_status` (read) |
| `NativeSupply(Address)` (persistent) | `enable_native_supply` (write), `native_supply_token`, `coverage`, `is_covered`, `coverage_status` (read) |
| `Report(Address, Tier)` (persistent) | `post_reserve` (write), `get_report`, `get_reports`, `coverage`, `is_covered`, `coverage_status` (read) |
| `Assets` (persistent) | `list_asset` (write), `asset_reporter_pairs` (read) |

**covered-vault**

| Entry | Extended by |
| --- | --- |
| `Config` (instance) | `__constructor`, `deposit`, `withdraw` |
| `Balance(Address)` (persistent) | `deposit` (write), `withdraw` (write), `balance` (read) |

### Archival behaviour

When an entry's TTL runs out the Soroban host **archives** it. Since Protocol
23 (CAP-0066) a transaction restores the archived persistent entries in its
footprint automatically: `simulateTransaction` marks them for restoration and
the submitter pays the rent for bringing them back. So the contracts never
observe an archived entry:

- **Reads and writes behave as if the entry were live.** No function returns
  `None`, or `false` for `is_covered`, because something it reads was
  archived; it answers from the restored value. A restored entry comes back
  with the network's minimum persistent TTL, and the next write or read that
  extends it brings it back to `PERSISTENT_EXTEND_TO`.
- **A transaction built without the restoration** (an old footprint, or a
  client that skips simulation) fails before the contract runs, as it would
  for any wrong footprint. That is a host error, not a contract error code.
- **`is_covered` after archival.** Reports and supply snapshots are written
  with a TTL of `PERSISTENT_EXTEND_TO` (about 90 days) and reads only extend
  it, so an entry can only be archived once at least that long has passed
  since it was posted. A restored report's `as_of`, or a restored snapshot's
  `timestamp`, is therefore at least 90 days old, and `is_covered` answers
  `false` for any `max_age` shorter than that, on age alone.

The test `archived_report_is_restored_and_judged_by_its_age` in
`examples/coverage-gate` pins this behaviour.

### Re-add after revoke

`reporter-registry.set_reporter` after a `revoke` creates a fresh
`ReporterInfo` with a new `added_at = current ledger timestamp`. The previous
`added_at` is not preserved. This is distinct from changing an existing
reporter's role/name, which keeps the original `added_at`.

## reporter-registry

Decides who may speak. Holds no money.

**Storage**

| Key | Storage | Value |
| --- | --- | --- |
| `Admin` | instance | `Address` |
| `Reporter(Address)` | persistent, TTL extended on write and read | `ReporterInfo` |
| `Metadata(Address)` | persistent, TTL extended on write and read | `ReporterMetadata` |
| `Stake(Address)` | persistent, TTL extended on write and read | `i128` |
| `PendingUpgrade` | instance | `PendingUpgrade` |

**Functions**

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(admin: Address)` | — | — | — | — |
| `admin()` | — | `Address` | — | — |
| `set_reporter(reporter: Address, role: Role, name: String)` | admin | `()` | `InvalidName` (len 0 or > 64) | `reporter_set {reporter} role, name` |
| `revoke(reporter: Address)` | admin | `()` | `ReporterNotFound` | `stake_changed {reporter} old_stake, new_stake` (if stake > 0), `reporter_revoked {reporter}` |
| `top_up_stake(reporter: Address, amount: i128)` | admin | `i128` (new stake) | `ReporterNotFound`, `InvalidStakeAmount` | `stake_changed {reporter} old_stake, new_stake` |
| `reduce_stake(reporter: Address, amount: i128)` | admin | `i128` (new stake) | `ReporterNotFound`, `InvalidStakeAmount`, `InsufficientStake` | `stake_changed {reporter} old_stake, new_stake` |
| `stake_of(reporter: Address)` | — | `i128` | — | — |
| `update_reporter_metadata(reporter: Address, name: Option<Symbol>, website_uri: Option<String>, contact_hash: Option<Bytes>)` | reporter | `()` | `ReporterNotFound`, `InvalidMetadata` (URI empty or > 256 bytes; contact hash > 64 bytes) | `reporter_metadata_updated {reporter} name, website_uri, contact_hash` |
| `get_reporter_metadata(reporter: Address)` | — | `Option<ReporterMetadata>` | — | — |
| `role_of(reporter: Address)` | — | `Option<Role>` | — | — |
| `get_reporter(reporter: Address)` | — | `Option<ReporterInfo>` | — | — |
| `transfer_admin(new_admin: Address)` | admin and new_admin | `()` | — | `admin_transferred {new_admin}` |
| `propose_upgrade(wasm_hash: BytesN<32>)` | admin | `()` | — | `upgrade_proposed {wasm_hash} executable_at_ledger` |
| `execute_upgrade()` | admin | `()` | `UpgradeNotProposed`, `UpgradeDelayNotElapsed` | `upgrade_executed {wasm_hash}` |
| `cancel_upgrade()` | admin | `()` | `UpgradeNotProposed` | `upgrade_cancelled {wasm_hash}` |
| `pending_upgrade()` | — | `Option<PendingUpgrade>` | — | — |

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`,
`InvalidMetadata = 5`, `InvalidStakeAmount = 6`, `InsufficientStake = 7`.

Reporter metadata is optional and lives under its own `Metadata(Address)` key,
not in `ReporterInfo`. A reporter registered before metadata existed therefore
keeps its stored entry, and `get_reporter` keeps its old shape. `revoke`
removes both entries (and clears any tracked `Stake(Address)` entry, emitting
`stake_changed` to `0` when the previous stake was positive). `update_reporter_metadata` replaces the whole metadata
record: a `None` field clears that field, and the reporter must be registered.
`set_reporter` does not touch metadata or stake.

Tracked stake lives under `Stake(Address)` and defaults to `0` when absent.
`top_up_stake` and `reduce_stake` require positive `amount` (`> 0`), and
`reduce_stake` removes the key when the balance reaches `0`. Both emit
`stake_changed {reporter} old_stake, new_stake` after the state change.
Causes and remedies: [ERRORS.md](ERRORS.md#registryerror).

## coverage-ledger

Records what was said, and answers coverage questions.

**Storage**

| Key | Storage | Value |
| --- | --- | --- |
| `Admin` | instance | `Address` |
| `Registry` | instance | `Address` |
| `PendingUpgrade` | instance | `PendingUpgrade` |
| `Paused` | instance | `bool` (absent means not paused) |
| `Asset(Address)` | persistent | `AssetRecord` |
| `Supply(Address)` | persistent | `SupplySnapshot` (latest only) |
| `NativeSupply(Address)` | persistent | `Address`: the SEP-41 token queried for supply when the opt-in is set |
| `ReportBy(Address, Tier, Address)` | persistent | `ReserveReport`: the latest from one reporter at one tier for one asset |
| `Reporters(Address, Tier)` | persistent | `Vec<Address>`: the reporters with a `ReportBy` entry at that tier, in the order they first posted; at most `MAX_REPORTERS_PER_TIER = 4` |
| `Assets` | persistent | `Vec<Address>`: every listed asset, in listing order. The `asset_reporter_pairs` index |
| `Report(Address, Tier)` | persistent | `ReserveReport`: the v0.1 layout (one per tier). Read until migrated, never written; see [Upgrading a v0.1 deployment](#upgrading-a-v01-deployment) |
| `Alias(Address)` | persistent | `Address`: maps a deprecated asset SAC to its successor SAC |

Every reporter keeps their own figure. Two auditors' reports coexist, and a
transcriber who posts a later `as_of` adds a fresher figure without erasing
anyone else's. The key includes the tier as well as the reporter so that a
reporter whose role changes (transcriber promoted to auditor, say) starts a
new entry at the new tier instead of moving the old one between indexes; the
old entry stops counting by the rule below.

A stored report **counts** while its reporter is still entitled to its tier:

- `IssuerSigned`: always. The issuer is fixed by the asset record.
- `AuditorSigned`: while `registry.role_of(reporter) == Some(Auditor)`.
- `Transcribed`: while `registry.role_of(reporter) == Some(Transcriber)`.

So `reporter-registry.revoke` takes a bad actor's figures out of
`coverage`, `is_covered`, `get_report` and `get_reports` in the same
transaction, with no second admin action, and restoring the role with
`set_reporter` brings them back. Issuer-signed reports never depend on the
registry. The reports are not deleted when the reporter is revoked; they stay
in storage until their slot is reclaimed (see the cap rule) and their history
is in events either way.

`coverage` first picks the report that would win on `as_of` and tier, then
asks the registry about that one only, and moves to the next only if it no
longer counts. So a fresh issuer-signed report costs no `role_of` call, a
fresh auditor or transcriber report costs one, and the worst case (every
fresher report revoked) is `2 * MAX_REPORTERS_PER_TIER` calls. The alternative, an admin-only
`void_report`, is cheaper to read but leaves the bad figure in force until a
maintainer remembers the second step. That gap is what this rule removes.

History lives in events. The indexer keeps the full series. Their topic and
data layouts, with real testnet examples, are in [EVENTS.md](EVENTS.md).

**Functions**

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(admin: Address, registry: Address)` | — | — | — | — |
| `list_asset(sac: Address)` | none (permissionless) | `AssetRecord` | `NotAStellarAsset`, `AlreadyListed` | `asset_listed {sac} code, issuer` |
| `post_supply(poster: Address, sac: Address, amount: i128, ledger: u32, breakdown_hash: BytesN<32>)` | poster | `()` | `NotSupplyPoster`, `AssetNotListed`, `InvalidAmount`, `FutureLedger`, `StaleLedger`, `LedgerTooOld` | `supply_posted {sac} amount, ledger, breakdown_hash, poster` |
| `post_reserve(reporter: Address, sac: Address, amount: i128, as_of: u64, doc_hash: BytesN<32>, doc_uri: String)` | reporter | `Tier` | `NotReporter`, `AssetNotListed`, `InvalidAmount`, `FutureTimestamp`, `StaleReport`, `InvalidUri`, `ImplausibleAmount`, `TooManyReporters` | `reserve_posted {sac, tier} amount, as_of, reporter, doc_hash, doc_uri` |
| `retract_reserve(reporter: Address, sac: Address)` | reporter | `()` | `AssetNotListed`, `NotReporter`, `NoReportToRetract` | `reserve_retracted {sac, tier} reporter, as_of` |
| `get_asset(sac)` | — | `Option<AssetRecord>` | — | — |
| `get_supply(sac)` | — | `Option<SupplySnapshot>` | — | — |
| `get_report(sac, tier: Tier)` | — | `Option<ReserveReport>` (the freshest report at that tier that counts) | — | — |
| `get_reports(sac)` | — | `Vec<ReserveReport>` (every stored report that counts: strongest tier first, then first-post order within a tier) | — | — |
| `reporter_last_post(reporter: Address, asset: Address)` | — | `Option<u64>` (the ledger timestamp of that reporter's most recent post for the asset) | — | — |
| `asset_reporter_pairs(cursor: Option<u32>, limit: u32)` | — | `Vec<(Address, Address)>` (up to `MAX_PAIRS_PER_CALL = 50` (asset, reporter) pairs) | — | — |
| `coverage(sac, min_tier: Tier)` | — | `Option<Coverage>` | — | — |
| `is_covered(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `bool` | `Paused` | — |
| `coverage_status(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `CoverageStatus` | — | — |
| `pause()` / `unpause()` | admin | `()` | — | `paused` / `unpaused` `by` |
| `is_paused()` | — | `bool` | — | — |
| `enable_native_supply(sac, token: Address)` | admin | `()` | `AssetNotListed` | `native_supply_enabled {sac} token` |
| `native_supply_token(sac)` | — | `Option<Address>` | — | — |
| `alias_asset(old: Address, new: Address)` | admin | `()` | `AssetNotListed` (either SAC unlisted), `SelfAlias` | `asset_aliased {old, new}` |
| `get_alias(asset: Address)` | — | `Option<Address>` | — | — |
| `registry()` / `admin()` | — | `Address` | — | — |
| `transfer_admin(new_admin)` | admin and new_admin | `()` | — | `admin_transferred` |
| `propose_upgrade(wasm_hash: BytesN<32>)` | admin | `()` | — | `upgrade_proposed {wasm_hash} executable_at_ledger` |
| `execute_upgrade()` | admin | `()` | `UpgradeNotProposed`, `UpgradeDelayNotElapsed` | `upgrade_executed {wasm_hash}` |
| `cancel_upgrade()` | admin | `()` | `UpgradeNotProposed` | `upgrade_cancelled {wasm_hash}` |
| `pending_upgrade()` | — | `Option<PendingUpgrade>` | — | — |

Rules:

- `list_asset` only accepts a genuine Stellar Asset Contract (the address's
  executable must be the built-in asset contract). The SAC's `name()` is
  `CODE:ISSUER`; the code and issuer are parsed from it, so nobody can list a
  look-alike token under a real issuer's name. Native XLM (`name() ==
  "native"`) is rejected: it has no issuer and no reserves.
- `post_reserve` tier resolution: if `reporter == asset.issuer` the tier is
  `IssuerSigned` (the classic issuer account signs the Soroban auth entry).
  Otherwise the registry role decides: `Auditor → AuditorSigned`,
  `Transcriber → Transcribed`. A `SupplyPoster` or unknown address gets
  `NotReporter`.
- Supply snapshots must move forward: `ledger` strictly greater than the stored
  one and not greater than the current ledger. Reports per tier must move
  forward per reporter: `as_of` strictly greater than that reporter's stored
  report at that tier, and not in the future. Another reporter's report at the
  same tier does not make a figure stale.
- A supply snapshot's `ledger` must be no more than 720 ledgers behind the
  current ledger. This is approximately one hour at Stellar's usual five
  seconds per ledger; the sequence-number bound avoids treating a fresh
  submission time as evidence that an old snapshot is fresh. A snapshot
  exactly 720 ledgers behind is accepted.
- `retract_reserve` resolves the reporter's tier exactly as `post_reserve`
  does, then removes only that reporter's stored report at that tier. If no
  such report exists, it fails with `NoReportToRetract`. Retraction frees the
  reporter's slot at that tier, and coverage falls back to the next qualifying
  report or returns `None`. The `reserve_retracted` event preserves the
  removed report's identity and `as_of`; history remains off-chain in events.
- Each tier holds reports from at most `MAX_REPORTERS_PER_TIER = 4` reporters
  per asset. When a reporter with no report at a tier posts to a full tier,
  `post_reserve` first drops the entries whose reporter no longer counts
  (revoked, or moved to another role), report and index entry both; if the
  tier is still full it fails with `TooManyReporters`. A maintainer frees a
  slot by revoking a reporter. The cap bounds what `coverage` reads, and so
  what every `is_covered` caller pays.
- `post_reserve` rejects a figure whose coverage against the stored supply
  would fall outside `MIN_PLAUSIBLE_BPS = 1` (0.01%) to
  `MAX_PLAUSIBLE_BPS = 1_000_000` (10,000%), with `ImplausibleAmount`. Amounts
  are in the asset's smallest unit, so a reporter who types whole units posts a
  figure 10,000,000 times too small and the asset reads as uncovered; that is
  indistinguishable from a real collapse, which makes it worth refusing rather
  than publishing. The check is skipped when there is no supply snapshot to
  judge against, or when the stored supply is zero.
  `amount == 0` is always allowed: no reserves at all is a statement the
  protocol exists to be able to carry, and unlike a small non-zero figure it
  cannot be the result of a unit slip.
- `coverage(sac, min_tier)` picks, among the stored reports at tiers
  `>= min_tier` that count, the one with the latest `as_of`. Ties go to the
  higher tier, then to the reporter who first posted at that tier. It returns
  `None` if there is no supply figure or no qualifying report. The supply
  figure is the latest snapshot, or the token's `total_supply()` when the
  asset has opted in to a native supply source (see below).
- `is_covered` is true only if `coverage` is `Some`, both the supply figure
  and the report are no older than `max_age` seconds, and `bps >= min_bps`.
  It raises `Paused` rather than answering while the ledger is paused.
  The comparison is **inclusive**: a coverage ratio exactly equal to `min_bps`
  returns `true`. This matters at exact round numbers such as `min_bps = 10_000`
  (100 %): an asset with reserves equal to its circulating supply passes the
  gate.

- `alias_asset(old, new)` creates or replaces the alias mapping `old → new`.
  Both `old` and `new` must be listed assets; `old == new` is rejected with
  `SelfAlias`. Only one alias hop is ever followed: `is_covered(old, ...)` and
  `coverage(old, ...)` resolve to `new`, but if `new` itself has an alias it is
  ignored (no chaining). The admin may call `alias_asset` a second time with the
  same `old` to point it at a different successor, or to `old` itself (which
  would be rejected with `SelfAlias`; to remove an alias, use a new listing for
  `old`).
- `get_alias(asset)` returns the successor address if one has been registered,
  or `None`.

Errors (`LedgerError`): `NotAStellarAsset = 1`, `AlreadyListed = 2`,
`AssetNotListed = 3`, `NotSupplyPoster = 4`, `NotReporter = 5`,
`InvalidAmount = 6`, `FutureLedger = 7`, `StaleLedger = 8`,
`FutureTimestamp = 9`, `StaleReport = 10`, `InvalidUri = 11`,
`ImplausibleAmount = 12`, `TooManyReporters = 13`, `UpgradeNotProposed = 14`,
`UpgradeDelayNotElapsed = 15`, `SelfAlias = 16`, `Paused = 17`,
`LedgerTooOld = 18`, `NoReportToRetract = 19`.
Causes, remedies and the order they are checked in: [ERRORS.md](ERRORS.md#ledgererror).

### Emergency pause

`pause()` and `unpause()` are admin-gated switches for an incident. While the
ledger is paused:

- `post_supply` and `post_reserve` fail with `Paused` before any other check
  except authentication, so no figure is recorded from either side.
- `is_covered` raises `Paused` instead of returning false. A consumer such as
  `covered-vault.deposit` propagates that error, so a pause reads as a pause
  and not as an under-backed asset.
- `coverage`, `coverage_status`, all views and `list_asset` keep working. A
  dashboard can still show the last recorded state, and `coverage_status`
  returns `Paused`.

Both calls are idempotent and publish an event every time: `paused {by}` or
`unpaused {by}`, where `by` is the admin that signed. Keeping `unpause`
non-failing means an admin who is unsure of the current state can still assert
the state they want. The flag lives in instance storage and defaults to false
when absent, so an upgrade from a deployment without it starts unpaused.

### Coverage status

`coverage_status(sac, min_bps, max_age, min_tier)` answers the same question as
`is_covered` with enough detail for a UI, in one call:

| Variant | Meaning |
| --- | --- |
| `Paused` | The ledger is paused. Nothing is being recorded or gated. |
| `Uncovered` | There is no supply figure, no qualifying report, or fresh figures give `bps < min_bps`. |
| `Stale(StaleInfo { last_bps, age_ledgers })` | Figures exist and at least one of supply or report is older than `max_age` seconds. `last_bps` is the ratio they last gave; `age_ledgers` is the age of the older of the two in whole ledgers (seconds divided by the 5-second close time, rounded down). |
| `Covered(CoveredInfo { bps })` | Fresh figures give at least `min_bps`. |

The order is deliberate: paused first, stale second (a stale ratio is not a
verdict on backing), then under-backed, then covered. `age_ledgers` is derived
from the supply snapshot's `ledger` and the report's `as_of` timestamp using
the 5-second ledger close; it is an estimate for display, not a consensus
value. `is_covered` is exactly `CoverageStatus::Covered` while unpaused.

### Native supply source

By default the liability side is a posted snapshot. An asset can opt in to
reading supply directly from a SEP-41 token contract instead:
`enable_native_supply(sac, token)` is admin-gated and requires `sac` to be
listed. It stores the token under `NativeSupply(sac)` and publishes
`native_supply_enabled {sac} token`; `native_supply_token(sac)` returns it.

When the opt-in is set, `coverage`, `is_covered` and `coverage_status` call
`token.total_supply()` and treat the result as a supply figure stamped at the
current ledger and timestamp, so it is always fresh. If the call fails (the
token does not implement SEP-41 `total_supply`, traps, or returns a negative
amount), those functions fall back to the posted `Supply(sac)` snapshot. The
fallback is silent by design: a token that stops answering degrades to the
last published figure rather than making every consumer fail. `post_supply`
is unaffected, so a poster can keep publishing snapshots as a backstop.

The snapshot itself is not deleted when the opt-in is enabled, and
`get_supply` still returns it. `post_reserve`'s plausibility check also keeps
using the posted snapshot: the check exists to catch a reporter's unit slip,
and the native figure is not a statement the reporter made.

### Upgrading a v0.1 deployment

The v0.1 ledger stored one report per tier under `Report(sac, tier)`. The
live testnet ledger holds such entries, and the deployed vaults store the
ledger's address in their config with no way to change it, so the ledger is
upgraded in place rather than redeployed.

The deployed v0.1 code predates time locks and still exposes the old immediate
`upgrade(wasm_hash)`. Use it once to install this version:

```bash
stellar contract build
HASH=$(stellar contract upload --wasm target/wasm32v1-none/release/coverage_ledger.wasm \
  --source plimsoll-admin --network testnet)
stellar contract invoke --id "$COVERAGE_LEDGER_ID" --source plimsoll-admin --network testnet -- \
  upgrade --wasm_hash "$HASH"
```

Then record `HASH` under `liveWasm.coverageLedger` in
`deployments/testnet.json`. From this version on, every upgrade uses the
two-step flow:

```bash
HASH=$(stellar contract upload --wasm target/wasm32v1-none/release/coverage_ledger.wasm \
  --source plimsoll-admin --network testnet)
stellar contract invoke --id "$COVERAGE_LEDGER_ID" --source plimsoll-admin --network testnet -- \
  propose_upgrade --wasm_hash "$HASH"
# after UPGRADE_DELAY_LEDGERS have closed:
stellar contract invoke --id "$COVERAGE_LEDGER_ID" --source plimsoll-admin --network testnet -- \
  execute_upgrade
```

No migration call is needed. The new code migrates lazily:

- **Reads** treat a legacy `Report(sac, tier)` as one more stored report at
  that tier, from the reporter recorded in it, subject to the same counting
  rule, ordered before the indexed reporters.
- **Writes**: the first `post_reserve` at a tier moves that tier's legacy
  entry, if any, to `ReportBy(sac, tier, reporter)`, makes its reporter the
  first entry of `Reporters(sac, tier)`, and removes the legacy key, before
  applying the staleness and cap rules. A legacy key therefore exists only
  while its tier's index is empty, so nothing is ever counted twice.

Contract ids, the coverage and query function signatures, and the existing
event layouts are unchanged, so the indexer, the dashboard and the vaults need
no change. The upgrade entry point changes from `upgrade(wasm_hash)` to
`propose_upgrade` / `execute_upgrade` / `cancel_upgrade`. Rolling back means
proposing and executing the previous hash through the same time lock; entries
that were already migrated would then be invisible to the old code, so a
rollback should be followed by each reporter posting again.

## covered-vault (reference consumer)

A deposit vault for one asset that refuses deposits while the asset is not
covered. Withdrawals are never blocked.

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(ledger: Address, asset: Address, min_bps: u32, max_age: u64, min_tier: Tier)` | — | — | — | — |
| `deposit(from: Address, amount: i128)` | from | `i128` new balance | `InvalidAmount`, `NotCovered` | `deposited {from} amount`, `coverage_dropped {asset}` |
| `withdraw(to: Address, amount: i128)` | to | `i128` new balance | `InvalidAmount`, `InsufficientBalance` | `withdrawn {to} amount` |
| `balance(of: Address)` | — | `i128` | — | — |
| `config()` | — | `VaultConfig` | — | — |
| `can_deposit()` | — | `bool` | — | — |

`can_deposit()` makes the same `is_covered(asset, min_bps, max_age,
min_tier)` call `deposit` makes, with the stored config, and returns its
answer. It is true exactly when a positive deposit would pass the coverage
gate, so a front end can show whether the vault is open without simulating a
deposit and reading a `NotCovered` failure. It says nothing about the
depositor's balance or authorisation.

`deposit` publishes `deposited` once the balance is written, then asks
`is_covered` again with the same config and publishes `coverage_dropped
{asset}` if that post-deposit answer is `false`. The second read is fresh
rather than the answer the gate used, so the event reports the coverage of the
vault's post-deposit state and a keeper watching for it can restate reserves.
A deposit writes no supply snapshot and no report, so on a ledger that behaves
as this document specifies the two reads agree and the event is not published;
it fires when the ledger's answer changes between the gate and the post-deposit
read — a later ledger version whose `is_covered` depends on live state, for
instance — which is exactly the case a keeper would otherwise miss. It costs
one cross-contract call per successful deposit.

A refused deposit publishes nothing at all: the `NotCovered` error rolls the
transaction back, and a rolled-back transaction keeps no events. A keeper
therefore cannot learn about a coverage drop from a refused deposit, only from
the `coverage_dropped` event on one that succeeds, or from `can_deposit()`.

Errors (`VaultError`): `InvalidAmount = 1`, `NotCovered = 2`,
`InsufficientBalance = 3`. Causes and remedies, and the asset-contract errors a
vault call can surface: [ERRORS.md](ERRORS.md#vaulterror).

## User flow → function map

| Step in the product | Function |
| --- | --- |
| Anyone adds an asset to the dashboard | `coverage-ledger.list_asset` |
| Indexer posts circulating supply every N minutes | `coverage-ledger.post_supply` |
| Issuer publishes monthly reserve figure | `coverage-ledger.post_reserve` (issuer account) |
| Auditor publishes attestation | `coverage-ledger.post_reserve` (auditor) |
| Volunteer copies a figure from a PDF report | `coverage-ledger.post_reserve` (transcriber) |
| Maintainer onboards an auditor / transcriber / poster | `reporter-registry.set_reporter` |
| Maintainer removes a bad actor | `reporter-registry.revoke` (their reports stop counting at once) |
| Dashboard shows ratio and freshness | `coverage-ledger.coverage`, `get_supply`, `get_reports` |
| Lending pool refuses an under-backed asset | `coverage-ledger.is_covered` (see `covered-vault`) |
| Front end shows whether the vault is taking deposits | `covered-vault.can_deposit` |
| Keeper restates reserves after the vault reports a coverage drop | `covered-vault.deposit` (emits `coverage_dropped`) |

## Out of scope for v0.1

- Listing a Soroban-native SEP-41 token as an asset. Its supply can be read
  through `enable_native_supply`, but `list_asset` still accepts only a
  Stellar Asset Contract, because the issuer and code come from the SAC name.
- On-chain dispute of supply figures. Watchers recompute off-chain; disputes
  are tracked as an open issue.
- Zero-knowledge proof of individual holder inclusion.
