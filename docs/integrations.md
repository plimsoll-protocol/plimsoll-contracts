# Coverage Ledger Client Helper

`plimsoll-types` provides `is_covered` so a Soroban contract can make the
coverage-ledger call without declaring the generated client itself:

```rust
use plimsoll_types::{is_covered, Tier};

let allowed = is_covered(
    &env,
    &coverage_ledger_address,
    &asset_sac_address,
    10_000,
    86_400,
    Tier::IssuerSigned,
);
if !allowed {
    return Err(MyError::AssetNotCovered);
}
```

The helper accepts the environment, ledger contract address, asset SAC address,
minimum coverage in basis points, maximum data age in seconds, and minimum
report tier. It returns `false` for every policy failure; it does not turn an
uncovered asset into a contract error.

For a complete contract, see [the coverage-gate example](../examples/coverage-gate/src/lib.rs).
Its integration tests call the helper against the real coverage-ledger and
reporter-registry contracts:

```sh
cargo test -p coverage-gate
```

Add `plimsoll-types` as a dependency from this repository at a pinned revision
and use the same `soroban-sdk` version as that revision.