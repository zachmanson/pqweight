Status: needs-triage

# P2PK migration template

## Problem Statement

Slice 7 recognizes P2PK spends as an Unmapped reason and tags them Exposed in output, but gives them no template. P2PK coins are mostly early coins with fully public keys, so they're the coins most at risk from a quantum attacker. Right now the migrated totals leave them out entirely.

## Rough shape

- A template for what a PQ spend of a P2PK coin would look like. This needs a stated assumption (ADR 0002): a P2PK output commits to the raw key, so there is no PQ key to swap in. The coin would have to be spent with its current ECDSA signature to a new PQ output.

## Open questions

- Is a template meaningful at all, or is "these coins must move first, and here is what moving them costs" the better model? That could be a different kind of result from a template.
- How many P2PK spends does a normal sample have? The September 2026 sample might have too few to matter.

## Source

Ticket 07 decision 1, pq-migration spec ("Other Baseline spend types", P2PK row).
