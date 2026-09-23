Status: needs-triage

# P2TR script-path template for single-key leaves (slice 5)

Blocked by: 04 (not a hard dependency; do it first so the next report measures both changes against one baseline)

## Problem Statement

P2TR script-path spends are 2.1% of Input weight in the September 2026 sample and all Unmapped. The shape breakdown in `docs/coverage/2026-09-sample.md` shows 209 of the 211 inputs (99.3% of that bucket's weight) share one structure: the leaf script has one 32-byte x-only key and one `OP_CHECKSIG` or `OP_CHECKSIGVERIFY`, the witness stack has one 64 or 65-byte Schnorr signature, and everything else is inert data. Mostly inscription envelopes (189), plus leaves with a dropped tag (`<data> OP_DROP <key> OP_CHECKSIG`, 17) and a few others. The report explains why this was picked over the P2WSH bucket.

## Solution

A new Baseline spend type for "P2TR script-path, single-key leaf", with a literal-swap template in the spirit of slice 3: replace the key in the leaf with a PQ public key and the Schnorr signature with a PQ signature, and keep every other byte (data pushes, other stack items, annex) as is.

## Open questions (grill before implementing)

- **Recognizing the leaf.** Proposed: parse the leaf into ops; exactly one `CHECKSIG`/`CHECKSIGVERIFY`, immediately preceded by a 32-byte push; no `CHECKSIGADD`, `CHECKMULTISIG`, or a second key check anywhere; exactly one stack item of 64 or 65 bytes, which is the signature. Is "no other signature opcode" enough, or should the leaf be matched against a short list of shapes (envelope, dropped tag, data drops)? Structure is more general; a shape list is easier to defend. Leaning structure.
- **Which stack item is the signature.** In an envelope leaf the signature is the only stack item. In the "data on the stack" leaves (`<key> CHECKSIGVERIFY DROP ... 1`) there are 65 items. Rule: the item consumed by the `CHECKSIG` is the last stack item (top of stack) when the key check is the first op. Leaves where the key check isn't first need a decision, or stay Unmapped.
- **The control block's internal key.** It's a 32-byte elliptic-curve key, and a quantum attacker who can find its private key can spend by key path. Options: (a) leave it as is and state that PQ security needs a NUMS (unspendable) internal key or the key path disabled by soft fork; (b) swap it for a PQ key (control block grows by the key size); (c) assume a new output type without a key path (BIP-360 style) where the control block keeps only the leaf version and Merkle path. Each is a different weight and a different assumption. Needs a decision and an ADR-0002 assumption line either way.
- **Merkle path.** 14 of the 209 inputs have one 32-byte path hash. Path hashes are SHA-256, not signatures, so leaving them unchanged seems right. Confirm.
- **Leaf size limits.** A 1,312-byte ML-DSA-44 key needs `OP_PUSHDATA2` and breaks the 520-byte element limit. The element limit is already an assumption (ADR-0002). Tapscript has no script size limit, so no new standardness assumption is needed, unlike slice 3.
- **Name.** Glossary entry in `CONTEXT.md` for the new Baseline spend type (e.g. "P2TR script-path single-key").

## Testing Decisions

- The existing `p2tr-scriptpath` Fixture (single pk leaf, unspendable internal key) becomes mapped. Hand-derive its migrated Input weight in a comment, as the other templates do.
- Record an inscription-style envelope leaf and a dropped-tag leaf on regtest with `record-fixtures.ps1 -Only …`, so the "data kept as is" rule is tested against real bytes.
- Near misses stay Unmapped with reason P2TR script-path: a multi_a leaf, a two-key `CHECKSIGVERIFY` chain, a leaf with no signature check, a stack with two 64-byte items.
- A test with an annex: the annex is kept and doesn't confuse the signature rule.

## Acceptance

1. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
2. Re-running `pqweight aggregate` on the sample maps about 209 of the 211 P2TR script-path inputs. Add a dated section to the coverage report with the new coverage and migrated totals, noting that inscription data dominates these inputs' weight, so their migrated-to-baseline ratio is much lower than for payments.

## Out of Scope

- multi_a and other multi-key leaves (next, if the data supports it).
- The P2WSH two-key hashlock/CLTV contract that dominates P2WSH non-multisig.
- Long/Short-exposure tagging.
