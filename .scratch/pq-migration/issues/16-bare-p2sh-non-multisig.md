Status: needs-triage

# Bare P2SH non-multisig spends

## Problem Statement

Slice 6 migrates custom scripts in P2WSH and P2SH-P2WSH, but bare P2SH non-multisig spends (redeem script in the scriptSig, no witness) stay Unmapped. Slice 3 already migrates bare P2SH multisig to a witness-carried script, so the pieces exist. They were left out because moving the script into the witness is a different template from slice 6's in-place swap.

## Rough shape

- Apply slice 6's key and signature recognition to the redeem script, then move the result into the witness the same way slice 3 does for multisig.

## Open questions

- How many are there? If ticket 08 finds almost none, this could be `wontfix`.

## Source

Ticket 06 (decision 7 and Out of Scope), pq-migration spec (custom-script row).
