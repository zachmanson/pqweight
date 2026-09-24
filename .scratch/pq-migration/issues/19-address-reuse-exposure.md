Status: wontfix

# Measure the address-reuse blind spot in Key exposure

## Problem Statement

Slice 7 tags Key exposure from each input's own data. A coin whose address was reused already has its key on-chain from the earlier spend, even if this input's output type would normally hide it. Ticket 07 decision 3 documents the tag as a lower bound for exactly this reason, but nothing measures how big the gap is.

## Rough shape

- Needs data from outside a single transaction: whether an input's script was spent before. Could be a separate analysis script rather than part of the library.

## Open questions

- Is this in scope for pqweight at all? The pq-migration spec rules out fetching data from a node at runtime.
- Would a published estimate of address reuse be good enough to quote in the coverage report instead of measuring it?

## Source

Ticket 07 (decisions 3 and 9).

## Comments

**2026-09-24, wontfix.** Measuring it needs chain history from outside the transaction, and the spec rules out fetching data from a node at runtime. The lower-bound caveat is already in CONTEXT.md and the coverage report. If a number is wanted, quoting a published address-reuse estimate in the coverage report is enough.
