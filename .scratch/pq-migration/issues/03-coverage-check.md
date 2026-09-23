Status: ready-for-agent

# Real-world coverage check (slice 4)

## Problem Statement

Slices 1 to 3 cover single-key and multisig spends, but we don't know how much of real mainnet traffic that is. **Unmapped** carries no information, so `aggregate` can say how many inputs failed but not what they were. Without that, the next template (probably P2TR script-path) and the revisit of the all-or-nothing migrated-total rule would be guesses.

## Solution

Make Unmapped say what it saw, handle the **Coinbase input** that every real block has, and have `aggregate` break its results down per **Baseline spend type** and per **Unmapped reason**. Then run it on a small mainnet sample and write down what the numbers say.

## Decisions

- **The question**: which spend shapes dominate Unmapped (this drives the next template), plus overall coverage. Both are measured by input count and by baseline **Input weight**, because a few large inputs can matter more than their count suggests.
- **Input weight**: an input's own bytes only. Outpoint, scriptSig and sequence count 4 WU each, and its witness counts 1 WU per byte. Transaction overhead and outputs belong to no input, so a transaction's Input weights don't sum to its Weight. This matches how `template_weight` is already defined, so baseline and migrated per-input weights are comparable. Rejected: spreading the whole transaction's weight across its inputs, because that invents an attribution rule we'd have to defend.
- **Coinbase input**: a new no-op Baseline spend type, like pay-to-anchor. It is mapped, has nothing to migrate and keeps its weight unchanged. It is detected exactly: the transaction has one input, its previous txid is all zeros and its index is `0xffffffff`. This rule lives in the library, not in the fetch script, because anyone feeding raw block contents hits it.
- **Unmapped reason**: `InputResult::Unmapped` carries the spend shape observed on the input. It is recognized from the spending side only and names what was seen, never a cost. The reasons are checked in this order, and the first match wins:
  1. P2TR script-path: after removing any annex, the last witness item is a control block of length 33 + 32k (k ≥ 0) with `byte0 & 0xfe == 0xc0`.
  2. P2TR key-path with annex: empty scriptSig and a witness of 2 items: one 64 or 65 bytes, then an annex starting `0x50`.
  3. P2WSH non-multisig: empty scriptSig, 2 or more witness items, not matched above.
  4. P2SH-wrapped segwit non-multisig: the scriptSig is one push of `0014<20 bytes>` or `0020<32 bytes>` that didn't map.
  5. P2SH non-multisig: empty witness, and the scriptSig's last push parses as a script (a redeem-script candidate).
  6. Legacy other: empty witness and any other non-empty scriptSig (P2PK, bare multisig, uncompressed-key oddities).
  7. Unknown: everything else.
  - These are heuristics, not proofs (a P2WSH script could in theory look like a control block). They are labels for what was observed, and the report lists them as assumptions, in the spirit of ADR-0002.
  - Don't split "P2SH non-multisig" further (timelocks, HTLCs) until the data says it's large.
  - Rejected: a separate diagnostic classifier outside `migrate()`, because it would be a second classifier that can drift from the first.
- **Breakdown in `aggregate`**: always printed, not behind a flag. It's a bounded number of rows (unlike the rejected per-transaction dump), and it's why `aggregate` exists.
  - One row per exact Baseline spend type, Multisig threshold included (2-of-3 and 3-of-5 P2WSH are separate rows, per CONTEXT.md). One row per Unmapped reason. Mapped and Unmapped rows are shown as separate sections.
  - Row columns: input count, % of all inputs, baseline Input weight, % of all baseline Input weight, and migrated Input weight (Mapped rows only).
  - Human output is a table. JSON adds a `breakdown` array.
- **`migrate` output** shows the Unmapped reason for each Unmapped input (human and JSON).
- **All-or-nothing migrated-total rule**: not changed in this ticket. The report must include the numbers that decision needs: the % of transactions that are partially mapped, and their share of baseline weight.
- **Sample source**: mempool.space `/api/block/:hash/raw`, one request per block. The local Bitcoin Core only runs throwaway regtest nodes and has no mainnet chain. The block is split into hex-per-line in the script, not in the Rust crate, which keeps block parsing out of the library (as slice 2 decided).
- **Fetch script**: `scripts/fetch-blocks.py` (Python 3), taking block hashes and writing hex-per-line. It walks transaction lengths (varints, segwit marker). It is untested and not run in CI. That's acceptable because a wrong split fails loudly: the next line fails to parse, and `aggregate` reports parse errors per line.
- **Sample**: about 3 recent blocks, spread over time (not consecutive heights) so one fee spike doesn't dominate. Commit the block hashes, not the hex (about 5 MB); blocks are immutable, so the hashes reproduce the sample exactly. The sample is not a test Fixture, because Fixtures are Oracle-checked single transactions and this is a measurement.
- **Report**: a dated file under `docs/coverage/` (e.g. `docs/coverage/2026-09-sample.md`). It holds the block hashes, the breakdown, which template the numbers justify next, the partially-mapped numbers above and the Unmapped reason heuristics as assumptions.
- **No ADR**: Unmapped reasons are cheap to change later, so they don't meet the "hard to reverse" bar.

## Testing Decisions

- Each Unmapped reason has a test from a Fixture, plus a near-miss test where it matters (for example, a control block of the wrong length, or wrong leaf-version bits, is not P2TR script-path).
- Existing Fixtures cover P2TR script-path (`p2tr-scriptpath`) and key-path with annex (`p2tr-keypath-annex`). Record the missing ones on regtest with `record-fixtures.ps1 -Only …` so they get real Oracle values: a P2WSH non-multisig spend (e.g. a timelock script), a P2SH non-multisig spend, and a coinbase transaction.
- Coinbase: mapped as the no-op type, and its transaction is fully mapped with migrated weight equal to baseline weight. A near-miss (index not `0xffffffff`, or a non-zero txid) is not a Coinbase input.
- Input weight: hand-derived for one legacy and one segwit Fixture.
- Breakdown: a batch of known Fixtures with hand-counted rows, counts and percentages, including one Unmapped input so both sections appear.
- The existing `aggregate` and `migrate` tests keep passing, adjusted only for the new Unmapped payload.
- CLI smoke tests: `aggregate` prints the breakdown table and the JSON has `breakdown`; `migrate` shows an Unmapped reason.

## Acceptance

1. The tested part is done: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are all clean.
2. `scripts/fetch-blocks.py` fetches the chosen blocks; `pqweight aggregate` runs over them with no parse errors.
3. The report is committed under `docs/coverage/`, with the block hashes.

## Out of Scope

- New Migration templates (P2TR script-path etc.). The report picks the next one.
- Changing the all-or-nothing migrated-total rule.
- Parsing blocks in the library.
- Long/Short-exposure tagging, `aggregate --verbose`.
