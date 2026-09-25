Status: done

# Second calculation of migrated weight

## Problem Statement

Baseline weight is checked against independent oracles (ADR 0001). Migrated weight, the project's headline number, is only checked against expected values worked out by hand in the tests. If the hand arithmetic and the code make the same mistake (for example a 1-byte push prefix where a 1312-byte key needs `OP_PUSHDATA2`), nothing catches it. The pq-migration spec suggests "a second calculation (for example a Python script building the migrated serialization)" as a later cross-check.

## Solution

A **Second calculation** (glossary term, in `CONTEXT.md`): a stdlib-only Python script that, for each mapped input, builds the actual migrated bytes for its Migration template, serializes the migrated transaction, and compares the measured weights with `pqweight migrate --json`. It runs on the Fixtures in CI and once by hand on the September Coverage sample. It is never an Oracle: both sides implement the same stated templates (ADR 0002), so agreement shows the arithmetic is right, not that the templates are.

## Decisions (grilled 2026-09-25)

1. **Check the template bytes, not classification.** The script takes each input's `spend_type` and `threshold` from `pqweight migrate --json` (as `shape-census.py` does) and parses the raw transaction itself. Classification already has a rule test and near-miss tests per rule; the template byte counts only have hand arithmetic. Sharing an output label is not sharing code.
2. **Compare both levels.** Every mapped input's `template_weight`, and for fully mapped transactions `migrated.weight`, `vsize`, `stripped_size` and `total_size`. The totals are where a legacy-only transaction's 2-byte segwit marker and flag show up. Partially mapped transactions get the per-input check only.
3. **Every template, all three parameter sets** (ML-DSA-44, Falcon-512, SLH-DSA-128s). The riskiest templates are the low-volume ones (contract key swaps inside the script, P2SH multisig moving to the witness, the script-path control block), and SLH-DSA-128s's 32-byte key is a direct push where the other two need `OP_PUSHDATA2`.
4. **Python, stdlib only**, like `fetch-blocks.py` and `shape-census.py`. Parsing isn't what's being checked, so a library dependency buys nothing.
5. **Write the templates down first, in `docs/migration-templates.md`.** For each Migration template, the exact migrated scriptSig and witness items (e.g. "P2WPKH: witness `[PQ signature, PQ public key]`, scriptSig unchanged"), plus the marker/flag rule. The Python is implemented from this doc only, not by reading `migration.rs`, so a misreading in the Rust can't be copied across. When the two disagree, the doc decides which side is wrong. Link it from ADR 0002, since it's where the stated assumptions are actually stated.
6. **Fixtures in CI.** After `cargo test`, a step builds the CLI and runs the Second calculation over every Fixture × all three parameter sets, failing on any mismatch. First Python in CI, justified because this is a regression guard, not a one-off report.
7. **Batch mode for `migrate`: `--json-lines [<path>]`.** Reads one hex per line (file, or stdin if no path), writes one JSON object per line in today's `migrate --json` shape. Blank lines skipped. A line that fails to parse prints `{"line":N,"error":"..."}` and processing continues; exit non-zero only if the file can't be read. `--json-lines` with a positional hex is a usage error. Without it, a sample run is about 44k process launches.
8. **Guard against checking nothing.** Print inputs compared per spend type × parameter set. Fail if any template the script implements was compared zero times across the Fixtures, except those on an explicit allow-list. On a mismatch, print fixture name or txid, input index, spend type, parameter set and both values, and exit non-zero. Unmapped inputs are counted and skipped.
9. **Pay-to-anchor goes on the allow-list** ("coverage sample only": 2,229 inputs in September). It is a no-op template with no arithmetic to catch, so recording a Core fixture isn't worth it. Every other template has at least one Fixture today (checked 2026-09-25); `p2pkh` and `p2sh-multisig` cover the marker/flag case.
10. **Record the sample run** in a "Second calculation" section of `docs/coverage/2026-09-sample.md`: inputs compared per parameter set, mismatches, the commit it ran at, and the command to reproduce.
11. **No ADR.** None of this is hard to reverse.

## Slices

1. `docs/migration-templates.md`, linked from ADR 0002.
2. `migrate --json-lines`, test-first, with a CLI test.
3. `scripts/second-calculation.py` and the CI step on the Fixtures (decisions 1–4, 6, 8, 9).
4. September sample run and doc section (decision 10).

## Acceptance

- Template doc covers every Migration template and is the only source the Python was written from.
- `migrate --json-lines` behaves as in decision 7, covered by CLI tests.
- CI runs the Second calculation on all Fixtures × 3 parameter sets and passes, with nonzero compare counts for every template except allow-listed pay-to-anchor.
- September sample run recorded with 0 mismatches, or each mismatch resolved against the template doc (a Rust bug gets a regression test in `migrate.rs`).

## Source

pq-migration spec (Testing Decisions), ADR 0001, ADR 0002.

## Comments

**2026-09-25, implemented** (commit 19ecf9f, sample record in the next commit).

- `docs/migration-templates.md` written from the pq-migration spec and tickets 02, 05, 06 and ADR 0002, not from `migration.rs`. The Python was written from the doc only.
- `migrate --json-lines [<path>]`: four CLI tests (file, stdin with a blank and a bad line, unreadable file, path plus hex). A lone argument after `--json-lines` is always read as the path, so `--json-lines <hex>` fails with "could not read" rather than the usage text. Error lines use a small JSON string escaper, which `assumptions` now also use instead of Rust `Debug` formatting.
- Fixtures: 0 mismatches, every template compared at each parameter set except allow-listed pay-to-anchor. Sensitivity checked by breaking the Python: a 1-byte prefix for `OP_PUSHDATA2` pushes gave 122 mismatches, dropping the marker and flag broke the totals, and an empty allow-list failed on pay-to-anchor.
- September sample: 25,154 mapped inputs compared per parameter set, 0 mismatches. See the "Second calculation" section of `docs/coverage/2026-09-sample.md`.
- The review found a crash in the Python strict-DER check on a short `30`-tagged item (fixed), and noted that "strict DER" here means BIP66's layout only, without its sign and padding rules. The doc now says so. Both sides check the same thing.
- The Python has no unit tests of its own. Like `shape-census.py`, it is a check, and it's checked end to end by the mutations above.
