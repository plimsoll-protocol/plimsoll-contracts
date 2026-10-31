# PUSD standing reserve statement — TESTNET DEMO

**This is a demonstration document for Stellar testnet. PUSD is not a real
asset and nothing here describes real money.**

| Field | Value |
| --- | --- |
| Asset | PUSD |
| Issuer | `GBIE3ANCRVCBWETUZXWYKRMP27LQVJHX757XXAQUT3LYHVTNFPPPXEY4` |
| Network | Stellar testnet |
| Reserves held | 1,020,000.00 USD-equivalent |

PUSD is the demo's well-backed asset: reserves exceed the roughly 999,900 PUSD
in circulation, so coverage sits a little over 100% and
`is_covered(PUSD, 10000, 604800, 2)` answers true.

## Reserve composition (illustrative)

| Holding | Amount |
| --- | --- |
| Cash at custodian bank | 255,000.00 |
| US Treasury bills, < 90 days | 765,000.00 |
| **Total** | **1,020,000.00** |

## Why this statement is not dated

The reserve figure above is fixed for the demo, but a reserve report has to be
*fresh* to satisfy the coverage check: the PUSD vault asks for one at most 7
days old. So this statement is restated on chain every few days by
`scripts/refresh-demo-reserves.sh`, and each `reserve_posted` event carries its
own `as_of`, `posted_at` and reporter.

Dating the document instead meant writing a new file per restatement, which
rotted the moment nobody wrote one: the newest statement here was dated
2026-10-07, so it expired on 2026-10-14, and for the sixteen days until
2026-10-30 the coverage check answered false for a 102%-backed asset and the
PUSD vault refused every deposit. The dated statements from that period are
kept alongside this file because reports already on chain point at them.

Current coverage, and the date of the latest restatement, are on chain:

```bash
stellar contract invoke --network testnet \
  --source-account GDZ4BDTLOG3HTYIRFKSZE3ZMDU5SYCTQ4WB6YBH5WWMEJRSNFOC2PBKG --send=no \
  --id CC2QQ7R4FLPHZP7KA5AO4IASXXICTG6N4GQXCXITTEBNYQEYTYXKMN4D \
  -- coverage --sac CCHPT4TEJDPZQDSUVW3NP6A35ROGV4WEFZKT45SKWYCFZSUB7R5HIM7S --min_tier 2
```

## How this document is used

The SHA-256 of this file is recorded on-chain as `doc_hash` in the
`reserve_posted` event of the Plimsoll coverage ledger, at the Issuer-signed
and Auditor-signed tiers. Anyone can download the file, hash it, and compare.
