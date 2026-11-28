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

- `covered-vault.deposit` and `execute_withdraw` move tokens through the asset's
  Stellar Asset Contract (SAC). If the transfer fails, the caller sees the
  **SAC's** error code, not a vault code. See
  [Errors from the asset contract](#errors-from-the-asset-contract).
- `coverage-ledger` calls `reporter-registry.role_of` and
  `has_minimum_stake`, and the SAC's `name` and `balance`. On a correctly
  configured deployment these calls succeed; an insufficient balance returns
  the ledger's `InsufficientStake` error.
- `coverage-ledger.is_covered` and `coverage` never fail. Every reason an
  asset might not qualify comes back as `false` or `None`.

- `coverage-ledger` calls `reporter-registry.role_of` and the SAC's `name`,
  but neither can fail on a valid deployment, so a ledger call only ever
  fails with a ledger code.
- `coverage-ledger` calls `reporter-registry.role_of`,
  `reporter-registry.max_reporters` and the SAC's `name`, but none of them can
  fail on a valid deployment, so a ledger call only ever fails with a ledger
  code. (A ledger upgraded ahead of its registry is not a valid deployment:
  see [SPEC.md](SPEC.md#upgrading-a-v01-deployment).)
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

[AGENTS.md](../AGENTS.md) makes this a rule for every change. The end-to-end
test in [tests/e2e](../tests/e2e/tests/scenario.rs) asserts several codes as
literal numbers, and CI runs [scripts/check_error_codes.py](../scripts/check_error_codes.py),
which compares every `#[contracterror]` enum with the base branch and fails on a
renumbering, a removal or a new variant that is not appended after the last
one. Run it yourself before pushing:

```bash
python3 scripts/check_error_codes.py --base origin/main
```

### Every code has a test

`cargo test` covers behaviour; a second check covers the list itself.
[`scripts/check_error_coverage.py`](../scripts/check_error_coverage.py) (the
**Error-code coverage** step in CI, or `python3
scripts/check_error_coverage.py` locally) reads every `#[contracterror]` enum
under `contracts/` and `examples/` and fails if some variant is named by no
test file in its package. It prints a per-enum covered count and every
uncovered `Type::Variant` with its number, so the failing output is the to-do
list. It also fails on a duplicate error number in one enum, because the
number is what the indexer decodes.

A code that genuinely cannot be reached from a test is listed with its reason
in `INTENTIONALLY_UNTESTED` at the top of that script. The check fails when a
listed entry has gone stale (the variant is gone, or a test now covers it), so
the list cannot be used to paper over a missing test. There are no such
entries today: every code below is covered.

## Quick lookup

| N | reporter-registry | coverage-ledger | covered-vault |
| ---: | --- | --- | --- |
| 1 | [`ReporterNotFound`](#1-reporternotfound) | [`NotAStellarAsset`](#1-notastellarasset) | [`InvalidAmount`](#1-invalidamount) |
| 2 | [`InvalidName`](#2-invalidname) | [`AlreadyListed`](#2-alreadylisted) | [`NotCovered`](#2-notcovered) |
| 3 | | [`AssetNotListed`](#3-assetnotlisted) | [`InsufficientBalance`](#3-insufficientbalance) |
| 4 | | | [`InvalidThreshold`](#4-invalidthreshold) |
| 4 | | [`NotSupplyPoster`](#4-notsupplyposter) | |
| 5 | [`InsufficientStake`](#5-insufficientstake) | [`NotReporter`](#5-notreporter) | |
| 6 | [`InvalidStakeAmount`](#6-invalidstakeamount) | [`InvalidAmount`](#6-invalidamount) | |
| 7 | [`ReporterAlreadyRegistered`](#7-reporteralreadyregistered) | [`FutureLedger`](#7-futureledger) | |

| 3 | `UpgradeNotProposed` | [`AssetNotListed`](#3-assetnotlisted) | [`InsufficientBalance`](#3-insufficientbalance) |
| 4 | `UpgradeDelayNotElapsed` | [`NotSupplyPoster`](#4-notsupplyposter) | [`NoPendingAdmin`](#4-nopendingadmin) |
| 5 | [`InvalidMetadata`](#5-invalidmetadata) | [`NotReporter`](#5-notreporter) | |
| 6 | [`InvalidStakeAmount`](#6-invalidstakeamount) | [`InvalidAmount`](#6-invalidamount) | |
| 7 | [`InsufficientStake`](#7-insufficientstake) | [`FutureLedger`](#7-futureledger) | |
| 8 | [`InvalidMaxReporters`](#8-invalidmaxreporters) | [`StaleLedger`](#8-staleledger) | |
| 9 | [`EmptyBatch`](#9-emptybatch) | [`FutureTimestamp`](#9-futuretimestamp) | |
| 6 | [`InvalidConfirmWindow`](#6-invalidconfirmwindow) | [`InvalidAmount`](#6-invalidamount) | |
| 7 | [`PendingRegistrationNotFound`](#7-pendingregistrationnotfound) | [`FutureLedger`](#7-futureledger) | |
| 8 | [`RegistrationExpired`](#8-registrationexpired) | [`StaleLedger`](#8-staleledger) | |
| 4 | `UpgradeDelayNotElapsed` | [`NotSupplyPoster`](#4-notsupplyposter) | [`InvalidDelay`](#4-invaliddelay) |
| 5 | [`InvalidMetadata`](#5-invalidmetadata) | [`NotReporter`](#5-notreporter) | [`NoPendingWithdrawal`](#5-nopendingwithdrawal) |
| 6 | | [`InvalidAmount`](#6-invalidamount) | [`WithdrawalDelayNotElapsed`](#6-withdrawaldelaynotelapsed) |
| 7 | | [`FutureLedger`](#7-futureledger) | |
| 8 | | [`StaleLedger`](#8-staleledger) | |
| 6 | [`InvalidConfirmWindow`](#6-invalidconfirmwindow) | [`InvalidAmount`](#6-invalidamount) | |
| 7 | [`PendingRegistrationNotFound`](#7-pendingregistrationnotfound) | [`FutureLedger`](#7-futureledger) | |
| 8 | [`RegistrationExpired`](#8-registrationexpired) | [`StaleLedger`](#8-staleledger) | |
| 9 | | [`FutureTimestamp`](#9-futuretimestamp) | |
| 10 | | [`StaleReport`](#10-stalereport) | |
| 11 | | [`InvalidUri`](#11-invaliduri) | |
| 12 | | [`ImplausibleAmount`](#12-implausibleamount) | |
| 13 | | [`TooManyReporters`](#13-toomanyreporters) | |
| 16 | | [`InsufficientStake`](#16-insufficientstake) | |

| 14 | | [`UpgradeNotProposed`](#14-upgradenotproposed) | |
| 15 | | [`UpgradeDelayNotElapsed`](#15-upgradedelaynotelapsed) | |
| 16 | | [`SelfAlias`](#16-selfalias) | |
| 17 | | [`Paused`](#17-paused) | |
| 18 | | [`LedgerTooOld`](#18-ledgertooold) | |
| 19 | | [`NoReportToRetract`](#19-noreporttoretract) | |
| 20 | | [`TooManyReportersPerAsset`](#20-toomanyreportersperasset) | |
| 20 | | [`UriTooShort`](#20-uritoshort) | |
| 21 | | [`InvalidHash`](#21-invalidhash) | |

## RegistryError

Raised by `reporter-registry`. These errors can be returned by admin
configuration or reporter self-registration.

Raised by `reporter-registry`. The role and upgrade errors are admin-only, so
they reach maintainers rather than end users; `InvalidMetadata` is raised to a
registered reporter updating their own entry.
Raised by `reporter-registry`. The role, upgrade, stake, cap and batch errors
are admin-only, so they reach maintainers rather than end users;
`InvalidMetadata` is raised to a registered reporter updating their own entry.

### 1 `ReporterNotFound`

- **Raised by:** `revoke(reporter)`.
- **Cause:** `reporter` has no entry in the registry. Either it was never
  added or it has already been revoked.
- **Remedy:** Check `get_reporter(reporter)` first. If it returns `None`, the
  address already has no role and nothing needs revoking. Make sure you passed
  the reporter's address, not their name.

### 2 `InvalidName`

- **Raised by:** `set_reporter(reporter, role, name)` and
  `register_reporter(reporter, name)`.
- **Raised by:** `set_reporter(reporter, role, name)`,
  `batch_register(reporters)`.
- **Cause:** `name` is empty or longer than 64 bytes. The limit is in UTF-8
  bytes, not characters, so a 30-character name in a non-Latin script can
  exceed it.
- **Remedy:** Use a short, non-empty display name of 1 to 64 bytes. In a batch
  one bad name fails the whole call and nothing is written; the entry that
  caused it is the one to fix.

### 5 `InsufficientStake`

- **Raised by:** `register_reporter(reporter, name)`.
- **Cause:** The reporter's current balance of the configured native XLM SAC
  is below `min_stake_stroops`.
- **Remedy:** Fund the reporter with the required XLM balance, then retry.

### 6 `InvalidStakeAmount`

- **Raised by:** `set_stake_requirement(stake_asset, min_stake_stroops)`.
- **Cause:** The configured minimum is negative.
- **Remedy:** Set a non-negative number of stroops.

### 7 `ReporterAlreadyRegistered`

- **Raised by:** `register_reporter(reporter, name)`.
- **Cause:** The address already has a role. Self-registration only creates a
  new supply-poster entry and cannot replace an admin-managed role.
- **Remedy:** Continue using the existing role, or ask the registry admin to
  change it with `set_reporter`.

### 5 `InsufficientStake`

- **Raised by:** `register_reporter(reporter, name)`.
- **Cause:** The reporter's current balance of the configured native XLM SAC
  is below `min_stake_stroops`.
- **Remedy:** Fund the reporter with the required XLM balance, then retry.

### 6 `InvalidStakeAmount`

- **Raised by:** `set_stake_requirement(stake_asset, min_stake_stroops)`.
- **Cause:** The configured minimum is negative.
- **Remedy:** Set a non-negative number of stroops.

### 7 `ReporterAlreadyRegistered`

- **Raised by:** `register_reporter(reporter, name)`.
- **Cause:** The address already has a role. Self-registration only creates a
  new supply-poster entry and cannot replace an admin-managed role.
- **Remedy:** Continue using the existing role, or ask the registry admin to
  change it with `set_reporter`.

### 5 `InvalidMetadata`

- **Raised by:** `update_reporter_metadata(reporter, name, website_uri,
  contact_hash)`.
- **Cause:** `Some(website_uri)` is empty or longer than 256 bytes, or
  `Some(contact_hash)` is empty or longer than 64 bytes. A `None` field is not
  an error; it clears that field.
- **Remedy:** Shorten the URL or omit those fields. The `name` field is a
  `Symbol` and is already limited to 32 bytes by the host.

### 6 `InvalidConfirmWindow`

- **Raised by:** `register_reporter(reporter, role, name, confirm_window_ledgers)`.
- **Cause:** `confirm_window_ledgers` is `0`.
- **Remedy:** Pass a positive confirmation window in ledgers.

### 7 `PendingRegistrationNotFound`

- **Raised by:** `confirm_registration(reporter)`.
- **Cause:** No pending registration exists for `reporter`.
- **Remedy:** Have the admin call `register_reporter` first.

### 8 `RegistrationExpired`

- **Raised by:** `confirm_registration(reporter)`.
- **Cause:** The current ledger sequence is past `expires_at_ledger`.
- **Remedy:** Clean up with `cleanup_expired_registration` and have the admin
  re-register the reporter.
### 6 `InvalidStakeAmount`

- **Raised by:** `top_up_stake(reporter, amount)`, `reduce_stake(reporter, amount)`.
- **Cause:** `amount` is zero or negative, or adding `amount` to the existing
  stake would overflow `i128`.
- **Remedy:** Pass a strictly positive `amount` within `i128` bounds.

### 7 `InsufficientStake`

- **Raised by:** `reduce_stake(reporter, amount)`.
- **Cause:** `amount` exceeds the reporter's currently tracked stake (`stake_of`).
- **Remedy:** Query `stake_of(reporter)` and reduce by at most that amount.

### 8 `InvalidMaxReporters`

- **Raised by:** `set_max_reporters(max)`.
- **Cause:** `max` is zero, which would stop every asset taking a new
  reporter.
- **Remedy:** Set at least 1. The default is 20, above what the per-tier caps
  allow, so a value only changes anything when the admin lowers it.

### 9 `EmptyBatch`

- **Raised by:** `batch_register(reporters)`.
- **Cause:** the list is empty, so the call would do nothing. It is refused
  rather than succeeding silently, because an empty list is usually a bug in
  the caller that built it.
- **Remedy:** Pass at least one entry. A single reporter can also go through
  `set_reporter`.

## LedgerError

Raised by `coverage-ledger`. Each write checks its errors in a fixed order, so
when several apply the caller sees the first:

| Function | Order checked |
| --- | --- |
| `list_asset` | `NotAStellarAsset` (not a SAC), `AlreadyListed`, `NotAStellarAsset` (SAC name has no issuer) |
| `post_supply` | auth, `NotSupplyPoster`, `AssetNotListed`, `InsufficientStake`, `InvalidAmount`, `FutureLedger`, `StaleLedger` |
| `post_reserve` | auth, `AssetNotListed`, `NotReporter`, `InvalidAmount`, `InvalidUri`, `FutureTimestamp`, `ImplausibleAmount`, `StaleReport`, `TooManyReporters` |

| `post_supply` | auth, `Paused`, `NotSupplyPoster`, `AssetNotListed`, `InvalidAmount`, `FutureLedger`, `StaleLedger` |
| `post_supply` | auth, `Paused`, `NotSupplyPoster`, `AssetNotListed`, `InvalidAmount`, `InvalidHash`, `FutureLedger`, `LedgerTooOld`, `StaleLedger` |
| `post_reserve` | auth, `Paused`, `AssetNotListed`, `NotReporter`, `InvalidAmount`, `InvalidHash`, `InvalidUri`, `FutureTimestamp`, `ImplausibleAmount`, `StaleReport`, `TooManyReporters`, `TooManyReportersPerAsset` |
| `is_covered_at` | `FutureLedger`, `Paused` |
| `get_coverage_snapshot` | `FutureLedger` |
| `post_reserve` | auth, `Paused`, `AssetNotListed`, `NotReporter`, `InvalidAmount`, `InvalidHash`, `InvalidUri`, `UriTooShort`, `FutureTimestamp`, `ImplausibleAmount`, `StaleReport`, `TooManyReporters` |
| `set_min_doc_uri_len` | auth, `InvalidUri` (above `MAX_URI_LEN`) |
| `retract_reserve` | auth, `AssetNotListed`, `NotReporter`, `NoReportToRetract` |
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
  grant the role with `reporter-registry.set_reporter(poster, SupplyPoster, name)`;
  the poster must also keep the configured minimum XLM balance.

### 16 `InsufficientStake`

- **Raised by:** `post_supply`.
- **Cause:** The poster is registered as a supply poster but its current XLM
  balance is below the minimum configured in reporter-registry.
- **Remedy:** Restore the required XLM balance and retry. The check is live on
  every submission; registering once does not lock or custody the balance.

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

- **Raised by:** `post_supply`, `is_covered_at`.
- **Cause:** A supplied sequence (`ledger` or `as_of_ledger`) is greater than
  the ledger the transaction executes in.
- **Remedy:** For `post_supply`, use the sequence number the supply was
  computed at. For `is_covered_at`, query a ledger no later than the one the
  transaction executes in.

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

- **Raised by:** `post_reserve`, `set_min_doc_uri_len`.
- **Cause:** `doc_uri` is empty or longer than 256 bytes.
- **Remedy:** Point at the source document with a URI of 1 to 256 bytes. Use a
  stable, shorter link, or content-addressed storage such as `ipfs://...`. The
  `doc_hash` is what proves the content, so the URI only has to find it.
  A non-empty URI below the configured minimum gets `UriTooShort` instead; see
  below.

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

### 17 `Paused`

- **Raised by:** `post_supply`, `post_reserve`, `is_covered`.
- **Cause:** An admin called `pause()` and has not called `unpause()` yet.
  Pausing stops new figures from being recorded and stops `is_covered` from
  answering, so a consumer never reads a pause as an under-backed asset.
- **Remedy:** Nothing is wrong with the caller's request. Wait for the admin
  to `unpause()`, or read `is_paused()` and `coverage_status(...)` first and
  show the paused state in the UI. `coverage`, `get_supply`, `get_reports`
  and `coverage_status` keep working while paused.

### 18 `LedgerTooOld`

- **Raised by:** `post_supply(poster, sac, amount, ledger, breakdown_hash)`.
- **Cause:** `ledger` is more than 720 ledgers behind the current ledger
  (approximately one hour at Stellar's usual five-second ledger cadence).
  `post_supply` stores the submission time, so without this sequence bound an
  old figure could be presented as a fresh snapshot.
- **Remedy:** Recompute supply from a recent ledger and post its sequence.
  Exactly 720 ledgers of lag is accepted.

### 19 `NoReportToRetract`

- **Raised by:** `retract_reserve(reporter, sac)`.
- **Cause:** The authorised reporter has no stored report for the tier their
  identity currently resolves to. This includes signing with a different
  reporter address, even when another reporter has a report at that tier.
- **Remedy:** Sign with the address that posted the report, and confirm the
  report still exists at the reporter's current tier.

### 20 `TooManyReportersPerAsset`

- **Raised by:** `post_reserve`.
- **Cause:** the asset already holds reports from
  `reporter-registry.max_reporters()` distinct reporters, and this reporter
  has no report for it at any tier. The default is 20. Before failing, the
  ledger drops the entries of reporters who no longer count (revoked, or
  moved to another role) at every tier, so this error means the cap is full
  of reporters that still count. The asset's issuer occupies a slot like
  anyone else, and a v0.1 report counts until its tier is migrated.
- **Remedy:** A reporter already speaking for the asset can keep posting new
  figures. A new reporter needs a maintainer to revoke one of the reporters
  holding a slot, or to raise the cap with
  `reporter-registry.set_max_reporters(n)`. With the default cap this error is
  unreachable, because the per-tier caps allow at most twelve entries per
  asset and `TooManyReporters` is checked first.
### 20 `UriTooShort`

- **Raised by:** `post_reserve`.
- **Cause:** `doc_uri` is non-empty but shorter than the configured minimum,
  `min_doc_uri_len()`, which is 10 bytes until an admin changes it. It exists
  to refuse placeholders such as `TODO` that would otherwise look like a
  source document.
- **Remedy:** Post the real location of the document. If the scheme genuinely
  needs fewer bytes, ask the maintainer to lower the floor with
  `set_min_doc_uri_len` (`0` disables it, and an empty URI is then still
  `InvalidUri`). `doc_hash` is what proves the content, so a stable, short
  link is fine.

### 21 `InvalidHash`

- **Raised by:** `post_supply`, `post_reserve`.
- **Cause:** `breakdown_hash` or `doc_hash` is all zeros (`[0u8; 32]`), which
  indicates an uncomputed hash and produces an unverifiable record.
- **Remedy:** Compute the SHA-256 digest of the off-chain breakdown or
  attestation document and pass the non-zero 32-byte hash.

## VaultError

Raised by `covered-vault`, the reference consumer. These are the errors end
users meet most.

### 1 `InvalidAmount`

- **Raised by:** `deposit`, `request_withdraw`.
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

- **Raised by:** `request_withdraw`, `execute_withdraw`.
- **Cause:** `amount` is more than the caller's balance in the vault.
- **Remedy:** Read `balance(of)` and withdraw at most that.

### 4 `InvalidThreshold`

- **Raised by:** the vault constructor.
- **Cause:** `large_withdrawal_threshold` is negative.
- **Remedy:** Configure a non-negative amount in the asset's smallest unit.
### 4 `NoPendingAdmin`

- **Raised by:** `accept_admin`.
- **Cause:** No admin transfer has been proposed with `propose_admin`.
- **Remedy:** Have the current vault admin call `propose_admin(new_admin)` first.
### 4 `InvalidDelay`

- **Raised by:** `set_withdrawal_delay`, `request_withdraw`.
- **Cause:** Adding the configured delay to the current ledger timestamp
  overflows `u64`.
- **Remedy:** Configure a delay that fits in the remaining timestamp range.

### 5 `NoPendingWithdrawal`

- **Raised by:** `execute_withdraw`.
- **Cause:** The caller has no queued withdrawal.
- **Remedy:** Call `request_withdraw` and execute it after its available time.

### 6 `WithdrawalDelayNotElapsed`

- **Raised by:** `execute_withdraw`.
- **Cause:** The queued withdrawal's `available_at` is later than the current
  ledger timestamp.
- **Remedy:** Retry at or after `available_at`.

## Errors from the asset contract

`covered-vault` moves tokens with the asset's SAC, so these SAC codes can
surface from `deposit` and `execute_withdraw`. They come from the Soroban host's
built-in token. The ones a vault user can hit:

| N | SAC error | When | What to do |
| ---: | --- | --- | --- |
| 10 | `BalanceError` | `deposit` of more than the wallet holds | Deposit at most the wallet's token balance. |
| 11 | `BalanceDeauthorizedError` | The issuer has not authorized, or has frozen, the account's trustline | Contact the issuer; the trustline must be authorized. |
| 13 | `TrustlineMissingError` | The account has no trustline to the asset (`deposit` from, or `execute_withdraw` to, it) | Add a trustline to the asset first. |

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
