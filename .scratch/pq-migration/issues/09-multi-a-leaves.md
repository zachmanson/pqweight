Status: needs-triage

# multi_a and other multi-key tapscript leaves

## Problem Statement

After slice 6, the only Unmapped inputs left in the September 2026 sample are the 2 P2TR two-key leaves (ticket 06, acceptance 2). They're tiny by weight today, but `multi_a` is the standard way to do multisig in tapscript, so their share may grow.

Ticket 06 also found an inconsistency to settle here: slice 5 keeps the empty-signature `NOTIF` leaf Unmapped, but slice 6 decision 5 migrates the same situation in P2WSH (a key that would grow even though no signature is present). Ticket 06 says to "revisit with multi_a".

## Rough shape

- A literal-swap template for `multi_a` leaves (every x-only key and every signature swapped), in the same style as slices 3 and 5.
- Decide the `NOTIF` leaf consistently with slice 6.

## Open questions

- Is it worth doing before ticket 08 shows how common these leaves are?
- Which other multi-key leaf shapes (`CHECKSIGADD` chains that aren't `multi_a`, the two-key leaves in the sample) belong in the same template?

## Source

Tickets 02, 05 and 06 (Out of Scope), ticket 07 decision 1.

## Comments

**2026-09-24, ticket 08 census: stays in the backlog.** Below the 0.5% of Input weight threshold in both samples: `multi_a` 109 inputs (0.20%) and other multi-key leaves 25 (0.09%) in the April to June 2026 sample, 2 two-key leaves (0.015%) in September. It's the only deciding shape that turned up, and it grew from 2 inputs to 134, so check it again in a third sample. See `docs/coverage/2026-q2-sample.md`.

**2026-09-28, triage: stays in the backlog.** Multi-key leaves are 0.29% of Input weight in the April to June sample, under ticket 08's 0.5% threshold. Pick it up when a Coverage sample puts them over 0.5%.
