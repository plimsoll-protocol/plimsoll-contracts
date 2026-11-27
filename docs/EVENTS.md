# Event layouts

Every figure Plimsoll publishes reaches off-chain readers as a contract event.
The indexer rebuilds its tables from these alone, so their shape is part of the
protocol's surface: changing a topic order or a data key breaks every reader.
This file is the normative record of that shape.

Events are declared with `#[contractevent]` in each contract, and the field
order below is the declaration order. Two rules follow from how soroban-sdk 28
encodes them:

- **Topics** are the event name first (the struct name in `snake_case`), then
  each `#[topic]` field in declaration order. Topics are filterable; the data
  map is not.
- **Data** is an `ScMap` of the remaining fields, keyed by symbol. A map's
  entries are sorted by key, so **do not read the data map positionally** —
  key order in the encoded event is alphabetical, not declaration order.

`Address` appears as a strkey (`G…` for an account, `C…` for a contract),
`BytesN<32>` as 64 hex characters, `i128` as a decimal string (amounts are in
the asset's smallest unit, 7 decimals for a Stellar asset), `u64` timestamps as
seconds since the Unix epoch, and `Tier` / `Role` as the `u32` discriminants in
[SPEC.md](SPEC.md).

## coverage-ledger

`CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D` on testnet.

### `asset_listed`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `asset_listed` | `Symbol` |
| topic 1 | `sac` | `Address` (contract) |
| data | `code` | `String` |
| data | `issuer` | `Address` (account) |

Raised once per asset by `list_asset`, which is permissionless. The code and
issuer are read from the asset's own Stellar Asset Contract, not supplied by the
caller, so a look-alike token cannot be listed under a real issuer.

Testnet: PUSD was listed in
[`4d000675…`](https://stellar.expert/explorer/testnet/tx/4d000675f11b64e0f699574a901f648724aae4e425646d20c388aeebc8c4a258),
with `code = "PUSD"` and
`issuer = GBIE3ANCRVCBWETUZXWYKRMP27LQVJHX757XXAQUT3LYHVTNFPPPXEY4`.

### `supply_posted`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `supply_posted` | `Symbol` |
| topic 1 | `sac` | `Address` (contract) |
| data | `amount` | `i128` |
| data | `ledger` | `u32` |
| data | `breakdown_hash` | `BytesN<32>` |
| data | `poster` | `Address` (account) |

`ledger` is the ledger the supply was counted at, which is behind the ledger the
event was emitted in. `breakdown_hash` is the SHA-256 of the canonical JSON
breakdown the poster computed; the indexer serves those exact bytes at
`/v1/breakdowns/{hash}` so anyone can recompute the hash and the total.

As `stellar events --output json` prints it, captured from testnet:

```json
{
  "id": "0023476291239972864-0000000000",
  "ledger": 5466000,
  "ledger_closed_at": "2026-10-30T09:47:20Z",
  "type": "contract",
  "contract_id": "CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D",
  "event_name": "SupplyPosted",
  "prefix_topics": ["supply_posted"],
  "params": {
    "sac": "CCHPT4TEJDPZQDSUVW3NP6A35ROGV4WEFZKT45SKWYCFZSUB7R5HIM7S",
    "amount": "9999000000000",
    "ledger": 5465998,
    "breakdown_hash": "2f54ff8cd9d84fa4f6f40a91f449de6fde92caa0b64b24d352e376d39f1384a5",
    "poster": "GDACI55IMI5DDZWMZ2EAXWK57AV75VE4626YYBAHQJYDV7QFPNNFK6HJ"
  }
}
```

The first supply post on testnet was
[`c3ae90c4…`](https://stellar.expert/explorer/testnet/tx/c3ae90c40b1dbf0575e0e9babaefd9ae20dee457fb5b37d2d40105f005f6b81b).

### `reserve_posted`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `reserve_posted` | `Symbol` |
| topic 1 | `sac` | `Address` (contract) |
| topic 2 | `tier` | `Tier` (`u32`) |
| data | `amount` | `i128` |
| data | `as_of` | `u64` |
| data | `reporter` | `Address` (account) |
| data | `doc_hash` | `BytesN<32>` |
| data | `doc_uri` | `String` (at most 256 characters) |

The tier is a topic, so a reader can subscribe to auditor-signed reports alone.
The contract decides the tier from who signed: the asset's own issuer account,
a registered auditor, or a registered transcriber. `as_of` is the date the
reserve figure describes, which is what freshness is judged on; the ledger
timestamp when it was posted is stored on chain as `posted_at` but is not in the
event.

Testnet, issuer-signed, 10,200,000 PUSD as of 2026-10-05:
[`2b41a94e…`](https://stellar.expert/explorer/testnet/tx/2b41a94efe4cf881ab097ce9b8e0e14f4cf90f9cc4fe38958b9b9d18e7bb9c0c).
Auditor-signed, 10,150,000 as of 2026-10-04:
[`b434db8d…`](https://stellar.expert/explorer/testnet/tx/b434db8d240502176ef22f066a1d16ad3e9a4e9c35a283d0e2837bede3a811ce).

### `reserve_retracted`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `reserve_retracted` | `Symbol` |
| topic 1 | `sac` | `Address` (contract) |
| topic 2 | `tier` | `Tier` (`u32`) |
| data | `reporter` | `Address` (account) |
| data | `as_of` | `u64` |

Raised after the reporter's latest report at the resolved tier is removed.
The reporter must be the one who posted that report. Events retain the
retracted report's identity and effective date for off-chain history; the next
qualifying report, if any, becomes eligible for coverage.

### `admin_transferred`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `admin_transferred` | `Symbol` |
| topic 1 | `new_admin` | `Address` (account) |

Raised by `transfer_admin`, which requires both the outgoing and incoming
admin to authorise. No testnet example: admin has never been transferred on the
demo deployment.

### `paused`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `paused` | `Symbol` |
| data | `by` | `Address` (account) |

Raised by `pause()` after the flag is set. The ledger stops recording figures
and `is_covered` raises `Paused`; see [SPEC.md](SPEC.md#emergency-pause).
Idempotent: calling `pause()` while already paused still emits the event, so a
watcher sees the admin asserting the state either way.

### `unpaused`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `unpaused` | `Symbol` |
| data | `by` | `Address` (account) |

Raised by `unpause()` after the flag is cleared.

### `native_supply_enabled`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `native_supply_enabled` | `Symbol` |
| topic 1 | `sac` | `Address` (contract) |
| data | `token` | `Address` (contract) |

Raised by `enable_native_supply`, which opts an asset in to reading supply
from a SEP-41 token's `total_supply()` instead of the posted snapshot. The
snapshot is kept and is used again if the token call fails.

### `upgrade_proposed`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `upgrade_proposed` | `Symbol` |
| topic 1 | `wasm_hash` | `BytesN<32>` |
| data | `executable_at_ledger` | `u32` |

Raised by `propose_upgrade`. `executable_at_ledger` is the first ledger at
which `execute_upgrade` may succeed. A new proposal replaces any pending one
and restarts the clock.

### `upgrade_executed`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `upgrade_executed` | `Symbol` |
| topic 1 | `wasm_hash` | `BytesN<32>` |

No data map. Raised when `execute_upgrade` installs the proposed wasm. If the
installation fails the transaction rolls back, so neither the event nor the
cleared proposal survives.

### `upgrade_cancelled`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `upgrade_cancelled` | `Symbol` |
| topic 1 | `wasm_hash` | `BytesN<32>` |

No data map. Raised by `cancel_upgrade`, which clears the pending proposal.

## reporter-registry

`CCZCBR7MGSW5LGIEUQR5TU5Z7JQWBDEESB3RGPTRPMCBUMOISRPY7GOB` on testnet.

### `reporter_set`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `reporter_set` | `Symbol` |
| topic 1 | `reporter` | `Address` (account) |
| data | `role` | `Role` (`u32`) |
| data | `name` | `String` (at most 64 characters) |

Raised both when a reporter is first registered and when its role or name
changes, so a reader should treat it as an upsert keyed on `reporter`.

Testnet: the indexer's supply poster was registered in
[`f4681dc3…`](https://stellar.expert/explorer/testnet/tx/f4681dc3ea065f770a1e715f1f43ae08171e40ef9cb6228c791d12c45a5b1c34)
(`role = 3`, `name = "Plimsoll indexer"`), the demo auditor in
[`44343a1e…`](https://stellar.expert/explorer/testnet/tx/44343a1e2ed5837159cc84b5a7cfd24b1d60ef12ce589b2b1a9d5ac7946dbc08)
(`role = 1`), and the demo transcriber in
[`6f9bce5a…`](https://stellar.expert/explorer/testnet/tx/6f9bce5af1b52dc53058c3d172591a36cb3b4e1839c726a56aca0a83ae3fd4ee)
(`role = 2`).

### `reporter_metadata_updated`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `reporter_metadata_updated` | `Symbol` |
| topic 1 | `reporter` | `Address` (account) |
| data | `name` | `Option<Symbol>` |
| data | `website_uri` | `Option<String>` |
| data | `contact_hash` | `Option<Bytes>` |

Raised by `update_reporter_metadata` when a registered reporter replaces their
optional public metadata. An absent field is encoded as ScVal void, so a reader
must treat "no key" and "void" as the same thing. The preimage of
`contact_hash` is never published on-chain. `revoke` removes the metadata
entry without a separate event; treat `reporter_revoked` as clearing it.

### `reporter_revoked`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `reporter_revoked` | `Symbol` |
| topic 1 | `reporter` | `Address` (account) |

### `stake_changed`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `stake_changed` | `Symbol` |
| topic 1 | `reporter` | `Address` (account) |
| data | `old_stake` | `i128` |
| data | `new_stake` | `i128` |

Raised by `top_up_stake` and `reduce_stake` whenever a reporter's tracked stake
is updated, and by `revoke` (with `new_stake = 0`) if the revoked reporter held
a positive tracked stake.

No data map. Revocation does not delete the reports a reporter already posted,
but from that transaction on the coverage ledger stops counting them:
`coverage`, `is_covered`, `get_report` and `get_reports` skip an auditor or
transcriber report whose reporter no longer holds the matching role (see
SPEC.md). No `reserve_posted` or other ledger event is raised for this; a
reader that wants to mirror `coverage` should do the same as the contract and
treat the reporter's reports as inactive from this event until a later
`reporter_set` gives the role back. The indexer leaves the history in place. No
testnet example: nothing has been revoked on the demo deployment.

### `admin_transferred`

Same layout as the coverage ledger's, above. The two are distinct events on
distinct contracts, so filter on `contract_id` as well as the topic.

### `upgrade_proposed`, `upgrade_executed`, `upgrade_cancelled`

Same layouts as the coverage ledger's upgrade events, above. The two contracts
emit them independently, so filter on `contract_id` as well as the topic.

## covered-vault

`CC3SUECAJSAC4PBV4QLUII7X5Y4WNVL5YGTBAQ4CHCAH6R3QLRYJUZHQ` on testnet (PUSD),
and `CBURLBHWLWOTKJ5NVF574NAYFM7Y25JB366ZDO7WGJXCVC4C72UVKTO5` (QUSD, the
deliberately under-backed demo asset, where deposits are refused).

The vault is an example consumer, not part of the protocol. The indexer does not
ingest its events.

### `deposited`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `deposited` | `Symbol` |
| topic 1 | `from` | `Address` |
| data | `amount` | `i128` |
| data | `balance_after` | `i128` |

Raised only after `is_covered` returned true, which is the point of the example:
a deposit of an under-backed asset never reaches this event.

Testnet:
[`f86eecaa…`](https://stellar.expert/explorer/testnet/tx/f86eecaa2eb6252bd4e79708966fc510573b8a9878771641b8f8b514d2804b5b).

### `coverage_dropped`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `coverage_dropped` | `Symbol` |
| topic 1 | `asset` | `Address` (contract) |

No data map. Raised by `deposit` after its own `deposited` event, when a
second `is_covered` call, made once the deposit is recorded, answers `false`
with the vault's stored config. The gate that let the deposit through asked
the same question before the transfer, and a deposit writes no supply snapshot
and no report, so on the ledger described in [SPEC.md](SPEC.md) the two
answers agree and this event is not published. It appears when the ledger's
answer changes between the two reads, which is the case a keeper watching the
vault needs in order to restate reserves; subscribe by topic and filter on
`contract_id` when several vaults share an asset.

A refused deposit publishes nothing at all: the `NotCovered` error rolls the
transaction back, and a rolled-back transaction keeps no events, so this is
the only vault event a keeper can learn a drop from. No testnet example: no
deposit has been made while coverage was failing.

### `withdrawn`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `withdrawn` | `Symbol` |
| topic 1 | `to` | `Address` |
| data | `amount` | `i128` |
| data | `balance_after` | `i128` |

Raised when a queued withdrawal is executed. Withdrawal is not gated on
coverage: a depositor can always leave once any configured delay has elapsed.
No testnet example yet.

### `withdrawal_queued`

| | Name | Type |
| --- | --- | --- |
| topic 0 | `withdrawal_queued` | `Symbol` |
| topic 1 | `to` | `Address` |
| data | `amount` | `i128` |
| data | `available_at` | `u64` |

Raised by `request_withdraw` after the request is stored. A new request for the
same address replaces the previous request and restarts the delay; changing the
configured delay does not affect a request already queued.

## Reading these events

Soroban RPC serves roughly the last seven days of events, so an indexer cannot
rebuild history from RPC alone past that window — see
[plimsoll-indexer](https://github.com/plimsoll-protocol/plimsoll-indexer), which
keeps them in Postgres and reads current state back from the contracts to repair
gaps.

```bash
# every event from one contract, one JSON object per event
stellar events --network testnet \
  --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D \
  --start-ledger 5466000 --count 10 --output json
```

`--topic` segments are base64 XDR `ScVal`s, not plain text, and `*` matches one
segment. The encodings for the event names above are fixed, so they can be
pasted:

| Event name topic | Base64 `ScVal` |
| --- | --- |
| `asset_listed` | `AAAADwAAAAxhc3NldF9saXN0ZWQ=` |
| `supply_posted` | `AAAADwAAAA1zdXBwbHlfcG9zdGVkAAAA` |
| `reserve_posted` | `AAAADwAAAA5yZXNlcnZlX3Bvc3RlZAAA` |
| `reporter_set` | `AAAADwAAAAxyZXBvcnRlcl9zZXQ=` |
| `reporter_metadata_updated` | `AAAADwAAABlyZXBvcnRlcl9tZXRhZGF0YV91cGRhdGVk` |
| `reporter_revoked` | `AAAADwAAABByZXBvcnRlcl9yZXZva2Vk` |
| `admin_transferred` | `AAAADwAAABFhZG1pbl90cmFuc2ZlcnJlZAAAAA==` |
| `upgrade_proposed` | `AAAADwAAABB1cGdyYWRlX3Byb3Bvc2Vk` |
| `upgrade_executed` | `AAAADwAAABB1cGdyYWRlX2V4ZWN1dGVk` |
| `upgrade_cancelled` | `AAAADwAAABF1cGdyYWRlX2NhbmNlbGxlZA==` |
| `paused` | `AAAADwAAAAZwYXVzZWQAAA==` |
| `unpaused` | `AAAADwAAAAh1bnBhdXNlZA==` |
| `native_supply_enabled` | `AAAADwAAABVuYXRpdmVfc3VwcGx5X2VuYWJsZWQAAAA=` |

```bash
# supply posts for any asset: name, then any sac
stellar events --network testnet \
  --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D \
  --start-ledger 5466000 --output json \
  --topic 'AAAADwAAAA1zdXBwbHlfcG9zdGVkAAAA,*'

# auditor-signed reports only: name, any sac, tier 3 (AAAAAwAAAAM= is u32 3)
stellar events --network testnet \
  --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D \
  --start-ledger 5466000 --output json \
  --topic 'AAAADwAAAA5yZXNlcnZlX3Bvc3RlZAAA,*,AAAAAwAAAAM='
```

## Snapshot tests

The shape of every event above is pinned by
[`tests/events`](../tests/events/tests/snapshots.rs). It drives the ledger and
the vault through every event they can emit, normalises addresses, byte arrays
and strings to type placeholders, and compares the resulting topics and data
to the checked-in files under [`tests/snapshots`](snapshots). A change to an
event name, a topic's position or type, a data key, or a field's type fails
that test, which is what makes an unintentional event-schema change fail CI.

When a change is intentional, update [SPEC.md](SPEC.md) and this file, then
regenerate and review the snapshot:

```bash
UPDATE_SNAPSHOTS=1 cargo test -p plimsoll-event-snapshots
```

The snapshot records field **types and values that are not caller-specific**
(numbers, symbols, the tier topic), not addresses or hashes, because those are
random in the test host and are not part of the schema.
