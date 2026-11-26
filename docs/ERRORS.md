# Error reference

Every error the three Plimsoll contracts can raise: its number, what triggers
it, and what the caller should do about it. Front ends and SDKs should map
errors from this page rather than keep their own tables.

The variants are defined in each contract's `lib.rs` (`RegistryError`,
`LedgerError`, `VaultError`). [SPEC.md](SPEC.md) says which function may
raise which error. This page explains each one.

## Reading an error

A failed call or simulation reports a contract error as
`Error(Contract, #N)`. **N only means something together with the contract
that raised it.** The three contracts number their errors independently, so
`#2` is `InvalidName` from the registry, `AlreadyListed` from the ledger and
`NotCovered` from the vault. Take the contract id from the failed invocation
(the diagnostic events name it) before you look N up.

When a contract calls another and the callee fails, the callee's error is the
one that comes back. In practice:

- `covered-vault.deposit` and `withdraw` move tokens through the asset's
  Stellar Asset Contract (SAC). If the transfer fails, the caller sees the
  **SAC's** error code, not a vault code. See
  [Errors from the asset contract](#errors-from-the-asset-contract).
- `coverage-ledger` calls `reporter-registry.role_of` and the SAC's `name`,
  but neither can fail on a valid deployment, so a ledger call only ever
  fails with a ledger code.
- `coverage-ledger.is_covered` fails only with `Paused`, and only while an
  admin has paused the ledger. Every other reason an asset might not qualify
  comes back as `false`. `coverage` never fails; it returns `None`.
- `covered-vault.deposit` propagates the ledger's `Paused` error unchanged
  when the ledger is paused, so a pause is distinguishable from
  `NotCovered`.

### Numbers are never reused or renumbered

The indexer, the web app and third-party front ends decode errors by number.
So a variant's number is fixed for good once it ships:

- New errors are **appended** with the next free number.
- A variant that stops being raised keeps its number, which is not given to
  anything else.
- No existing variant is renumbered, whatever the reason.

[AGENTS.md](../AGENTS.md) makes this a rule for every change, and the
end-to-end test in [tests/e2e](../tests/e2e/tests/scenario.rs) asserts several
codes as literal numbers, so a renumbering fails CI.

## Quick lookup

| N | reporter-registry | coverage-ledger | covered-vault |
| ---: | --- | --- | --- |
| 1 | [`ReporterNotFound`](#1-reporternotfound) | [`NotAStellarAsset`](#1-notastellarasset) | [`InvalidAmount`](#1-invalidamount) |
| 2 | [`InvalidName`](#2-invalidname) | [`AlreadyListed`](#2-alreadylisted) | [`NotCovered`](#2-notcovered) |
| 3 | `UpgradeNotProposed` | [`AssetNotListed`](#3-assetnotlisted) | [`InsufficientBalance`](#3-insufficientbalance) |
| 4 | `UpgradeDelayNotElapsed` | [`NotSupplyPoster`](#4-notsupplyposter) | |
| 5 | [`InvalidMetadata`](#5-invalidmetadata) | [`NotReporter`](#5-notreporter) | |
| 6 | | [`InvalidAmount`](#6-invalidamount) | |
| 7 | | [`FutureLedger`](#7-futureledger) | |
| 8 | | [`StaleLedger`](#8-staleledger) | |
| 9 | | [`FutureTimestamp`](#9-futuretimestamp) | |
| 10 | | [`StaleReport`](#10-stalereport) | |
| 11 | | [`InvalidUri`](#11-invaliduri) | |
| 12 | | [`ImplausibleAmount`](#12-implausibleamount) | |
| 13 | | [`TooManyReporters`](#13-toomanyreporters) | |
| 14 | | [`UpgradeNotProposed`](#14-upgradenotproposed) | |
| 15 | | [`UpgradeDelayNotElapsed`](#15-upgradedelaynotelapsed) | |
| 16 | | [`Paused`](#16-paused) | |

## RegistryError

Raised by `reporter-registry`. The role and upgrade errors are admin-only, so
they reach maintainers rather than end users; `InvalidMetadata` is raised to a
registered reporter updating their own entry.

### 1 `ReporterNotFound`

- **Raised by:** `revoke(reporter)`.
- **Cause:** `reporter` has no entry in the registry. Either it was never
  added or it has already been revoked.
- **Remedy:** Check `get_reporter(reporter)` first. If it returns `None`, the
  address already has no role and nothing needs revoking. Make sure you passed
  the reporter's address, not their name.

### 2 `InvalidName`

- **Raised by:** `set_reporter(reporter, role, name)`.
- **Cause:** `name` is empty or longer than 64 bytes. The limit is in UTF-8
  bytes, not characters, so a 30-character name in a non-Latin script can
  exceed it.
- **Remedy:** Use a short, non-empty display name of 1 to 64 bytes.

### 5 `InvalidMetadata`

- **Raised by:** `update_reporter_metadata(reporter, name, website_uri,
  contact_hash)`.
- **Cause:** `Some(website_uri)` is empty or longer than 256 bytes, or
  `Some(contact_hash)` is empty or longer than 64 bytes. A `None` field is not
  an error; it clears that field.
- **Remedy:** Shorten the URL or omit those fields. The `name` field is a
  `Symbol` and is already limited to 32 bytes by the host.

## LedgerError

Raised by `coverage-ledger`. Each write checks its errors in a fixed order, so
when several apply the caller sees the first:

| Function | Order checked |
| --- | --- |
| `list_asset` | `NotAStellarAsset` (not a SAC), `AlreadyListed`, `NotAStellarAsset` (SAC name has no issuer) |
| `post_supply` | auth, `Paused`, `NotSupplyPoster`, `AssetNotListed`, `InvalidAmount`, `FutureLedger`, `StaleLedger` |
| `post_reserve` | auth, `Paused`, `AssetNotListed`, `NotReporter`, `InvalidAmount`, `InvalidUri`, `FutureTimestamp`, `ImplausibleAmount`, `StaleReport`, `TooManyReporters` |
| `enable_native_supply` | auth, `AssetNotListed` |

### 1 `NotAStellarAsset`

- **Raised by:** `list_asset(sac)`.
- **Cause:** `sac` is not the built-in Stellar Asset Contract of an issued
  asset. Any of these:
  - The address is a Wasm contract, such as a custom SEP-41 token. Those are
    out of scope for v0.1 because they have no standard total-supply source.
  - The address is an account (`G...`), or a contract that does not exist.
  - The address is the SAC for native XLM, whose `name()` is `native`. XLM has
    no issuer and no reserves.
- **Remedy:** Pass the SAC address of a classic issued asset. Derive it with
  `stellar contract id asset --asset CODE:ISSUER --network <network>`, and
  deploy it with `stellar contract asset deploy` if nobody has yet.

### 2 `AlreadyListed`

- **Raised by:** `list_asset(sac)`.
- **Cause:** The asset is already listed. Listing is permissionless and
  happens once per asset.
- **Remedy:** Nothing is wrong. Treat it as success and read the existing
  record with `get_asset(sac)`.

### 3 `AssetNotListed`

- **Raised by:** `post_supply`, `post_reserve`.
- **Cause:** No asset record exists for `sac`.
- **Remedy:** Call `list_asset(sac)` (anyone can), then post again. Check that
  `sac` is the asset's SAC address and not the issuer's account address.

### 4 `NotSupplyPoster`

- **Raised by:** `post_supply`.
- **Cause:** `poster` does not hold the `SupplyPoster` role in the registry.
  It was never added, was revoked, or holds a different role. An auditor
  cannot post supply.
- **Remedy:** Sign with the registered supply-poster account. A maintainer can
  grant the role with `reporter-registry.set_reporter(poster, SupplyPoster, name)`.

### 5 `NotReporter`

- **Raised by:** `post_reserve`.
- **Cause:** `reporter` is not the asset's issuer account and holds neither
  the `Auditor` nor the `Transcriber` role. A `SupplyPoster`, a revoked
  reporter and an unknown address all land here.
- **Remedy:** Sign with the issuer account (gives the `IssuerSigned` tier), or
  ask a maintainer to register the address as an auditor or transcriber.
  Issuer-signed reports must come from the exact issuer account in
  `get_asset(sac).issuer`, not from a distribution account.

### 6 `InvalidAmount`

- **Raised by:** `post_supply`, `post_reserve`.
- **Cause:** `amount` is negative.
- **Remedy:** Post a non-negative figure in the asset's smallest unit. Zero is
  allowed for both: zero supply means nothing is circulating, and zero
  reserves is a statement the protocol exists to be able to carry.

### 7 `FutureLedger`

- **Raised by:** `post_supply`.
- **Cause:** `ledger` is greater than the ledger the transaction executes in.
- **Remedy:** Post the sequence number of the ledger the supply was actually
  computed at, which is in the past by the time the transaction lands.

### 8 `StaleLedger`

- **Raised by:** `post_supply`.
- **Cause:** `ledger` is not greater than the stored snapshot's `ledger`.
  Snapshots must move strictly forward. A retried submission whose first
  attempt actually succeeded also lands here.
- **Remedy:** Read `get_supply(sac)`. If its `ledger` is the one you were
  posting, your figure is already on chain. Otherwise recompute at a later
  ledger and post that.

### 9 `FutureTimestamp`

- **Raised by:** `post_reserve`.
- **Cause:** `as_of` is later than the ledger's close time. The ledger clock
  can lag wall-clock time by several seconds, so "now" from your machine can
  be in the ledger's future.
- **Remedy:** Set `as_of` to the date the reserve statement speaks for, which
  is in the past. If you mean "now", use the latest ledger's close time, not
  your local clock.

### 10 `StaleReport`

- **Raised by:** `post_reserve`.
- **Cause:** `as_of` is not later than the `as_of` of this reporter's stored
  report at this tier. Staleness is per reporter: another reporter's report,
  however recent, never causes it. A retried submission whose first attempt
  succeeded also lands here.
- **Remedy:** Check `get_reports(sac)` for your existing report. If it is
  there, your figure is already on chain. Restating needs a later `as_of`
  than the one you posted before.

### 11 `InvalidUri`

- **Raised by:** `post_reserve`.
- **Cause:** `doc_uri` is empty or longer than 256 bytes.
- **Remedy:** Point at the source document with a URI of 1 to 256 bytes. Use a
  stable, shorter link, or content-addressed storage such as `ipfs://...`. The
  `doc_hash` is what proves the content, so the URI only has to find it.

### 12 `ImplausibleAmount`

- **Raised by:** `post_reserve`.
- **Cause:** The figure would put coverage outside 1 bps (0.01%) to
  1,000,000 bps (10,000%) of the stored supply. The usual reason is a unit
  slip: amounts are in the smallest unit, 7 decimals for classic assets, so a
  figure typed in whole units is 10,000,000 times too small. The check is
  skipped when `amount` is zero, when there is no supply snapshot yet, and
  when the stored supply is zero.
- **Remedy:** Multiply whole units by `10_000_000` (for 7 decimals) and post
  again. If the figure really is correct, compare it with `get_supply(sac)`:
  a wrong supply snapshot needs correcting by the supply poster, since the
  check judges the report against it.

### 13 `TooManyReporters`

- **Raised by:** `post_reserve`.
- **Cause:** The asset already holds reports from
  `MAX_REPORTERS_PER_TIER = 4` reporters at this tier, every one of them still
  counts, and this reporter is not one of them. Before failing, the ledger
  frees the slots of reporters who were revoked or changed role, so this
  error means all four are active.
- **Remedy:** Reporters already holding a slot can keep posting. A new
  reporter needs a maintainer to revoke one of the four with
  `reporter-registry.revoke`. Their slot is reclaimed by the next
  `post_reserve` at that tier.

### 14 `UpgradeNotProposed`

- **Raised by:** `execute_upgrade`, `cancel_upgrade`.
- **Cause:** Nothing is awaiting execution. The proposal was never made, was
  already executed, or was cancelled. A new proposal also replaces the old
  one, so only the latest hash is ever pending.
- **Remedy:** Read `pending_upgrade()`. If it is `None`, call
  `propose_upgrade(wasm_hash)` and wait `UPGRADE_DELAY_LEDGERS` before
  executing.

### 15 `UpgradeDelayNotElapsed`

- **Raised by:** `execute_upgrade`.
- **Cause:** The pending proposal's `executable_at_ledger` has not been
  reached yet. The delay is `UPGRADE_DELAY_LEDGERS` ledgers from the proposal.
- **Remedy:** Wait until the current ledger sequence is at least
  `pending_upgrade().executable_at_ledger`, then execute. Watching
  `upgrade_proposed` is the intended signal to raise an alarm before that
  ledger closes.

### 16 `Paused`

- **Raised by:** `post_supply`, `post_reserve`, `is_covered`.
- **Cause:** An admin called `pause()` and has not called `unpause()` yet.
  Pausing stops new figures from being recorded and stops `is_covered` from
  answering, so a consumer never reads a pause as an under-backed asset.
- **Remedy:** Nothing is wrong with the caller's request. Wait for the admin
  to `unpause()`, or read `is_paused()` and `coverage_status(...)` first and
  show the paused state in the UI. `coverage`, `get_supply`, `get_reports`
  and `coverage_status` keep working while paused.

## VaultError

Raised by `covered-vault`, the reference consumer. These are the errors end
users meet most.

### 1 `InvalidAmount`

- **Raised by:** `deposit`, `withdraw`.
- **Cause:** `amount` is zero or negative. On `deposit` it is also raised if
  the new balance would overflow `i128`, which no real asset can reach.
- **Remedy:** Send a positive amount in the asset's smallest unit.

### 2 `NotCovered`

- **Raised by:** `deposit`.
- **Cause:** `coverage-ledger.is_covered(asset, min_bps, max_age, min_tier)`
  answered `false` with the vault's stored config. This error covers all of
  the following, which the vault does not tell apart:
  - No supply snapshot exists, or no report counts at `min_tier` or above.
  - The supply snapshot was posted more than `max_age` seconds ago.
  - The chosen report's `as_of` is more than `max_age` seconds ago.
  - Coverage is below `min_bps`.
- **Remedy:** Users should wait. The asset will be accepted again once fresh
  figures show it is covered, and withdrawals are never blocked. A paused
  ledger raises `Paused` from `deposit` instead of `NotCovered`, so check
  `coverage-ledger.is_paused()` to tell the two apart. Front ends should call
  `can_deposit()` before offering a deposit, and to explain the
  refusal, read `config()` and then
  `coverage-ledger.coverage(asset, min_tier)`:
  - `None` means a figure is missing.
  - `bps < min_bps` means the asset is under-backed.
  - `now - supply_timestamp > max_age` or `now - report_as_of > max_age`
    means the figures are out of date.

### 3 `InsufficientBalance`

- **Raised by:** `withdraw`.
- **Cause:** `amount` is more than the caller's balance in the vault.
- **Remedy:** Read `balance(of)` and withdraw at most that.

## Errors from the asset contract

`covered-vault` moves tokens with the asset's SAC, so these SAC codes can
surface from `deposit` and `withdraw`. They come from the Soroban host's
built-in token. The ones a vault user can hit:

| N | SAC error | When | What to do |
| ---: | --- | --- | --- |
| 10 | `BalanceError` | `deposit` of more than the wallet holds | Deposit at most the wallet's token balance. |
| 11 | `BalanceDeauthorizedError` | The issuer has not authorized, or has frozen, the account's trustline | Contact the issuer; the trustline must be authorized. |
| 13 | `TrustlineMissingError` | The account has no trustline to the asset (`deposit` from, or `withdraw` to, it) | Add a trustline to the asset first. |

The end-to-end test asserts that an oversized deposit fails with `#10` from
the SAC.

## Errors that are not contract errors

These carry no Plimsoll code. They come from the host and look the same for
every contract:

- **`Error(Auth, InvalidAction)`**: the address whose authority the call uses
  did not sign. That is the admin for registry writes, `transfer_admin` and
  `upgrade`; the poster or reporter for ledger posts; `from` or `to` for vault
  calls.
- **`Error(Budget, ExceededLimit)`**: the transaction ran out of CPU or
  memory. Re-simulate to get current resource figures.
- **A footprint or archived-entry failure**: the transaction was built without
  restoring an archived entry it needs. Re-simulate and submit what
  simulation returns. See [SPEC.md](SPEC.md#archival-behaviour).
