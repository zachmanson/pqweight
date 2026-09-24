Status: needs-triage

# Second coverage sample

## Problem Statement

Every coverage number so far comes from one sample, `docs/coverage/2026-09-sample.md`. That sample has at least one known skew: 160 of its 189 inscription inputs are in a single block (ticket 05). Several deferred items (tickets 09 and 16, closed tickets 18 and 20, and the annex rule in ticket 05) were put off with "only if the data shows it matters", and that data has only ever come from this one sample.

## Rough shape

- Pick a second sample from a different period, ideally one that avoids the inscription burst.
- Re-run `pqweight aggregate` on it and add a dated section to the coverage report. Compare Unmapped reasons, shape breakdowns and Key exposure tables with the September 2026 sample.
- While doing it, count the things that decide other tickets: `multi_a` and other multi-key leaves (09), bare P2SH non-multisig (16), script-path stacks with more than one 64 or 65-byte item (18, closed unless found), script-path spends with an annex (ticket 05, decision 6), and single-push scriptSigs that could be P2SH misfiled as P2PK (20, closed unless found).

## Open questions

- Which period or blocks, and how many transactions?
- Should the report compare the two samples side by side, or just add another dated section?

## Source

Ticket 05 (inscription burst, positional-signature follow-up), ticket 07 decision 1 (second sample listed as an alternative).
