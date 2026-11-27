# Concepts

## How `max_age` interacts with Stellar's 5-second ledger close time

`max_age` — passed to `coverage-ledger.is_covered`, `coverage-ledger.coverage_status`, and stored in `covered-vault`'s `VaultConfig` — is measured in **seconds**, not ledger sequences.

Stellar closes a ledger roughly every **5 seconds** (`SECONDS_PER_LEDGER = 5` in `plimsoll-types`). When you want to express a freshness window in ledgers (or convert a ledger-based cadence into `max_age`), use:

```text
max_age_seconds = max_age_ledgers * 5
```

For example:

| Target window | Approximate ledgers (`max_age_ledgers`) | `max_age` (`max_age_seconds`) |
| --- | ---: | ---: |
| 1 hour | 720 | `3_600` |
| 1 day | 17,280 (`DAY_IN_LEDGERS`) | `86_400` |
| 7 days (PUSD demo) | 120,960 | `604_800` |

### Why freshness is checked in seconds

- **Timestamps are wall-clock time.** Reserve reports carry an `as_of` Unix timestamp from the attestation document, and supply snapshots record the ledger close `timestamp`. Comparing `now - timestamp <= max_age` in seconds keeps both checks on the same unit and is inclusive at the boundary (`age == max_age` is still fresh; see [INTEGRATING.md](INTEGRATING.md#max_age-how-recent-in-seconds)).
- **Ledger close time is an estimate, not consensus.** Actual ledger close intervals can vary slightly around 5 seconds. Using seconds avoids drifting away from real-world attestation dates; `SECONDS_PER_LEDGER = 5` is used only for display (such as `StaleInfo.age_ledgers = age / SECONDS_PER_LEDGER` in `coverage_status`) and for TTL constants (`DAY_IN_LEDGERS = 17_280`).
- **Demo 7-day expiry.** The PUSD demo vault sets `max_age = 604_800` seconds (7 days, or `120_960` ledgers at 5 seconds per ledger). When the standing statement is not restated within that window, `is_covered` returns `false` and the vault refuses deposits; see [docs/demo/pusd-reserves.md](demo/pusd-reserves.md#why-this-statement-is-not-dated).

### Related

- [SPEC.md](SPEC.md#coverage-ledger) — `is_covered`, `coverage_status`, and `StaleInfo.age_ledgers`.
- [INTEGRATING.md](INTEGRATING.md#max_age-how-recent-in-seconds) — choosing `max_age` for your integration.
- [docs/demo/pusd-reserves.md](demo/pusd-reserves.md) — the testnet PUSD demo's 7-day expiry in practice.
