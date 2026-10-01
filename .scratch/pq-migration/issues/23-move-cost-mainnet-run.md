Status: done

# Move cost on the 935k mainnet snapshot

## Problem Statement

Ticket 22 built `pqweight move-cost` and showed it correct on a regtest snapshot Fixture, but the question it exists for has no answer yet: **how many weight units, and how many blocks, would it take to move every unspent Exposed coin on mainnet to PQ outputs?** This ticket is ticket 22's slice 4, split off on 2026-09-30 so the code could close without the big download.

## What to do

1. Download the assumeutxo snapshot at height 935,000 (about 9.4 GB; mirrors with direct HTTP links: bitcoin-snapshots.jaonoctus.dev). Keep it outside the repo.
2. Verify it once, as ADR 0003 says: start Core v31 on a throwaway mainnet datadir, let it sync headers only (it needs the header chain up to 935,000; about 80 MB, no blocks), then `loadtxoutset <path>`. Any mismatch fails with "Bad snapshot content hash". Record the `loadtxoutset` result (coins loaded, base hash) for the report. Delete the datadir afterwards.
3. Run `pqweight move-cost <snapshot>` and `--json`. Check `scanned.coins` equals the header's count and the `loadtxoutset` count.
4. Write `docs/move-cost/935k-snapshot.md`, in the style of `docs/coverage/`: how to reproduce (snapshot source, hash check, commands), the table, and the stated assumptions from `docs/migration-templates.md` "Move layouts". It must say: the numbers are a lower bound (reuse exposure not counted, P2TR key-path only); blocks hold nothing but moves; the ≥546-sat columns leave out postage and data outputs.

## Notes from ticket 22

- The parser streams, so memory stays flat; expect run time to be dominated by reading 9.4 GB (release build: `cargo run --release -p pqweight-cli -- move-cost ...`).
- Bare multisig gets one row per threshold. Expect many 1-of-3 rows from data-carrying protocols (Counterparty, Stamps) whose "keys" are data; worth a sentence in the report, since nobody holds keys for most of them.
- Not checked against Core per coin on mainnet: the per-coin Oracle is regtest only. The mainnet check is `loadtxoutset`'s hash plus the coin count.

## Comments

- 2026-09-30: Done. Report `docs/move-cost/935k-snapshot.md`. `loadtxoutset` on Core v31.1.0 accepted the file (164,241,311 coins, base `…0fb5ee`); header count, `loadtxoutset` count and `scanned.coins` all 164,241,311; scanned value 19,984,148.03 BTC vs 19,984,375 issued (gap = known unspendable). Total: 3,444 floor / 6,507 ceiling blocks; P2TR is 91% of the weight, P2PK 90% of the value in ~5 blocks. Snapshot kept at `C:\Users\zachm\pqweight-data\utxo-935000.dat` (outside the repo, SHA-256 `e572ddbe…d486`); throwaway datadir deleted. Scan takes ~40 s in release.
