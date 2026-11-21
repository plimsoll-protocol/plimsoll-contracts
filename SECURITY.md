# Security policy

## Audit status

**These contracts have not been audited.** They are deployed on Stellar
testnet only. Do not use them to protect real funds.

## Reporting a vulnerability

Report privately through GitHub:
[Security → Report a vulnerability](https://github.com/plimsoll-protocol/plimsoll-contracts/security/advisories/new).

Please include:

- the contract and function affected,
- a description of the impact,
- steps or a test case that reproduces it.

Do not open a public issue or pull request for a vulnerability. We aim to
acknowledge reports within 72 hours and to agree a disclosure date with you.

## Scope

In scope:

- `reporter-registry`, `coverage-ledger`, `covered-vault` and `plimsoll-types`
- Authorisation bypasses (posting as a tier you do not hold, listing a fake asset)
- Logic errors that make `coverage` or `is_covered` return a wrong answer
- Storage/TTL issues that can lose or freeze state

Out of scope:

- The accuracy of reserve figures themselves. Plimsoll records who signed a
  figure; it does not audit the figure.
- Testnet resets and RPC availability.
- Issues in third-party dependencies with no exploitable path here (report
  those upstream).

## Known trust assumptions

- The registry admin decides who is an auditor, transcriber or supply poster.
- Supply snapshots are computed off-chain by a registered poster. They are
  verifiable (the breakdown hash is on-chain) but not proven on-chain.

## Upgrades

Both stateful contracts (`reporter-registry`, `coverage-ledger`) upgrade in
place. The admin proposes a wasm hash with `propose_upgrade`; it can only be
installed by `execute_upgrade` after `UPGRADE_DELAY_LEDGERS = 17_280` ledgers
(about one day at a five-second close time). The admin can cancel a proposal,
and a new proposal replaces the old one and restarts the delay.

This means a compromised admin key cannot swap code instantly. It does not
make the admin trustworthy: it can still propose and eventually execute any
code. The delay is the window in which holders, vault depositors and the
indexer can observe the `upgrade_proposed` event and stop interacting with, or
exit, the affected contracts. The vaults have no admin pause, so withdrawals
remain available throughout. Consumers that must not run changed code should
pin the wasm hash they audited and treat a proposal as a signal to migrate.

The upgrade entry point is itself admin-only and emits an event for every
step, so a proposal that was never announced on-chain did not come from this
contract.
