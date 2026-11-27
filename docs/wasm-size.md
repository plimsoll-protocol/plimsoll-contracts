# Wasm binary size

Soroban charges for code by the byte: a contract's wasm is a ledger entry that
is paid for at upload and, on every invocation, is part of the footprint the
transaction must carry. A wasm that grows by a kilobyte is a cost every user of
that contract pays for as long as it is deployed. This page records where the
bytes are, which features are the expensive ones, and how the repository keeps
growth visible.

## How size is checked

Two checks run in the `test` job of `.github/workflows/ci.yml`, in this order,
after `stellar contract build`:

| Check | File | Behaviour |
| --- | --- | --- |
| Hard ceiling | `size-budgets.json`, `scripts/check_wasm_sizes.py` | Fails the job when a wasm is over its budget, or when a built wasm has no budget. |
| Early warning | `wasm-baseline.json`, `scripts/check_wasm_growth.py` | Prints a `::warning::` annotation when a wasm is more than 5% larger than its recorded baseline. **Never fails the job.** |

The two work together. The budget is the number that must not be crossed; it
carries deliberate headroom (roughly 10-20% when it was last raised), so a
change can eat that headroom one feature at a time without any single pull
request looking wrong. The 5% warning fires while the headroom is still being
spent, when it is cheap to review, and the budget fires when it is gone.

A warning is not a verdict. When growth is intended, re-measure, record the new
size in `wasm-baseline.json` in the same pull request, and say why in the
description. When it is not intended, the sections below say where to look.

## Measuring a build

```bash
stellar contract build --locked

# Section-level sizes: the shape of the binary. Works on any build.
wasm-objdump -h target/wasm32v1-none/release/coverage_ledger.wasm

# Item-level sizes: the largest functions and data segments.
twiggy top -n 20 target/wasm32v1-none/release/coverage_ledger.wasm
twiggy dominators target/wasm32v1-none/release/coverage_ledger.wasm

# What the optimizer could still take out.
wasm-opt -Oz target/wasm32v1-none/release/coverage_ledger.wasm -o /tmp/ledger-oz.wasm
ls -l target/wasm32v1-none/release/coverage_ledger.wasm /tmp/ledger-oz.wasm
```

`[profile.release]` sets `strip = "symbols"`, `opt-level = "z"`, `lto = true`,
`codegen-units = 1` and `panic = "abort"`, which is already the smallest
configuration the toolchain offers. One consequence matters for profiling:
stripping removes the wasm name section, so in a release build `twiggy top`
can rank items only as `<unnamed>`. To read function names, temporarily build a
copy with `strip = "none"`, profile that, and revert — the named copy is a few
percent larger, so use it to find the hot items, not to set a budget.

## Where the bytes are

The only committed build artifact is the published release, so this table is
measured from **v0.1.0**, `coverage_ledger.wasm`, 14,313 bytes,
sha256 `040daa9a49cd01768fc1ab90854839c8bad28579e24745dda91ecd37ed772d65`
(2026-10-05). Regenerate the same table for the current tree with `wasm-objdump -h` after
`stellar contract build`. The current totals are recorded in
`size-budgets.json` (26,990 bytes for `coverage_ledger` on 2026-11-26, the last
measured value there); this page does not restate a per-section split it has
not measured. The reason the split is stable is visible in the table: only
`code` and `contractspecv0` grow with added code, and every feature since
v0.1.0 has added both.

Top 10 sections of `coverage_ledger.wasm`, by payload size:

| # | Section | Bytes | Share | What it is |
| --: | --- | --: | --: | --- |
| 1 | `code` | 9,084 | 63.5% | Every compiled function body in the contract. |
| 2 | `custom:contractspecv0` | 3,567 | 24.9% | The Soroban contract spec: an entry per exported function, its argument and return types, and every `LedgerError` variant. |
| 3 | `data` | 757 | 5.3% | Static data, including string constants and the panic/formatting tables the SDK pulls in. |
| 4 | `import` | 199 | 1.4% | Host functions the contract calls (`env.storage`, `env.ledger`, token calls, …). |
| 5 | `export` | 180 | 1.3% | The functions exposed to the host: `__constructor` and every `#[contractimpl]` entry point. |
| 6 | `type` | 148 | 1.0% | Function signatures shared by the type/import/function sections. |
| 7 | `custom:contractmetav0` | 147 | 1.0% | SDK metadata (compiler version, SDK version). |
| 8 | `custom:contractmetav0` | 83 | 0.6% | Contract metadata for the crate. |
| 9 | `function` | 56 | 0.4% | The function index. |
| 10 | `custom:contractenvmetav0` | 30 | 0.2% | Environment protocol metadata. |

The remaining 62 bytes are the `global` (17), `memory` (3) and `datacount` (1)
sections plus 41 bytes of section headers and the module header. For
comparison, the same release's other two contracts are
4,043 bytes (`reporter_registry`, code 49.3% / spec 29.5%) and 4,828 bytes
(`covered_vault`, code 58.1% / spec 24.3%).

Two things follow from the table, and they drive every decision below:

1. **The spec is a quarter of the binary.** Every public function, every
   parameter and every error variant is encoded twice: once as code and once
   as spec. Removing a feature saves its code *and* its spec entry, so public
   surface is the most expensive thing a contract can carry.
2. **Small contracts pay proportionally more for their spec.** The registry is
   half the ledger's size but its spec share is higher, because it has little
   logic and a lot of exported calls.

## Which features could be feature-flagged

The candidates below are ordered by expected saving, from the current source
(`contracts/`), reasoning from the sections above rather than from a build this
page does not contain. None of them are implemented as cargo features today;
this is the map a maintainer would use if a deployment needed a smaller
contract.

| Candidate | Where | What it would save | What it would cost |
| --- | --- | --- | --- |
| Time-locked upgrades (`propose_upgrade`, `execute_upgrade`, `cancel_upgrade`, `pending_upgrade`, `PendingUpgrade`) | Both contracts | Four exported functions, one type and three spec entries per contract, plus the `deployer().update_current_contract` path in code. The single largest removable block. | A deployment with no upgrade path cannot be fixed in place; the only way to change it is to redeploy and migrate every consumer. Not recommended for the live testnet ids. |
| Reporter metadata and display names (`update_reporter_metadata`, `get_reporter_metadata`, `set_reporter_name`, `get_reporter_name`) | `reporter-registry` | Three `Option<...>` parameters (the most expensive kind to encode), two storage keys and four exported functions — a large share of the registry's spec. | Dashboards lose the human-readable names and contact commitments; the `ReporterInfo.name` field still carries a short label. |
| Asset aliasing (`alias_asset`, `get_alias`, `Alias`) | `coverage-ledger` | Two exported functions, one key, and the alias resolution inside `coverage`, `is_covered` and `get_coverage_snapshot`. | A migrated asset SAC needs a redeploy of the ledger, which the vaults pin in their config; this is exactly the case aliasing was added for. |
| Native supply (`enable_native_supply`, `native_supply_token`, `SupplyTokenClient::total_supply`) | `coverage-ledger` | One cross-contract client, two exported functions, and the fallback branch in `effective_supply`. | Assets must keep a supply poster; `total_supply()` stops being an option. |
| Incident controls (`pause`, `unpause`, `is_paused`, `CoverageStatus`) | `coverage-ledger` | Three exported functions, one enum type with four variants in the spec, and the pause check at the top of both write paths. | An incident can no longer be answered on chain; consumers see an under-backed asset instead of a pause. |
| Retraction and preflight (`retract_reserve`, `can_post_report`) | `coverage-ledger` | Two exported functions and the `Result<(), LedgerError>` return type, which encodes every error variant again. | A reporter who posted a wrong figure must post a correcting one instead of withdrawing it. |

Feature-flagging a contract is not free even when the bytes are: every variant
is a different binary that needs its own audit, its own storage-compatibility
story (the `DataKey` discriminants must stay stable across variants) and its own
deployment record. It is a tool for a specific size problem, not a default.

## Rules for a change that grows the binary

- Raise a budget only in the pull request that needs the room, and say why in
  `size-budgets.json` with the measured size, as the existing entries do.
- If the 5% warning fires, either shrink the change or record the new baseline
  in `wasm-baseline.json` in the same pull request and explain the growth here
  or in the description.
- Prefer removing public surface to shrinking bodies: one dropped exported
  function saves its code, its export, its type entries and its spec entry.
