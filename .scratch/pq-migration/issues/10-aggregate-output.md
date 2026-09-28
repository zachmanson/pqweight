Status: done

# `aggregate` output: `--json`, Added weight by spend type, per-transaction detail via `migrate`

Merges the former tickets 10 (`--json`), 12 (`--verbose`) and 13 (per-spend-type breakdown). All three change what the tools report, so the JSON shapes are designed once rather than broken twice.

## Problem Statement

- **No machine-readable output from `aggregate`.** `weight` and `migrate` have `--json`, `aggregate` doesn't. Ticket 03 first asked for a JSON `breakdown` array, then dropped it for this reason. Coverage report numbers can only be scraped from human-readable text.
- **Per-transaction detail is incomplete.** Ticket 01 left a per-transaction dump open as a later `--verbose`. Since ticket 21, `migrate --json-lines` already prints one line per transaction with every input's spend type, template weight or Unmapped reason, but its successful lines carry no line number and it reports no baseline weight at all, so it can't answer "which transaction produced this odd total".
- **No Added weight by Baseline spend type.** The breakdown table already has one row per Baseline spend type (inputs, baseline and migrated Input weight), but no Added weight column, so "how much of the Added weight comes from P2WPKH versus P2TR versus multisig?" takes hand arithmetic.

## Decisions (grilled 2026-09-28)

1. **No new per-spend-type table.** The existing breakdown (`AggregateResult.breakdown`, `Mapped(BaselineSpendType)` rows) is it. The library gains `BreakdownRow::added_weight() -> Option<i64>` (`None` for Unmapped rows), like `ExposureRow::added_weight()`.
2. **Human breakdown table: two new columns** on mapped rows, `added input weight` and `% of it`, where the % is of the total Added weight over all mapped rows (equal to the Key exposure table's total, so the two tables agree). Unmapped rows show `-` in both. **Mapped rows are now sorted by Added weight, largest first**; unmapped rows stay sorted by baseline Input weight. Shown by default, as today.
3. **No `aggregate --verbose`.** Per-transaction detail is `migrate --json-lines`' job: `aggregate` is the summary, `migrate --json-lines` the per-item view, piped through `jq` when needed. One per-transaction format, not two that drift. `AggregateResult` keeps no per-transaction state (answers the ticket's old open question on memory).
4. **Additions to `migrate --json` / `--json-lines`** (additive only, so `second-calculation.py` and `shape-census.py` keep working):
   - `--json-lines`: every successful line gets `"line": N` as its first field, N 1-indexed counting blank lines, same as the error lines. Plain `migrate --json` does not.
   - `migrate --json`: a transaction-level `"baseline": {"weight","vsize","stripped_size","total_size"}` mirroring `migrated`, and `"baseline_weight"` (Input weight today) on every input, mapped and unmapped. No per-input `added_weight` (it's `template_weight - baseline_weight`).
   - Human `migrate` output unchanged.
5. **`aggregate --json` shape**, reusing `migrate --json`'s conventions so the same `jq` filters work:

   ```json
   {
     "scheme": "ml-dsa-44",
     "fee_rate": 10,
     "counts": {"parsed":0, "fully_mapped":0, "partially_mapped":0, "unmapped_inputs":0, "parse_errors":0},
     "baseline": {"weight":0, "vsize":0},
     "migrated": {"weight":0, "vsize":0},
     "partially_mapped": {"weight":0, "vsize":0},
     "fee": {"baseline":0, "migrated":0},
     "breakdown": [
       {"status":"mapped", "spend_type":"P2WSH multisig", "threshold":{"m":2,"n":3}, "key_exposure":"Hashed until spend",
        "inputs":0, "baseline_weight":0, "migrated_weight":0, "added_weight":0},
       {"status":"unmapped", "reason":"P2TR script-path", "key_exposure":"Exposed in output",
        "inputs":0, "baseline_weight":0}
     ],
     "key_exposure": [
       {"key_exposure":"Exposed in output", "mapped_inputs":0, "baseline_weight":0, "migrated_weight":0,
        "added_weight":0, "unmapped_inputs":0, "unmapped_baseline_weight":0}
     ],
     "errors": [{"line":0, "message":"..."}]
   }
   ```

   - `scheme` and `fee_rate` echo the inputs so a saved file says what produced it (human output doesn't need to).
   - `migrated` is `null` when no transaction is fully mapped; `fee.migrated` likewise.
   - `fee` is **omitted** without `--fee-rate`, as in `migrate --json`. `fee_rate` is then `null`.
   - `threshold` only on multisig rows, as in `migrate --json`. Names come from the same `spend_type_name` / `unmapped_reason_name` / `key_exposure_name` helpers.
   - One `breakdown` array with `status`, in the human table's order (decision 2). `key_exposure` on every row.
   - `key_exposure` rows: always all four, library order.
   - `added_weight` included (glossary term); no percentages (derivable).
6. **JSON written by hand**, like `weight --json` and `migrate --json`. No serde in the library or CLI; tests parse with the existing `serde_json` dev-dependency.
7. **No new glossary terms, no ADR.** Added weight, Baseline spend type, Unmapped reason and Key exposure already cover it; everything here is cheap to reverse.

## Slices

1. **Library + human table**: `BreakdownRow::added_weight()`, test in `tests/aggregate.rs`, including that mapped rows' Added weight sums to the Key exposure rows' total. CLI columns and sort order (decision 2), with a CLI test on the column and order.
2. **`migrate` additions** (decision 4), test-first in `tests/cli.rs`: `line` on every `--json-lines` line, `baseline` object, per-input `baseline_weight`.
3. **`aggregate --json`** (decision 5) with CLI smoke tests parsing via `serde_json`: with and without `--fee-rate`, a batch with a parse error, a batch with no fully mapped transaction (`migrated: null`). Plus a **consistency test**: on the same Fixture batch, grouping `migrate --json-lines`' per-input `baseline_weight` / `template_weight` by spend type (and threshold) and by Unmapped reason reproduces `aggregate --json`'s breakdown rows exactly. Update `USAGE` and README.

## Acceptance

- `aggregate` human output shows Added weight and its share per mapped spend type, mapped rows sorted by Added weight.
- `aggregate --json` matches decision 5, covered by the smoke tests above.
- `migrate --json` / `--json-lines` carry the fields in decision 4; existing fields unchanged.
- Consistency test passes; `second-calculation.py` CI step still passes unchanged.
- `cargo test`, clippy and CI green.

## Source

Ticket 01 (Out of Scope), ticket 02 (Out of Scope, and "`aggregate` is unchanged"), ticket 03 (the dropped `breakdown` array), ticket 07 decision 1, ticket 21 decision 7 (`--json-lines`).

## Done (2026-09-28)

- Report order (Mapped by Added weight, then Unmapped by baseline Input weight) lives in the CLI (`sorted_breakdown`), shared by the human table and `--json`. `AggregateResult.breakdown` stays in first-appearance order.
- `FeeRate` gained `Display` (plain decimal, digits as given, e.g. `1.50`, `.5` -> `0.5`) so `fee_rate` echoes as a valid JSON number.
- `docs/coverage/*.md` still show the old breakdown table (no Added weight columns); they're dated snapshots, left as recorded.
