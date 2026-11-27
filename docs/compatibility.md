# Version and protocol compatibility

Which contract wasm runs on which network, and which toolchain produces it.
This page is the compatibility contract for build and deploy: if a combination
is not in a table here, it is not supported, whatever happens to compile.

Two different things are being pinned, and mixing them up is the usual source
of a deployment that builds but will not install:

- The **contract wasm** embeds the interface and spec of the `soroban-sdk` it
  was compiled against. A network whose protocol is older than that SDK will
  reject the wasm at upload or install.
- The **build toolchain** (Stellar CLI, Rust, wasm target) decides the bytes of
  that wasm. The same source with a different CLI version produces a different
  hash, so a deployment can only be reproduced with the CLI version recorded in
  the release.

## Compatibility matrix

`soroban-sdk`'s major version tracks the Stellar protocol it targets: SDK 28
targets protocol 28, SDK 29 targets protocol 29. A contract built with SDK *N*
can be uploaded to any network running protocol *N* or later; the reverse is
not true.

| Stellar protocol | Rust XDR | Smart contract host | `soroban-sdk` | Stellar CLI | Stellar RPC | Plimsoll contract wasm |
| --- | --- | --- | --- | --- | --- | --- |
| 28 (mainnet 2026-09-16) | `28.0.0` | `28.0.2` | `28.0.0` | `28.1.0` (min `25.2.0` for SDK 28+) | `v28.0.1` | wasm built from this source with SDK 28 |
| 29 (mainnet 2026-10-01) | `28.0.1` | `29.0.0` | `29.0.0` / `29.0.1` | `28.1.0` | `v29.0.0` | wasm built from this source with SDK 28 or 29 (SDK 29 is what `main` pins) |
| 30 (mainnet 2026-11-23) | `30.0.0` | `30.0.1` | `29.0.1` recommended; `30.0.0-rc.1` for protocol-30 features | `30.0.1` (CLI 30+ is required to build SDK 30) | `v30.0.1` | SDK 28/29 wasm continues to run unchanged; no Plimsoll release targets SDK 30 yet |

Notes on the rows:

- The protocol numbers, host environment versions and RPC versions come from
  Stellar's [software versions page](https://developers.stellar.org/docs/networks/software-versions).
  Re-read it before adding a row; it is the source of truth, not this file.
- Protocol 30's own SDK is still a release candidate, so Stellar recommends the
  SDK 29 line (`29.0.1`) for contracts on a protocol-30 network. That is what
  this repository pins.
- RPC versions move with the network, not with the wasm. An RPC client (the
  indexer, a web app, the Stellar CLI) must speak the protocol of the network
  it talks to; it does not constrain which SDK built the contract.
- Protocol 27 and earlier are not supported: the workspace pins `soroban-sdk`
  29.0.1, and an SDK 29 wasm cannot be installed on those networks.

## What this repository pins

| Pin | Version | Where |
| --- | --- | --- |
| Rust toolchain | `1.100.0` | [`rust-toolchain.toml`](../rust-toolchain.toml) |
| wasm target | `wasm32v1-none` | `rust-toolchain.toml` (`targets`) |
| `soroban-sdk` | `29.0.1` | [`Cargo.toml`](../Cargo.toml) workspace dependency, resolved in `Cargo.lock` |
| Stellar CLI | `28.1.0` | [`.github/actions/install-stellar-cli/action.yml`](../.github/actions/install-stellar-cli/action.yml), with the archive's SHA-256 |
| Contract version | `0.1.0` | `[workspace.package] version` in `Cargo.toml`; release tags publish it |
| wasm size ceilings | per contract | [`size-budgets.json`](../size-budgets.json) |
| CPU and memory ceilings | per hot call | [`resource-budgets.json`](../resource-budgets.json) |

CI installs the Rust toolchain through
[`.github/actions/setup-rust`](../.github/actions/setup-rust/action.yml) and the
CLI through the action above, so both are identical in CI, in a release build
and on a maintainer's machine. `stellar contract build` is required: from SDK 28
on, a plain `cargo build --target wasm32v1-none` refuses to build a contract,
because the CLI strips unreachable spec entries after compilation. The CLI
version also ends up inside the wasm, which is why a wasm built with a
different CLI has a different hash.

## Before you upgrade any of these

Changing the SDK, the CLI or the Rust toolchain changes the wasm bytes and can
change the size and resource budgets. Treat an upgrade as a release-sized
change, not a dependency bump:

1. Read the target network's row on the
   [software versions page](https://developers.stellar.org/docs/networks/software-versions)
   and the SDK's release notes for the migration steps and the minimum CLI.
   SDK *N* requires a CLI at least as new as the version named in its release
   notes (CLI `25.2.0` for SDK 28+).
2. Bump the workspace `soroban-sdk` in `Cargo.toml`, then refresh `Cargo.lock`
   (for example `cargo update -p soroban-sdk`), so CI and local builds resolve
   the same version.
3. If the new SDK raises the minimum supported Rust version above
   `rust-toolchain.toml`, bump the toolchain in its own commit. The pin is an
   exact version, never `stable`.
4. Bump `STELLAR_CLI_VERSION` and `STELLAR_CLI_SHA256` in
   [`.github/actions/install-stellar-cli/action.yml`](../.github/actions/install-stellar-cli/action.yml)
   together, taking the hash from the release asset's `.digest` and confirming
   it with `sha256sum`. CI refuses to extract a mismatched archive.
5. Build and run the checks in the order CI does:

   ```bash
   cargo fmt --all --check
   cargo clippy --all-targets -- -D warnings
   stellar contract build --locked
   python3 scripts/check_wasm_sizes.py
   cargo test
   python3 scripts/check_error_coverage.py
   ```

   If `tests/events` reports a changed spec or event schema, decide whether it
   is expected; regenerate snapshots only with `UPDATE_SNAPSHOTS=1 cargo test -p
   plimsoll-event-snapshots`, alongside the `docs/SPEC.md` / `docs/EVENTS.md`
   change that explains it.
6. If a size or resource budget moved, raise it in the same commit and say why
   in `size-budgets.json` / `resource-budgets.json`. Do not absorb growth by
   quietly increasing a number in a later commit.
7. Deploy to testnet, then record the new wasm hash in
   `deployments/testnet.json` (`liveWasm`) and verify with
   `./scripts/verify-deployment.sh testnet`. Build with the CLI version CI pins,
   or the hash will differ for toolchain reasons alone. The script prints the
   rustc, `soroban-sdk` and stellar-cli versions behind each side, precisely so
   a toolchain difference is not mistaken for a code difference.
8. Update this page (add or amend the row), the version table in
   [`AGENTS.md`](../AGENTS.md) and the `soroban-sdk` badge in
   [`README.md`](../README.md), then commit. Conventional commit types:
   `ci:` for the toolchain and CLI pins, `spec:` or `chore:` for this page.

## Verifying a deployed contract's toolchain

Given a running network and a `deployments/<network>.json`, the quickest check
that a deployment matches this checkout is:

```bash
./scripts/verify-deployment.sh testnet
```

It prints `ok` or `MISMATCH` per contract with both hashes, exits non-zero on
any mismatch, and needs no keys. The
[Verify deployment](../.github/workflows/verify-deployment.yml) workflow runs
the same script on demand with CI's pinned toolchain. A GitHub release (`v*`
tag) publishes the three wasm files, their interface specs and `SHA256SUMS`, so
anyone can rebuild and compare without trusting this checkout.

## Known constraints

- **`wasm32v1-none` only.** `wasm32-unknown-unknown` is not supported by the
  Soroban runtime; on Rust 1.82+ it enables `reference-types` and
  `multi-value`, which the runtime rejects. Build through
  `stellar contract build`.
- **No `cargo build` for wasm.** Since SDK 28 the spec must be post-processed
  by the CLI. Only native `cargo test` and `cargo clippy` work without it.
- **Toolchain-specific hashes.** The wasm is reproducible for a fixed CLI,
  Rust and SDK version, but not across CLI versions: the CLI records its own
  version in the wasm.
- **Release candidates are not supported.** An `-rc` SDK is pinned explicitly
  with its exact version string or not at all; nothing here floats.
- **The matrix is forward-only.** A row is added when the repository starts
  targeting a protocol, and old rows stay so an integrator can tell whether an
  old deployment is still installable.
