Status: wontfix

# Bare P2SH non-multisig spends

Reopen if: any Coverage sample finds a bare P2SH non-multisig spend

## Problem Statement

Slice 6 migrates custom scripts in P2WSH and P2SH-P2WSH, but bare P2SH non-multisig spends (redeem script in the scriptSig, no witness) stay Unmapped. Slice 3 already migrates bare P2SH multisig to a witness-carried script, so the pieces exist. They were left out because moving the script into the witness is a different template from slice 6's in-place swap.

## Rough shape

- Apply slice 6's key and signature recognition to the redeem script, then move the result into the witness the same way slice 3 does for multisig.

## Open questions

- How many are there? If ticket 08 finds almost none, this could be `wontfix`.

## Source

Ticket 06 (decision 7 and Out of Scope), pq-migration spec (custom-script row).

## Comments

**2026-09-24, ticket 08 census: stays in the backlog.** No bare P2SH non-multisig spend in either sample (0 of 25,156 inputs in September, 0 of 69,810 in April to June 2026). See `docs/coverage/2026-q2-sample.md`.

**2026-09-28, wontfix.** 0 inputs in both Coverage samples (see the 2026-09-24 comment). Reopen if any later sample finds a bare P2SH non-multisig spend, the same rule as tickets 18 and 20.
