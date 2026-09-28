Status: done

# Strict DER checking in the fixed-layout templates

## Problem Statement

Ticket 04 loosened the signature check in the fixed-layout templates (P2WPKH, P2PKH and so on) to "first byte `0x30`, length 9 to 73". It doesn't check the `r` and `s` lengths or the leading-zero rules. Slices 6 and 7 later added a strict-DER check to recognize signatures in contract scripts and P2PK. So the codebase now has two definitions of "looks like a signature", and ticket 06 put off making them the same.

## Rough shape

- Decide whether the fixed-layout templates should use the strict check too. If they do, find out what, if anything, moves to Unmapped in the sample.

## Open questions

- Is there a real misclassification risk, or is this only a consistency issue? Ticket 04 argued that length plus first byte is enough to tell a signature from a key or a script in those fixed layouts.

## Source

Ticket 04 (Out of Scope), ticket 06 (Out of Scope).

## Comments

**2026-09-28, triage: ready-for-agent.** Decisions:

1. **The fixed-layout templates (P2WPKH, P2SH-P2WPKH, P2PKH) switch to the strict DER check** that slices 6 and 7 use, so the codebase has one definition of "looks like a signature". Ticket 04's short-DER bug showed a loose check can misfile inputs, so this isn't only cosmetic.
2. **Acceptance:**
   - a unit test per fixed-layout template: a stack item that starts `0x30` with a valid overall length but inconsistent inner `r`/`s` lengths is no longer accepted as a signature;
   - all existing Fixtures and the Second calculation still pass;
   - rerun the September Coverage sample and record in `docs/coverage/2026-09-sample.md` whether any input changes Baseline spend type or becomes Unmapped (expected: none).

**2026-09-28, done.**

- P2WPKH, P2SH-P2WPKH and P2PKH use `is_strict_der_signature`. So do the three standard multisig templates (P2WSH, P2SH-P2WSH, P2SH), which the list above didn't name. Leaving them loose would have kept two definitions for templates.
- One loose check is kept on purpose: `looks_like_der_signature` in `is_redeem_script_candidate`. It only chooses between Unmapped reasons (it rules a DER-looking last push out as a P2SH redeem script) and never decides a template. Switching it would relabel a `0x30`-prefixed junk push from LegacyOther to P2shNonMultisig.
- Tests: one `*_must_have_consistent_der_lengths` test per template (P2WPKH, P2SH-P2WPKH, P2PKH, and one for P2WSH and P2SH multisig). The shared test helper `der_signature(len)` now builds real DER; `der_lookalike(len)` builds the old tag-plus-zeros filler for tests that want a non-signature.
- September sample: `migrate --json-lines` output is byte-identical before and after across all 14,692 transactions. No Baseline spend type changed and nothing became Unmapped. Second calculation: 0 mismatches on the sample and the Fixtures. Recorded in `docs/coverage/2026-09-sample.md`, "Strict DER in every template".
- "Strict" is still BIP66's layout only, without its rules on negative or zero-padded numbers (stated in `docs/migration-templates.md`).
