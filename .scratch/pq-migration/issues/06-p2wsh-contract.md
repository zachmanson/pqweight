Status: ready-for-agent

# P2WSH and P2SH-P2WSH contract template (slice 6)

Blocked by: 05 (done)

## Problem Statement

After slice 5, Unmapped is about 2.1% of Input weight in the September 2026 sample, and almost all of it is P2WSH non-multisig (297 inputs, 139,603 WU) plus P2SH-wrapped segwit non-multisig (12 inputs, 6,532 WU). The shape breakdown in `docs/coverage/2026-09-sample.md` found one exact two-key hashlock/CLTV script from one unidentified service in 280 of them:

```
<key A> CHECKSIG NOTIF
    DUP HASH160 <20> EQUALVERIFY CHECKSIGVERIFY <locktime> CHECKLOCKTIMEVERIFY
ELSE
    <key B> CHECKSIGVERIFY SIZE 32 EQUALVERIFY HASH160 <20> EQUAL
ENDIF
```

Claim branch (278): witness `[preimage32, sigB, sigA, script]`. Refund branch (2): `[sigC, pubkeyC, <empty>, script]`, where the empty item fails the A check on purpose and key C is checked against a HASH160 commitment in the script.

The rest (29 inputs) are Lightning anchors and `to_remote`, CSV-delayed two-key scripts, contracts with an embedded `CHECKMULTISIG`, and a P2SH-P2WSH key-hash selector script.

The report argued against a template for this bucket because it would recognize one protocol's script. This ticket answers that with a general rule instead.

## Solution

A literal-swap template for any witnessScript that isn't standard multisig, in two wrappers: **P2WSH contract** and **P2SH-P2WSH contract**. Weight only depends on which bytes are keys and signatures, so the template doesn't need to understand what a contract does: find every key and every signature, swap each for its PQ counterpart, and carry every other byte unchanged.

A scratch run of this rule over the sample (2026-09-23, not committed) maps all 309 inputs in the two buckets.

## Decisions (grilled 2026-09-23)

1. **A structural rule, not the one exact script.** It covers the whole bucket, answers the report's "one protocol's script" objection, and follows the same reasoning as slice 5's single-key leaf rule.
2. **Keys: every key-shaped push, anywhere.** A push of 33 bytes starting `02`/`03`, or 65 bytes starting `04`, is a public key, whether it's in the script or a stack item. No check of where the push sits or where it flows.
   - Why: "a 33-byte push right before `CHECKSIG`" misses real keys. `<K1> SWAP <K2> CHECKSIG …` (5 inputs) and contracts with an embedded `CHECKMULTISIG` (2) put keys elsewhere. Script simulation would be exact but a lot of code for no gain in the sample. A key in the script grows even when this spend's branch never uses it, because the whole script is in the witness, and this rule gets that right.
   - Accepted risk: a 33-byte data item starting `02`/`03` would be read as a key.
3. **Signatures: strict DER parse, for this template only.** A stack item is a signature when it parses as `30 len 02 rlen r 02 slen s` followed by a sighash byte, with every length consistent (the BIP66 structure).
   - Why: ticket 04's loose check (`0x30` + 9 to 73 bytes) reads 2 of the 280 claim-branch preimages as signatures, because random 32-byte preimages start with `0x30` about once in 256. The fixed-layout templates (P2WPKH, P2PKH, P2SH-P2WPKH, multisig) know where the signature is, so they keep the loose check.
4. **Everything else is carried unchanged:** 20-byte hash commitments (including HASH160 of a key checked on the stack: the stack key grows, the commitment stays 20 bytes, as in P2PKH), preimages, `SIZE 32`, locktimes, selector bytes, empty dummy items and every opcode.
5. **Signatures can be zero.** A spend is mapped when its script or stack has at least one key, even if every signature item is empty. The 4 Lightning anchors swept after 16 blocks with an empty signature still have a key in the script that grows.
   - This reverses the first-round answer ("all signatures empty means Unmapped") once the anchors showed there is still something to swap. It's inconsistent with slice 5, which keeps the empty-signature `NOTIF` leaf Unmapped even though its key would grow too. See Out of Scope.
6. **Unmapped guards.** A spend that would otherwise be a contract stays Unmapped, with its current reason (`P2WSH non-multisig` or `P2SH-wrapped segwit non-multisig`), when:
   1. the script has a truncated push;
   2. the script has no `CHECKSIG`, `CHECKSIGVERIFY`, `CHECKMULTISIG` or `CHECKMULTISIGVERIFY`;
   3. neither the script nor the stack has a key-shaped item.
   - Standard multisig is matched first by the slice 3 templates, which carry the Multisig threshold. Contracts with `CHECKMULTISIG` inside that aren't standard multisig are contracts.
   - The reason names are kept, though "non-multisig" is now slightly off, to avoid churning the JSON output.
7. **Wrappers: P2WSH and P2SH-P2WSH.** P2SH-P2WSH is the same witness behind the fixed `0020<hash>` scriptSig push. Bare P2SH non-multisig (script in the scriptSig) is out of scope: slice 3 migrates bare P2SH to a witness-carried script, which is a different template.
8. **Push encoding: same as slice 5.** Each migrated key in the script uses the shortest push: `OP_PUSHBYTES_32` for SLH-DSA-128s, `OP_PUSHDATA2` + 2-byte length for ML-DSA-44 (1,312) and Falcon-512 (897). Compute the migrated script's whole length first, then its compact-size prefix. Key stack items become `public_key_size()` bytes, and signature items `signature_size()` bytes whatever their DER length (no separate sighash byte, as in every other template). Reuse slice 5's code path where it fits.
9. **Name.** Labels `P2WSH contract` and `P2SH-P2WSH contract`, enum variants `BaselineSpendType::P2wshContract` and `BaselineSpendType::P2shP2wshContract` with no data (key and signature counts are read from the input when computing weight). Glossary term **Contract script** (already added to `CONTEXT.md`).
10. **Assumptions, no ADR.** Inputs mapped to these types print:
    - slice 3's existing line: "the 10,000-byte script size limit and witnessScript standardness limits (3,600 bytes, 100 stack items) are raised by a soft fork";
    - slice 3's `CHECKMULTISIG` layout line, only when the script contains `CHECKMULTISIG` or `CHECKMULTISIGVERIFY`;
    - a new line: "in a P2WSH contract, every key-shaped push (33 bytes starting 02 or 03, or 65 bytes starting 04) is a public key and every strict-DER stack item is a signature; all other bytes are carried unchanged".
    - No ADR: the recognition rule is how the tool reads spends, like the Unmapped reason heuristics, and is cheap to change. The consensus assumptions it relies on are already in ADR-0002.

## Doc edits (part of this ticket)

- **`CONTEXT.md`**: already done during grilling (the **Contract script** term, and `P2WSH contract` in the Baseline spend type examples).
- **`spec.md`**: replace the "Custom-script P2WSH / P2SH (Lightning, timelocks) | Probably stays Unmapped" row of the "Other Baseline spend types" table with a pointer to this ticket (bare P2SH non-multisig still Unmapped), and add the new heuristic to the assumptions list.
- **Coverage report**: add the recognition rule to the Assumptions section, next to the Unmapped reason heuristics.
- **`UnmappedReason::P2wshNonMultisig` doc comment**: it now covers spends that fail the guards in decision 6, not "timelocks, HTLCs, single-key scripts".

## Testing Decisions

- **Fixtures from mainnet**, as in slice 5: Core's wallet can't sign these scripts (not miniscript), so take real transactions from the sample, with oracle values from `bitcoin-tx -json`. Each one gets a hand-derived migrated Input weight in a comment. One fixture each:
  1. a claim spend of the main contract (`[preimage32, sigB, sigA, script]`);
  2. a refund spend (`[sigC, pubkeyC, <empty>, script]`): stack key swapped, HASH160 commitment unchanged, empty item unchanged;
  3. a claim spend whose preimage starts with `0x30`: regression test for decision 3, where the preimage has to stay 32 bytes;
  4. a Lightning anchor spent with an empty signature: mapped with no signature swap (decision 5);
  5. a contract with an embedded `CHECKMULTISIG`: prints the `CHECKMULTISIG` layout assumption;
  6. a P2SH-P2WSH contract.
- **Hand-built unit tests:**
  - near misses stay Unmapped with their current reason, one per guard in decision 6: a truncated push, no signature-check opcode, no key anywhere;
  - a key that isn't next to `CHECKSIG` (`<K1> SWAP <K2> CHECKSIG`) is swapped;
  - a 65-byte `04` key is swapped;
  - a DER-looking item that fails the strict parse (for example `0x30` followed by inconsistent lengths) is carried unchanged;
  - standard multisig still maps to the slice 3 templates, not to contract.
- **Per parameter set:** migrated key pushes use `OP_PUSHBYTES_32` (SLH-DSA-128s) or `OP_PUSHDATA2` (ML-DSA-44, Falcon-512), and the script's compact-size prefix is recomputed.

## Acceptance

1. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
2. Re-running `pqweight aggregate` on the sample maps all 309 inputs of the P2WSH non-multisig and P2SH-wrapped segwit non-multisig buckets. The only Unmapped inputs left should be the 2 P2TR two-key leaves (about 99.98% of Input weight mapped). If any input fails the guards, report how many and why.
3. Add a dated "After slice 6" section to the coverage report with the before/after coverage and the migrated totals. Say that the general rule answers the "one protocol's script" objection from "Why P2TR script-path", and note that the 280 contract inputs come from 170 transactions, one of which sweeps 87 of them.
4. The doc edits above are made.

## Out of Scope

- Bare P2SH non-multisig (script in the scriptSig).
- Script simulation to track which pushes reach a signature check.
- Tightening ticket 04's loose DER check in the fixed-layout templates.
- Slice 5's empty-signature `NOTIF` leaf: it stays Unmapped even though its key in the leaf would grow. Inconsistent with decision 5, but none occurred in the sample. Revisit with multi_a.
- multi_a and the P2TR two-key leaves.
- Long/Short-exposure tagging.
