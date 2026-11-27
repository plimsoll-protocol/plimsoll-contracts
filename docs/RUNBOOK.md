# Admin and upgrade runbook

This runbook covers every privileged operation on the Plimsoll contracts:
building and uploading wasm, upgrading, verifying a deployment, moving admin
to a multisig, and onboarding and revoking reporters. Each procedure is a
numbered list of commands. Each command shows the output it should print, and
every state change is followed by a step that checks it.

Read the whole procedure before running its first step. Rehearse it on a
throwaway deployment first ([section 1](#1-rehearse-on-a-throwaway-deployment)).
The first real upgrade should not be the first upgrade.

What each function does, who may call it and what it emits is in
[SPEC.md](SPEC.md). The [threat model](THREAT_MODEL.md) explains why these
operations matter to integrators.

## Contents

0. [Before you start](#0-before-you-start)
1. [Rehearse on a throwaway deployment](#1-rehearse-on-a-throwaway-deployment)
2. [Build, hash and upload the wasm](#2-build-hash-and-upload-the-wasm)
3. [First upgrade of a pre-time-lock deployment](#3-first-upgrade-of-a-pre-time-lock-deployment)
4. [Time-locked upgrade](#4-time-locked-upgrade)
5. [Cancel or roll back an upgrade](#5-cancel-or-roll-back-an-upgrade)
6. [Verify a deployment](#6-verify-a-deployment)
7. [Move admin to a multisig](#7-move-admin-to-a-multisig)
8. [Run an admin call as the multisig](#8-run-an-admin-call-as-the-multisig)
9. [Onboard a reporter](#9-onboard-a-reporter)
10. [Revoke a reporter](#10-revoke-a-reporter)
11. [Errors you may see](#11-errors-you-may-see)
12. [Rehearsal record](#12-rehearsal-record)

## 0. Before you start

**Tools:**

- The Stellar CLI at the version CI pins (`STELLAR_CLI_VERSION` in
  [`.github/actions/install-stellar-cli/action.yml`](../.github/actions/install-stellar-cli/action.yml)).
  The CLI writes its version into every wasm it builds, so a different version
  produces a different hash for the same source.
- The Rust toolchain from [`rust-toolchain.toml`](../rust-toolchain.toml).
  rustup selects it automatically inside the repository.
- `jq` and `curl`.

**Keys:** the admin identity must be in the CLI keystore
(`stellar keys ls`). Once admin is a multisig (section 7), admin calls follow
section 8 instead.

**Session variables.** Run these once per shell, from the repository root. Every
command below uses them.

```bash
export NETWORK=testnet                  # a network name known to `stellar network ls`
export ADMIN_IDENTITY=plimsoll-admin    # the admin key in the CLI keystore
export DEP=deployments/$NETWORK.json
export REGISTRY=$(jq -r .contracts.reporterRegistry "$DEP")
export LEDGER=$(jq -r .contracts.coverageLedger "$DEP")
echo "registry $REGISTRY"; echo "ledger   $LEDGER"
```

Expected: two contract ids starting with `C`, matching the
[testnet table in the README](../README.md#testnet-deployment) when
`NETWORK=testnet`.

**Reading the expected output:**

- Lines shown after a command are what it prints on success. `<...>` marks a
  value that differs per run.
- A command that sends a transaction prints `🔗 https://stellar.expert/...` and,
  for functions that emit one, a `📅 <contract> - Success - Event: ...` line.
  The event line is the confirmation that matters.
- A failed call prints `❌ error: ...`. Contract errors look like
  `HostError: Error(Contract, #N)`. Section 11 lists the ones these procedures
  can produce.

## 1. Rehearse on a throwaway deployment

This creates a fresh pair of contracts on testnet under a second network name,
so that `scripts/deploy.sh` writes `deployments/plimsoll-rehearsal.json` and
never touches `deployments/testnet.json`.

1. Register testnet under a rehearsal name:

   ```bash
   stellar network add plimsoll-rehearsal \
     --rpc-url https://soroban-testnet.stellar.org \
     --network-passphrase "Test SDF Network ; September 2015"
   ```

   Expected: no output. `stellar network ls` now lists `plimsoll-rehearsal`.

2. Create and fund a throwaway admin:

   ```bash
   stellar keys generate rehearsal-admin --network plimsoll-rehearsal --fund
   ```

   Expected:

   ```
   ✅ Key saved with alias rehearsal-admin in "<config dir>/identity/rehearsal-admin.toml"
   ✅ Account rehearsal-admin funded on "Test SDF Network ; September 2015"
   ```

3. Deploy:

   ```bash
   ADMIN_IDENTITY=rehearsal-admin bash scripts/deploy.sh plimsoll-rehearsal
   ```

   Expected, at the end:

   ```
   ================ Plimsoll plimsoll-rehearsal deployment ================
   REPORTER_REGISTRY_ID=C<...>
   COVERAGE_LEDGER_ID=C<...>
   DEPLOYED_AT_LEDGER=<...>
   =============================================================
   Written to deployments/plimsoll-rehearsal.json
   ```

4. Verify the fresh deployment against your build:

   ```bash
   ./scripts/verify-deployment.sh plimsoll-rehearsal
   ```

   Expected, ending with:

   ```
   ok        reporterRegistry   C<...>  <64 hex>
   ok        coverageLedger     C<...>  <64 hex>
   All contracts on plimsoll-rehearsal match the local build.
   ```

5. Point the session at the rehearsal deployment, and run the procedure you
   are about to perform for real:

   ```bash
   export NETWORK=plimsoll-rehearsal ADMIN_IDENTITY=rehearsal-admin
   export DEP=deployments/$NETWORK.json
   export REGISTRY=$(jq -r .contracts.reporterRegistry "$DEP")
   export LEDGER=$(jq -r .contracts.coverageLedger "$DEP")
   ```

   **To rehearse section 3**, the deployment has to run the old code first.
   Deploy a pair from the wasm hashes the live contracts run (already on
   testnet, so nothing is uploaded). Then write a deployments file for the pair
   so `verify-deployment.sh` can check it:

   ```bash
   OLD_REGISTRY_WASM=$(stellar contract info hash --id "$(jq -r .contracts.reporterRegistry deployments/testnet.json)" --network testnet)
   OLD_LEDGER_WASM=$(stellar contract info hash --id "$(jq -r .contracts.coverageLedger deployments/testnet.json)" --network testnet)
   ADMIN=$(stellar keys address rehearsal-admin)
   REGISTRY=$(stellar contract deploy --wasm-hash "$OLD_REGISTRY_WASM" --source rehearsal-admin --network plimsoll-rehearsal -- --admin "$ADMIN")
   LEDGER=$(stellar contract deploy --wasm-hash "$OLD_LEDGER_WASM" --source rehearsal-admin --network plimsoll-rehearsal -- --admin "$ADMIN" --registry "$REGISTRY")
   cat > deployments/plimsoll-rehearsal-v01.json <<EOF
   {
     "network": "plimsoll-rehearsal-v01",
     "rpcUrl": "https://soroban-testnet.stellar.org",
     "networkPassphrase": "Test SDF Network ; September 2015",
     "contracts": { "reporterRegistry": "$REGISTRY", "coverageLedger": "$LEDGER" }
   }
   EOF
   export DEP=deployments/plimsoll-rehearsal-v01.json REGISTRY LEDGER
   ```

   `SKIP_BUILD=1 ./scripts/verify-deployment.sh plimsoll-rehearsal-v01` then
   reports `MISMATCH` for both, which is the starting point section 3 expects.

6. When you are done, delete the rehearsal deployments files. They are local
   and must not be committed:

   ```bash
   rm -f deployments/plimsoll-rehearsal.json deployments/plimsoll-rehearsal-v01.json
   ```

## 2. Build, hash and upload the wasm

Run this from a clean checkout of the exact commit you intend to install,
ideally a release tag.

1. Check out the commit and confirm the tree is clean:

   ```bash
   git fetch --tags && git checkout <tag or commit>
   git status --porcelain
   ```

   Expected: no output from `git status --porcelain`.

2. Confirm the toolchain:

   ```bash
   rustc --version
   stellar --version | head -1
   ```

   Expected: `rustc <channel from rust-toolchain.toml> (...)` and
   `stellar <STELLAR_CLI_VERSION from the install action> (...)`.

3. Build:

   ```bash
   stellar contract build --locked
   ```

   Expected: one `✅ Build Complete` per contract, and each summary prints a
   `Wasm Hash:`.

4. Hash the wasm you will install. The contract you are upgrading decides the
   file: `reporter_registry.wasm` or `coverage_ledger.wasm`.

   ```bash
   export WASM=target/wasm32v1-none/release/coverage_ledger.wasm
   export LOCAL_HASH=$(stellar contract info hash --wasm "$WASM"); echo "$LOCAL_HASH"
   ```

   Expected: 64 hex characters. If the commit is a release tag, this must equal
   that file's line in the release's `SHA256SUMS`, because the wasm hash is its
   SHA-256.

5. Upload it. Uploading installs the code on the network without attaching it
   to any contract:

   ```bash
   export HASH=$(stellar contract upload --wasm "$WASM" --source "$ADMIN_IDENTITY" --network "$NETWORK")
   echo "$HASH"
   ```

   Expected: the CLI logs `✅ Transaction submitted successfully!`, or
   `ℹ️  Skipping install because wasm already installed` if these exact bytes
   are already on the network. Either way, `echo` prints the same 64 hex
   characters as `LOCAL_HASH`.

6. **Verify** the network holds exactly these bytes:

   ```bash
   stellar contract fetch --wasm-hash "$HASH" --network "$NETWORK" -o /tmp/uploaded.wasm
   cmp /tmp/uploaded.wasm "$WASM" && echo identical
   ```

   Expected: `identical`.

## 3. First upgrade of a pre-time-lock deployment

The contracts first deployed to testnet expose the old immediate
`upgrade(wasm_hash)` and none of the time-lock functions. This procedure runs
once per contract to move it onto the code in this repository, which has the
time lock. From then on, use section 4.

Upgrade `reporter-registry` **before** `coverage-ledger`. The ledger asks the
registry for `max_reporters` when a reporter posts for an asset for the first
time, so a ledger running ahead of its registry fails those calls with a host
error for the missing function. The registry first is safe: it keeps
`role_of`, and answers the default cap until an admin sets one.

1. Confirm the contract really is pre-time-lock:

   ```bash
   stellar contract info interface --id "$LEDGER" --network "$NETWORK" --output json-formatted \
     | jq -r '.[] | select(.function_v0) | .function_v0.name' | grep upgrade
   ```

   Expected: exactly `upgrade`. If you see `propose_upgrade` instead, stop and
   use section 4.

2. Record what is installed now, for the change log and for a rollback:

   ```bash
   export PREVIOUS_HASH=$(stellar contract info hash --id "$LEDGER" --network "$NETWORK"); echo "$PREVIOUS_HASH"
   ```

   Expected: 64 hex characters. For the live testnet ledger this matches
   `liveWasm.coverageLedger` in `deployments/testnet.json`.

3. Record a state sample to compare after the upgrade:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- admin
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- \
     get_reports --sac "$(jq -r .assets.PUSD.sac "$DEP")"
   ```

   Expected: the admin's `G...` address, and a JSON array of reports. Save both.
   (On a rehearsal deployment, use a SAC you listed there.)

4. Build, hash, upload and verify the new wasm with section 2. `HASH` is now
   set.

5. Upgrade. This takes effect immediately and **emits no event** on the old
   code ([#14](https://github.com/plimsoll-protocol/plimsoll-contracts/issues/14)):

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     upgrade --wasm_hash "$HASH"
   ```

   Expected:

   ```
   ✅ Transaction submitted successfully!
   🔗 https://stellar.expert/explorer/testnet/tx/<...>
   ```

6. **Verify the code:**

   ```bash
   stellar contract info hash --id "$LEDGER" --network "$NETWORK"
   stellar contract info interface --id "$LEDGER" --network "$NETWORK" --output json-formatted \
     | jq -r '.[] | select(.function_v0) | .function_v0.name' | grep upgrade
   ```

   Expected: `$HASH`, then `cancel_upgrade`, `execute_upgrade`,
   `pending_upgrade` and `propose_upgrade`, with no plain `upgrade`.

7. **Verify the state** by re-running step 3 and adding a check that nothing is pending:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- pending_upgrade
   ```

   Expected: the same admin and the same reports as in step 3, then `null`.

8. Repeat steps 1 to 7 with `REGISTRY` and `reporter_registry.wasm`. For the
   state sample in step 3, use `admin` and
   `role_of --reporter <a known auditor>`. Expect the same role number
   afterwards.

9. Record the upgrade: section 6, steps 3 and 4.

## 4. Time-locked upgrade

The contract installs nothing until `UPGRADE_DELAY_LEDGERS` (17 280 ledgers,
about 24 hours) after the proposal. Plan around that delay. Announce the
proposal so integrators can review the wasm in the window.

1. Build, hash, upload and verify the new wasm with section 2. `HASH` is now
   set. The commands below use `LEDGER`; substitute `REGISTRY` for the registry.

2. Check nothing else is pending:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- pending_upgrade
   ```

   Expected: `null`. A new proposal **replaces** a pending one and restarts the
   clock, so if something is pending, find out why before continuing.

3. Propose:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     propose_upgrade --wasm_hash "$HASH"
   ```

   Expected:

   ```
   ✅ Transaction submitted successfully!
   📅 <LEDGER> - Success - Event: UpgradeProposed (upgrade_proposed), wasm_hash: "<HASH>", executable_at_ledger: <N>
   ```

4. **Verify the proposal:**

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- pending_upgrade
   stellar ledger latest --network "$NETWORK" | head -1
   ```

   Expected: `{"executable_at_ledger":<N>,"wasm_hash":"<HASH>"}` and
   `Sequence: <S>`, with `N - S` close to 17 280. The wait is roughly
   `(N - S) × 5` seconds.

5. Optional: confirm the time lock is enforced. Executing early is refused in
   simulation, so nothing is sent:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- execute_upgrade
   ```

   Expected: `❌ error: transaction simulation failed: HostError: Error(Contract, #15)`
   on the ledger, or `#4` on the registry: `UpgradeDelayNotElapsed`.

6. Announce the proposal with the contract, the hash, the commit or release it
   was built from, and `executable_at_ledger`. Reviewers can fetch the exact
   bytes with
   `stellar contract fetch --wasm-hash <HASH> --network <network> -o proposed.wasm`.

7. Once `stellar ledger latest` reports a sequence of at least `N`, execute:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- execute_upgrade
   ```

   Expected:

   ```
   ✅ Transaction submitted successfully!
   📅 <LEDGER> - Success - Event: UpgradeExecuted (upgrade_executed), wasm_hash: "<HASH>"
   ```

8. **Verify the upgrade:**

   ```bash
   stellar contract info hash --id "$LEDGER" --network "$NETWORK"
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- pending_upgrade
   ```

   Expected: `$HASH`, then `null`. Then run section 6.

## 5. Cancel or roll back an upgrade

**Cancel a pending proposal** (before it is executed):

1. Cancel it:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- cancel_upgrade
   ```

   Expected:

   ```
   📅 <LEDGER> - Success - Event: UpgradeCancelled (upgrade_cancelled), wasm_hash: "<HASH>"
   ```

2. **Verify:**

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- pending_upgrade
   ```

   Expected: `null`. Cancelling again fails with `Error(Contract, #14)` on the
   ledger, or `#3` on the registry: `UpgradeNotProposed`.

**Roll back an executed upgrade:** there is no shortcut. Run section 4 with
`HASH` set to the previous wasm hash (`PREVIOUS_HASH` from section 3, or the
hash in the `upgrade_executed` event before it). The rollback waits out the
same delay. On the ledger, reports already migrated to the per-reporter layout
are invisible to v0.1 code, so after rolling back to v0.1 each reporter must
post again (SPEC.md, *Upgrading a v0.1 deployment*).

## 6. Verify a deployment

Run this after every upgrade, and whenever you want to know what a deployment
is running. It needs no keys.

1. Compare every contract in the deployments file with a local build:

   ```bash
   ./scripts/verify-deployment.sh "$NETWORK"
   ```

   Expected, when everything matches:

   ```
   ==> stellar contract build --locked
   ==> testnet (https://soroban-testnet.stellar.org)
   ok        reporterRegistry   C<...>  <hash>
   ok        coverageLedger     C<...>  <hash>
   ok        coveredVault       C<...>  <hash>
   All contracts on testnet match the local build.
   ```

   On a mismatch, it prints `MISMATCH` with the expected and actual hashes and
   the rustc, soroban-sdk and stellar-cli versions each side was built with,
   then exits 1. A mismatch in which only the toolchain differs means the build
   inputs differ, not necessarily the source. Rebuild with the versions the
   on-chain side reports before drawing conclusions. covered-vault has no
   upgrade path, so a vault deployed from older code stays `MISMATCH` until it
   is redeployed.

2. Check the contracts answer as before:

   ```bash
   ./scripts/refresh-demo-reserves.sh verify     # testnet only: the demo's coverage answers
   ```

   Expected: every line starts with `ok`, and there is no closing
   `The demo is not showing what it claims` message. That message means a
   coverage answer is wrong. It is not always caused by the upgrade: stale
   reserves produce it too, and `post` fixes those.

3. Record the new hash in the deployments file, under `liveWasm`, keyed by
   contract:

   ```bash
   jq --arg h "$HASH" '.liveWasm.coverageLedger = $h' "$DEP" > "$DEP.tmp" && mv "$DEP.tmp" "$DEP"
   git diff "$DEP"
   ```

   Expected: exactly one changed or added line, under `liveWasm`.

4. Commit the deployments file on its own, as
   `chore(deploy): record <contract> upgrade to <short hash>`, through a pull
   request.

## 7. Move admin to a multisig

The target is a classic Stellar account controlled by M of N signer keys, with
its own master key at weight 0. The contracts see one address, the account.
The Stellar network enforces the threshold on every transaction the account
signs. Do this for each contract: `transfer_admin` moves one contract at a time.

Choose M and N so that losing one key does not lock you out. A 2-of-3 is the
smallest arrangement that survives one lost key. **No recovery exists:** if
fewer than M signers remain, the admin role is lost for good, and so is the
ability to upgrade.

1. Each signer creates a key on their own machine and shares only the public
   address:

   ```bash
   stellar keys generate signer-<name>
   stellar keys address signer-<name>
   ```

   Expected: a `G...` address. Collect the N addresses as `SIGNER_1` ... `SIGNER_N`.

2. Create and fund the multisig account. On testnet:

   ```bash
   stellar keys generate plimsoll-ops --network "$NETWORK" --fund
   export OPS=$(stellar keys address plimsoll-ops); echo "$OPS"
   ```

   Expected: `✅ Account plimsoll-ops funded ...` and a `G...` address. On a
   network without friendbot, create it from a funded account with
   `stellar tx new create-account --source "$ADMIN_IDENTITY" --destination "$OPS" --starting-balance 50000000 --network "$NETWORK"`.

3. Transfer admin while `plimsoll-ops` is still single-key. Both the current
   and the new admin must authorise. The CLI signs the new admin's
   authorisation with the matching identity in the local keystore, so run this
   where both keys are present:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     transfer_admin --new_admin "$OPS"
   ```

   Expected:

   ```
   ✅ Transaction submitted successfully!
   📅 <REGISTRY> - Success - Event: AdminTransferred (admin_transferred), new_admin: "<OPS>"
   ```

4. **Verify the transfer:**

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- admin
   ```

   Expected: `"<OPS>"`. Any admin call sourced from the old key now stops at
   signing with `❌ error: Missing signing key for account <OPS>`.

5. Repeat steps 3 and 4 with `LEDGER`.

6. Add the signers, one per transaction, each at weight 1:

   ```bash
   stellar tx new set-options --source plimsoll-ops --network "$NETWORK" --signer "$SIGNER_1" --signer-weight 1
   stellar tx new set-options --source plimsoll-ops --network "$NETWORK" --signer "$SIGNER_2" --signer-weight 1
   stellar tx new set-options --source plimsoll-ops --network "$NETWORK" --signer "$SIGNER_3" --signer-weight 1
   ```

   Expected: each prints `ℹ️  Signing transaction: <hash>` and returns with no
   error.

7. **Verify the signers before going further.** The next step removes the
   master key:

   ```bash
   curl -s "https://horizon-testnet.stellar.org/accounts/$OPS" | jq '[.signers[] | {key, weight}]'
   ```

   Expected: the three `SIGNER_*` addresses at weight 1, and `OPS` itself at
   weight 1. (Use your network's Horizon URL off testnet.)

8. Set the threshold and zero the master key in one transaction:

   ```bash
   stellar tx new set-options --source plimsoll-ops --network "$NETWORK" \
     --master-weight 0 --low-threshold 2 --med-threshold 2 --high-threshold 2
   ```

   Expected: `ℹ️  Signing transaction: <hash>` and no error. This is the last
   transaction the `plimsoll-ops` key can sign alone. Remove it from the
   keystore afterwards (`stellar keys rm plimsoll-ops`).

9. **Verify the multisig:**

   ```bash
   curl -s "https://horizon-testnet.stellar.org/accounts/$OPS" | jq '{thresholds, signers: [.signers[] | {key, weight}]}'
   ```

   Expected:

   ```json
   {
     "thresholds": { "low_threshold": 2, "med_threshold": 2, "high_threshold": 2 },
     "signers": [
       { "key": "<SIGNER_*>", "weight": 1 },
       { "key": "<SIGNER_*>", "weight": 1 },
       { "key": "<SIGNER_*>", "weight": 1 },
       { "key": "<OPS>", "weight": 0 }
     ]
   }
   ```

10. Prove a single signer cannot act and two can: section 8, steps 4 and 5.

## 8. Run an admin call as the multisig

With the multisig as the transaction's source account, its `require_auth` is
satisfied by the transaction's own signatures, so the CLI's
build, simulate, sign and send commands are all that is needed. The example
onboards a reporter. Any admin call works the same way, with its arguments
after `--`.

1. Build the transaction, using only public data:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source-account "$OPS" --network "$NETWORK" --build-only -- \
     set_reporter --reporter "$REPORTER" --role 1 --name "Example Auditor" > tx.xdr
   ```

   Expected: no output. `tx.xdr` holds one line of base64.

2. Simulate it to add the footprint and resource fee:

   ```bash
   stellar tx simulate --source-account "$OPS" --network "$NETWORK" < tx.xdr > tx.sim.xdr
   ```

   Expected: no error. `tx.sim.xdr` holds one line of base64. A simulation
   failure such as `Error(Contract, #2)` means the call itself is invalid. Fix
   it before collecting signatures.

3. Each signer signs in turn, passing the file along:

   ```bash
   stellar tx sign --sign-with-key signer-<name> --network "$NETWORK" < tx.sim.xdr > tx.1.xdr   # first signer
   stellar tx sign --sign-with-key signer-<name> --network "$NETWORK" < tx.1.xdr > tx.2.xdr     # second signer
   ```

   Expected: each prints `ℹ️  Signing transaction: <hash>`, with the **same**
   hash both times. A different hash means a signer signed a different
   transaction.

4. Optional, once after section 7: confirm one signature is not enough:

   ```bash
   stellar tx send --network "$NETWORK" < tx.1.xdr
   ```

   Expected: `❌ error: transaction submission failed: TxBadAuth`. Nothing is
   applied and the sequence number is not consumed, so step 5 still works.

5. Send the fully signed transaction:

   ```bash
   stellar tx send --network "$NETWORK" < tx.2.xdr
   ```

   Expected: `ℹ️  Transaction hash is <hash>`, a stellar.expert link, and a
   JSON result containing `"status": "SUCCESS"`.

6. **Verify** with the read for that call. For this example:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source-account "$OPS" --network "$NETWORK" --send=no -- role_of --reporter "$REPORTER"
   ```

   Expected: `1`.

Sign and send promptly. The transaction carries the account's next sequence
number, so if anything else is sent from `OPS` in between, rebuild from step 1.

## 9. Onboard a reporter

Roles: `1` Auditor (tier 3 reports), `2` Transcriber (tier 1 reports), `3`
Supply poster. Issuers need no role: an asset's issuer account always posts at
tier 2. Names are 1 to 64 characters.

Supply posters must keep the configured minimum native XLM balance. The
registry does not hold or lock their XLM; the ledger checks the poster's
current balance each time it accepts a supply snapshot. Admin-managed
`set_reporter` remains available for all roles, but it does not bypass this
live balance check.

To configure the minimum (with a multisig admin, run this call through section
8), set `MIN_STAKE_STROOPS` to a non-negative amount in stroops:

```bash
export MIN_STAKE_STROOPS=100000000
export NATIVE_XLM_SAC=$(stellar contract id asset --asset native --network "$NETWORK")
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
   set_stake_requirement --stake_asset "$NATIVE_XLM_SAC" --min_stake_stroops "$MIN_STAKE_STROOPS"
```

The `stake_requirement_set` event confirms the SAC address and minimum. Read
back the amount with `min_stake_stroops` before onboarding posters.

1. Confirm the address has no role yet:

   ```bash
   export REPORTER_IDENTITY=plimsoll-indexer
   export REPORTER=$(stellar keys address "$REPORTER_IDENTITY")
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- role_of --reporter "$REPORTER"
   ```

   Expected: `null`. Any other value means the reporter already has a role.
   `set_reporter` would change it while keeping its original `added_at`.

2. For a supply poster, register the role directly from that account after
   funding it with at least `MIN_STAKE_STROOPS` native XLM:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$REPORTER_IDENTITY" --network "$NETWORK" -- \
     register_reporter --reporter "$REPORTER" --name "Example Indexer"
   ```

   Expected: `ReporterSet` with role `3`. Verify `has_minimum_stake` returns
   `true`. If the admin registers a supply poster with `set_reporter` instead,
   it must still meet the balance requirement before posting.

3. For an auditor or transcriber, grant the role as admin (with a multisig
   admin, run this call through section 8):

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     set_reporter --reporter "$REPORTER" --role 1 --name "Example Auditor"
   ```

   Expected:

   ```
   📅 <REGISTRY> - Success - Event: ReporterSet (reporter_set), reporter: "<REPORTER>", role: 1, name: "Example Auditor"
   ```

4. **Verify** the role and reporter record:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- role_of --reporter "$REPORTER"
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reporter --reporter "$REPORTER"
   ```

   Expected: the selected role (`1`, `2`, or `3`), then its reporter record.

5. Optional: once the reporter has posted a figure, confirm it counts:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reports --sac "$SAC"
   ```

   Expected: an entry with `"reporter":"<REPORTER>"` and `"tier":3` for an
   auditor (`1` for a transcriber).

### Onboarding several reporters at once

`set_reporter` writes one reporter per transaction. To populate a fresh
registry, `batch_register` takes the same arguments as a list and applies them
all or nothing: one bad name fails the whole call and nothing is written.

1. Check the current cap, which bounds how many reporters one asset may hold:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- max_reporters
   ```

   Expected: `20` on a registry that has never had it set.

2. Register the batch (with a multisig admin, run this call through
   section 8):

   ```bash
   export REPORTER_1=G<...> REPORTER_2=G<...>
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     batch_register --reporters "$(jq -nc --arg one "$REPORTER_1" --arg two "$REPORTER_2" \
       '[{reporter:$one, role:1, name:"Auditor One"},{reporter:$two, role:2, name:"Transcriber Two"}]')"
   ```

   Expected: one `📅 ReporterSet (reporter_set)` line per entry, in the order
   given. A `#2` (`InvalidName`) or `#6` (`EmptyBatch`) failure changes
   nothing.

3. **Verify:** repeat step 3 of the single-reporter procedure for each
   address.

To change how many reporters an asset may hold, set the cap and check it:

```bash
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
  set_max_reporters --max 12
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- max_reporters
```

Expected: a `MaxReportersSet (max_reporters_set)` event with `max: 12`, then
`12`. The cap is enforced by `coverage-ledger.post_reserve` for reporters new
to an asset, so a `#16` (`TooManyReportersPerAsset`) there is the cap doing its
job. `--max 0` is refused with `#5` (`InvalidMaxReporters`).

## 10. Revoke a reporter

Revoking takes effect at once. The reporter's figures stop counting in
`coverage`, `is_covered`, `get_report` and `get_reports` in the same
transaction, with no second step. Their reports stay in storage, and in the
event history, and count again if the role is restored.

1. Record what the reporter currently contributes, for the incident log:

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reporter --reporter "$REPORTER"
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reports --sac "$SAC"
   ```

   Expected: the reporter's record, and the reports for each asset you care
   about, with their entries among them.

2. Revoke (with a multisig admin, run this call through section 8):

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
     revoke --reporter "$REPORTER"
   ```

   Expected:

   ```
   📅 <REGISTRY> - Success - Event: ReporterRevoked (reporter_revoked), reporter: "<REPORTER>"
   ```

3. **Verify the role is gone:**

   ```bash
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- role_of --reporter "$REPORTER"
   stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reporter --reporter "$REPORTER"
   ```

   Expected: `null`, then `null`.

4. **Verify their figures no longer count**, by repeating the `get_reports`
   call from step 1:

   ```bash
   stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" --send=no -- get_reports --sac "$SAC"
   ```

   Expected: the same array without the revoked reporter's entries (`[]` if
   they were the only reporter).

5. For a supply poster, the latest snapshot stays in force until another
   poster replaces it. Check `get_supply --sac "$SAC"`. If its `poster` is the
   revoked address, have an honest poster post a fresh snapshot.

Revoking an address with no role fails with `Error(Contract, #1)`
(`ReporterNotFound`). That is also what a second revoke returns.

## 11. Errors you may see

| Where | Error | Meaning |
| --- | --- | --- |
| reporter-registry | `Error(Contract, #1)` `ReporterNotFound` | `revoke` on an address with no role. |
| reporter-registry | `Error(Contract, #2)` `InvalidName` | `set_reporter` or `batch_register` name empty or over 64 characters. |
| reporter-registry | `Error(Contract, #3)` `UpgradeNotProposed` | `execute_upgrade` or `cancel_upgrade` with nothing pending. |
| reporter-registry | `Error(Contract, #4)` `UpgradeDelayNotElapsed` | `execute_upgrade` before `executable_at_ledger`. |
| reporter-registry | `Error(Contract, #5)` `InsufficientStake` | `register_reporter` balance is below the configured minimum. |
| reporter-registry | `Error(Contract, #6)` `InvalidStakeAmount` | The configured minimum is negative. |
| reporter-registry | `Error(Contract, #7)` `ReporterAlreadyRegistered` | `register_reporter` called for an address that already has a role. |
| coverage-ledger | `Error(Contract, #16)` `InsufficientStake` | A supply poster's current XLM balance is below the minimum. |
| reporter-registry | `Error(Contract, #5)` `InvalidMaxReporters` | `set_max_reporters` with `0`. |
| reporter-registry | `Error(Contract, #6)` `EmptyBatch` | `batch_register` with an empty list. |
| coverage-ledger | `Error(Contract, #14)` `UpgradeNotProposed` | As registry `#3`. |
| coverage-ledger | `Error(Contract, #15)` `UpgradeDelayNotElapsed` | As registry `#4`. |
| coverage-ledger | `Error(Contract, #16)` `TooManyReportersPerAsset` | The asset already holds `max_reporters` reporters that count, and this one is new to it. Revoke one or raise the cap. |
| CLI | `Missing signing key for account <G...>` | The call needs that account's authorisation and the key is not in the keystore. For a multisig admin, use section 8. |
| network | `TxBadAuth` | The signatures do not meet the source account's threshold. Collect another signer's signature. |

The full list of contract errors is in [ERRORS.md](ERRORS.md).

## 12. Rehearsal record

These procedures were followed end to end on 2026-11-22 against fresh testnet
deployments made with `scripts/deploy.sh` (section 1), with rustc 1.100.0 and
stellar-cli 28.0.0.

| Procedure | Deployment | Result |
| --- | --- | --- |
| 1 Rehearsal setup, `verify-deployment.sh` all `ok` | registry `CAQJP74FE6D7BKN7K7B6KOZJL4XQO4FUOTF7NDNYN7RZOYUQN5RFYRJ7`, ledger `CDP4TXRETPTNBHVXDNPE2U53IKIAXZL5WDFT4M23UZUC5OEBPFLGXGI5` | as documented |
| 2 Build, hash, upload, `fetch` + `cmp` | same | as documented |
| 3 First upgrade from the live testnet wasm (`400f5677…`, `fa78245f…`) to this build, state preserved | registry `CAHXWKIPHEGDCKPTGDLHT7YK4E4UZAKKWRXMOE36JLDDMWV7MO53HNHT`, ledger `CCP6VX7OFCJJLI3EZ4RIEVXENB7FCIEPFFNKRA2GRDQV5IZSAPSHZ2OE` | as documented; `verify-deployment.sh` went from `MISMATCH` to `ok` |
| 4 Propose, `pending_upgrade`, early `execute_upgrade` refused (`#4`, `#15`) | first pair | as documented |
| 4.7 `execute_upgrade` after the delay | first pair | proposals stand at `executable_at_ledger` 5 880 025 (registry) and 5 880 027 (ledger); the delay had not elapsed when this record was written |
| 5 Cancel, cancel again (`#3`), re-propose | first pair | as documented |
| 7 `transfer_admin` to a fresh account, then 2-of-3 with master weight 0 | first pair's registry | as documented |
| 8 One signature `TxBadAuth`, two signatures `SUCCESS` (`set_reporter`, `revoke`) | first pair's registry | as documented |
| 9, 10 Onboard, revoke, `get_reports` before and after, second revoke `#1` | first pair | as documented |

Re-run the rehearsal and update this table whenever a procedure changes.
