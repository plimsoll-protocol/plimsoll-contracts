# Authorization model

Plimsoll has no single super-user. Authority is an address, checked either by
`require_auth` or by the reporter registry, and each contract has its own
admin. This page maps every public entry point to the role that may call it and
says what happens when someone else tries. The normative details are in
[SPEC.md](SPEC.md); error codes are in [ERRORS.md](ERRORS.md), event layouts in
[EVENTS.md](EVENTS.md).

## Roles

| Role | How it is held | What it can do |
| --- | --- | --- |
| **Admin** | `Admin` in each contract's instance storage, set by the constructor. There is one admin per contract; the registry admin is not the ledger admin. | The admin-only writes on that contract: onboarding and revoking reporters, transferring admin, and the timelocked upgrade functions. |
| **Auditor** | `Role::Auditor` (`1`) in `reporter-registry`. | Post a reserve report, which lands at `Tier::AuditorSigned`; update their own metadata; every read. |
| **Transcriber** | `Role::Transcriber` (`2`) in `reporter-registry`. | Post a reserve report, which lands at `Tier::Transcribed`; update their own metadata; every read. |
| **Supply poster** | `Role::SupplyPoster` (`3`) in `reporter-registry`. | Post circulating supply for a listed asset; update their own metadata; every read. |
| **Asset issuer** | The issuer account named in the asset's Stellar Asset Contract. No registration is needed. | Post reserve reports for its own listed asset at `Tier::IssuerSigned`; every read. |
| **Watcher** | Not an on-chain role. Anyone reading events and state. | Every read, including `pending_upgrade`, plus events. Watchers cannot write anything. |
| **Public / holder** | Any Stellar account. | Every read; `list_asset` (permissionless); `deposit` and `withdraw` in the example vault, with their own funds. |

A single address can hold several roles: an auditor can also be the issuer of
an asset, and an admin can also be a registered reporter. Each call is checked
against the role that call needs.

## Entry points

Every public function in the three contracts is listed. "Authorised caller" is
who must sign; reads need no signature.

### reporter-registry

| Entry point | Authorised caller | Unauthorised result |
| --- | --- | --- |
| `admin()` | anyone (read) | — |
| `get_reporter(reporter)` | anyone (read) | — |
| `get_reporter_metadata(reporter)` | anyone (read) | — |
| `stake_of(reporter)` | anyone (read) | — |
| `role_of(reporter)` | anyone (read) | — |
| `pending_upgrade()` | anyone (read) | — |
| `set_reporter(reporter, role, name)` | the registry admin | Host auth failure |
| `revoke(reporter)` | the registry admin | Host auth failure |
| `top_up_stake(reporter, amount)` | the registry admin | Host auth failure; `ReporterNotFound` / `InvalidStakeAmount` after a valid signature |
| `reduce_stake(reporter, amount)` | the registry admin | Host auth failure; `ReporterNotFound` / `InvalidStakeAmount` / `InsufficientStake` after a valid signature |
| `update_reporter_metadata(reporter, name, website_uri, contact_hash)` | `reporter` itself, and it must be registered | Host auth failure; `ReporterNotFound` if the signature is valid but the address is not registered |
| `transfer_admin(new_admin)` | the current admin **and** `new_admin` | Host auth failure unless both sign |
| `propose_upgrade(wasm_hash)` | the registry admin | Host auth failure |
| `execute_upgrade()` | the registry admin | Host auth failure; `UpgradeNotProposed`, `UpgradeDelayNotElapsed` after a valid signature |
| `cancel_upgrade()` | the registry admin | Host auth failure; `UpgradeNotProposed` after a valid signature |

### coverage-ledger

| Entry point | Authorised caller | Unauthorised result |
| --- | --- | --- |
| `admin()`, `registry()` | anyone (read) | — |
| `get_asset(sac)`, `get_supply(sac)` | anyone (read) | — |
| `get_report(sac, tier)`, `get_reports(sac)` | anyone (read) | — |
| `coverage(sac, min_tier)`, `is_covered(sac, min_bps, max_age, min_tier)` | anyone (read) | — |
| `pending_upgrade()` | anyone (read) | — |
| `list_asset(sac)` | anyone | Only `NotAStellarAsset` / `AlreadyListed`; listing is permissionless |
| `post_supply(poster, sac, amount, ledger, breakdown_hash)` | a registered **supply poster**, signing as `poster` | Host auth failure, or `NotSupplyPoster` (`4`) when a different address signed |
| `post_reserve(reporter, sac, amount, as_of, doc_hash, doc_uri)` | the asset's **issuer**, or a registered **auditor** or **transcriber**, signing as `reporter` | Host auth failure, or `NotReporter` (`5`) when a different address signed |
| `transfer_admin(new_admin)` | the current ledger admin **and** `new_admin` | Host auth failure unless both sign |
| `propose_upgrade(wasm_hash)`, `execute_upgrade()`, `cancel_upgrade()` | the ledger admin | Host auth failure; the same upgrade errors as the registry after a valid signature |

### covered-vault (example consumer)

| Entry point | Authorised caller | Unauthorised result |
| --- | --- | --- |
| `balance(of)`, `config()`, `can_deposit()` | anyone (read) | — |
| `deposit(from, amount)` | `from`, with its own funds | Host auth failure; `NotCovered` (`2`) when the asset is not covered under the vault's policy; `InvalidAmount` for a non-positive amount |
| `request_withdraw(to, amount)` | `to`, for its own balance | Host auth failure; `InsufficientBalance` (`3`) when the balance is too small; `InvalidAmount` for a non-positive amount |
| `execute_withdraw(to)` | `to`, for its own queued withdrawal | Host auth failure; `NoPendingWithdrawal` (`5`) or `WithdrawalDelayNotElapsed` (`6`) |
| `set_withdrawal_delay(delay_seconds)` | vault deployer/admin | Host auth failure; `InvalidDelay` (`4`) if the timestamp would overflow |

`__constructor` is called once by the deployer when the contract is deployed
and is not callable afterwards. The deployer supplies the vault admin address
as a constructor argument.

## What "unauthorised" looks like

There are two different failures, and they are worth telling apart when
debugging a transaction.

**Host auth failure.** `Address::require_auth()` is checked by the Soroban
host before the function body runs. A transaction that is missing a signature,
or carries one for the wrong address, fails with a host `Error(Auth, …)` — it
does not return a `RegistryError` or `LedgerError` variant, and the contract
code never executes. Nothing is written and no event is published. When
simulating, the RPC returns an error such as `Error(Auth, InvalidAction)`.

**Contract role error.** When the signature is valid but the role is not, the
contract body runs and raises its own error. `post_supply` returns
`NotSupplyPoster` (`4`), and `post_reserve` returns `NotReporter` (`5`). A
reporter whose role was revoked gets `NotReporter`; reports they posted before
revocation stay in storage but coverage queries skip them until a later
`set_reporter` restores the role. See
[ERRORS.md](ERRORS.md#ledgererror) for the exact order in which each write
checks its errors.

**Timelocked upgrades.** Admin is enough to *propose* new code, not to install
it. `execute_upgrade` fails with `UpgradeDelayNotElapsed` until
`UPGRADE_DELAY_LEDGERS` have passed, which gives watchers a window to see the
proposal and act. See [SPEC.md](SPEC.md#time-locked-upgrades).

**No admin can rewrite history.** Neither admin can post reserve figures or
supply snapshots on someone else's behalf: those calls require the reporter's
or poster's own signature. The admin's power over data is limited to granting
and revoking roles, which changes how *future* coverage queries weigh reports.
