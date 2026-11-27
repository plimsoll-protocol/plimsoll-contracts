# Threat model

Plimsoll records who said what about an asset's backing and answers
`is_covered(asset, min_bps, max_age, min_tier)`. It does not check that any
figure is true. This document is for anyone deciding whether to rely on that
answer. For each actor who can write to the contracts, it says what they could
make the ledger say that is false, what stops them today, and what you can do
about it.

Function names, auth rules and error codes are defined in [SPEC.md](SPEC.md).
Gaps that have an issue link to it. Where there is no issue yet, the gap says
so.

## Contents

- [Summary](#summary)
- [How the answer is assembled](#how-the-answer-is-assembled)
- [Registry admin](#registry-admin)
- [Ledger admin](#ledger-admin)
- [Supply poster](#supply-poster)
- [Auditor](#auditor)
- [Transcriber](#transcriber)
- [Issuer](#issuer)
- [Arbitrary caller](#arbitrary-caller)
- [Known gaps](#known-gaps)
- [What an integrator should do](#what-an-integrator-should-do)

## Summary

| Actor | Can call | Worst false statement they can make the ledger carry |
| --- | --- | --- |
| Registry admin | `set_reporter`, `revoke`, `transfer_admin`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` on reporter-registry | Any supply and any auditor or transcriber figure, through reporters it appoints; after the upgrade delay, any registry behaviour at all |
| Ledger admin | `transfer_admin`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` on coverage-ledger | Nothing directly; after the upgrade delay, any ledger behaviour, including any `is_covered` answer |
| Supply poster | `post_supply` | Any circulating supply for any listed asset, including `0`, which makes the asset read as covered |
| Auditor | `post_reserve` (tier 3) | Any reserve figure within the plausibility band, for any listed asset, dated "now" |
| Transcriber | `post_reserve` (tier 1) | The same at tier 1, which outranks older auditor figures when `min_tier` is 1 |
| Issuer | `post_reserve` (tier 2) for its own assets | Any reserve figure within the band for its own asset; the registry cannot revoke it |
| Arbitrary caller | `list_asset`, every view, `deposit` / `request_withdraw` / `execute_withdraw` on a vault for themselves | Nothing about coverage. They can list a look-alike asset under their own issuer |

## How the answer is assembled

`is_covered` is true when all of these hold. The last column shows who controls
each input.

| Input | Where it comes from | Who controls it |
| --- | --- | --- |
| Circulating supply | Latest `post_supply` for the asset | Any address with the `SupplyPoster` role, appointed by the registry admin |
| Supply freshness | The ledger timestamp when the snapshot was **posted** | The supply poster, by choosing when to post (see [#24](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/24)) |
| Reserve figure | Among reports at a tier `>= min_tier` that still count, the one with the latest `as_of` (ties go to the higher tier) | Whoever signed that report |
| Reserve freshness | That report's `as_of` | The signer, who may pick any `as_of` up to the current ledger time that is later than their own previous report |
| Tier | Fixed by who signed: issuer account → 2, `Auditor` role → 3, `Transcriber` role → 1 | The issuer's key, and the registry admin for roles |
| Contract code | The installed wasm | Each contract's admin, through the upgrade delay |

So every answer rests on the registry admin, the supply poster and the reporter
who signed the winning report. An integrator can narrow the set of reporters
with `min_tier`. The supply poster and the two admins cannot be narrowed.

## Registry admin

The address stored as `Admin` in reporter-registry. It decides who may speak.

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `reporter-registry.set_reporter(reporter, role, name)` | Gives any address the `Auditor`, `Transcriber` or `SupplyPoster` role, or changes an existing reporter's role or name. |
| `reporter-registry.revoke(reporter)` | Removes a role. The reporter's figures stop counting in the same transaction. |
| `reporter-registry.transfer_admin(new_admin)` | Hands the role to another address. Needs both the old and the new admin's signatures. |
| `reporter-registry.propose_upgrade(wasm_hash)` | Schedules new registry code to become executable after `UPGRADE_DELAY_LEDGERS` (17 280 ledgers, about a day). |
| `reporter-registry.execute_upgrade()` | Installs the proposed code once the delay has passed. |
| `reporter-registry.cancel_upgrade()` | Drops the pending proposal. |

**What it can falsify:**

- **Supply, for every asset.** It can appoint any address, its own included,
  as a supply poster, which can then post any supply (see
  [Supply poster](#supply-poster)).
- **Auditor and transcriber figures, for every asset.** It can appoint any
  address as an auditor and have it post any figure in the plausibility band,
  dated now, at tier 3.
- **Removing honest figures.** Revoking an honest auditor takes their reports
  out of `coverage`, `get_report` and `get_reports` immediately. That can turn
  a covered asset into an uncovered one, or let an older and worse figure win.
- **Anything at all, after a day.** An upgrade can make `role_of` return any
  role for any address, which feeds every check above.

**What limits it today:**

- **Issuer-signed reports never consult the registry.** With `min_tier = 2`,
  the registry admin can still add its own auditor figures at tier 3, but it
  cannot remove or forge the issuer's figure. It still controls supply.
- **Every action emits an event:** `reporter_set`, `reporter_revoked`,
  `admin_transferred`, `upgrade_proposed`, `upgrade_executed`,
  `upgrade_cancelled`. The indexer keeps them all, so a rogue appointment is
  public and attributable.
- **Upgrades are time-locked** by 17 280 ledgers on the code in this
  repository. *The live testnet registry still runs pre-time-lock code with an
  immediate `upgrade(wasm_hash)`.* It moves to the time lock once the procedure
  in [RUNBOOK.md](RUNBOOK.md#3-first-upgrade-of-a-pre-time-lock-deployment)
  has been run. Check what a deployment runs with
  `scripts/verify-deployment.sh`.
- **`transfer_admin` needs the new admin's signature**, so the role cannot be
  sent to a mistyped or unowned address.

**Gaps:**

- Roles never expire: [#10](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/10).
- There is no on-chain list of reporters, so the current set has to be rebuilt
  from events: [#29](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/29).
- The admin is a single address. Nothing in the contract requires a multisig;
  that is operational practice ([RUNBOOK.md](RUNBOOK.md#7-move-admin-to-a-multisig)).
- Admin transfer happens in one step, with no propose-and-accept: [#3](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/3). The flow as implemented, and what to do when a transfer should not go ahead, is in [admin-transfer.md](admin-transfer.md).

**What an integrator should do:**

- Use `min_tier = 2` (`IssuerSigned`) if you would rather trust the issuer than
  the registry admin's choice of auditors.
- Watch `reporter_set` events for `role = 1` (auditor) and `role = 3` (supply
  poster). A new auditor that immediately posts for your asset is worth a look.
- Watch `upgrade_proposed` on the registry. You have about a day to stop
  relying on it before the code can change.

## Ledger admin

The address stored as `Admin` in coverage-ledger.

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.transfer_admin(new_admin)` | Hands the role to another address. Needs both signatures. |
| `coverage-ledger.propose_upgrade(wasm_hash)` | Schedules new ledger code after `UPGRADE_DELAY_LEDGERS`. |
| `coverage-ledger.execute_upgrade()` | Installs the proposed code once the delay has passed. |
| `coverage-ledger.cancel_upgrade()` | Drops the pending proposal. |

**What it can falsify:**

- **Nothing directly.** The ledger admin cannot list or delist assets, post
  figures, change the registry address, or edit stored reports.
- **Everything, after the delay.** New code can return any `coverage` or
  `is_covered` answer, rewrite stored reports, or point at a different
  registry. The covered-vault, and every consumer like it, calls whatever code
  is installed at the ledger's address.

**What limits it today:**

- **The upgrade time lock** of 17 280 ledgers, with an `upgrade_proposed` event
  that names the wasm hash, on the code in this repository. *The live testnet
  ledger still runs pre-time-lock code with an immediate `upgrade(wasm_hash)`*
  (see [Registry admin](#registry-admin)).
- **The registry address is fixed** at construction, so the ledger admin cannot
  quietly swap in a friendlier registry without an upgrade.
- **The pending wasm is public.** Anyone can fetch it by hash
  (`stellar contract fetch --wasm-hash`) and compare it with a build of a tagged
  release.

**Gaps:**

- The ledger cannot be repointed at a new registry, even legitimately, without
  an upgrade: [#21](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/21).
- An asset cannot be delisted: [#20](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/20).
- The contracts expose no version, so the only way to identify installed code
  is its wasm hash: [#31](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/31).
- The pre-time-lock `upgrade` that testnet still runs emits no event:
  [#14](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/14).
  Time-locked upgrades are tracked in
  [#30](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/30).

**What an integrator should do:**

- Watch `upgrade_proposed` on the ledger. When one appears, fetch the proposed
  wasm, compare it with a release build, and decide before
  `executable_at_ledger` whether to keep calling the ledger.
- If you must not run changed code, record the wasm hash you reviewed. Refuse
  to act if `stellar contract info hash --id <ledger>` changes, or if
  `pending_upgrade()` is not `None`.

## Supply poster

Any address with the `SupplyPoster` role in reporter-registry. In the demo
this is the indexer.

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.post_supply(poster, sac, amount, ledger, breakdown_hash)` | Replaces the single stored supply snapshot for `sac`. |

**What it can falsify:**

- **The supply of every listed asset.** There is one snapshot per asset, and
  any supply poster can replace it. The amount only has to be non-negative.
- **Zero supply makes any asset read as covered.** With `supply = 0`,
  `bps = u32::MAX`. Every `is_covered` with a fresh report then answers
  `true`, whatever the reserves, zero included. (No issue yet.)
- **A small supply inflates coverage, and a large one deflates it.** The
  plausibility band on reserve figures (0.01% to 10,000%) is checked only when
  a reserve is posted, against the supply stored at that moment. It is not
  re-checked when supply changes.
- **Old supply presented as fresh.** `ledger` only has to be later than the
  previous snapshot's ledger and not in the future. The snapshot's `timestamp`
  is when it was posted, so a figure computed from an old ledger is judged as
  fresh: [#24](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/24).

**What limits it today:**

- **Revocation.** `reporter-registry.revoke(poster)` stops further posts. A
  snapshot already posted stays until the next honest post replaces it.
- **Monotonic ledgers.** A snapshot can never go back to an earlier ledger
  than the stored one, so an honest poster's next post always replaces a bad
  one.
- **`breakdown_hash`.** The per-bucket breakdown is committed on-chain, so a
  watcher can recompute supply from public ledger data and show a mismatch. A
  mismatch is evidence; the contract does not act on it.

**Gaps:**

- Supply cannot be disputed on-chain: [#1](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/1).
- An all-zero `breakdown_hash` is accepted, which commits to nothing: [#23](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/23).
- `post_supply` accepts `amount = 0`, which reads as fully covered (no issue
  yet).
- Supply freshness is post time, not data time: [#24](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/24).

**What an integrator should do:**

- **Do not rely on `is_covered` alone when supply can be zero.** Call
  `coverage(sac, min_tier)` and require `supply > 0`, or check
  `get_supply(sac)` yourself. See
  [INTEGRATING.md](INTEGRATING.md#zero-supply-reads-as-u32max-bps-and-counts-as-covered).
- Choose `max_age` knowing that it bounds when the supply was posted, not the
  ledger it describes.
- Run, or rely on someone who runs, an off-chain recomputation of supply that
  compares against `breakdown_hash`.

## Auditor

Any address with the `Auditor` role. Its reports are `AuditorSigned`
(tier 3), the strongest tier.

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.post_reserve(reporter, sac, amount, as_of, doc_hash, doc_uri)` | Records or replaces this auditor's own report for `sac` at tier 3. |

**What it can falsify:**

- **A reserve figure for any listed asset**, not only assets it audits. The
  figure must fall within 0.01% to 10,000% of the stored supply (with no stored
  supply, any figure is accepted).
- **Freshness.** `as_of` is the auditor's own claim. It can date any figure "now", and
  `coverage` picks the latest `as_of`, so it wins over every other report at
  tiers `>= min_tier`.
- **The document.** `doc_hash` and `doc_uri` are commitments the contract
  stores and does not check. An empty URI is rejected, but anything else is
  accepted: [#22](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/22),
  [#23](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/23).
- **Slots.** Each tier holds at most four reporters per asset. Auditors
  appointed by the registry admin can fill a tier so that a further honest
  auditor gets `TooManyReporters` until one of them is revoked.

**What limits it today:**

- **Revocation is immediate.** After `revoke`, none of its reports count
  anywhere, with no second action needed.
- **Its own history only moves forward.** Each new report must have a later
  `as_of` than its previous one. It cannot overwrite another auditor's report
  ([#26](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/26),
  closed).
- **The plausibility band** catches unit slips (whole units instead of
  stroops), but not deliberate lies within the band.
- **Events** carry every figure, with its `doc_hash` and `doc_uri`, so a false
  statement stays attributable after it is replaced.

**Gaps:**

- A reporter cannot retract a report, so a mistake stays in force until a later
  figure replaces it: [#25](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/25).
- Roles have no expiry: [#10](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/10).

**What an integrator should do:**

- Prefer `min_tier = 3` only if you trust the registry admin's choice of
  auditors more than the issuer. Otherwise `min_tier = 2` admits both.
- Keep `max_age` short enough that a revoked auditor's influence is bounded
  even if nobody revokes it promptly.
- To check who signed the figure behind an answer, read `get_reports(sac)`. It
  lists every counting report, with its reporter, `doc_hash` and `doc_uri`.

## Transcriber

Any address with the `Transcriber` role. Its reports are `Transcribed`
(tier 1): a figure copied from a published report.

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.post_reserve(...)` | Records or replaces this transcriber's own report for `sac` at tier 1. |

**What it can falsify:**

- Everything an [auditor](#auditor) can, at tier 1.
- **Lower tiers can override higher ones.** When `min_tier = 1`, a transcribed
  figure with a later `as_of` beats an older auditor or issuer figure, because
  `coverage` picks by `as_of` first and uses tier only to break ties.

**What limits it today:**

- The same as for an auditor: immediate revocation, a per-reporter forward-only
  `as_of`, the plausibility band, and events.
- With `min_tier >= 2`, transcribed reports are ignored completely.

**What an integrator should do:**

- **Use `min_tier = 1` only if you are prepared to trust any registered
  transcriber** as much as the issuer. Anything that guards real value should
  use `min_tier >= 2`.

## Issuer

The classic Stellar account that issued the asset, as recorded in the asset's
listing (parsed from the Stellar Asset Contract's `name()`, so it cannot be
spoofed). Its reports are `IssuerSigned` (tier 2).

**Privileged functions:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.post_reserve(...)` signed by `asset.issuer` | Records or replaces the issuer's own report for its own asset at tier 2. |

**What it can falsify:**

- **Its own asset's reserves**, within the plausibility band, dated now.
  Issuer-signed is a self-declaration: the party with the most to gain from
  reading as covered signs it.
- **Real supply after the snapshot.** The issuer can mint between snapshots.
  `is_covered` only knows the last posted supply.
- **Outside the contracts**, an issuer with `AUTH_REVOCABLE` or clawback
  enabled can freeze or claw back balances, including a vault's.

**What limits it today:**

- **Only its own assets.** The tier comes from `reporter == asset.issuer`, and
  the asset record's issuer is parsed from the genuine SAC. Nobody else can
  sign as the issuer, and the issuer cannot sign for anyone else's asset.
- **The registry cannot revoke it.** That protects the issuer from a rogue
  registry admin, but it also means no maintainer can take a lying issuer's
  figure out of `coverage`. Only a fresher report from someone else at a tier
  `>= min_tier` displaces it.
- The plausibility band, forward-only `as_of`, and events, as for every
  reporter.

**Gaps:**

- The issuer cannot delegate signing to another key, so the issuer's master key
  or signers have to sign each report: [#28](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/28).
- No retraction: [#25](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/25).

**What an integrator should do:**

- **Treat `min_tier = 2` as "the issuer says so, or an auditor does".** Use
  `min_tier = 3` if a self-declaration is not enough.
- Pair `max_age` with how often the issuer actually publishes, and check the
  issuer account's flags if the asset's freezability matters to you.

## Arbitrary caller

Anyone, with no role.

**Functions they can call:**

| Function | What it does |
| --- | --- |
| `coverage-ledger.list_asset(sac)` | Lists any genuine Stellar Asset Contract that is not native XLM. Permissionless. |
| Every view (`coverage`, `is_covered`, `get_*`, `role_of`, `pending_upgrade`, `admin`, ...) | Reads, and extends the TTL of the entries read. |
| `covered-vault.deposit(from, amount)` / `request_withdraw(to, amount)` / `execute_withdraw(to)` | Moves or queues their own funds. Each needs its own address's signature. |
| `__constructor` on a fresh deployment | Only at deploy time, by whoever deploys. A deployer sets the admins, registry address and vault thresholds; the vault admin can later update its withdrawal delay. |

**What they can falsify:**

- **Nothing about coverage.** They cannot post supply or reserves, and cannot
  change roles.
- **Look-alike assets.** Anyone can issue `USDC` from their own account and
  list it. The listing records the true issuer, so it cannot pose as Circle's
  USDC to a contract that checks by SAC address. It can fool a person or a
  front end that matches on the asset code.
- **Their own asset.** By issuing an asset, anyone becomes its issuer, with
  everything in [Issuer](#issuer).

**What limits them today:**

- `list_asset` accepts only the built-in asset contract's executable, and
  rejects native XLM. Listing changes no coverage answer until someone with a
  role posts for the asset.
- Reads cost the caller and cannot change an answer. Restoring an archived
  entry brings back what was stored, which is then judged by its age (SPEC.md,
  *Archival behaviour*).
- Withdrawals from a covered-vault are never blocked by coverage.

**Gaps:**

- A listing can never be removed: [#20](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/20).
- SAC name parsing is not yet fuzzed: [#13](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/13).
- A covered-vault's constructor arguments are not validated, so a deployer
  can create a vault with meaningless thresholds: [#16](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/16).

**What an integrator should do:**

- **Key everything by SAC contract address**, never by asset code.
- Show users the issuer from `get_asset(sac)` next to the code.

## Known gaps

Collected from the sections above.

| Gap | Who can exploit it | Issue |
| --- | --- | --- |
| Supply `0` reads as fully covered for any fresh report | Supply poster, or the registry admin through one | No issue yet |
| Supply freshness is post time, not data time | Supply poster | [#24](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/24) |
| No on-chain dispute of supply | Supply poster | [#1](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/1) |
| All-zero `doc_hash` / `breakdown_hash` accepted | Any poster or reporter | [#23](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/23) |
| `doc_uri` scheme not checked | Any reporter | [#22](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/22) |
| Reports cannot be retracted | Any reporter (by mistake) | [#25](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/25) |
| Roles never expire | Registry admin (by neglect) | [#10](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/10) |
| No on-chain reporter list | Registry admin (opacity) | [#29](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/29) |
| One-step admin transfer | Either admin | [#3](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/3) |
| Live testnet still runs the immediate, event-less `upgrade` | Either admin | [#30](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/30), [#14](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/14) |
| No contract version, only the wasm hash | Either admin (opacity) | [#31](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/31) |
| Ledger cannot be repointed at a new registry | Registry admin (lock-in) | [#21](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/21) |
| No delisting | Arbitrary caller | [#20](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/20) |
| Issuer cannot delegate signing | Issuer (operational) | [#28](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/28) |
| Vault constructor arguments not validated | Deployer | [#16](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/16) |
| SAC name parsing not fuzzed | Arbitrary caller | [#13](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/13) |

## What an integrator should do

1. **Pick `min_tier` by whom you trust.** Use 3 to trust the registry
   admin's auditors only, 2 to also accept the issuer's self-declaration, and
   1 only if any registered transcriber is good enough.
2. **Check supply is positive** before treating `is_covered == true` as
   meaningful. Call `coverage()` and look at `supply`.
3. **Keep `max_age` as short as the issuer's real reporting cadence allows.**
   It bounds how long a bad figure, or a revoked reporter's figure, can matter.
4. **Key assets by SAC address**, never by code.
5. **Watch the admin events** on both contracts: `reporter_set`,
   `reporter_revoked`, `admin_transferred` and `upgrade_proposed`. The upgrade
   delay is only useful to someone who is watching.
6. **Pin what you reviewed.** Record the wasm hashes of the registry and ledger
   you rely on, and check them with `scripts/verify-deployment.sh` or
   `stellar contract info hash --id`.
7. **Never block withdrawals on coverage.** covered-vault shows the pattern: a
   wrong answer then delays new deposits, but never traps funds.
