Status: ready-for-agent

# PQ migration templates and fee model (slice 1)

## Problem Statement

I want to know how much block space and fee a real Bitcoin transaction would cost if its signatures moved to a post-quantum **Signature scheme**. `pqweight` measures today's **Weight** and checks it against the **Oracle**, but nothing models the PQ side. Bitcoin has no PQ opcode, so any number has to come from stated assumptions, not from a real transaction.

## Solution

A library function `migrate(bytes, ParameterSet)` returns an **Input result** for every input of a transaction: either a **Migration template** weight or **Unmapped**. A migrated total (weight, vsize) exists only when every input is mapped. A `migrate` CLI subcommand exposes it with `--scheme`, `--fee-rate` and `--json`. Every result carries its stated assumptions, per ADR-0002.

## Decisions

- Slice 1 works on one transaction. Aggregation over many transactions, multisig and script-path templates, and Long/Short-exposure tagging are later slices.
- **Baseline spend types** in slice 1: P2WPKH, P2TR key-path, P2SH-P2WPKH, P2PKH, plus pay-to-anchor as a no-op (empty witness, nothing to migrate, must not count as Unmapped).
- Classification is conservative and based on the spending side only (scriptSig and witness shape):
  - P2WPKH: empty scriptSig, witness of 2 items, a DER signature (70 to 73 bytes; widened to `0x30` and 9 to 73 bytes by `issues/04-short-der-signatures.md`, then to a consistent DER structure, the check every template uses, by `issues/17-strict-der-in-fixed-templates.md`) and a 33-byte pubkey.
  - P2TR key-path: empty scriptSig, witness of exactly 1 item of 64 or 65 bytes. Anything with an annex is Unmapped.
  - P2SH-P2WPKH: scriptSig is one push of exactly `0014<20 bytes>`, witness as for P2WPKH.
  - P2PKH: empty witness, scriptSig of two pushes, a DER signature and a 33 or 65-byte pubkey.
  - Everything else is **Unmapped**. No signature or hash checking is done.
- Every template's witness is `[pq_signature, pq_pubkey]`, counting the item-count byte and each item's compact-size length prefix. A transaction with no witness before migration gains the 2-byte segwit marker and flag. PQ bytes always sit in the witness (no legacy-placement variant). Outputs are unchanged.
- Signature and public key sizes for ML-DSA-44, Falcon-512 (fixed 666-byte padded signature) and SLH-DSA-128s live in one table with source citations. Check the figures against FIPS 204, FIPS 205 and the Falcon specification before hardcoding.
- A total exists only when every input is mapped. Per-input results are always reported. Revisit once aggregation can measure how often Unmapped occurs.
- Fee = fee rate x vsize. `--fee-rate` is sat/vB and may be fractional. Fee is rounded up to whole satoshis. Use exact arithmetic, and reject negative or non-numeric rates. Report baseline fee, PQ fee, absolute difference and ratio.
- The 400,000-weight relay limit is checked on the migrated total and reported as policy, not consensus (ADR-0002).
- Human output prints an Assumptions block and JSON has an `assumptions` array. Assumptions include: PQ outputs commit to a hash of the pubkey, the 520-byte stack element limit (witness items and pushes inside a script) and sigop accounting are changed by a soft fork, outputs are unchanged.
- Multisig inputs (slice 3) add assumptions: script size and witnessScript standardness limits are raised by a soft fork, PQ multisig keeps the `OP_CHECKMULTISIG` layout with its dummy and every public key in the script, and P2SH spends migrate to a witness-carried script. See `issues/02-multisig-templates.md`.
- P2WSH and P2SH-P2WSH contract inputs (slice 6) add the script size assumption above, the `OP_CHECKMULTISIG` layout one when the script uses that opcode, and a recognition heuristic: every key-shaped push (33 bytes starting `02`/`03`, or 65 bytes starting `04`) is a public key and every strict-DER stack item is a signature; all other bytes are carried unchanged. See `issues/06-p2wsh-contract.md`.
- No consensus validation, as in the weight-computation spec.

## Other Baseline spend types (not in slice 1)

| Type | Plan |
|---|---|
| P2PK | Slice 7: recognized as an Unmapped reason and tagged Exposed in output, no template, see `issues/07-key-exposure.md`. Mostly early coins. No template: ticket 11 wontfix (no hash to swap, 0 spends in either sample); the cost of moving unspent Exposed coins is ticket 22 |
| P2WSH / P2SH multisig, P2SH-P2WSH | Slice 3: literal-swap `CHECKMULTISIG` template, see `issues/02-multisig-templates.md` |
| P2TR script-path | Slice 5: single-key leaves (one key, one `CHECKSIG`/`CHECKSIGVERIFY`), literal-swap template with a BIP-360-style control block, see `issues/05-p2tr-single-key-leaf.md`. `multi_a` and other multi-key leaves later |
| Custom-script P2WSH / P2SH-P2WSH (hashlocks, timelocks, Lightning) | Slice 6: literal-swap template for any non-multisig witnessScript (every key and signature swapped, all else kept), see `issues/06-p2wsh-contract.md`. Bare P2SH non-multisig stays Unmapped |
| Bare multisig | Slice 7: recognized as an Unmapped reason (`OP_0` then strict-DER signatures) and tagged Exposed in output, no template, see `issues/07-key-exposure.md`. Rare |
| OP_RETURN | No inputs, no template |

## Testing Decisions

- No Oracle exists for PQ weight. Tests use hand-computed expected values with the arithmetic written in comments, run on the existing Fixtures.
- Each classification rule has a test, and each near-miss (wrong length, extra item, annex) has a test expecting Unmapped.
- A property test: for any input, migrated weight is at least baseline weight minus the removed signature bytes, which catches sign errors.
- One CLI smoke test for `migrate`, and one for bad input and a bad fee rate.
- A second calculation (for example a Python script building the migrated serialization) is a possible later cross-check.
- CI runs `cargo test` with no Bitcoin Core node.

## Out of Scope

- Aggregation over blocks or many transactions, and reporting how many inputs are Unmapped overall.
- Multisig, script-path and custom-script templates.
- Long-exposure and Short-exposure tagging or analysis. Added in slice 7 as Key exposure tagging, see `issues/07-key-exposure.md`.
- Fee-rate market modelling, such as block-space scarcity effects.
- Fetching transactions or prevouts from a node at runtime.

## Further Notes

- Use the `CONTEXT.md` terms (Weight, vsize, Migration template, Unmapped, Baseline spend type, Input result, Signature scheme, Parameter set) in code, comments and docs.
- Because Falcon signatures are variable-length, the fixed padded size is an assumption and must appear in the Assumptions block.
