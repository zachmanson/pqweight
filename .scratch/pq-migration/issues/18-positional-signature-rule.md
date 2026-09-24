Status: wontfix

# Positional signature rule for script-path stacks

Reopen if: ticket 08 finds such stacks

## Problem Statement

Slice 5's single-key leaf template finds the signature by size: a 64 or 65-byte stack item. If a stack has more than one item of that size (for example inert data that happens to be 64 bytes), size alone can't tell which one is the signature. Ticket 05 put off a positional rule "only if acceptance 2 shows it matters", and asked the slice 5 report to count such inputs.

## Rough shape

- Work out which stack position the leaf's `CHECKSIG` consumes, and use that position instead of size.

## Open questions

- Does it happen often enough to bother? Check the slice 5 section of the coverage report and ticket 08's sample before deciding.

## Source

Ticket 05 (Out of Scope and acceptance 2).

## Comments

**2026-09-24, wontfix.** The coverage report's "After slice 5" section says the size rule "turned away no inputs" in the September 2026 sample, and the size rule is simpler than tracking stack positions. Reopen if ticket 08's sample finds script-path stacks with more than one 64 or 65-byte item.
