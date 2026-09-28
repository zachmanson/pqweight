Status: wontfix

# P2PK migration template

Reopen if: a concrete proposal specifies a rescue spend for P2PK coins with a real witness layout

## Problem Statement

Slice 7 recognizes P2PK spends as an Unmapped reason and tags them Exposed in output, but gives them no template. P2PK coins are mostly early coins with fully public keys, so they're the coins most at risk from a quantum attacker. Right now the migrated totals leave them out entirely.

## Rough shape

- A template for what a PQ spend of a P2PK coin would look like. This needs a stated assumption (ADR 0002): a P2PK output commits to the raw key, so there is no PQ key to swap in. The coin would have to be spent with its current ECDSA signature to a new PQ output.

## Open questions

- Is a template meaningful at all, or is "these coins must move first, and here is what moving them costs" the better model? That could be a different kind of result from a template.
- How many P2PK spends does a normal sample have? The September 2026 sample might have too few to matter.

## Source

Ticket 07 decision 1, pq-migration spec ("Other Baseline spend types", P2PK row).

## Comments

**2026-09-28, wontfix.** No input was filed P2PK in either Coverage sample (0 of 25,156 inputs in September, 0 of 69,810 in April to June 2026). A template also can't add anything: a P2PK output commits to the raw key, so there's no hash to swap for a PQ key. Before an ECDSA-disabling soft fork the spend is today's ECDSA spend unchanged (0 Added weight); after it the coin can't be spent at all without a rescue mechanism nobody has specified. The question behind this ticket, what it costs to move Exposed coins to PQ outputs, is about unspent coins rather than spends, so it moved to `22-move-cost-exposed-coins.md`.
