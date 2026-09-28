Status: needs-triage

# Move cost of Exposed coins

## Problem Statement

Every result so far is about spends: how much weight real transactions would gain if their signatures moved to PQ. The coins most at risk from a quantum attacker are the ones whose keys sit in an unspent output (Key exposure: Exposed in output), and many of them, such as early P2PK coins, are never spent in a normal sample (0 P2PK spends in about 95,000 inputs across both Coverage samples). The question is: **how many weight units, and how many blocks, would it take to move a set of unspent Exposed coins to PQ outputs?** That's a different kind of result, computed over unspent coins, not over Coverage samples.

## Rough shape

- For a set of unspent Exposed coins, compute the weight of the transactions that move them to PQ outputs, and the block count that implies.

## Open questions

- **Data source.** There's no mainnet node here: Bitcoin Core only runs in regtest for Fixtures, and mainnet blocks come from mempool.space. Options: sync a mainnet node and use `dumptxoutset`, an outside dataset, or published aggregate counts. The last one breaks the pattern of pqweight computing every number from bytes it parsed itself (ADR 0001).
- **Which coins:** P2PK only, or every Exposed in output coin (P2TR, bare multisig)? Reused addresses can't be seen from the UTXO set alone either.
- **When the moves happen:** before an ECDSA-disabling soft fork (today's spends, unchanged) or after (needs a rescue mechanism nobody has specified).
- **Consolidation:** how many coins one move transaction sweeps into one PQ output. This probably dominates the result.
- **Where it lives:** in the library, or a script plus a doc like the coverage reports.

## Source

Ticket 11 (wontfix 2026-09-28): the P2PK template was reframed as this question during grilling.

## Comments
