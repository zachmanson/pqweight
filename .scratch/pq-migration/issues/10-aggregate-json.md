Status: needs-triage

# `aggregate --json`

## Problem Statement

`weight` and `migrate` have `--json` output, but `aggregate` only prints a table. Ticket 03 first asked for a JSON `breakdown` array, then dropped it because `aggregate` has no JSON mode. So the coverage report numbers can only be scraped from human-readable text.

## Rough shape

- A `--json` flag on `pqweight aggregate` that covers everything the table shows: totals, counts, parse errors, the Unmapped breakdown and the Key exposure rows.
- The library already returns `AggregateResult`, so this is mostly serialization plus CLI smoke tests.

## Open questions

- Should the JSON shape be designed together with ticket 12 (`--verbose`) and ticket 13 (per-spend-type breakdown), so it doesn't break when those land?

## Source

Ticket 03 (the dropped `breakdown` array), ticket 07 decision 1.
