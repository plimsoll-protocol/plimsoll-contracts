<p align="center">
  <img src="docs/banner.svg" alt="Plimsoll" width="100%" />
</p>



<p align="center">
  <a href="https://github.com/plimsoll-protocol/plimsoll-contracts/actions/workflows/ci.yml"><img src="https://github.com/plimsoll-protocol/plimsoll-contracts/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <img src="https://img.shields.io/badge/soroban--sdk-28.0.0-0f1d2b" alt="soroban-sdk 28" />
  <img src="https://img.shields.io/badge/network-testnet-c8341f" alt="testnet" />
  <a href="https://plimsoll-protocol.gitbook.io/plimsoll-protocol-docs/"><img src="https://img.shields.io/badge/docs-gitbook-0f1d2b" alt="Docs" /></a>
  <img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="Apache-2.0" />
  <img src="https://img.shields.io/badge/audit-none-lightgrey" alt="unaudited" />
</p>

# Plimsoll contracts

Soroban contracts that answer one question about a Stellar-issued asset: **how
much of what is circulating is backed, according to whom, and how recently?**

Circulating supply is computed from public ledger data and posted on-chain with
a hash of its breakdown. Reserve figures are posted by the issuer's own
account, a registered auditor, or a registered transcriber, and the signer sets
the report's tier. Any contract can then call:

```rust
is_covered(asset, min_bps, max_age, min_tier) -> bool
```

and refuse an asset that is under-backed or whose figures are out of date.
[docs/INTEGRATING.md](docs/INTEGRATING.md) walks through gating your own
contract on it: the dependency, the call, choosing each parameter, and the
edge cases.

| Repo | What it is |
| --- | --- |
| **plimsoll-contracts** (this repo) | Rust/Soroban contracts |
| [plimsoll-app](https://github.com/plimsoll-protocol/plimsoll-app) | Web app and TypeScript SDK |
| [plimsoll-indexer](https://github.com/plimsoll-protocol/plimsoll-indexer) | Go service: supply poster, event indexer, read API |

**Live app:** https://plimsoll-app-gths-amber.vercel.app/ (mirror: https://plimsoll-protocol.github.io/plimsoll-app/)  
**Docs:** https://plimsoll-protocol.gitbook.io/plimsoll-protocol-docs/

## Maintainers

| Maintainer | GitHub | Contact |
| --- | --- | --- |
| dunnidev | [@dunnidev](https://github.com/dunnidev) | [GitHub Discussions](https://github.com/plimsoll-protocol/plimsoll-contracts/discussions) |

Questions, ideas and contributor coordination happen in
[Discussions](https://github.com/plimsoll-protocol/plimsoll-contracts/discussions).

## Contracts

| Contract | Responsibility |
| --- | --- |
| [`reporter-registry`](contracts/reporter-registry) | Who may speak: auditors, transcribers and supply posters, managed by an admin. |
| [`coverage-ledger`](contracts/coverage-ledger) | What was said: asset listings, supply snapshots, reserve reports, and the `coverage` / `is_covered` queries. |
| [`covered-vault`](contracts/covered-vault) | Reference consumer: refuses deposits while the asset is not covered; never blocks withdrawals. |
| [`plimsoll-types`](crates/types) | Shared types, basis-point math and cross-contract client traits. |

```
reporter-registry  ◀── role_of ──  coverage-ledger  ◀── is_covered ──  covered-vault
                                         │
                                         └── name() ──▶ Stellar Asset Contract
```

The full function-by-function specification is in [docs/SPEC.md](docs/SPEC.md),
and the event topic and data layouts every off-chain reader depends on are in
[docs/EVENTS.md](docs/EVENTS.md). What each error code means, from
`Error(Contract, #1)` up, and what a caller should do about it is in
[docs/ERRORS.md](docs/ERRORS.md).

What each actor (admins, supply poster, reporters, issuers, anyone) can make
the ledger say, what limits them and what an integrator should do about it is
in [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md). Every admin operation
(upgrades, moving admin to a multisig, onboarding and revoking reporters) is a
tested, step-by-step procedure in [docs/RUNBOOK.md](docs/RUNBOOK.md).

### Why it is Stellar-specific

- **Issued assets are a protocol feature.** Supply for any classic asset can be summed from trustlines, claimable balances, liquidity pools and contract balances, with no cooperation from the issuer.
- **The issuer account can sign Soroban auth.** A report signed by `asset.issuer` is unforgeable, which is what the Issuer-signed tier relies on.
- **Every asset has a built-in Stellar Asset Contract** whose `name()` is `CODE:ISSUER`. `list_asset` accepts only genuine SACs and parses that name, so a look-alike token cannot be listed under a real issuer.

### Tiers

| Tier | Value | Who signs |
| --- | --- | --- |
| Auditor-signed | 3 | Address registered with the `Auditor` role |
| Issuer-signed | 2 | The asset's issuer account |
| Transcribed | 1 | Address registered with the `Transcriber` role |

## Testnet deployment

| Contract | Address |
| --- | --- |
| reporter-registry | [`CCZCBR7MGSW5LGIEUQR5TU5Z7JQWBDEESB3RGPTRPMCBUMOISRPY7GOB`](https://stellar.expert/explorer/testnet/contract/CCZCBR7MGSW5LGIEUQR5TU5Z7JQWBDEESB3RGPTRPMCBUMOISRPY7GOB) |
| coverage-ledger | [`CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D`](https://stellar.expert/explorer/testnet/contract/CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D) |
| covered-vault | [`CC3SUECAJSAC4PBV4QLUII7X5Y4WNVL5YGTBAQ4CHCAH6R3QLRYJUZHQ`](https://stellar.expert/explorer/testnet/contract/CC3SUECAJSAC4PBV4QLUII7X5Y4WNVL5YGTBAQ4CHCAH6R3QLRYJUZHQ) |

Listed demo assets: `PUSD` (demo issuer, SAC `CCHPT4…HIM7S`) and Circle's testnet `USDC`
(SAC `CBIELT…DAMA`). Full record: [deployments/testnet.json](deployments/testnet.json).

## Quick start

Requirements: Rust at the exact version pinned in
[rust-toolchain.toml](rust-toolchain.toml) (rustup installs it, with the
`wasm32v1-none` target, the first time you run `cargo` here), and
[Stellar CLI](https://developers.stellar.org/docs/tools/cli) 28 or later. To
reproduce CI's wasm hashes exactly, use the CLI version CI pins (see
[Bumping the Rust toolchain and the Stellar CLI](#bumping-the-rust-toolchain-and-the-stellar-cli)).

```bash
git clone https://github.com/plimsoll-protocol/plimsoll-contracts
cd plimsoll-contracts

stellar contract build     # wasm in target/wasm32v1-none/release/
cargo test                 # 72 tests across all crates

python3 scripts/check_wasm_sizes.py   # each wasm against its size budget
```

The size budgets live in [size-budgets.json](size-budgets.json) and CI enforces
them, so a contract cannot quietly grow towards the install limit. Raising one
is fine; doing it in the commit that needs the room is the point.

The per-call CPU and memory ceilings live in
[resource-budgets.json](resource-budgets.json). The tests in
`contracts/coverage-ledger` measure `is_covered`, `post_supply` and
`post_reserve` with `env.cost_estimate()` and fail if a call grows past its
ceiling, printing what it used against what it is allowed. Raise a ceiling
deliberately, in the commit that needs the room, and say why; otherwise a new
storage read or cross-contract call can slip onto another contract's hot path
unnoticed.

Deploy your own copy to testnet. `deploy-testnet.sh` remains the one-command
demo path and is a thin wrapper around the two scripts below:

```bash
./scripts/deploy-testnet.sh
```

### Deploying to any network

Deployment and demo seeding are separate:

```bash
./scripts/deploy.sh <network>       # contracts only, writes deployments/<network>.json
./scripts/seed-demo.sh <network>    # demo identities, PUSD and its vault
```

`deploy.sh` creates no identities, funds nothing and seeds no demo data. It
needs a funded identity named `plimsoll-admin` (override with
`ADMIN_IDENTITY`), builds the wasm, and writes the deployment record. `testnet`,
`futurenet` and `local` are built into the Stellar CLI; add any other network
with `stellar network add`. For a network whose built-in entry has no RPC URL
(mainnet), set `STELLAR_RPC_URL`. `seed-demo.sh` refuses to run on mainnet,
by name or by passphrase.

A local quickstart network works end to end:

```bash
# terminal 1: a local Stellar network with Soroban RPC on localhost:8000
docker run --rm -p 8000:8000 --name stellar stellar/quickstart:latest --local

# terminal 2: from the repository
stellar keys generate plimsoll-admin --network local --fund
./scripts/deploy.sh local
./scripts/seed-demo.sh local
```

The ids of the local deployment, including the demo vault, are written to
`deployments/local.json`.

Query the live deployment:

```bash
stellar contract invoke --network testnet --send=no \
  --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D -- \
  coverage --sac CCHPT4TEJDPZQDSUVW3NP6A35ROGV4WEFZKT45SKWYCFZSUB7R5HIM7S --min_tier 1
```

### Verifying a deployed contract against a release

Every `v*` tag publishes a GitHub release containing the three wasm files,
their interface specs (`*.spec.json`) and `SHA256SUMS`. Download a release and
check it before comparing it with what is deployed:

```bash
gh release download v0.1.0 --pattern '*'
sha256sum -c SHA256SUMS

stellar contract info hash --wasm coverage_ledger.wasm
stellar contract info hash --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D --network testnet
```

The two hashes must match. `deployments/testnet.json` records the wasm hash
currently installed under `liveWasm` whenever a contract has been upgraded past
its first deployment; for contracts not listed there, the release that matches
the deployment is the one whose wasm hash matches `contract info hash --id`.
The spec JSON is what SDK authors should generate bindings from, so they do not
need to build the wasm to know a release's interface.

### Verifying a deployment against this checkout

`scripts/verify-deployment.sh` does the comparison above for every contract in
a deployments file at once, against a local build of whatever is checked out:

```bash
./scripts/verify-deployment.sh testnet    # reads deployments/testnet.json, needs no keys
```

It prints `ok` or `MISMATCH` per contract, with the expected (local) and actual
(on-chain) hashes, and exits non-zero on any mismatch. The wasm is identical
across Linux and macOS for the same inputs, but the Stellar CLI writes its own
version into the wasm, so build with the CLI version CI pins. On a mismatch the
script prints the rustc, soroban-sdk and stellar-cli versions each side was
built with, so a toolchain difference is not mistaken for a code difference.
The [Verify deployment](.github/workflows/verify-deployment.yml) workflow runs
it on demand with CI's pinned toolchain.

### Keeping the demo honest

A reserve report expires. The PUSD vault asks for one at most 7 days old, so
the demo needs its reserves restated or it quietly stops demonstrating
anything — `is_covered` starts answering false for a well-backed asset, and
both demo vaults refuse deposits for different reasons that look identical
from outside.

```bash
./scripts/refresh-demo-reserves.sh verify   # does the demo still say what it claims?
./scripts/refresh-demo-reserves.sh post     # restate the reserves (needs the reporter keys)
```

`verify` needs no keys: it asks the contract whether PUSD is covered and QUSD
is refused, and checks that every report's `doc_hash` still matches the
document it points at. The [Demo coverage](.github/workflows/demo.yml) workflow
runs it twice a day and restates the reserves when they are more than three
days old, which keeps the standing statements in
[docs/demo](docs/demo) matching what is on chain.

### End-to-end test against the built wasm

The per-contract tests run natively. `tests/e2e` instead loads the three
compiled contracts from `target/wasm32v1-none/release/`, the same files the
deploy scripts upload, and runs the whole flow: register reporters, list an
asset, post supply and reserves, deposit, let the report go stale, deposit
refused, withdraw still works. It only compiles once the wasm exists, so it is
not part of a plain `cargo test`:

```bash
stellar contract build
cargo test -p plimsoll-e2e
```

CI runs it straight after the wasm build.

### Test coverage

CI measures which contract lines the tests reach with
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov). Every pull request
gets a per-crate table in the `coverage` job's log and on the run's summary
page, and the full lcov report as the `coverage-lcov` artifact. To produce the
same report locally:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked

./scripts/coverage.sh          # target/coverage/lcov.info + per-crate summary
./scripts/coverage.sh --html   # also target/coverage/html/index.html
```

Test files are left out of the figures, and so are `tests/e2e` (it runs the
compiled wasm, which carries no coverage instrumentation) and
`examples/coverage-gate` (documentation, not a deployed contract).

### Bumping the Rust toolchain and the Stellar CLI

Both are pinned, so the compiler and CLI that check the size budgets, run
clippy and build the release wasm only change in a commit.

**Rust.** The version lives only in [rust-toolchain.toml](rust-toolchain.toml).
Locally rustup reads it, and in CI
[`.github/actions/setup-rust`](.github/actions/setup-rust/action.yml) installs it
and fails if any other `rustc` ends up active. To bump:

1. Set `channel` in `rust-toolchain.toml` to the new exact version (for example
   `"1.101.0"`; a bare `stable` is rejected by CI).
2. Run the full check locally: `cargo fmt --all --check`,
   `cargo clippy --all-targets -- -D warnings`, `stellar contract build --locked`,
   `python3 scripts/check_wasm_sizes.py`, `cargo test`.
3. If a size or resource budget moves, adjust it in the same commit and say why.
4. Commit as `ci: bump Rust to <version>`.

**Stellar CLI.** The version and the SHA-256 of its Linux archive live only in
[`.github/actions/install-stellar-cli/action.yml`](.github/actions/install-stellar-cli/action.yml),
which every workflow uses. CI refuses to extract an archive whose checksum does
not match. To bump:

1. Read the new release's checksum from GitHub:

   ```bash
   gh api repos/stellar/stellar-cli/releases/tags/v<version> \
     -q '.assets[] | select(.name | test("x86_64-unknown-linux-gnu.tar.gz")) | .digest'
   ```

2. Confirm it independently against the archive itself:

   ```bash
   curl -sSfLO https://github.com/stellar/stellar-cli/releases/download/v<version>/stellar-cli-<version>-x86_64-unknown-linux-gnu.tar.gz
   sha256sum stellar-cli-<version>-x86_64-unknown-linux-gnu.tar.gz
   ```

3. Set `STELLAR_CLI_VERSION` and `STELLAR_CLI_SHA256` in the action to the new
   version and the matching hash, and update the CLI row in
   [AGENTS.md](AGENTS.md).
4. Commit as `ci: bump Stellar CLI to <version>`. The wasm hashes change with
   the CLI version, so note it in the next release.

## Contributing

Issues labelled [`good first issue`](https://github.com/plimsoll-protocol/plimsoll-contracts/labels/good%20first%20issue)
are scoped for a first pull request. Read [CONTRIBUTING.md](CONTRIBUTING.md) before you start,
and comment on an issue to have it assigned.

Security reports: see [SECURITY.md](SECURITY.md). Do not open public issues for vulnerabilities.
Trust assumptions per actor: [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

## Contributors

<a href="https://github.com/plimsoll-protocol/plimsoll-contracts/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=plimsoll-protocol/plimsoll-contracts" alt="Contributors" />
</a>

## License

[Apache-2.0](LICENSE)



