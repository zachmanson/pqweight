Status: needs-triage

# Per-spend-type breakdown in `aggregate`

## Problem Statement

`aggregate` breaks results down by Unmapped reason (slice 3) and by Key exposure (slice 7), but not by Baseline spend type. So it can't answer "how much of the Added weight comes from P2WPKH versus P2TR versus multisig?", which is the most direct way to see which templates drive the result.

## Rough shape

- One row per Baseline spend type: input count, baseline Input weight, migrated Input weight, Added weight.

## Open questions

- Should this replace any rows in the existing tables, or be a third table?

## Source

Ticket 02 (Out of Scope, and "`aggregate` is unchanged").
