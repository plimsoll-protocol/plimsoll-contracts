# QUSD standing reserve statement — TESTNET DEMO

**This is a demonstration document for Stellar testnet. QUSD is not a real
asset and nothing here describes real money.**

| Field | Value |
| --- | --- |
| Asset | QUSD |
| Issuer | `GCCMGQ3UGITN2HUUSQKTDMBFZ34QQTKMODR56NT26JMPH2MHVYWZMXNE` |
| Network | Stellar testnet |
| Reserves held | 480,000.00 USD-equivalent |

QUSD is the demo's **deliberately under-backed** asset: 480,000 of reserves
against 500,000 in circulation is 96% coverage, so
`is_covered(QUSD, 10000, 2678400, 1)` answers false and the QUSD vault refuses
deposits with `NotCovered`. That refusal is the point of the asset.

The refusal has to happen for the right reason. A report that has gone stale
also makes `is_covered` false, which looks identical from outside and would
turn the demo into a demonstration of nothing. So this statement is restated
on chain every few days, keeping it fresh and leaving coverage as the only
thing failing.

## Reserve composition (illustrative)

| Holding | Amount |
| --- | --- |
| Cash at custodian bank | 120,000.00 |
| US Treasury bills, < 90 days | 360,000.00 |
| **Total** | **480,000.00** |

## Why this statement is not dated

See [pusd-reserves.md](pusd-reserves.md): the figure above is fixed, while
freshness lives on chain in each `reserve_posted` event's `as_of`. Restating
is handled by `scripts/refresh-demo-reserves.sh`. The earlier dated statements
are kept because reports already on chain point at them.

## How this document is used

The SHA-256 of this file is recorded on-chain as `doc_hash` in the
`reserve_posted` event of the Plimsoll coverage ledger, at the Transcribed
tier, posted by the demo's registered transcriber. Anyone can download the
file, hash it, and compare.
