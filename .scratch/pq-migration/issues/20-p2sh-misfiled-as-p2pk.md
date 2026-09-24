Status: wontfix

# P2SH spends misfiled as P2PK

## Problem Statement

Slice 7 recognizes P2PK from the spending side: a scriptSig that is a single strict-DER push. A P2SH spend whose redeem script is itself a single DER-shaped push would be misfiled as P2PK and tagged Exposed in output. Ticket 07 recorded this in the coverage report's Assumptions list but didn't guard against it.

## Rough shape

- Probably just a check that it doesn't happen in practice, for example counting single-push scriptSigs in ticket 08's sample whose push also parses as a plausible redeem script.

## Open questions

- Is documenting it enough? Likely `wontfix` unless ticket 08 finds a case.

## Source

Ticket 07 (coverage report Assumptions edit).

## Comments

**2026-09-24, wontfix.** A P2SH redeem script that is itself a valid strict-DER signature is not a script anyone would realistically use. Slice 7 already documents the case in the coverage report's Assumptions list, and that is enough. Reopen if ticket 08 finds a case.
