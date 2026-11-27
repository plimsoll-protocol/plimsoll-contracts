# Contributing to Plimsoll contracts

Thanks for helping. This file covers how to pick up work, what a pull request
must contain, and the standards the code is held to.

## Picking up an issue

1. Find an open issue. `good first issue` ones are small and self-contained.
2. Comment that you want it. Wait to be assigned before starting, so two people
   do not build the same thing.
3. If the issue is unclear, ask in the issue. Do not guess at contract behaviour.

During a Drips Wave, assignment works the same way; points are attached to the
issue, not to the pull request.

## Local setup

```bash
rustup target add wasm32v1-none
cargo test
stellar contract build
```

Dependency policy is checked with `cargo-deny`:

```bash
cargo install cargo-deny --locked
cargo deny check
```

`deny.toml` covers advisories, licences and duplicate versions. Any ignored
advisory has a comment explaining why it cannot be fixed yet.

## Pull request checklist

- [ ] One logical change per pull request. Link the issue (`Closes #12`).
- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo deny check` pass.
- [ ] If an event changed, `tests/snapshots/` is regenerated with
      `UPDATE_SNAPSHOTS=1 cargo test -p plimsoll-event-snapshots` and the diff
      is reviewed alongside [docs/EVENTS.md](docs/EVENTS.md).
- [ ] New public functions have tests for the success path **and** every error they can return.
- [ ] Every `#[contracterror]` variant is asserted by a test:
      `python3 scripts/check_error_coverage.py` passes. A variant that cannot be
      reached is listed in that script with a reason.
- [ ] If behaviour changed, [docs/SPEC.md](docs/SPEC.md) is updated in the same pull request.
- [ ] Commit messages use [Conventional Commits](https://www.conventionalcommits.org/): `feat(ledger): …`, `fix(registry): …`, `test(vault): …`.

## Code standards

- No `unwrap()` or `expect()` on user-controlled data outside tests. `expect` is
  allowed only for invariants set in the constructor (e.g. the admin key).
- Errors are `#[contracterror]` enums raised with `panic_with_error!`. Never
  renumber an existing error code.
- No floats. Ratios are basis points (`10_000 = 100%`) computed in `i128`.
- Every persistent write extends TTL with the constants in `plimsoll-types`.
- Every state change emits a `#[contractevent]`. The indexer depends on them;
  changing an event's fields is a breaking change and needs an issue first.
  `tests/events` pins the topic and data shape of every ledger and vault event
  against the files in `tests/snapshots/`; an unintentional change fails
  `cargo test`.
- `require_auth()` on the address whose authority is used, before any write.

## AI-assisted contributions

Allowed, but you must understand and test every line you submit. Untested or
unexplained generated code will be closed, in line with the Drips Wave rules.

## Code of conduct

Be direct and kind. Review the code, not the person.
