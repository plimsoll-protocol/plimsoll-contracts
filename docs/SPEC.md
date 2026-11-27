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
      ├── coverage-ledger     ──calls──▶ reporter-registry.role_of, max_reporters
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
| `Role` | `u32` enum | `Auditor = 1`, `Transcriber = 2`, `SupplyPoster = 3`, `Watcher = 4` |
| `Tier` | `u32` enum | `Transcribed = 1`, `IssuerSigned = 2`, `AuditorSigned = 3` (ordered: higher is stronger) |
| `CoverageStatus` | `u32` enum | `Uncovered = 0`, `Stale = 1`, `Covered = 2` (ordered: higher is better) |
| `ReporterInfo` | struct | `role: Role`, `name: String`, `added_at: u64` |
| `ReporterMetadata` | struct | `name: Option<Symbol>`, `website_uri: Option<String>`, `contact_hash: Option<Bytes>` |
| `ReporterEntry` | struct | `reporter: Address`, `role: Role`, `name: String` |
| `AssetRecord` | struct | `sac: Address`, `code: String`, `issuer: Address`, `listed_at: u64` |
| `AssetRecord` | struct | `sac: Address`, `code: String`, `issuer: Address`, `listed_at: u64`, `display_name: Option<Symbol>` |
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
| `Admin`, `MaxReporters` (instance) | `__constructor`, `transfer_admin`, `upgrade`, `set_reporter`, `batch_register`, `revoke`, `top_up_stake`, `reduce_stake`, `set_max_reporters` |
| `Reporter(Address)` (persistent) | `set_reporter` / `batch_register` (write), `get_reporter` / `role_of` (read) |
| `Admin` (instance) | `__constructor`, `transfer_admin`, `upgrade`, `set_reporter`, `register_reporter`, `confirm_registration`, `cleanup_expired_registration`, `revoke` |
| `Reporter(Address)` (persistent) | `set_reporter` / `confirm_registration` (write), `get_reporter` / `role_of` (read) |
| `PendingReporter(Address)` (persistent) | `register_reporter` (write), `get_pending_reporter` (read), `confirm_registration` / `cleanup_expired_registration` / `revoke` (remove) |
| `Admin` (instance) | `__constructor`, `transfer_admin`, `upgrade`, `set_reporter`, `revoke`, `top_up_stake`, `reduce_stake` |
| `Reporter(Address)` (persistent) | `set_reporter` (write), `get_reporter` / `role_of` (read) |
| `ReporterDisplayName(Address)` (persistent) | optional `Symbol`; `set_reporter_name` (write), `get_reporter_name` (read) |

| `Metadata(Address)` (persistent) | `update_reporter_metadata` (write), `get_reporter_metadata` (read) |
| `DisplayName(Address)` (persistent) | `set_reporter_name` (write), `get_reporter_name` (read) |
| `Stake(Address)` (persistent) | `top_up_stake`, `reduce_stake` (write), `stake_of` (read) |

**coverage-ledger**

| Entry | Extended by |
| --- | --- |
| `Admin`, `Registry`, `Paused` (instance) | `__constructor`, `transfer_admin`, `list_asset`, `post_supply`, `post_reserve`, `pause`, `unpause`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` |
| `Asset(Address)` (persistent) | `list_asset` (write), `get_asset`, `last_updated` (existence read) |
| `Supply(Address)` (persistent) | `post_supply` (write), `get_supply`, `coverage`, `is_covered`, `is_covered_at`, `coverage_status` (read) |
| `SupplyPostedLedger(Address)` (persistent) | `post_supply` (write), `is_covered_at`, `historical_coverage_bps` (read) |
| `NativeSupply(Address)` (persistent) | `enable_native_supply` (write), `native_supply_token`, `coverage`, `is_covered`, `coverage_status` (read) |
| `Report(Address, Tier)` (persistent) | `post_reserve` (write), `get_report`, `get_reports`, `get_all_reports`, `last_updated`, `get_all_latest_reports`, `coverage`, `is_covered`, `is_covered_at`, `coverage_status` (read) |
| `ReportPostedLedger(Address, Tier, Address)` (persistent) | `post_reserve` (write), `is_covered_at`, `historical_coverage_bps` (read) |
| `Assets` (persistent) | `list_asset` (write), `asset_reporter_pairs`, `get_all_latest_reports` (read) |

**covered-vault**

| Entry | Extended by |
| --- | --- |
| `Config`, `Admin`, `PendingAdmin` (instance) | `__constructor`, `deposit`, `withdraw`, `propose_admin`, `accept_admin` |
| `Balance(Address)` (persistent) | `deposit` (write), `withdraw` (write), `balance` (read) |
| `Config`, `Admin`, `WithdrawalDelay` (instance) | `__constructor`, `deposit`, `request_withdraw`, `execute_withdraw`, `set_withdrawal_delay` |
| `Balance(Address)` (persistent) | `deposit`, `execute_withdraw` (write), `balance`, `request_withdraw` (read) |
| `PendingWithdrawal(Address)` (persistent) | `request_withdraw` (write), `execute_withdraw` (read and remove) |

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
| `StakeAsset` | instance | `Address` (native XLM SAC) |
| `MinStakeStroops` | instance | `i128` |
| `MaxReporters` | instance | `u32` (default `20`) |
| `PendingUpgrade` | instance | `PendingUpgrade` |
| `DisputeQuorum` | instance | `u32` (absent means `1`) |
| `Reporter(Address)` | persistent, TTL extended on write and read | `ReporterInfo` |
| `ReporterIndex` | persistent, TTL extended on write and read | `Vec<Address>`: every registered reporter, in the order they were first indexed |

| `ReporterDisplayName(Address)` | persistent, TTL extended on write and read | `Symbol` (optional, at most 32 characters) |

| `PendingReporter(Address)` | persistent, TTL extended on write and read | `PendingReporter` |
| `Metadata(Address)` | persistent, TTL extended on write and read | `ReporterMetadata` |
| `Dispute(BytesN<32>)` | persistent, TTL extended on write and read | `DisputeRecord` |
| `DisplayName(Address)` | persistent, TTL extended on write and read | `Symbol` |
| `Stake(Address)` | persistent, TTL extended on write and read | `i128` |
| `PendingUpgrade` | instance | `PendingUpgrade` |

**Functions**

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(admin: Address)` | — | — | — | — |
| `admin()` | — | `Address` | — | — |
| `set_reporter(reporter: Address, role: Role, name: String)` | admin | `()` | `InvalidName` (len 0 or > 64) | `reporter_set {reporter} role, name` |
| `set_stake_requirement(stake_asset: Address, min_stake_stroops: i128)` | admin | `()` | `InvalidStakeAmount` | `stake_requirement_set {stake_asset} min_stake_stroops` |
| `register_reporter(reporter: Address, name: String)` | reporter | `()` | `InvalidName`, `ReporterAlreadyRegistered`, `InsufficientStake` | `reporter_set {reporter} SupplyPoster, name` |
| `has_minimum_stake(reporter: Address)` | — | `bool` | — | — |
| `min_stake_stroops()` | — | `i128` | — | — |
| `batch_register(reporters: Vec<ReporterEntry>)` | admin | `()` | `InvalidName`, `EmptyBatch` | one `reporter_set {reporter} role, name` per entry |
| `register_reporter(reporter: Address, role: Role, name: String, confirm_window_ledgers: u32)` | admin | `()` | `InvalidName` (len 0 or > 64), `InvalidConfirmWindow` (0) | `reporter_registration_proposed {reporter} role, name, expires_at_ledger` |
| `confirm_registration(reporter: Address)` | reporter | `()` | `PendingRegistrationNotFound`, `RegistrationExpired` | `reporter_registration_confirmed {reporter} role, name` |
| `cleanup_expired_registration(reporter: Address)` | — | `bool` | — | `reporter_registration_expired {reporter}` (when expired) |
| `get_pending_reporter(reporter: Address)` | — | `Option<PendingReporter>` | — | — |
| `revoke(reporter: Address)` | admin | `()` | `ReporterNotFound` | `reporter_revoked {reporter}` |
| `revoke(reporter: Address)` | admin | `()` | `ReporterNotFound` | `stake_changed {reporter} old_stake, new_stake` (if stake > 0), `reporter_revoked {reporter}` |
| `top_up_stake(reporter: Address, amount: i128)` | admin | `i128` (new stake) | `ReporterNotFound`, `InvalidStakeAmount` | `stake_changed {reporter} old_stake, new_stake` |
| `reduce_stake(reporter: Address, amount: i128)` | admin | `i128` (new stake) | `ReporterNotFound`, `InvalidStakeAmount`, `InsufficientStake` | `stake_changed {reporter} old_stake, new_stake` |
| `stake_of(reporter: Address)` | — | `i128` | — | — |
| `update_reporter_metadata(reporter: Address, name: Option<Symbol>, website_uri: Option<String>, contact_hash: Option<Bytes>)` | reporter | `()` | `ReporterNotFound`, `InvalidMetadata` (URI empty or > 256 bytes; contact hash > 64 bytes) | `reporter_metadata_updated {reporter} name, website_uri, contact_hash` |
| `get_reporter_metadata(reporter: Address)` | — | `Option<ReporterMetadata>` | — | — |
| `set_reporter_name(reporter: Address, name: Symbol)` | reporter | `()` | `ReporterNotFound` | — |
| `get_reporter_name(reporter: Address)` | — | `Option<Symbol>` | — | — |
| `role_of(reporter: Address)` | — | `Option<Role>` | — | — |
| `get_reporter(reporter: Address)` | — | `Option<ReporterInfo>` | — | — |
| `reporter_count()` | — | `u32` | — | — |
| `reporters(start: u32, limit: u32)` | — | `Vec<Address>` | — | — |
| `max_reporters()` | — | `u32` | — | — |
| `set_max_reporters(max: u32)` | admin | `()` | `InvalidMaxReporters` (zero) | `max_reporters_set {max}` |
| `set_reporter_name(reporter: Address, name: Symbol)` | reporter | `()` | `ReporterNotFound` | — |
| `get_reporter_name(reporter: Address)` | — | `Option<Symbol>` | — | — |
| `set_dispute_quorum(quorum: u32)` | admin | `()` | `InvalidQuorum` (`quorum == 0`) | `dispute_quorum_set quorum` |
| `dispute_quorum()` | — | `u32` | — | — |
| `dispute_report(watcher: Address, report_id: BytesN<32>)` | watcher | `DisputeStatus` | `DuplicateDispute`, `DisputeAlreadyRejected`, `HoldingPeriodElapsed` | `report_disputed {report_id} watcher, dispute_count`, plus `report_rejected {report_id} dispute_count` when quorum is met |
| `get_dispute(report_id: BytesN<32>)` | — | `Option<DisputeRecord>` | — | — |
| `transfer_admin(new_admin: Address)` | admin and new_admin | `()` | — | `admin_transferred {new_admin}` |
| `propose_upgrade(wasm_hash: BytesN<32>)` | admin | `()` | — | `upgrade_proposed {wasm_hash} executable_at_ledger` |
| `execute_upgrade()` | admin | `()` | `UpgradeNotProposed`, `UpgradeDelayNotElapsed` | `upgrade_executed {wasm_hash}` |
| `cancel_upgrade()` | admin | `()` | `UpgradeNotProposed` | `upgrade_cancelled {wasm_hash}` |
| `pending_upgrade()` | — | `Option<PendingUpgrade>` | — | — |
| `version()` | — | `u32` | — | — |

Rules:

- `set_reporter` appends the reporter to `ReporterIndex` if it is not already
  there. Changing an existing reporter's role or name leaves its position
  unchanged.
- `revoke` removes the reporter from `ReporterIndex`; the reporters after it
  move up one place, so the order stays first-indexed order.
- `reporter_count()` is the length of `ReporterIndex`.
- `reporters(start, limit)` returns up to `min(limit, 100)` addresses from
  `ReporterIndex` starting at position `start`, and an empty list when `start`
  is past the end.
- Soroban cannot enumerate storage keys, so a deployment upgraded from a
  version without `ReporterIndex` starts with an empty index. Reporters
  registered before the upgrade join it when the admin calls `set_reporter`
  for them again with their current role and name.
- `version()` returns the contract's `VERSION` constant, raised on every
  behavioural change. The wasm also carries `description` and `repository`
  entries in its contract metadata.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`, `UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`.

Rules:

- `set_stake_requirement` configures the native XLM Stellar Asset Contract and
  a non-negative minimum balance in stroops. The admin supplies the network's
  native XLM SAC address.
- `register_reporter` lets an address register itself only as a `SupplyPoster`.
  It requires the reporter's auth, rejects an address already in the registry,
  and checks that its current XLM balance meets `MinStakeStroops`.
- `has_minimum_stake` compares the reporter's current XLM balance with the
  configured minimum. `coverage-ledger.post_supply` checks it on every post,
  so a reporter that later falls below the requirement cannot submit a
  snapshot. This is a balance requirement, not a transfer or custody of XLM.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`,
`UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`,
`InsufficientStake = 5`, `InvalidStakeAmount = 6`,
`ReporterAlreadyRegistered = 7`.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`,
`UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`, `UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`.
Rules:

- `set_stake_requirement` configures the native XLM Stellar Asset Contract and
  a non-negative minimum balance in stroops. The admin supplies the network's
  native XLM SAC address.
- `register_reporter` lets an address register itself only as a `SupplyPoster`.
  It requires the reporter's auth, rejects an address already in the registry,
  and checks that its current XLM balance meets `MinStakeStroops`.
- `has_minimum_stake` compares the reporter's current XLM balance with the
  configured minimum. `coverage-ledger.post_supply` checks it on every post,
  so a reporter that later falls below the requirement cannot submit a
  snapshot. This is a balance requirement, not a transfer or custody of XLM.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`,
`UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`,
`InsufficientStake = 5`, `InvalidStakeAmount = 6`,
`ReporterAlreadyRegistered = 7`.
Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`.
The display name is stored separately from `ReporterInfo` so existing reporter
records remain decodable. Soroban `Symbol` values are limited to 32 characters.

Errors (`RegistryError`): `ReporterNotFound = 1`, `InvalidName = 2`,
`UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`,
`InvalidMetadata = 5`, `InvalidStakeAmount = 6`, `InsufficientStake = 7`,
`InvalidMaxReporters = 8`, `EmptyBatch = 9`.
`UpgradeNotProposed = 3`, `UpgradeDelayNotElapsed = 4`, `InvalidMetadata = 5`,
`InvalidQuorum = 6`, `DuplicateDispute = 7`, `DisputeAlreadyRejected = 8`,
`HoldingPeriodElapsed = 9`.
`InvalidConfirmWindow = 6`, `PendingRegistrationNotFound = 7`,
`RegistrationExpired = 8`.

Two-factor reporter activation splits onboarding into `register_reporter`
(admin creates a `PendingReporter` entry expiring at `current_ledger +
confirm_window_ledgers`) and `confirm_registration` (the reporter themselves
accepts before expiry, moving the entry to `Reporter(Address)`). While pending,
`role_of` and `get_reporter` return `None`, so pending entries cannot be used
for `post_reserve` or `post_supply`. Expired pending entries are cleaned up by
`cleanup_expired_registration`, which removes the entry and emits
`reporter_registration_expired`.
`InvalidMetadata = 5`, `InvalidStakeAmount = 6`, `InsufficientStake = 7`.

Reporter metadata is optional and lives under its own `Metadata(Address)` key,
not in `ReporterInfo`. A reporter registered before metadata existed therefore
keeps its stored entry, and `get_reporter` keeps its old shape. `revoke`
removes both entries (and clears any tracked `Stake(Address)` entry, emitting
`stake_changed` to `0` when the previous stake was positive). `update_reporter_metadata` replaces the whole metadata
record: a `None` field clears that field, and the reporter must be registered.
`set_reporter` does not touch metadata or stake.

A reporter's on-chain display name is optional and lives under its own
`DisplayName(Address)` key, so entries stored before it existed are
unaffected and `get_reporter` keeps its shape. Only the registered reporter
can set it, with `set_reporter_name`; calling it again replaces the name.
The 32-character maximum is enforced by the `Symbol` type itself (a longer
symbol cannot be constructed or passed in). `get_reporter_name` returns
`None` until a name is set, and `revoke` removes the name with the other
entries. `set_reporter` does not touch it.

Tracked stake lives under `Stake(Address)` and defaults to `0` when absent.
`top_up_stake` and `reduce_stake` require positive `amount` (`> 0`), and
`reduce_stake` removes the key when the balance reaches `0`. Both emit
`stake_changed {reporter} old_stake, new_stake` after the state change.
Rules:

- **Batch registration.** `batch_register` is `set_reporter` for a list: the
  same name rule per entry, the same upsert (an existing reporter keeps its
  `added_at` and changes role and name), and one `reporter_set` event per
  entry. Every entry is validated before the first is written, and a failed
  call reverts the transaction, so the batch is all or nothing. An empty list
  is refused with `EmptyBatch` rather than silently doing nothing. Duplicate
  addresses in one batch apply in order; the last entry wins. Metadata and
  stake are untouched, exactly as with `set_reporter`.
- **The per-asset reporter cap.** `max_reporters()` is the registry-level cap
  on how many reporters one asset may hold, default
  `DEFAULT_MAX_REPORTERS_PER_ASSET = 20`, set with `set_max_reporters` and read
  by the coverage ledger when a reporter posts for an asset for the first
  time. The registry does not know which assets a reporter will speak for, so
  it stores the setting and does not enforce it: `set_reporter` and
  `batch_register` cannot bypass it, because the ledger applies it at post
  time. See [the coverage-ledger rules](#rules-1).

Causes and remedies: [ERRORS.md](ERRORS.md#registryerror).

### Reporter expiry (issue #92)

An admin may set a Unix-timestamp expiry on any registered reporter with
`set_expiry`. The view `is_reporter_active` returns `false` once
`env.ledger().timestamp() >= expires_at`; the comparison is always against the
**ledger timestamp** (`u64`, Unix seconds), never against the ledger sequence
number (`u32`). A reporter with no expiry entry is permanently active.

| Key | Storage | Value |
| --- | --- | --- |
| `Expiry(Address)` | persistent, TTL extended on write | `u64` (Unix timestamp) |

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `set_expiry(reporter: Address, expires_at: u64)` | admin | `()` | `ReporterNotFound` | `reporter_expiry_set {reporter} expires_at` |
| `is_reporter_active(reporter: Address)` | — | `bool` | — | — |

## coverage-ledger

Records what was said, and answers coverage questions.

**Storage**

| Key | Storage | Value |
| --- | --- | --- |
| `Admin` | instance | `Address` |
| `Registry` | instance | `Address` |
| `PendingUpgrade` | instance | `PendingUpgrade` |
| `TtlGrace` | instance | `u32` (ledger count; default 0) |
| `Paused` | instance | `bool` (absent means not paused) |
| `Asset(Address)` | persistent | `AssetRecord` |
| `Supply(Address)` | persistent | `SupplySnapshot` (latest only) |
| `NativeSupply(Address)` | persistent | `Address`: the SEP-41 token queried for supply when the opt-in is set |
| `ReportBy(Address, Tier, Address)` | persistent | `ReserveReport`: the latest from one reporter at one tier for one asset |
| `Reporters(Address, Tier)` | persistent | `Vec<Address>`: the reporters with a `ReportBy` entry at that tier, in the order they first posted; at most `MAX_REPORTERS_PER_TIER = 4` |
| `Assets` | persistent | `Vec<Address>`: every listed asset, in listing order. The `asset_reporter_pairs` index |
| `Report(Address, Tier)` | persistent | `ReserveReport`: the v0.1 layout (one per tier). Read until migrated, never written; see [Upgrading a v0.1 deployment](#upgrading-a-v01-deployment) |
| `IssuerDelegate(Address)` | persistent | `Address`: the account the asset's issuer appointed to post `IssuerSigned` reports for it; absent when none is set |
| `RecentReports` | persistent | `Vec<RecentReport>`: the most recently posted reports across all assets, newest first, capped at 50 |
| `Dispute(Address, u64)` | persistent | `DisputeRecord`: an active dispute keyed by `(sac, as_of)` |
| `DisputeHold` | instance | `u32`: number of ledgers a disputed report is frozen for (default: `DAY_IN_LEDGERS`) |

`RecentReport` is a local `#[contracttype]` struct `{ sac: Address, report: ReserveReport }`.
`DisputeRecord` is a local `#[contracttype]` struct `{ disputer: Address, hold_until_ledger: u32 }`.
| `Alias(Address)` | persistent | `Address`: maps a deprecated asset SAC to its successor SAC |
Every reporter keeps their own figure. Two auditors' reports coexist, and a
transcriber who posts a later `as_of` adds a fresher figure without erasing
anyone else's. The key includes the tier as well as the reporter so that a
reporter whose role changes (transcriber promoted to auditor, say) starts a
new entry at the new tier instead of moving the old one between indexes; the
old entry stops counting by the rule below.

A stored report **counts** while its reporter is still entitled to its tier:

- `IssuerSigned`: while the reporter is the asset's issuer (fixed by the asset
  record, so always) or the asset's current issuer delegate. A delegate the
  issuer has cleared or replaced no longer speaks for the figures it posted.
- `AuditorSigned`: while `registry.role_of(reporter) == Some(Auditor)`.
- `Transcribed`: while `registry.role_of(reporter) == Some(Transcriber)`.

So `reporter-registry.revoke` takes a bad actor's figures out of
`coverage`, `is_covered`, `get_report` and `get_reports` in the same
transaction, with no second admin action, and restoring the role with
`set_reporter` brings them back. Issuer-signed reports never depend on the
registry; `set_issuer_delegate` does the same for a delegate's figures,
taking them out when the delegate is cleared or replaced and bringing them
back if it is appointed again. The reports are not deleted when the reporter is revoked; they stay
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
| `post_supply(poster: Address, sac: Address, amount: i128, ledger: u32, breakdown_hash: BytesN<32>)` | poster | `()` | `NotSupplyPoster`, `AssetNotListed`, `InsufficientStake`, `InvalidAmount`, `FutureLedger`, `StaleLedger` | `supply_posted {sac} amount, ledger, breakdown_hash, poster` |
| `batch_delist(assets: Vec<Address>)` | admin | `()` | — | `asset_delisted {sac}` for each listed asset removed |
| `post_supply(poster: Address, sac: Address, amount: i128, ledger: u32, breakdown_hash: BytesN<32>)` | poster | `()` | `NotSupplyPoster`, `AssetNotListed`, `InvalidAmount`, `FutureLedger`, `StaleLedger`, `LedgerTooOld` | `supply_posted {sac} amount, ledger, breakdown_hash, poster` |
| `post_reserve(reporter: Address, sac: Address, amount: i128, as_of: u64, doc_hash: BytesN<32>, doc_uri: String)` | reporter | `Tier` | `NotReporter`, `AssetNotListed`, `InvalidAmount`, `FutureTimestamp`, `StaleReport`, `InvalidUri`, `ImplausibleAmount`, `TooManyReporters`, `TooManyReportersPerAsset` | `reserve_posted {sac, tier} amount, as_of, reporter, doc_hash, doc_uri` |
| `post_reserve(reporter: Address, sac: Address, amount: i128, as_of: u64, doc_hash: BytesN<32>, doc_uri: String)` | reporter | `Tier` | `NotReporter`, `AssetNotListed`, `InvalidAmount`, `FutureTimestamp`, `StaleReport`, `InvalidUri`, `ImplausibleAmount`, `TooManyReporters` | `reserve_posted {sac, tier} amount, as_of, reporter, doc_hash, doc_uri` |
| `set_issuer_delegate(sac: Address, delegate: Option<Address>)` | asset's issuer | `()` | `AssetNotListed` | `issuer_delegate_set {sac} delegate` when `Some`, `issuer_delegate_cleared {sac}` when `None` |
| `get_issuer_delegate(sac: Address)` | — | `Option<Address>` | — | — |
| `can_post_report(reporter: Address, sac: Address, amount: i128, as_of: u64, doc_hash: BytesN<32>, doc_uri: String)` | — | `Result<(), LedgerError>` | `Paused`, `AssetNotListed`, `NotReporter`, `InvalidAmount`, `InvalidUri`, `FutureTimestamp`, `ImplausibleAmount`, `StaleReport`, `TooManyReporters` | — |
| `retract_reserve(reporter: Address, sac: Address)` | reporter | `()` | `AssetNotListed`, `NotReporter`, `NoReportToRetract` | `reserve_retracted {sac, tier} reporter, as_of` |
| `get_asset(sac)` | — | `Option<AssetRecord>` | — | — |
| `get_supply(sac)` | — | `Option<SupplySnapshot>` | — | — |
| `get_report(sac, tier: Tier)` | — | `Option<ReserveReport>` (the freshest report at that tier that counts) | — | — |
| `get_reports(sac)` | — | `Vec<ReserveReport>` (every stored report that counts: strongest tier first, then first-post order within a tier) | — | — |
| `get_recent_reports(n: u32)` | — | `Vec<RecentReport>` (the min(n, 50) most recently posted reports across all assets, newest first) | — | — |
| `coverage(sac, min_tier: Tier)` | — | `Option<Coverage>` | — | — |
| `coverage_bps(sac: Address, max_age: u64)` | — | `Option<u128>` (basis-point coverage as u128; `None` when no live report or supply is within `max_age` seconds) | — | — |
| `is_covered(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `bool` | — | — |
| `dispute_report(watcher: Address, sac: Address, as_of: u64)` | watcher (must have `Role::Watcher`) | `()` | `AssetNotListed`, `NotAWatcher`, `AlreadyDisputed` | `report_disputed {sac, as_of} disputer, hold_until_ledger` |
| `get_dispute_status(sac: Address, as_of: u64)` | — | `Option<DisputeRecord>` | — | — |
| `resolve_dispute(sac: Address, as_of: u64)` | admin | `()` | `DisputeNotFound` | `dispute_resolved {sac, as_of}` |
| `set_dispute_hold(n: u32)` | admin | `()` | — | — |
| `get_dispute_hold()` | — | `u32` | — | — |
| `can_post_report(reporter: Address, asset: Address)` | — | `Result<(), LedgerError>` | `Paused`, `AssetNotListed`, `NotReporter`, `TooManyReporters` | — |
| `get_all_reports(sac)` | — | `Vec<ReserveReport>` (live, non-expired counting reports, strongest tier first, capped at `MAX_ALL_REPORTS = 20`) | — | — |
| `last_updated(asset: Address)` | — | `Option<u64>` (the ledger timestamp of the asset's most recent reserve post; `None` when unlisted or never reported) | — | — |
| `get_all_latest_reports(cursor: Option<u32>, limit: u32)` | — | `Vec<ReserveReport>` (one figure per (asset, reporter) pair, up to `MAX_PAIRS_PER_CALL = 50`) | — | — |
| `historical_coverage_bps(asset: Address, max_age: u32, as_of_ledger: u32)` | — | `Option<u128>` (coverage as the figures posted by `as_of_ledger` gave it; `None` outside the `max_age`-ledger window) | — | — |
| `reporter_last_post(reporter: Address, asset: Address)` | — | `Option<u64>` (the ledger timestamp of that reporter's most recent post for the asset) | — | — |
| `asset_reporter_pairs(cursor: Option<u32>, limit: u32)` | — | `Vec<(Address, Address)>` (up to `MAX_PAIRS_PER_CALL = 50` (asset, reporter) pairs) | — | — |
| `coverage(sac, min_tier: Tier)` | — | `Option<Coverage>` | — | — |
| `is_covered(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `bool` | — | — |
| `sign_coverage(asset: Address, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `Bytes` | — | — |

| `is_covered(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `CoverageStatus` | — | — |
| `set_ttl_grace(admin: Address, grace_ledgers: u32)` | admin | `()` | — | `ttl_grace_updated grace_ledgers` |
| `ttl_grace()` | — | `u32` | — | — |
| `is_covered(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `bool` | `Paused` | — |
| `is_covered_at(sac, min_bps: u32, max_age: u64, min_tier: Tier, as_of_ledger: u32)` | — | `bool` | `FutureLedger`, `Paused` | — |
| `coverage_status(sac, min_bps: u32, max_age: u64, min_tier: Tier)` | — | `CoverageStatus` | — | — |
| `pause()` / `unpause()` | admin | `()` | — | `paused` / `unpaused` `by` |
| `is_paused()` | — | `bool` | — | — |
| `enable_native_supply(sac, token: Address)` | admin | `()` | `AssetNotListed` | `native_supply_enabled {sac} token` |
| `native_supply_token(sac)` | — | `Option<Address>` | — | — |
| `alias_asset(old: Address, new: Address)` | admin | `()` | `AssetNotListed` (either SAC unlisted), `SelfAlias` | `asset_aliased {old, new}` |
| `get_alias(asset: Address)` | — | `Option<Address>` | — | — || `registry()` / `admin()` | — | `Address` | — | — |
| `transfer_admin(new_admin)` | admin and new_admin | `()` | — | `admin_transferred` |
| `propose_upgrade(wasm_hash: BytesN<32>)` | admin | `()` | — | `upgrade_proposed {wasm_hash} executable_at_ledger` |
| `execute_upgrade()` | admin | `()` | `UpgradeNotProposed`, `UpgradeDelayNotElapsed` | `upgrade_executed {wasm_hash}` |
| `cancel_upgrade()` | admin | `()` | `UpgradeNotProposed` | `upgrade_cancelled {wasm_hash}` |
| `pending_upgrade()` | — | `Option<PendingUpgrade>` | — | — |
| `version()` | — | `u32` | — | — |

Rules:

- `list_asset` only accepts a genuine Stellar Asset Contract (the address's
  executable must be the built-in asset contract). The SAC's `name()` is
  `CODE:ISSUER`; the code and issuer are parsed from it, so nobody can list a
  look-alike token under a real issuer's name. Native XLM (`name() ==
  "native"`) is rejected: it has no issuer and no reserves.
- `post_reserve` tier resolution: if `reporter == asset.issuer` the tier is
  `IssuerSigned` (the classic issuer account signs the Soroban auth entry).
  If a delegate is set for the asset via `set_issuer_delegate` and
  `reporter == delegate`, the tier is also `IssuerSigned`. Otherwise the
  registry role decides: `Auditor → AuditorSigned`, `Transcriber →
  Transcribed`. A `SupplyPoster` or unknown address gets `NotReporter`.
  The report's `reporter` field records the delegate's address when a delegate
  posts; consumers infer `IssuerSigned` from the tier and that the reporter is
  the delegate, not the issuer itself.
- `set_issuer_delegate` needs the asset's issuer account to authorise, so only
  the issuer can appoint, replace or clear its delegate. There is at most one
  delegate per asset; setting a new one replaces the old.
- `version()` returns the contract's `VERSION` constant, raised on every
  behavioural change. The wasm also carries `description` and `repository`
  entries in its contract metadata.
- Supply snapshots must move forward: `ledger` strictly greater than the stored
  one and not greater than the current ledger. Reports per tier must move
  forward per reporter: `as_of` strictly greater than that reporter's stored
  report at that tier, and not in the future. Another reporter's report at the
  same tier does not make a figure stale.
- Before accepting a supply snapshot, `post_supply` verifies that the poster
  still has at least the reporter registry's configured native XLM balance.
- A supply snapshot's `ledger` must be no more than 720 ledgers behind the
  current ledger. This is approximately one hour at Stellar's usual five
  seconds per ledger; the sequence-number bound avoids treating a fresh
  submission time as evidence that an old snapshot is fresh. A snapshot
  exactly 720 ledgers behind is accepted.
- `can_post_report` is a read-only pre-flight for `post_reserve`. It runs
  the checks that depend only on the reporter and the asset, in the same
  order and with the same errors: `Paused`, `AssetNotListed`, `NotReporter`
  (tier resolution), then `TooManyReporters` (counting only slots that
  `post_reserve` would not reclaim). It writes no storage and extends no TTL.
  Checks that depend on the figure itself (`InvalidAmount`, `InvalidUri`,
  `FutureTimestamp`, `StaleReport`, `ImplausibleAmount`) are not covered,
  so `Ok(())` means the reporter may post to the asset, not that any given
  figure will be accepted.
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
- On top of the per-tier caps, `post_reserve` enforces the registry's cap on
  how many reporters one asset may hold. A reporter new to the asset -- no
  stored report for it at any tier -- is refused with
  `TooManyReportersPerAsset` when the asset already holds
  `reporter-registry.max_reporters()` distinct reporters. Before failing, the
  ledger drops the entries of reporters who no longer count at every tier. The
  cap counts reporters, not entries: a reporter whose role changed and who now
  posts at a new tier keeps their slot, and the asset's issuer occupies one
  like anyone else. A v0.1 `Report(sac, tier)` entry counts until its tier is
  migrated. When both caps apply, `TooManyReporters` (the tier) is checked
  first. The setting defaults to `DEFAULT_MAX_REPORTERS_PER_ASSET = 20`, above
  the twelve entries the per-tier caps allow (four at each of three tiers), so
  it only binds once an admin lowers it. It is the knob for bounding how many
  figures `get_reports` walks.
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

- `can_post_report` applies the same reporter, asset, amount, URI, timestamp,
  plausibility, freshness and reporter-cap checks as `post_reserve`, but does
  not change report, index or contract configuration values. It returns the
  first applicable `LedgerError`, or `Ok(())`. The reporter-registry role
  lookup still follows the registry's persistent TTL policy. Because the check
  and the later post are separate calls, state can change between them.
- `coverage(sac, min_tier)` picks, among the stored reports at tiers
  `>= min_tier` that count, the one with the latest `as_of`. Ties go to the
  higher tier, then to the reporter who first posted at that tier. It returns
  `None` if there is no supply snapshot or no qualifying report.
- `is_covered` returns `CoverageStatus`:
  - `Uncovered` when `coverage` returns `None`, or `bps < min_bps`, or age
    exceeds `max_age + grace_secs` where `grace_secs = ttl_grace_ledgers * 5`.
  - `Stale` when bps and tier are satisfied but age is in the range
    `(max_age, max_age + grace_secs]`. Grace secs is zero when `ttl_grace_ledgers`
    is not set.
  - `Covered` when bps and tier are satisfied and age does not exceed `max_age`.
  - Age is the larger of `now - supply_timestamp` and `now - report_as_of`.
- `set_ttl_grace` sets the TTL grace window in ledgers. Each ledger is 5 seconds.
  Callers receive `Stale` during the grace window instead of `Uncovered`, which
  lets them decide whether stale data is acceptable. Zero disables the grace window.
- `ttl_grace` returns the stored grace window, or `0` when not set.
  `None` if there is no supply figure or no qualifying report. The supply
  figure is the latest snapshot, or the token's `total_supply()` when the
  asset has opted in to a native supply source (see below).
- `is_covered` is true only if `coverage` is `Some`, both the supply figure
  and the report are no older than `max_age` seconds, and `bps >= min_bps`.
- `sign_coverage(asset, min_bps, max_age, min_tier)` evaluates
  `is_covered(asset, min_bps, max_age, min_tier)` and returns a compact,
  domain-separated SHA-256-authenticated binary attestation (`Bytes`) encoding
  the asset, policy parameters, boolean result, ledger sequence, ledger
  timestamp, and ledger contract address so a third party can verify the
  payload off-chain without an RPC call via `plimsoll_types::attestation`.

### Coverage attestation proof format (v1)

All multi-byte integers are big-endian.

| Offset | Size | Field | Encoding |
| --- | --- | --- | --- |
| `0` | 4 | `magic` | ASCII `b"PLMS"` |
| `4` | 1 | `version` | `0x01` |
| `5` | 1 | `covered` | `0x01` if covered, `0x00` otherwise |
| `6` | 4 | `min_bps` | `u32` big-endian |
| `10` | 8 | `max_age` | `u64` big-endian |
| `18` | 4 | `min_tier` | `u32` big-endian (`1` = Transcribed, `2` = IssuerSigned, `3` = AuditorSigned) |
| `22` | 4 | `ledger_sequence` | `u32` big-endian |
| `26` | 8 | `ledger_timestamp` | `u64` big-endian |
| `34` | 2 | `asset_xdr_len` | `u16` big-endian (`N`) |
| `36` | `N` | `asset_xdr` | `asset.to_xdr()` |
| `36 + N` | 2 | `ledger_xdr_len` | `u16` big-endian (`M`) |
| `38 + N` | `M` | `ledger_xdr` | `ledger_contract.to_xdr()` |
| `38 + N + M` | 32 | `digest` | `sha256(b"plimsoll-coverage-attestation-v1" \|\| body)` |

`body` is the exact byte slice `0..(38 + N + M)`. Verification checks the
magic, version, `covered` byte (`0` or `1`), `min_tier` (`1..=3`), XDR-decodes
both addresses, and confirms `digest` matches the domain-separated SHA-256 hash.

  It raises `Paused` rather than answering while the ledger is paused.
  The comparison is **inclusive**: a coverage ratio exactly equal to `min_bps`
  returns `true`. This matters at exact round numbers such as `min_bps = 10_000`
  (100 %): an asset with reserves equal to its circulating supply passes the
  gate.
- `is_covered_at` evaluates the latest stored supply snapshot and the latest
  stored report per reporter as they stood no later than `as_of_ledger`. It
  raises `FutureLedger` when `as_of_ledger` exceeds the executing ledger, and
  raises `Paused` while the ledger is paused. The target ledger's timestamp is
  estimated from the current ledger timestamp and the five-second ledger
  cadence; `max_age` remains in seconds. Historical queries use posted supply
  snapshots, not a live native-supply token. The ledger does not retain prior
  versions of replaced snapshots or reports, so it cannot reconstruct a
  figure that has since been replaced; such a value is excluded if its current
  stored version was posted after `as_of_ledger`. Entries written before this
  query was introduced have no posting-ledger metadata and are eligible only
  for a query at the current ledger.
- `last_updated(asset)` answers when the asset was last reported on: the latest
  `posted_at` across every tier, including a not-yet-migrated v0.1 entry. The
  registry is not consulted, so a figure whose reporter was later revoked still
  counts as a post, and a stale figure answers with when it was posted;
  `get_reports` answers which figures count now. `None` means the asset is
  unlisted or has never been reported on, which is not the same answer as a very
  old post. Like `coverage`, a registered alias is followed.
- `asset_reporter_pairs` and `get_all_latest_reports` walk the pair index the
  same way: assets in listing order, tiers strongest first within an asset,
  first-post order within a tier, one entry per reporter per asset.
  `get_all_latest_reports` carries that reporter's freshest figure (latest
  `as_of`, ties to the stronger tier). Both page with `cursor` (the index of the
  first pair, `0` when absent) and `limit`, capped at `MAX_PAIRS_PER_CALL = 50`;
  an empty page means the cursor is at or past the last pair. Neither consults
  the registry, so a revoked reporter's figure is still listed. A figure that
  exists only as a not-yet-migrated v0.1 entry is listed by neither: the pair
  index is the per-tier reporter index.
- `historical_coverage_bps(asset, max_age, as_of_ledger)` reads the figures
  posted no later than `as_of_ledger` and returns
  `coverage_bps(reserves, supply)` widened to `u128`. The supply snapshot and
  the chosen report (the freshest at `Transcribed` or stronger posted by then)
  must both have been posted at most `max_age` **ledgers** before
  `as_of_ledger`, inclusive — the ledger equivalent of `is_covered`'s seconds.
  It returns `None` when `as_of_ledger` is in the future (the view cannot fail,
  so it answers `None` rather than raising `FutureLedger`), when either side has
  no figure posted by then, or when either is older than `max_age`. The target
  is a ledger, not a timestamp, so no cadence is assumed. Like `is_covered_at`
  it reads the stored figures: replaced and retracted ones are not retained, so
  a ratio they gave cannot be reconstructed here, and the events are the source
  for that.

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
`ImplausibleAmount = 12`, `TooManyReporters = 13`,
`UpgradeNotProposed = 14`, `UpgradeDelayNotElapsed = 15`,
`SelfAlias = 16`, `InsufficientStake = 17`.

`ImplausibleAmount = 12`, `TooManyReporters = 13`, `UpgradeNotProposed = 14`,
`UpgradeDelayNotElapsed = 15`, `SelfAlias = 16`, `Paused = 17`,
`LedgerTooOld = 18`, `NoReportToRetract = 19`,
`TooManyReportersPerAsset = 20`.
Causes, remedies and the order they are checked in: [ERRORS.md](ERRORS.md#ledgererror).

### Emergency pause

`pause()` and `unpause()` are admin-gated switches for an incident. See [pause.md](pause.md) for what happens to in-flight reports during a pause and how to unpause safely. While the
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

Upgrade `reporter-registry` before `coverage-ledger`. The ledger asks the
registry for `max_reporters` when a reporter posts for an asset for the first
time, so a ledger running ahead of its registry would fail those calls with a
host error for the missing function. The registry first is safe: it keeps
`role_of`, and until an admin sets a cap it answers the default.

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
covered. Withdrawals are not coverage-gated, but withdrawals above the
configured threshold also require an approver.

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(ledger: Address, asset: Address, min_bps: u32, max_age: u64, min_tier: Tier, approver: Address, large_withdrawal_threshold: i128)` | — | — | `InvalidThreshold` | — |
| `deposit(from: Address, amount: i128)` | from | `i128` new balance | `InvalidAmount`, `NotCovered` | `deposited {from} amount` |
| `withdraw(to: Address, amount: i128)` | to and approver when `amount > large_withdrawal_threshold` | `i128` new balance | `InvalidAmount`, `InsufficientBalance` | `withdrawn {to} amount` |

| `__constructor(ledger: Address, asset: Address, min_bps: u32, max_age: u64, min_tier: Tier)` | — | — | — | — |
covered. The deployer supplies the vault admin, who can configure a withdrawal delay.
Withdrawals are never gated on coverage, but when the delay is non-zero they
must be requested before they can be executed.

**Storage**

| Key | Storage | Value |
| --- | --- | --- |
| `Admin` | instance | `Address` |
| `Config` | instance | `VaultConfig` |
| `Balance(Address)` | persistent | `i128` |

**Functions**

| Function | Auth | Returns | Errors | Event |
| --- | --- | --- | --- | --- |
| `__constructor(ledger: Address, asset: Address, min_bps: u32, max_age: u64, min_tier: Tier, admin: Address)` | — | — | — | — |
| `deposit(from: Address, amount: i128)` | from | `i128` new balance | `InvalidAmount`, `NotCovered` | `deposited {from} amount`, `coverage_dropped {asset}` |
| `deposit(from: Address, amount: i128)` | from | `i128` new balance | `InvalidAmount`, `NotCovered` | `deposited {from} amount` |
| `withdraw(to: Address, amount: i128)` | to | `i128` new balance | `InvalidAmount`, `InsufficientBalance` | `withdrawn {to} amount` |
| `__constructor(admin: Address, ledger: Address, asset: Address, min_bps: u32, max_age: u64, min_tier: Tier)` | — | — | — | — |
| `deposit(from: Address, amount: i128)` | from | `i128` new balance | `InvalidAmount`, `NotCovered` | `deposited {from} amount, balance_after`, `coverage_dropped {asset}` |
| `request_withdraw(to: Address, amount: i128)` | to | `u64 available_at` | `InvalidAmount`, `InsufficientBalance`, `InvalidDelay` | `withdrawal_queued {to} amount, available_at` |
| `execute_withdraw(to: Address)` | to | `i128` new balance | `NoPendingWithdrawal`, `WithdrawalDelayNotElapsed`, `InsufficientBalance` | `withdrawn {to} amount, balance_after` |
| `set_withdrawal_delay(delay_seconds: u64)` | admin | `()` | `InvalidDelay` | — |
| `withdrawal_delay_seconds()` | — | `u64` | — | — |
| `balance(of: Address)` | — | `i128` | — | — |
| `config()` | — | `VaultConfig` | — | — |
| `version()` | — | `u32` | — | — |
| `admin()` | — | `Address` | — | — |
| `can_deposit()` | — | `bool` | — | — |
| `simulate_coverage(proposed_reserve: i128)` | — | `u128` | — | — |
| `admin()` | — | `Address` | — | — |
| `pending_admin()` | — | `Option<Address>` | — | — |
| `propose_admin(new_admin: Address)` | admin | `()` | — | `vault_admin_proposed {new_admin}` |
| `accept_admin()` | pending admin | `()` | `NoPendingAdmin` | `vault_admin_transferred {old_admin, new_admin}` |
| `update_thresholds(min_bps: u32, max_age: u64, min_tier: Tier)` | admin | `()` | — | `thresholds_updated old_min_bps, old_max_age, old_min_tier, new_min_bps, new_max_age, new_min_tier` |

`can_deposit()` returns true exactly when a positive deposit would pass the
coverage gate (`is_covered` returns `Covered`). A front end can call it to
show whether the vault is open without simulating a deposit and reading a
`NotCovered` failure.

`update_thresholds` replaces the stored `min_bps`, `max_age`, and `min_tier` in
one atomic call. The admin's auth is required. Existing deposits are unaffected;
only future deposit attempts see the new thresholds.
`request_withdraw` replaces any existing request for the same address and
restarts its delay. It checks the current balance, stores the requested amount
and `available_at = current timestamp + withdrawal_delay_seconds`, then emits
`withdrawal_queued`. `execute_withdraw` requires the owner's auth, fails until
the request's `available_at`, then removes the request, transfers the tokens,
updates the balance, and emits `withdrawn`. A delay of zero is valid but still
uses the two calls. The deployer supplies the vault admin as the `admin` constructor argument.
`set_withdrawal_delay` requires that address's auth and fails with
`InvalidDelay` if adding the delay to the current timestamp overflows `u64`;
changing the delay affects newly queued withdrawals and does not change an
existing request's `available_at`.

`can_deposit()` makes the same `is_covered(asset, min_bps, max_age,
min_tier)` call `deposit` makes, with the stored config, and returns its
answer. It is true exactly when a positive deposit would pass the coverage
gate, so a front end can show whether the vault is open without simulating a
deposit and reading a `NotCovered` failure. It says nothing about the
depositor's balance or authorisation.

`simulate_coverage(proposed_reserve)` is a pure view function (no vault state
writes) that reads the current on-chain supply and qualifying reserves from the
configured `coverage-ledger` and returns the resulting `coverage_bps` (as
`u128`) if `proposed_reserve` were added to the current reserves.
`withdraw` always requires `to` authorization. When `amount` is strictly
greater than `large_withdrawal_threshold`, it also requires authorization from
the configured `approver`; an amount equal to the threshold needs only `to`.
The threshold must be non-negative. Withdrawals remain available regardless
of coverage status.

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
`InsufficientBalance = 3`, `InvalidThreshold = 4`. Causes and remedies, and the
asset-contract errors a vault call can surface: [ERRORS.md](ERRORS.md#vaulterror).
`InsufficientBalance = 3`, `NoPendingAdmin = 4`. Causes and remedies, and the asset-contract errors a
vault call can surface: [ERRORS.md](ERRORS.md#vaulterror).
`InsufficientBalance = 3`, `InvalidDelay = 4`, `NoPendingWithdrawal = 5`,
`WithdrawalDelayNotElapsed = 6`. Causes and remedies, and the asset-contract
errors a vault call can surface: [ERRORS.md](ERRORS.md#vaulterror).

## User flow → function map

| Step in the product | Function |
| --- | --- |
| Anyone adds an asset to the dashboard | `coverage-ledger.list_asset` |
| Indexer posts circulating supply every N minutes | `coverage-ledger.post_supply` |
| Issuer publishes monthly reserve figure | `coverage-ledger.post_reserve` (issuer account) |
| Auditor publishes attestation | `coverage-ledger.post_reserve` (auditor) |
| Volunteer copies a figure from a PDF report | `coverage-ledger.post_reserve` (transcriber) |
| Front end checks a reporter may post before asking them to sign | `coverage-ledger.can_post_report` |
| Reporter sets a human-readable name shown by the dashboard | `reporter-registry.set_reporter_name`, `get_reporter_name` |
| Maintainer onboards an auditor / transcriber / poster | `reporter-registry.set_reporter` |
| Supply poster self-registers | `reporter-registry.register_reporter` (minimum XLM balance required) |
| Maintainer onboards several reporters at once | `reporter-registry.batch_register` |
| Maintainer caps how many reporters an asset may hold | `reporter-registry.set_max_reporters` |
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
