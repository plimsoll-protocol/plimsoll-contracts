# Time-To-Live (TTL) and archival reference

All three Plimsoll contracts share a single TTL policy defined in
[`crates/types/src/lib.rs`](../crates/types/src/lib.rs). For the complete
per-contract `DataKey` inventory, storage tiers, and exact bump operations, see
[storage.md](storage.md).

## Policy constants

At Stellar's 5-second average ledger close time:

| Constant | Value | Equivalent time | Role |
| --- | ---: | --- | --- |
| `DAY_IN_LEDGERS` | `17_280` | 1 day | Base unit for ledger-measured durations. |
| `PERSISTENT_THRESHOLD` | `518_400` | 30 days | Extend persistent entries whose remaining TTL is below this. |
| `PERSISTENT_EXTEND_TO` | `1_555_200` | 90 days | Target TTL when extending persistent entries. |
| `INSTANCE_THRESHOLD` | `518_400` | 30 days | Extend instance storage when its remaining TTL is below this. |
| `INSTANCE_EXTEND_TO` | `1_555_200` | 90 days | Target TTL when extending instance storage. |

## Archival and restoration (CAP-0066)

When a persistent entry's TTL expires, the Soroban host archives it. Under
Protocol 23 (CAP-0066), `simulateTransaction` automatically includes archived
entries in the transaction's restoration footprint so the submitter restores them
as part of execution:

- Contract reads and writes behave as if live; a restored entry returns with the
  network minimum TTL and is extended back to `PERSISTENT_EXTEND_TO` on the
  next extending read or write.
- Restored `ReserveReport` (`as_of`) and `SupplySnapshot` (`timestamp`) values
  are at least 90 days old by the time they could archive, so `is_covered`
  naturally returns `false` for any shorter `max_age`.

See [storage.md](storage.md) for every `DataKey` variant, its storage tier
(`instance` vs `persistent`), and the exact functions that bump it.
