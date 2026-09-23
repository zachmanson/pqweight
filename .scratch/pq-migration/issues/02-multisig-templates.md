Status: ready-for-agent

# Multisig migration templates (slice 3)

## Problem Statement

Slices 1 and 2 migrate only single-key spends. Every multisig input comes back **Unmapped**, so any transaction with one never gets a migrated total, and batches with custody or exchange traffic are under-counted. Multisig is also where PQ costs grow fastest, because every spend reveals all n public keys.

## Solution

Teach `migrate()` to classify standard `OP_CHECKMULTISIG` spends in three wrappers and price them with a literal-swap **Migration template**. The **Baseline spend type** carries its **Multisig threshold** (m-of-n). `aggregate()` needs no change: newly mapped inputs flow into the existing totals.

## Decisions

- **Baseline spend types in this slice**: P2WSH multisig, P2SH multisig, P2SH-P2WSH multisig. All three reveal the same script `OP_m <n keys> OP_n OP_CHECKMULTISIG` at spend time, so one script matcher serves all three.
  - P2TR script-path (`multi_a`, `OP_CHECKSIGADD`) is deferred to its own slice: the control block, Merkle path and empty items for non-signers need their own design.
  - Bare multisig stays Unmapped (rare).
- **Classification** is conservative and spending-side only, as in slice 1:
  - The script (last witness item, or last scriptSig push for P2SH) must be exactly `OP_m`, n direct key pushes, `OP_n`, `OP_CHECKMULTISIG`, with 1 ≤ m ≤ n ≤ 20. No trailing or extra opcodes.
  - Before the script: one empty dummy item, then exactly m items that are 70 to 73 bytes (DER signature plus sighash byte). A 0-byte "signature" makes the input Unmapped.
  - Keys: 33 bytes in P2WSH and P2SH-P2WSH. 33 or 65 bytes in P2SH, mixing allowed, since segwit policy bans uncompressed keys but legacy P2SH has them.
  - P2WSH: empty scriptSig, witness as above.
  - P2SH-P2WSH: scriptSig is one push of exactly `0020<32 bytes>`, witness as above.
  - P2SH: empty witness, scriptSig made only of direct pushes: `OP_0` dummy, m signatures, then the script push. The script is at most 520 bytes today, so it may use `OP_PUSHDATA1`.
  - Anything else is **Unmapped**. No signature, hash or script-hash checks.
- **Template (literal swap)**: the witness is `[dummy, pq_sig_1 … pq_sig_m, script]`, where the script embeds all n full PQ public keys: `OP_m <pq_pk_1> … <pq_pk_n> OP_n OP_CHECKMULTISIG`.
  - Chosen over a hashed-key variant (script commits to key hashes, witness reveals only the m signing keys). That variant needs invented opcode semantics; the literal swap only swaps sizes and is a defensible upper bound. The hashed-key variant is a possible later option.
  - The `CHECKMULTISIG` dummy (1 byte) is kept, for consistency with the literal swap.
  - Push opcodes inside the script are counted exactly: 1 byte for 1 to 75, 2 bytes (`OP_PUSHDATA1`) up to 255, 3 bytes (`OP_PUSHDATA2`) up to 65,535. ML-DSA-44 and Falcon-512 keys need `OP_PUSHDATA2`; SLH-DSA-128s keys (32 bytes) use a direct push.
  - Witness item sizes count their compact-size length prefix, plus the item-count byte, as in slice 1.
- **scriptSig after migration**:
  - P2WSH: empty (unchanged).
  - P2SH-P2WSH: keeps its `0020<32>` redeem push, like P2SH-P2WPKH.
  - P2SH: becomes empty; signatures and script move to the witness, like P2PKH in slice 1 (a 520-byte push can't hold PQ keys). A transaction with no witness before migration gains the 2-byte marker and flag.
- **Limits are stated, not checked** (ADR-0002). New assumptions for multisig inputs:
  - "the 10,000-byte script size limit and witnessScript standardness limits (3,600 bytes, 100 stack items) are raised by a soft fork"
  - "PQ multisig keeps today's OP_CHECKMULTISIG layout, including its dummy element, with every public key in the script"
  - "P2SH spends migrate to a witness-carried script with an empty scriptSig"
  Only the 400,000 WU relay limit is checked, as before. Add a multisig assumption only when the transaction has a multisig input, or always (the implementer's choice, but be consistent and test it).
- **Domain model**: `BaselineSpendType` gains multisig variants carrying `m` and `n` (see **Multisig threshold** in `CONTEXT.md`).
- **Output**:
  - Human: `input 0: P2WSH multisig 2-of-3, template weight 8963`.
  - JSON: `"spend_type":"P2WSH multisig"` plus `"threshold":{"m":2,"n":3}`, the latter present only on multisig inputs. Display names: `P2WSH multisig`, `P2SH multisig`, `P2SH-P2WSH multisig`.
- `aggregate` is unchanged. A per-spend-type breakdown is a separate later feature, like `--verbose`.

## Worked example (tracer bullet)

Fixture `p2wsh-multisig` (2-of-3, Oracle weight 582) at ML-DSA-44:

- Baseline input witness: count 1 + dummy 1 + sig (1+72) + sig (1+71) + script (1+105) = 253 B. Input weight = 4 × 41 + 253 = 417 WU.
- Migrated witness: count 1 + dummy 1 + 2 × (3 + 2420) + script, where script = 1 + 3 × (3 + 1312) + 1 + 1 = 3948 B, pushed with a 3-byte prefix = 3951 B. Total 1 + 1 + 4846 + 3951 = 8799 B.
- Template weight = 4 × 41 + 8799 = 8963 WU (about 21× baseline).
- Transaction: stripped size and marker unchanged, so migrated weight = 582 − 253 + 8799 = 9128 WU, vsize 2282.

Re-derive these in the test comment; don't copy them blindly.

## Testing Decisions

- Tracer bullet: the existing `p2wsh-multisig` fixture, with the arithmetic above for at least ML-DSA-44 and SLH-DSA-128s (the direct-push case).
- Extend `scripts/record-fixtures.ps1` to record 2-of-3 P2SH and P2SH-P2WSH multisig spends. It needs a local Bitcoin Core, so the user runs it; commit the resulting `.hex`/`.json`. Until then, those tests may use synthetic transactions.
- Synthetic tests (hand-computed expected values): 1-of-1, 3-of-5, 15-of-15, and a P2SH script mixing 33 and 65-byte keys.
- Near-misses, each expecting Unmapped: m+1 or m−1 signatures, non-empty dummy, 0-byte signature, extra opcode after `OP_CHECKMULTISIG`, m > n, a 65-byte key in P2WSH, an `OP_n` that doesn't match the key count, a non-multisig witness script (e.g. a timelock).
- Extend the existing property test (migrated weight ≥ baseline minus removed signature bytes) to cover multisig inputs.
- CLI smoke tests: human and JSON output for the multisig fixture, including the `threshold` object.
- CI stays `cargo test` with no node.

## Out of Scope

- P2TR script-path multisig (`multi_a`) and other tapscript templates.
- Hashed-key multisig template variant.
- Bare multisig, custom scripts (Lightning, timelocks, miniscript).
- Per-spend-type breakdown in `aggregate`.
- Long-exposure and Short-exposure tagging.
