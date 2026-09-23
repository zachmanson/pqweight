Status: ready-for-agent

# Accept short DER signatures in the ECDSA templates

## Problem Statement

The September 2026 coverage sample (`docs/coverage/2026-09-sample.md`) found 26 P2WPKH inputs filed as Unmapped "P2WSH non-multisig". Their witness is a DER signature and a 33-byte compressed key, and mempool.space confirms the outputs they spend are P2WPKH. The signatures are 68 or 69 bytes. The P2WPKH template only accepts 70 to 73 bytes, so they fall through to the catch-all "empty scriptSig, 2 or more witness items".

A DER signature is shorter when `r` or `s` has leading zero bytes, so short signatures are rare but valid. The same 70 to 73 range is used by the P2PKH, P2SH-P2WPKH and multisig templates, so they can misfile the same way (not seen in the sample).

## Solution

Widen the signature length check in all four places to match the shape already used by `is_redeem_script_candidate`: first byte `0x30`, 9 to 73 bytes.

## Decisions

- **One rule, one helper.** Today the range is repeated in `is_der_signature_and_legacy_pubkey`, `is_der_signature_and_compressed_pubkey` and `multisig_threshold`. Replace them with one `looks_like_der_signature` helper and use it in `is_redeem_script_candidate` too, so classification and the Unmapped heuristic can't drift apart.
- **Checking the `0x30` byte is new for the templates.** It narrows them a little (a 71-byte item not starting with `0x30` stops matching), which is correct: that item isn't a DER signature.
- **No change to migrated weight.** The template replaces the signature, so its baseline length only affects classification and baseline Input weight, not the migrated witness.
- **No ADR.** It fixes a heuristic, it doesn't change a stated assumption.

## Testing Decisions

- P2WPKH with a 69-byte signature and a 68-byte signature: mapped as P2WPKH. Build them by editing a copy of the `p2wpkh` Fixture's witness in the test (the weights are hand-derived; there's no Oracle for a hand-edited transaction).
- The same for P2PKH and P2SH-P2WPKH (one short-signature case each) and for one multisig signature.
- Near misses stay Unmapped: a 69-byte item not starting with `0x30`, and an 8-byte `0x30` item.
- Existing Fixture tests keep passing unchanged.

## Acceptance

1. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
2. Re-running `pqweight aggregate` on the sample moves 26 inputs (6,993 WU) from P2WSH non-multisig to P2WPKH. Update the report's coverage tables and note the change under "What the check changed".

## Out of Scope

- Checking full DER structure (`r` and `s` lengths, leading-zero rules). Length and first byte are enough to separate a signature from a key or a script.
- Any new template.
