# Plimsoll authorization audit

Systematic review of every `#[contractimpl]` function in
`reporter-registry`, `coverage-ledger`, and `covered-vault` for missing
or insufficient `require_auth` calls.

Generated as part of issue #79.

## Methodology

For every public function we ask three questions:

1. **State change?** Does the function write to any storage key?
2. **Who should authorize?** Which principal's authority is consumed?
3. **Is `require_auth` called before the first side effect?**

Read-only view functions need no auth. Functions that gate on a role
(e.g. `post_supply` checks the caller's role after requiring auth) must
still call `require_auth` so the host records the auth entry.

---

## `reporter-registry`

| Function | State change | Expected auth | Has `require_auth` | Verdict |
| --- | --- | --- | --- | --- |
| `__constructor(admin)` | Yes (instance) | — (constructors cannot require auth in Soroban) | N/A | ✅ Correct |
| `admin()` | No | — | — | ✅ View only |
| `set_reporter(reporter, role, name)` | Yes (persistent reporter entry) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `revoke(reporter)` | Yes (removes persistent reporter entry) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `role_of(reporter)` | No (read + TTL extend is non-observable) | — | — | ✅ View only |
| `get_reporter(reporter)` | No | — | — | ✅ View only |
| `transfer_admin(new_admin)` | Yes (instance admin key) | admin **and** new_admin | `admin(&env).require_auth()` then `new_admin.require_auth()` | ✅ Correct — dual-party requirement prevents hostile takeover |
| `propose_upgrade(wasm_hash)` | Yes (instance PendingUpgrade) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `execute_upgrade()` | Yes (removes PendingUpgrade, installs wasm) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `cancel_upgrade()` | Yes (removes PendingUpgrade) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `pending_upgrade()` | No | — | — | ✅ View only |

**Gaps found:** none.

---

## `coverage-ledger`

| Function | State change | Expected auth | Has `require_auth` | Verdict |
| --- | --- | --- | --- | --- |
| `__constructor(admin, registry)` | Yes (instance) | — | N/A | ✅ Correct |
| `admin()` | No | — | — | ✅ View only |
| `registry()` | No | — | — | ✅ View only |
| `list_asset(sac)` | Yes (persistent Asset entry) | Permissionless by design — anyone may list a genuine SAC | — | ✅ Correct per spec |
| `post_supply(poster, sac, …)` | Yes (persistent Supply entry) | poster (must hold `SupplyPoster` role) | `poster.require_auth()` before the role check | ✅ Correct — auth precedes role gate |
| `post_reserve(reporter, sac, …)` | Yes (persistent ReportBy + Reporters) | reporter (must be issuer, Auditor, or Transcriber) | `reporter.require_auth()` before tier resolution | ✅ Correct |
| `get_asset(sac)` | No | — | — | ✅ View only |
| `get_supply(sac)` | No | — | — | ✅ View only |
| `alias_asset(old, new)` | Yes (persistent Alias entry) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `get_alias(asset)` | No | — | — | ✅ View only |
| `get_report(sac, tier)` | No | — | — | ✅ View only |
| `get_reports(sac)` | No | — | — | ✅ View only |
| `coverage(sac, min_tier)` | No | — | — | ✅ View only |
| `is_covered(sac, min_bps, max_age, min_tier)` | No | — | — | ✅ View only |
| `transfer_admin(new_admin)` | Yes (instance admin key) | admin **and** new_admin | Both `require_auth` calls present | ✅ Correct |
| `propose_upgrade(wasm_hash)` | Yes (instance PendingUpgrade) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `execute_upgrade()` | Yes (removes PendingUpgrade, installs wasm) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `cancel_upgrade()` | Yes (removes PendingUpgrade) | admin | `admin(&env).require_auth()` | ✅ Correct |
| `pending_upgrade()` | No | — | — | ✅ View only |

**Gaps found:** none.

**Notes:**

- `list_asset` is intentionally permissionless. The SAC address is
  verified to be a genuine Stellar Asset Contract
  (`address.executable() == Some(Executable::StellarAsset)`), and the
  code/issuer are read from the token itself — no caller-supplied strings
  are trusted for identity. This prevents griefing via look-alike tokens.
- `post_supply` and `post_reserve` both call `require_auth` before any
  storage read or cross-contract call, satisfying the "auth before side
  effects" rule.

---

## `covered-vault`

| Function | State change | Expected auth | Has `require_auth` | Verdict |
| --- | --- | --- | --- | --- |
| `__constructor(admin, ledger, asset, min_bps, max_age, min_tier)` | Yes (instance Config, Admin, WithdrawalDelay) | admin supplied by deployer | N/A | ✅ Correct |
| `deposit(from, amount)` | Yes (persistent Balance + SAC transfer) | from | `from.require_auth()` before coverage check and transfer | ✅ Correct |
| `request_withdraw(to, amount)` | Yes (persistent PendingWithdrawal) | to | `to.require_auth()` before balance read and queue write | ✅ Correct |
| `execute_withdraw(to)` | Yes (persistent Balance + SAC transfer) | to | `to.require_auth()` before queue read and transfer | ✅ Correct |
| `set_withdrawal_delay(delay_seconds)` | Yes (instance WithdrawalDelay) | vault admin | `admin.require_auth()` before delay update | ✅ Correct |
| `balance(of)` | No | — | — | ✅ View only |
| `config()` | No | — | — | ✅ View only |
| `can_deposit()` | No | — | — | ✅ View only |

**Gaps found:** none.

---

## Cross-cutting observations

1. **Admin key management.** Both `reporter-registry` and
   `coverage-ledger` store a single admin key in instance storage.
   Compromise of that key is the main escalation path. The time-locked
   upgrade mechanism (issue #76) already mitigates the upgrade-path risk
   with a one-day delay. The `transfer_admin` dual-party requirement
   prevents silent handoff.

2. **Registry cross-contract calls.** `coverage-ledger` calls
   `registry.role_of` without passing auth. This is correct — `role_of`
   is a pure view that requires no auth and trusts the registry contract's
   own state. A compromised registry could lie about roles; this is a
   trust dependency documented in `docs/THREAT_MODEL.md`.

3. **SAC transfers in `covered-vault`.** The vault calls
   `token::TokenClient::transfer`, which will check that
   `from.require_auth()` (for `deposit`) or the vault's own contract
   address (for `withdraw`) has authorised the transfer. The vault's
   `from.require_auth()` satisfies the deposit side; the vault itself is
   the sender for withdrawals, so no extra auth is needed there.

4. **Follow-up issues filed:** none — all entry points are correctly
   guarded.
