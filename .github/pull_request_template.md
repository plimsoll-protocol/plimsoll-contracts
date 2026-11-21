# Pull request checklist

- [ ] One logical change per pull request. Link the issue (`Closes #12`).
- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.
- [ ] New public functions have tests for the success path **and** every error they can return.
- [ ] If behaviour changed, [docs/SPEC.md](docs/SPEC.md) is updated in the same pull request.
- [ ] Commit messages use [Conventional Commits](https://www.conventionalcommits.org/): `feat(ledger): …`, `fix(registry): …`, `test(vault): …`.

## Summary

<!-- What does this PR do? Why? Link the issue. -->

Closes #

## Changes

<!-- List the key changes made in this PR. -->

-
-
-

## Testing

<!-- How did you test this? -->

-
-
-

## SPEC.md updates

<!-- If behaviour changed, what changed in SPEC.md? -->

-
-
-

## Deployment notes

<!-- Any special deployment considerations? Contract addresses, migration steps, etc. -->

-