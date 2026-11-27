# Storage layout reference

This page lists every `DataKey` variant across all three Plimsoll contracts,
its Soroban storage tier, value type, and the TTL bump applied when it is
written or read.

For the TTL policy constants and CAP-0066 archival/restoration behavior, see
[ttl.md](ttl.md).

## Instance vs persistent storage

Soroban contracts store state in two main tiers used by Plimsoll:

| Property | `instance()` storage | `persistent()` storage |
| --- | --- | --- |
| **Ledger footprint** | Stored inside the single `ContractData` entry for the contract instance itself. | Stored as an independent `ContractData` ledger entry per key. |
| **TTL behavior** | Extending TTL (`env.storage().instance().extend_ttl(...)`) bumps the entire instance entry—all `instance` keys share one TTL. | Extending TTL (`env.storage().persistent().extend_ttl(&key, ...)`) bumps only that single key's entry. |
| **Cost & size** | Loaded together on every contract call; best for small, bounded, global config (`Admin`, `Registry`, `Config`, `PendingUpgrade`). | Loaded only when a specific key is accessed; scales to unbounded per-asset, per-reporter, or per-user maps without bloating every call. |
| **Archival** | If instance TTL expires, the contract instance entry is archived and must be restored before any contract call succeeds. | Individual keys archive independently once their TTL reaches zero and are automatically marked for restoration by `simulateTransaction` (CAP-0066). |

## TTL bump constants (`crates/types/src/lib.rs`)

Assuming a 5-second ledger close time (`DAY_IN_LEDGERS = 17_280`):

| Constant | Value (ledgers) | Duration | Used for |
| --- | ---: | --- | --- |
| `INSTANCE_THRESHOLD` | `518_400` | 30 days | Extend `instance()` storage when remaining TTL falls below this threshold. |
| `INSTANCE_EXTEND_TO` | `1_555_200` | 90 days | Target TTL when extending `instance()` storage. |
| `PERSISTENT_THRESHOLD` | `518_400` | 30 days | Extend a `persistent()` entry when remaining TTL falls below this threshold. |
| `PERSISTENT_EXTEND_TO` | `1_555_200` | 90 days | Target TTL when extending a `persistent()` entry. |

---

## `coverage-ledger` (`contracts/coverage-ledger/src/lib.rs`)

| `DataKey` variant | Tier | Value type | Bump amount (`threshold` → `extend_to`) | Extended by |
| --- | --- | --- | --- | --- |
| `Admin` | `instance` | `Address` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | `__constructor`, `list_asset`, `post_supply`, `post_reserve`, `transfer_admin`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `pending_upgrade` (if `Some`) |
| `Registry` | `instance` | `Address` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | Shares `instance()` TTL with `Admin` |
| `PendingUpgrade` | `instance` | `PendingUpgrade` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | Shares `instance()` TTL (`propose_upgrade`, `execute_upgrade` / `cancel_upgrade` remove it, `pending_upgrade` bumps when `Some`) |
| `Asset(Address)` | `persistent` | `AssetRecord` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `list_asset` (write), `get_asset` (read), `post_supply` & `post_reserve` (`require_listed` read) |
| `Supply(Address)` | `persistent` | `SupplySnapshot` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `post_supply` (write & stale check read), `post_reserve` (plausibility check read), `get_supply`, `coverage`, `is_covered` (read) |
| `Report(Address, Tier)` | `persistent` | `ReserveReport` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | Legacy v0.1 key (never written). Extended on read via `reports_at` (`get_report`, `get_reports`, `get_all_reports`, `last_updated`, `get_all_latest_reports`, `coverage`, `is_covered`); removed by `migrate_legacy` on first `post_reserve` at that tier |
| `ReportBy(Address, Tier, Address)` | `persistent` | `ReserveReport` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `post_reserve` (write & stale check read), `migrate_legacy` (write), `reports_at` (read in `get_report`, `get_reports`, `get_all_reports`, `coverage`, `is_covered`); removed by `reclaim_slots` when tier is full and reporter no longer counts |
| `Reporters(Address, Tier)` | `persistent` | `Vec<Address>` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `post_reserve` (write & read), `migrate_legacy` (write), `reports_at` (read), `pair_page` (read, via `asset_reporter_pairs` and `get_all_latest_reports`) |
| `Assets` | `persistent` | `Vec<Address>` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `list_asset` (write), `pair_page` (read, via `asset_reporter_pairs` and `get_all_latest_reports`) |
| `SupplyPostedLedger(Address)` | `persistent` | `u32` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `post_supply` (write), `posted_supply_at_ledger` (read, via `is_covered_at` and `historical_coverage_bps`) |
| `ReportPostedLedger(Address, Tier, Address)` | `persistent` | `u32` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `post_reserve` (write), `reports_at_ledger` (read, via `is_covered_at` and `historical_coverage_bps`); removed by `reclaim_slots` with its report |

---

## `reporter-registry` (`contracts/reporter-registry/src/lib.rs`)

| `DataKey` variant | Tier | Value type | Bump amount (`threshold` → `extend_to`) | Extended by |
| --- | --- | --- | --- | --- |
| `Admin` | `instance` | `Address` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | `__constructor`, `set_reporter`, `revoke`, `transfer_admin`, `propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `pending_upgrade` (if `Some`) |
| `Reporter(Address)` | `persistent` | `ReporterInfo` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `set_reporter` (write), `get_reporter` & `role_of` (read when `Some`); removed on `revoke` |
| `PendingUpgrade` | `instance` | `PendingUpgrade` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | Shares `instance()` TTL (`propose_upgrade`, `execute_upgrade` / `cancel_upgrade` remove it, `pending_upgrade` bumps when `Some`) |

---

## `covered-vault` (`contracts/covered-vault/src/lib.rs`)

| `DataKey` variant | Tier | Value type | Bump amount (`threshold` → `extend_to`) | Extended by |
| --- | --- | --- | --- | --- |
| `Config` | `instance` | `VaultConfig` | `INSTANCE_THRESHOLD` (`518_400`) → `INSTANCE_EXTEND_TO` (`1_555_200`) | `__constructor`, `deposit` (note: `withdraw`, `balance`, `config`, `can_deposit` read `Config` without extending instance TTL) |
| `Balance(Address)` | `persistent` | `i128` | `PERSISTENT_THRESHOLD` (`518_400`) → `PERSISTENT_EXTEND_TO` (`1_555_200`) | `deposit` (read & write), `withdraw` (read & write when non-zero; removed when balance reaches `0`), `balance` (read when `Some`) |
