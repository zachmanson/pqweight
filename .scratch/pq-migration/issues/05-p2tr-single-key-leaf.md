Status: done

# P2TR script-path template for single-key leaves (slice 5)

Blocked by: 04 (done)

## Problem Statement

P2TR script-path spends are 2.1% of Input weight in the September 2026 sample and all Unmapped. The shape breakdown in `docs/coverage/2026-09-sample.md` shows 209 of the 211 inputs (99.3% of that bucket's weight) share one structure: the leaf script has one 32-byte x-only key and one `OP_CHECKSIG` or `OP_CHECKSIGVERIFY`, the witness stack has one 64 or 65-byte Schnorr signature, and everything else is inert data. Mostly inscription envelopes (189), plus leaves with a dropped tag (`<data> OP_DROP <key> OP_CHECKSIG`, 17) and a few others. The report explains why this was picked over the P2WSH bucket.

## Solution

A new Baseline spend type, **P2TR script-path single-key**, with a literal-swap template in the spirit of slice 3: replace the key in the leaf with a PQ public key and the Schnorr signature with a PQ signature, keep every other byte of the leaf and every other stack item as is, and shrink the control block to the leaf-version byte plus the Merkle path.

## Decisions (grilled 2026-09-23)

1. **Control block: no internal key (BIP-360 style).** The migrated output commits to the script tree's Merkle root directly, with no internal key and no key path. The migrated control block is 1 byte (leaf version) plus the Merkle path.
   - Why: a NUMS internal key does not protect Taproot from a quantum attacker, who gets Q's private key with Shor and spends by key path. Only removing the key path does. Swapping P for a PQ key is incoherent (P goes into `P + t·G`). Keeping P with the key path disabled by a soft fork would work, but no proposal specifies it, and the P2TR key-path template already assumes PQ outputs commit to a hash, not an EC key. So both P2TR templates assume the same kind of output.
2. **Merkle path: unchanged, 32 bytes per level.** The tree's shape doesn't change; only one leaf's contents do. SHA-256 keeps about 128-bit preimage security under Grover, and BIP-360 keeps 32-byte hashes.
3. **Recognizing the leaf: a structural rule, not a shape list.** Weight only depends on which bytes are keys and signatures, so the rule tests exactly that. A script-path spend is P2TR script-path single-key when all of these hold:
   1. Leaf version (control block byte 0, masked with `0xfe`) is `0xc0`.
   2. The leaf parses into ops; a truncated push means Unmapped.
   3. Exactly one `OP_CHECKSIG` or `OP_CHECKSIGVERIFY` in the leaf, and the op right before it is a push of exactly 32 bytes (so the key is in the leaf, and not a tapscript "unknown key type" push).
   4. No `OP_CHECKSIGADD`, `OP_CHECKMULTISIG` or `OP_CHECKMULTISIGVERIFY` anywhere.
   5. Exactly one stack item of 64 or 65 bytes (stack = witness minus control block and leaf). This rejects the `<key> CHECKSIG NOTIF …` shape spent with an empty signature, where there is nothing to swap.
4. **Which stack item is the signature: the unique 64 or 65-byte item (the size rule from 3.5).** No positional logic and no stack simulation. A second 64 or 65-byte item means Unmapped. The migrated signature item is `signature_size()` bytes whether the original was 64 or 65, the same as every other template (no separate sighash byte).
5. **Push encoding: literal swap with the shortest push.** `OP_PUSHBYTES_32` for SLH-DSA-128s (32-byte key, leaf size unchanged), `OP_PUSHDATA2` + 2-byte length for ML-DSA-44 (1,312) and Falcon-512 (897). Compute the whole migrated leaf length first, then its compact-size prefix: a 34-byte leaf becomes 1,316 bytes at ML-DSA-44 and its prefix goes from 1 to 3 bytes. Keys aren't moved onto the stack behind a hash (P2WPKH style): BIP-360 keeps keys in leaves, and the byte total would be about the same.
   - Tapscript has no script size limit, so there's no new standardness assumption. The PQ key push and PQ signature break the 520-byte limit, which is already an ADR-0002 assumption, but its wording says "witness item" and a push inside the leaf isn't one. Reword it to "stack element".
6. **Annex: Unmapped**, reason P2TR script-path (no new reason). Matches the key-path template's rule. The annex is reserved for future soft forks, and a PQ soft fork might redefine it, so "carried as is" would be a guess. It's non-standard, so it should almost never occur. The classifier still has to detect it (2 or more items, last starts with `0x50`) to find the control block and leaf.
7. **Name.** Label `P2TR script-path single-key`, enum `BaselineSpendType::P2trScriptPathSingleKey` with no data (the Merkle path length and leaf size are read from the input when computing weight). "P2TR script-path" stays the Unmapped reason for everything else (two-key, multi_a, annex, etc.).

## Doc edits (part of this ticket)

- **ADR-0002**: add the assumption from decision 1: *"Migrated script-path outputs commit to the script tree's Merkle root directly (no internal key, no key path, as in BIP-360); the control block is the leaf-version byte plus the Merkle path."* Reword the 520-byte limit as a stack element limit (covers witness items and pushes inside a script).
- **`spec.md`**: same rewording of the assumption on line 28, and update the P2TR script-path row of the "Other Baseline spend types" table to point at this ticket.
- **Assumptions output** (human block and JSON `assumptions`): add the decision 1 line for inputs mapped to this type, and use the reworded 520-byte line.
- **`CONTEXT.md`**: add the glossary term below, and add "P2TR script-path single-key" to the examples in the Baseline spend type entry.

  > **Single-key leaf**:
  > A tapscript leaf (version `0xc0`) whose only signature check is one 32-byte key pushed right before `OP_CHECKSIG` or `OP_CHECKSIGVERIFY`, spent with exactly one 64 or 65-byte stack item. Every other op and data push, such as an inscription envelope, is carried unchanged by its Migration template.
  > _Avoid_: inscription leaf, pk leaf

## Testing Decisions

- The existing `p2tr-scriptpath` Fixture (single pk leaf, NUMS internal key) becomes mapped. Hand-derive its migrated Input weight in a comment, as the other templates do. Its witness today is `[64-byte sig, 34-byte leaf 20<key>ac, 33-byte control block c1<NUMS>]`, 135 bytes. At ML-DSA-44 the witness should be 1 + (3 + 2,420) + (3 + 1,316) + (1 + 1) = 3,745 bytes. Check this against the code rather than copying it.
- Record an inscription-style envelope leaf and a dropped-tag leaf on regtest with `record-fixtures.ps1 -Only …`, so the "data kept as is" rule is tested against real bytes.
- A leaf with a one-level Merkle path: the control block migrates from 65 to 33 bytes.
- Near misses stay Unmapped with reason P2TR script-path, one per guard in decision 3: leaf version other than `0xc0`; a truncated push; a two-key `CHECKSIGVERIFY` chain; a key check whose key comes from the stack or is not 32 bytes; a multi_a (`CHECKSIGADD`) leaf; a leaf with no signature check; a stack with two 64-byte items; a stack with no 64 or 65-byte item (empty-signature `NOTIF` shape).
- A single-key leaf spent with an annex stays Unmapped with reason P2TR script-path.
- Per parameter set, the migrated leaf push uses `OP_PUSHBYTES_32` (SLH-DSA-128s) or `OP_PUSHDATA2` (ML-DSA-44, Falcon-512), and the leaf's compact-size prefix is recomputed.

## Acceptance

1. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
2. Re-running `pqweight aggregate` on the sample maps about 209 of the 211 P2TR script-path inputs. Add a dated section to the coverage report with the new coverage and migrated totals. It should say that inscription data dominates these inputs' weight, so their migrated-to-baseline ratio is much lower than for payments, and that inscription volume is bursty (160 of the 189 are in one block). If the size rule (decision 4) leaves out inputs that had a second 64 or 65-byte data item, report how many.
3. The doc edits above are made.

## Implementation notes (2026-09-23)

- **Real-byte fixtures come from mainnet, not regtest.** Core's wallet signs tapscript leaves only when it can parse them as miniscript, and an inscription envelope or dropped-tag leaf isn't miniscript, so `record-fixtures.ps1` can't produce them. `p2tr-scriptpath-envelope` and `p2tr-scriptpath-dropped-tag` are mainnet transactions from the sample (txids in their `.json`), with oracle values from Core's `bitcoin-tx -json`. They pass the same three-way check as every other Fixture.
- **One guard beyond decision 3:** a leaf containing any BIP-342 `OP_SUCCESSx` opcode is Unmapped. Such a leaf succeeds without running, so its "signature" is never checked and it isn't really single-key. None occurred in the sample.
- Acceptance run: 209 of 211 mapped; the 2 left are the two-key leaves. The size rule turned away no inputs. Results are in the coverage report's "After slice 5" section.

## Out of Scope

- multi_a and other multi-key leaves (next, if the data supports it).
- A positional signature rule for stacks with more than one 64 or 65-byte item (follow-up only if acceptance 2 shows it matters).
- Script-path spends with an annex.
- The P2WSH two-key hashlock/CLTV contract that dominates P2WSH non-multisig.
- Long/Short-exposure tagging.
