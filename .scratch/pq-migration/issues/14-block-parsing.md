Status: done

# Verified blocks

## Problem Statement

`aggregate` reads one transaction hex per line. Coverage samples are named by block hash, but the transactions come from `scripts/fetch-blocks.py`, which splits whatever bytes mempool.space returned. That split is untested Python, and nothing checks that the bytes really are the block the hash names, or that they're complete. (The Coinbase input part of the original ticket was already done in ticket 03.)

The goal: **`aggregate` takes whole blocks, proves each one is intact, and reports per block.** The numbers it already reports don't change. What's new is that a sample is checked rather than assumed, plus a per-block view ("block X weighs W today, would weigh W' after migration").

Honest framing: no current result is wrong or blocked without this. Its value is rigor (samples verified from their bytes) and showing we understand block serialization (ADR 0001's spirit).

## Decisions (grilled 2026-10-01)

1. **In the library.** A new `block.rs` with something like `parse_block(bytes) -> Result<Block, BlockError>`. This reverses ticket 03's "not in the library": back then block parsing was just splitting; now it's verification logic, which needs unit tests and Fixtures. CLI code is only smoke-tested.
2. **SHA-256 from the `sha2` crate**, the library's first runtime dependency. The compression function isn't Bitcoin knowledge; double-SHA256, byte order, txid vs wtxid, the merkle tree and the witness commitment are, and we write those ourselves.
3. **What a block must pass.** All of these, or the block is rejected:
   - It parses as 80-byte header, CompactSize transaction count, then exactly that many transactions (existing transaction parser rules), with no trailing bytes.
   - **Block hash** = double-SHA256 of the header, shown reversed like Core.
   - **Txid merkle root** matches the header. Odd levels duplicate the last hash. A mutated tree (two identical adjacent hashes at any level, CVE-2012-2459) is rejected, as Core does.
   - **Witness commitment** (BIP 141): if any transaction has witness data, the coinbase must have an output whose script starts `6a24aa21a9ed`; if several do, the last one counts. Its 32 bytes must equal double-SHA256(wtxid merkle root ‖ witness reserved value). The coinbase's wtxid counts as all zeros, and the reserved value is the coinbase's single 32-byte witness item. If no transaction has witness data, the commitment may be absent.
   - Proof of work is **not** checked: a fabricated block would get a different hash, and the hash is printed (decision 4).
4. **Hash binding: print, don't compare.** `aggregate --blocks` prints the hash computed from each block's bytes. There's no expected-hash input to keep in sync. Reports quote the printed hashes.
5. **Input format: `pqweight aggregate --blocks [path]`**, one block hex per line (what `bitcoin-cli getblock <hash> 0` prints), blank lines skipped, file or stdin like today. Without `--blocks` nothing changes.
6. **A failing block is dropped whole**: it's recorded as an `AggregateError` with its line number and reason, and the run continues. Never aggregate part of a block: if the merkle root doesn't match, we don't know which transaction is wrong.
7. **Per-block rows** (human output and `--json`, only with `--blocks`): block hash, transaction count, Block weight today, migrated Block weight and its multiple of the 4,000,000 WU limit (for example "×7.9"), or "—" plus the Partially mapped count when any transaction is Partially mapped (the all-or-nothing rule). The note says over the limit means "these transactions wouldn't fit in one block", not "invalid block". Migrated Block weight = 4 × (80 + the count's CompactSize length) + the migrated transaction weights; the count doesn't change.
8. **Sample-wide totals stay sums of transaction weights**, exactly as today, so `--blocks` and the transaction-per-line mode give identical totals on the same blocks. Block weight appears only in the per-block rows.
9. **`pqweight split-blocks [path]`**: reads one block per line, verifies each (decision 3) and prints one transaction hex per line, with each block's hash and transaction count on stderr. A failing block is an error that stops the command (it feeds other tools, so it must not quietly drop data). This replaces the Python split, so `second-calculation.py` and `shape-census.py` keep working on verified input.
10. **`fetch-blocks.py` only downloads**: one block hex per line. Its split code is deleted. Update the usage notes in `second-calculation.py`, `shape-census.py` and both coverage reports (`fetch-blocks.py … | pqweight split-blocks` or `aggregate --blocks`).
11. **Glossary**: Block weight added; Coverage sample now "identified by the hashes computed from their bytes" (`CONTEXT.md`, done in grilling). **No ADR**: library placement and the `sha2` dependency are cheap to reverse.

## Testing

Three-way, per ADR 0001: our code, `rust-bitcoin` (dev-dependency: `Block` consensus decode, `block_hash`, `check_merkle_root`, `check_witness_commitment`, `weight`) and Bitcoin Core.

- **Regtest block Fixture**: a new recording script in the `record-fixtures.ps1` style. Core mines a block containing at least one segwit and one legacy transaction (so the commitment is present). It stores `getblock <hash> 0` as hex plus Core's `hash`, `merkleroot`, `weight`, `size`, `strippedsize` and `nTx`. Put block fixtures in their own folder (`tests/fixtures/block/`) so the transaction Fixture loops and `second-calculation.py --fixtures` don't pick them up.
- **Mainnet block 170** (tiny, pre-segwit, commitment absent, real known hash), fetched from mempool.space like the `p2pk` Fixture. Oracle values: its known hash and merkle root, plus weight from `rust-bitcoin`, noted as such.
- **Near-misses** built from the regtest Fixture: a flipped byte in a non-witness field → merkle mismatch; a flipped witness byte → commitment mismatch; witness data with the commitment output removed → error; a mutated tree (last transaction duplicated, count bumped) → rejected even though the root matches; trailing bytes; a count larger than the transactions present → Truncated.
- CLI smoke tests: `aggregate --blocks` text and `--json` on the Fixture; `split-blocks` output equals the transactions; `aggregate` on that output gives the same totals as `aggregate --blocks`.

## Slices

1. `sha2` + `parse_block` (parse, hash, merkle, commitment, Block weight) + both block Fixtures + near-miss tests.
2. `aggregate --blocks`: per-block results in the library result, per-block rows in text and `--json`, failing blocks as errors.
3. `pqweight split-blocks`; `fetch-blocks.py` becomes download-only; usage notes updated in both scripts and both reports.
4. Re-run both Coverage samples (`docs/coverage/2026-09-sample.md`, 3 blocks; `docs/coverage/2026-q2-sample.md`, 10 blocks) through `--blocks`. Pass = every block verifies, the totals equal the committed reports exactly, and the computed hashes equal the hashes already listed. Add the per-block table to each report.

## Out of Scope

- Proof-of-work and any context-dependent checks (BIP 34 height, timestamps, the block weight limit as a validity rule, signatures).
- Binary block files; `migrate --json-lines --blocks`.
- Per-block Key exposure or spend-type breakdowns.

## Source

Tickets 01 and 03 (Out of Scope).

## Comments

**2026-10-01, slices 1-4 done.**

- `parse_block` and `Block` (hash, merkle root, transactions, Block weight) live in `src/block.rs`. `aggregate_blocks` and `BlockRow` live in `src/aggregate.rs`, which shares its per-transaction summing with `aggregate`, so the totals match by construction. The CLI gains `aggregate --blocks` and `split-blocks`. `fetch-blocks.py` now only downloads.
- Fixtures are in `tests/fixtures/block/`. `regtest-block` was recorded by `scripts/record-block-fixture.ps1`: a coinbase plus a legacy spend and a P2WPKH spend. The node needs `-changetype=bech32`, otherwise the segwit spend can pick the legacy change and end up with no witness. `block-170` came from mempool.space.
- Slice 4: all 13 blocks verified. The computed hashes and transaction counts equal the report tables, and the totals equal the reports. The per-block tables were added under "Verified blocks" in both reports.
- Decisions made while implementing:
  - The CLI prints `x8.2` and `-`, not `×` and `—`, because its output is ASCII elsewhere. The reports keep their own `8.2×` style.
  - `--json` gives `migrated_limit_multiple` as an exact decimal (weight × 25 / 10^8).
  - A zero transaction count is its own error (`NoTransactions`).
- Known differences from Core's context-free `CheckBlock`, left out because the spec didn't ask for them; the merkle check bounds their risk:
  - Transaction 0 isn't checked to be a coinbase (null prevout), and later transactions aren't checked not to be one.
  - Core v27+ `IsBlockMutated` also rejects any transaction whose witness-stripped size is 64 bytes. We don't.

**2026-10-02, code review.**

- Fixed: one public `MAX_BLOCK_WEIGHT` (block.rs) used by move-cost and the CLI; `BlockError::Block` renamed `Framing`; block Fixtures are found by scanning `tests/fixtures/block/` instead of a hand-kept list; the regtest test also checks Core's `size` and `strippedsize`; the `split-blocks` CLI test compares every line; the 2026-09 report's ticket 21 run log keeps the command that ran at `19ecf9f`, with a note on how to reproduce today.
- Left alone: `counts.parse_errors` also counts blocks that fail verification (renaming it changes the `--json` contract); the CLI keeps `x8.2` and `-` (decision above).
