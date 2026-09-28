Status: ready-for-agent

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
