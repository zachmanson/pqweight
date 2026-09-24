Status: needs-triage

# Strict DER checking in the fixed-layout templates

## Problem Statement

Ticket 04 loosened the signature check in the fixed-layout templates (P2WPKH, P2PKH and so on) to "first byte `0x30`, length 9 to 73". It doesn't check the `r` and `s` lengths or the leading-zero rules. Slices 6 and 7 later added a strict-DER check to recognize signatures in contract scripts and P2PK. So the codebase now has two definitions of "looks like a signature", and ticket 06 put off making them the same.

## Rough shape

- Decide whether the fixed-layout templates should use the strict check too. If they do, find out what, if anything, moves to Unmapped in the sample.

## Open questions

- Is there a real misclassification risk, or is this only a consistency issue? Ticket 04 argued that length plus first byte is enough to tell a signature from a key or a script in those fixed layouts.

## Source

Ticket 04 (Out of Scope), ticket 06 (Out of Scope).
