Status: wontfix

# Hashed-key multisig template (lower bound)

Reopen if: a concrete proposal (BIP or equivalent) specifies hashed-key multisig

## Problem Statement

The slice 3 multisig template is a literal swap: every PQ public key sits in the script, so an m-of-n spend reveals all n keys. Ticket 02 chose this on purpose as a defensible upper bound. A hashed-key variant, where the script commits to key hashes and the witness reveals only the m signing keys, would cost much less for large n. Showing both would bracket the true cost of PQ multisig instead of only giving the ceiling.

## Rough shape

- A second multisig template behind a switch, reported alongside the literal swap rather than replacing it.

## Open questions

- The variant needs opcode semantics that don't exist yet (ticket 02). Which proposal, if any, should it follow, and does that need an ADR next to ADR 0002?
- Should the same idea apply to `multi_a` (ticket 09)?

## Source

Ticket 02 (literal swap vs hashed-key decision, and Out of Scope).

## Comments

**2026-09-28, wontfix for now.** The variant needs opcode semantics no proposal defines. Modelling it now would mean inventing an opcode, which ADR 0002 exists to avoid: templates are stated assumptions about proposals, not new designs. Reopen when a concrete proposal (a BIP or equivalent) specifies hashed-key multisig; that proposal then answers the ADR question.
