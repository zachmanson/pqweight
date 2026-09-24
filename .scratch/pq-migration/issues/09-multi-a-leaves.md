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
