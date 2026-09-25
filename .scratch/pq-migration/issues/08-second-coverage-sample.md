Status: done

# Second coverage sample

## Problem Statement

Every coverage number so far comes from one sample, `docs/coverage/2026-09-sample.md`. That sample has at least one known skew: 160 of its 189 inscription inputs are in a single block (ticket 05). Several deferred items (tickets 09 and 16, closed tickets 18 and 20, and the annex rule in ticket 05) were put off with "only if the data shows it matters", and that data has only ever come from this one sample.

## Solution

Measure a second **Coverage sample** (glossary term, in `CONTEXT.md`) chosen by a rule stated in advance, compare it with September, and count the rare shapes that decide other tickets, in both samples, with a committed script. No change to the crate.

## Decisions (grilled 2026-09-24)

1. **Recent traffic from a different stretch, not pre-inscription history.** The question is whether September's skew changes any decision about today's traffic.
2. **10 blocks, chosen by a fixed rule: heights 943000, 944400, …, 955600** (every 1,400 blocks, about April to June 2026). A 3-month spread dilutes any one burst. The rule goes into the report before anything is fetched; if a burst shows up anyway, that's a finding. Real dates come from the fetch.
3. **Sample checks as in September:** 0 parse errors, per-block transaction counts match mempool.space's `tx_count`, and summed baseline Weight equals the summed block weights minus block overhead (80-byte header + compact-size transaction count, × 4, per block).
4. **Thresholds, written into the report before the run:**
   - **18 (positional signatures), 20 (P2SH misfiled as P2PK), annex (ticket 05, decision 6):** any occurrence in either sample reopens the ticket.
   - **09 (`multi_a` and other multi-key leaves), 16 (bare P2SH non-multisig):** reopen if the shape reaches ≥ 0.5% of Input weight in either sample. Below that, it stays in the backlog.
5. **Commit `scripts/shape-census.py`.** Untested and not in CI, like `fetch-blocks.py`, but reproducible (September's shape breakdown wasn't). It counts, with inputs and Input weight:
   - `multi_a` (`CHECKSIGADD`) and other multi-key leaves among P2TR script-path spends;
   - bare P2SH non-multisig;
   - script-path stacks with more than one 64 or 65-byte item;
   - script-path spends with an annex;
   - single-push scriptSigs classified P2PK that are really P2SH.
6. **Bucket membership comes from pqweight, not a second classifier.** The census pre-filters in Python (e.g. transactions with a taproot control block or a single-push scriptSig) and runs `pqweight migrate --json` on those, the same way September's shape tables came from `migrate()`. For P2PK candidates only, it looks up the spent output on mempool.space to tell P2PK from P2SH.
7. **Run the census on both samples**, so September's zeros are counted by the same rules. That means re-fetching September's three blocks.
8. **New report file** `docs/coverage/2026-XX-sample.md`, with XX named after the period the blocks are from. The September file stays as it is. Contents:
   - the sample: height rule, then hash, time and transaction count per block, plus the checks from decision 3;
   - the thresholds from decision 4;
   - `aggregate` results at all three parameter sets, side by side with September's after-slice-7 numbers: coverage, Unmapped reasons, Mapped spend types, Key exposure and Added weight;
   - the census table for both samples;
   - a verdict per threshold.
9. **Afterwards, update statuses** of tickets 09, 16, 18, 20 and the annex note in 05 to match the verdicts.
10. **No ADR.** The thresholds are easy to revise; they're in the report, not a hard-to-reverse decision.

## Acceptance

- `scripts/shape-census.py` committed and documented in its docstring like `fetch-blocks.py`.
- New coverage report with every section in decision 8, thresholds stated before results.
- The ten-block sample passes the decision 3 checks.
- Ticket statuses updated per the verdicts.

## Source

Ticket 05 (inscription burst, positional-signature follow-up), ticket 07 decision 1 (second sample listed as an alternative).

## Comments

**2026-09-24, done.** Results in `docs/coverage/2026-q2-sample.md`. Only multi-key leaves turned up (0.29% of Input weight, below the threshold), so 09 and 16 stay in the backlog, 18 and 20 stay wontfix, and the annex rule is unchanged. The ticket 18 count is restricted to leaves with one signature check: two-key leaves carry two 64-byte signatures legitimately.
