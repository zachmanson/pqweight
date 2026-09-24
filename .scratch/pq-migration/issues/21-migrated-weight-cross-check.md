Status: needs-triage

# Independent cross-check of migrated weight

## Problem Statement

Baseline weight is checked against independent oracles (ADR 0001). Migrated weight, the project's headline number, is only checked against expected values worked out by hand in the tests. The pq-migration spec suggests "a second calculation (for example a Python script building the migrated serialization)" as a later cross-check.

## Rough shape

- A separate implementation, in another language and sharing no code with the crate, that builds the migrated serialization for each template and computes its weight. Run it on the fixtures and the coverage sample, and compare.
- Decide whether it runs in CI. CI has no Bitcoin Core node, but a standalone script wouldn't need one.

## Open questions

- Every template, or only the high-volume ones (P2WPKH, P2TR key-path)?

## Source

pq-migration spec (Testing Decisions), ADR 0001.
