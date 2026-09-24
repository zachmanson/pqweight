Status: needs-triage

# `aggregate` output: `--json`, `--verbose` and a per-spend-type breakdown

Merges the former tickets 10 (`--json`), 12 (`--verbose`) and 13 (per-spend-type breakdown). All three change what `aggregate` reports, so the JSON shape is designed once rather than broken twice.

## Problem Statement

`aggregate` only prints a totals table. That causes three gaps:

- **No machine-readable output.** `weight` and `migrate` have `--json`, but `aggregate` doesn't. Ticket 03 first asked for a JSON `breakdown` array, then dropped it for this reason. Coverage report numbers can only be scraped from human-readable text.
- **No per-transaction detail.** Ticket 01 rejected a per-transaction dump as the default because it's too big for a real batch, but left it open as a later `--verbose` flag. Right now the only way to find out which transaction produced an odd total is to re-run `migrate` on lines one at a time.
- **No breakdown by Baseline spend type.** `aggregate` splits results by Unmapped reason (slice 3) and by Key exposure (slice 7), but not by spend type. So it can't answer "how much of the Added weight comes from P2WPKH versus P2TR versus multisig?", which is the most direct way to see which templates drive the result.

## Rough shape

- **Per-spend-type table:** one row per Baseline spend type, with input count, baseline Input weight, migrated Input weight and Added weight. Shown by default, next to the Unmapped and Key exposure tables.
- **`--verbose`:** one line per transaction, giving line number, baseline and migrated weight, and each Unmapped input with its reason.
- **`--json`:** everything the human output shows (totals, counts, parse errors, all three breakdowns), plus a per-transaction array when combined with `--verbose`.
- The library already returns `AggregateResult`. The spend-type breakdown is a library change. `--json` and `--verbose` are mostly serialization and CLI work, tested with smoke tests like `migrate`'s.

## Open questions

- Should the per-spend-type table be shown by default or behind a flag?
- Should `--verbose` keep per-transaction results in `AggregateResult` (memory grows with batch size), or stream them from the CLI as each transaction is processed?

## Source

Ticket 01 (Out of Scope), ticket 02 (Out of Scope, and "`aggregate` is unchanged"), ticket 03 (the dropped `breakdown` array), ticket 07 decision 1.
