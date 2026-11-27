# Emergency Pause Behaviour

When `coverage-ledger` is paused via `pause()`, any in-flight `post_supply` or `post_reserve` transaction that arrives while the ledger is paused is **rejected immediately** with `LedgerError::Paused` (`#17`) — it is neither queued on-chain nor silently dropped. Because `require_not_paused` runs right after caller authentication and before any storage mutation or validation, no partial state is written and the submitter receives a deterministic contract error, allowing off-chain posters and reporters to detect the rejection and retry once the pause is lifted.

## Unpausing Safely

To unpause safely, the admin should first confirm that the underlying incident is resolved (for example, that any compromised reporter or supply poster has been revoked in `reporter-registry` via `revoke`) and that off-chain indexers and reporters are ready to resume posting. The admin then calls `unpause()`, which is idempotent and emits an `unpaused {by}` event. Because reports rejected during the pause were not queued, reporters and supply posters must re-submit any missed snapshots or reserve reports after `unpause()` completes; existing stored figures may also have aged past `max_age` during the pause, so `is_covered` will continue returning `false` (and `coverage_status` will return `Stale`) until fresh figures are posted.
