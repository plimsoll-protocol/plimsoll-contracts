# Moving the admin role

Both contracts keep one admin address in instance storage and answer it with
`admin()`. That address can appoint reporters, revoke them, and propose,
execute or cancel upgrades. What each admin can and cannot do, and what a
compromised one could falsify, is in the governance document:
[THREAT_MODEL.md](THREAT_MODEL.md#registry-admin) (registry admin,
[ledger admin](THREAT_MODEL.md#ledger-admin)).

Moving the role is `transfer_admin(new_admin)` on each contract. This page
explains the two steps it takes, why both are required, and what to do when a
transfer should not go ahead.

## The two steps

`transfer_admin(new_admin)` requires **two** authorisations, in this order:

1. the address stored as `Admin` now — the *proposal*;
2. `new_admin` — the *acceptance*.

```rust
pub fn transfer_admin(env: Env, new_admin: Address) {
    admin(&env).require_auth();
    new_admin.require_auth();
    env.storage().instance().set(&DataKey::Admin, &new_admin);
    bump_instance(&env);
    AdminTransferred { new_admin }.publish(&env);
}
```

Both checks happen before the stored `Admin` is replaced, so a transaction
missing either signature fails as a whole and leaves the role where it was.
The call is atomic: there is no window in which the role belongs to nobody,
and no second call to finish the move.

What the ledger records:

| | |
| --- | --- |
| Storage | `Admin` (instance) replaced |
| Event | `admin_transferred {new_admin}` (see [EVENTS.md](EVENTS.md#admin_transferred)) |
| Errors | none — a missing signature is a host auth failure, not a contract error code |
| View | `admin()` returns the new address |

The full behaviour, including the TTL bump, is in
[SPEC.md](SPEC.md#reporter-registry) (registry) and
[SPEC.md](SPEC.md#coverage-ledger) (ledger).

## Why both steps are needed

- **The new admin must sign, so the role cannot be sent to a mistyped or
  unowned address.** Without that signature, one wrong character in a
  `G...` address would hand the role to an account nobody controls, and the
  contract has no way to take it back.
- **The current admin must sign, so nobody else can move the role.** Holding
  the new key is not enough; the outgoing key has to agree to hand it over.
- **Both signatures are bound to the exact call.** A Soroban authorisation
  entry names the contract address, the function name and the arguments, so a
  signature collected for one `new_admin` cannot be reused for another.

Neither signature has to come from a single key: the address is usually a
multisig account, and the Stellar network enforces its thresholds before the
contract sees the authorisation. Step 3 of
[RUNBOOK.md](RUNBOOK.md#7-move-admin-to-a-multisig) uses this to move the role
to an operations account while it is still single-key, and then turns that
account into an M-of-N multisig.

## Cancelling a pending transfer

There is no pending transfer to cancel on-chain. A transfer exists only as a
signed transaction between the two signatures and disappears when that
transaction is dropped:

- **Before submission** — the request has not been signed by both parties, or
  it has but was never sent — discard the envelope. No state was written, no
  event was emitted, and no cleanup call exists or is needed.
- **After submission** — the transaction either succeeded, in which case the
  new admin holds the role, or it failed and changed nothing. There is no
  half-finished transfer to unwind. If the transfer should be undone, run
  `transfer_admin` again from the new admin, with both signatures.
- **If the outgoing key is lost mid-flight**, nothing on-chain can recover the
  role: the outgoing signature is required, and no recovery path exists. Do
  the transfer while both keys are available, then retire the old one.

This is different from an upgrade, which is a two-transaction flow with a
one-day delay and a real on-chain proposal to drop:
`propose_upgrade` stores the hash, and `cancel_upgrade` clears it before
`execute_upgrade` runs (see
[SPEC.md](SPEC.md#time-locked-upgrades)). A transfer deliberately has no such
window: the role either moves in the transaction that both parties signed, or
it does not move. If you need a delay or a veto before the role changes hands,
put the admin role behind a multisig so a second signer can refuse.

An on-chain propose-and-accept, where the proposal itself is stored and can be
cancelled by a later transaction, is tracked in
[#3](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/3). This
page describes the flow the contracts implement today.

## Command

Both keys must be present in the local keystore, because the CLI signs the new
admin's authorisation as well as the current admin's:

```bash
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
  transfer_admin --new_admin "$OPS"
```

Expected:

```
✅ Transaction submitted successfully!
📅 <REGISTRY> - Success - Event: AdminTransferred (admin_transferred), new_admin: "<OPS>"
```

Verify it took effect before retiring the old key:

```bash
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- admin
```

Expected: `"<OPS>"`. Repeat with `$LEDGER`: `transfer_admin` moves one
contract at a time. Steps 3 to 5 of
[RUNBOOK.md](RUNBOOK.md#7-move-admin-to-a-multisig) do both.

## Related

- [THREAT_MODEL.md](THREAT_MODEL.md#registry-admin) — who holds the role, what
  a compromised key could do, and the gaps that remain.
- [SPEC.md](SPEC.md#reporter-registry) — the function table, storage keys and
  TTL rules for both contracts.
- [RUNBOOK.md](RUNBOOK.md#7-move-admin-to-a-multisig) — the operational
  procedure, including the multisig setup that follows the transfer.
- [ERRORS.md](ERRORS.md#errors-that-are-not-contract-errors) — what a missing
  signature looks like to a caller.
