Status: needs-triage

# Parse real serialized blocks

## Problem Statement

`aggregate` reads one transaction hex per line. To say "this is what block N would cost after migration", someone has to extract the transactions by hand first, and nothing checks that they're all there. Tickets 01 and 03 both left block parsing out of scope because it adds block header and coinbase handling.

## Rough shape

- Accept a raw serialized block (for example the output of `bitcoin-cli getblock <hash> 0`), parse the header and the transaction list, and feed the transactions to `aggregate`.
- Decide how to treat the coinbase: it has no real inputs to migrate, so it must not count as Unmapped.
- An oracle check against Bitcoin Core's reported block weight would fit ADR 0001's three-way approach.

## Open questions

- Library or CLI only? Ticket 03 specifically said "not in the library".

## Source

Tickets 01 and 03 (Out of Scope).
