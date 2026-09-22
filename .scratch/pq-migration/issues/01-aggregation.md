Status: ready-for-agent

# Aggregate migration over many transactions (slice 2)

## Problem Statement

Slice 1 (`migrate()`, merged in PR #3) answers "what would this one transaction cost under PQ?" It doesn't answer "what would this actually cost the network" or "how often does our template coverage fail" — both need results summed over many transactions.

## Solution

A library function `aggregate(lines, ParameterSet) -> AggregateResult` that takes an iterator of raw transaction hex strings (one per input line) and runs `migrate()` over each, then sums the results. Exposed via a `pqweight aggregate` CLI subcommand.

## Decisions

- **Input**: one hex-per-line text file (or stdin, matching `weight`/`migrate`'s existing convention). Blank lines are skipped. This was chosen over parsing a raw serialized block (extra scope: coinbase handling, block-level parsing) or a directory of fixture files (less realistic as a real workflow).
- **Malformed lines**: a line that fails to parse is *not* silently dropped and does not abort the whole batch. It's recorded (line number + error message) and reported alongside the totals — same "never silently skew totals" principle CONTEXT.md states for Unmapped inputs.
- **Fee totaling**: sum of *per-transaction* fees — `fee(tx1.vsize, rate) + fee(tx2.vsize, rate) + ...` — not one `fee()` call on the summed vsize. Each real transaction pays its own fee and its own round-up separately; computing on the sum would undercount by up to N-1 satoshis for N transactions. Decided directly (not asked) because there is a single correct answer here, not a design tradeoff.
- **Migrated totals**: summed only over transactions where every input is mapped (i.e. `migrate()` returned `Some` for `migrated`), consistent with slice 1's "a transaction has a migrated total only when every input is mapped" rule. Baseline totals (today's weight) are summed over every transaction that parsed, mapped or not, since baseline weight doesn't depend on migration.
- **Report contents**: baseline weight/vsize totals, migrated weight/vsize totals (over fully-mapped transactions only), fee totals at a given rate (if provided) using the per-transaction summing above, and counts: transactions parsed, fully mapped, partially mapped (at least one Unmapped input), total Unmapped input count, and parse errors. No full per-transaction detail dump by default (that was considered and rejected as the default — too large for a real batch — but could be a later `--verbose` addition, out of scope here).
- **Seams** (per TDD discipline): `aggregate()` as the library seam, tested directly with hand-derived expected sums the same way `migrate()`'s tests worked from fixtures. `pqweight aggregate` CLI tested only as smoke tests, same split as `migrate`.

## Testing Decisions

- Tracer bullet: two known Fixtures (e.g. p2wpkh + p2sh-p2wpkh) in one input, hand-summed baseline and migrated totals.
- A case with one malformed line among valid ones: parse error recorded, other lines still aggregated.
- A case with an Unmapped input in one transaction: that transaction's baseline weight still counts toward the baseline total; its weight does not count toward the migrated total; the migrated total is still present (from the other, fully-mapped transactions) as long as at least one transaction is fully mapped.
- A case where every transaction has at least one Unmapped input: migrated total is `None` for the whole aggregate (nothing to sum).
- Fee total: two fixtures whose vsizes' sum-then-round differs from their individually-rounded-then-summed fee, to prove per-transaction summing is what's implemented (not accidentally simplified to summed-vsize).
- CLI smoke test with a small multi-line hex file fixture, plus one for a file containing a bad line.

## Out of Scope

- Full per-transaction detail output (`--verbose` or similar) — later, if needed.
- Parsing real serialized blocks.
- Long/Short-exposure tagging, multisig/script-path templates — separate slices per the parent spec.

## Open questions for the next session

None blocking — the decisions above are final enough to start the TDD loop directly. If anything, the CLI input source (file path arg vs. stdin-only) is worth a quick check with the user before writing the CLI smoke tests, mirroring how `weight`/`migrate` accept either.
