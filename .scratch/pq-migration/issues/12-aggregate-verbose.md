Status: needs-triage

# `aggregate --verbose` per-transaction detail

## Problem Statement

`aggregate` only reports totals. Ticket 01 considered a full per-transaction dump and rejected it as the default because it's too big for a real batch, but left it open as a later `--verbose` flag. Without it, the only way to find out which transaction produced an odd total is to re-run `migrate` on lines one at a time.

## Rough shape

- `--verbose` prints one line (or one JSON object, with ticket 10) per transaction: line number, baseline and migrated weight, and Unmapped inputs with their reasons.

## Open questions

- Do this before or together with ticket 10, so both the human and the JSON output get it?

## Source

Ticket 01 (Out of Scope), ticket 03 (Out of Scope).
