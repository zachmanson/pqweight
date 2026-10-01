# Move cost: mainnet UTXO set at height 935,000

How many weight units, and how many blocks, would it take to move every unspent **Exposed coin** on mainnet to PQ outputs, spending each with today's signatures before any soft fork disables them? Measured with `pqweight move-cost` as of ticket 23 (`.scratch/pq-migration/issues/23-move-cost-mainnet-run.md`). The layouts and rules are in `docs/migration-templates.md`, "Move layouts"; ADR 0003 explains why the input is an assumeutxo snapshot.

## Snapshot

| | |
|---|---|
| File | `utxo-935000.dat`, 9,387,990,306 bytes |
| Source | https://files-vps02.jaonoctus.dev/utxo-935000.dat (listed on bitcoin-snapshots.jaonoctus.dev) |
| SHA-256 of the file | `e572ddbe456d254f05fb004cebe225bdb3656074b66f0e9b1c7fa83e1301d486` |
| Base block | 935,000, `0000000000000000000147034958af1652b2b91bba607beacc5e72a56f0fb5ee` |
| Coins | 164,241,311 |

The SHA-256 is only so a second run can tell it has the same file. It is not the trust anchor; the trust anchor is the check below.

Checks on the snapshot itself:

- **Content hash.** Bitcoin Core v31.1.0 loaded it with `loadtxoutset` on a throwaway mainnet datadir that had synced headers only. Core compares the coins against the `hash_serialized_3` hardcoded in its `chainparams.cpp` and refuses the file with "Bad snapshot content hash" on any mismatch. It returned `coins_loaded: 164241311`, `tip_hash: …0fb5ee`, `base_height: 935000`.
- **Coin count.** The snapshot header says 164,241,311 coins, `loadtxoutset` loaded 164,241,311, and `pqweight move-cost` scanned 164,241,311 (`scanned.coins` in `--json`). All three agree, so the parser read every coin.
- **Total value.** The scanned coins hold 19,984,148.03 BTC. The subsidy issued up to block 935,000 is 19,687,500 (to height 840,000) + 95,000 × 3.125 = 19,984,375 BTC. The ~227 BTC gap is coins that never entered the UTXO set (the genesis output, the two duplicated coinbases, provably unspendable outputs, subsidy miners didn't claim). So the parser reads amounts correctly too, not just the coin count.

The per-coin check against Core (the Oracle) runs on regtest only, so on mainnet these three checks are all there is.

To reproduce:

```
curl -O https://files-vps02.jaonoctus.dev/utxo-935000.dat

# once: verify it against Core's hardcoded hash, on a throwaway datadir
bitcoind -datadir=<tmp> -prune=550
#   wait until getblockchaininfo shows headers >= 935000 (~a minute, no blocks needed)
bitcoin-cli -datadir=<tmp> -rpcclienttimeout=0 loadtxoutset <path>/utxo-935000.dat
bitcoin-cli -datadir=<tmp> stop    # then delete <tmp>

cargo run --release -p pqweight-cli -- move-cost <path>/utxo-935000.dat
cargo run --release -p pqweight-cli -- move-cost --json <path>/utxo-935000.dat
```

The scan takes about 40 seconds on a laptop SSD; memory stays flat because the parser streams. `loadtxoutset` takes about 15 minutes. The file isn't committed.

## Result

All Exposed coins. Blocks = weight ÷ 4,000,000. Days assume 144 blocks a day.

| Exposed coin | Coins | BTC | Floor WU | Floor blocks | Ceiling WU | Ceiling blocks |
|---|---|---|---|---|---|---|
| P2PK compressed | 10,280 | 6,536.62 | 4,687,680 | 1.17 | 6,867,040 | 1.72 |
| P2PK uncompressed | 34,335 | 1,709,732.97 | 15,656,760 | 3.91 | 22,935,780 | 5.73 |
| bare multisig 1-of-1 | 1,121 | 0.05 | 515,660 | 0.13 | 753,312 | 0.19 |
| bare multisig 1-of-2 | 204,617 | 19.76 | 94,123,820 | 23.53 | 137,502,624 | 34.38 |
| bare multisig 1-of-3 | 2,345,243 | 41.48 | 1,078,811,780 | 269.70 | 1,576,003,296 | 394.00 |
| bare multisig 2-of-2 | 3,265 | 3.76 | 2,455,280 | 0.61 | 3,147,460 | 0.79 |
| bare multisig 2-of-3 | 511 | 4.86 | 384,272 | 0.10 | 492,604 | 0.12 |
| bare multisig 3-of-3 | 20 | 0.11 | 20,880 | 0.01 | 25,120 | 0.01 |
| P2TR | 54,685,124 | 195,907.37 | 12,577,578,520 | 3,144.39 | 24,280,195,056 | 6,070.05 |
| **Total** | **57,284,516** | **1,912,246.99** | **13,774,234,652** | **3,443.56** | **26,027,922,292** | **6,506.98** |

Coins of at least 546 sats only:

| Exposed coin | Coins | BTC | Floor WU | Floor blocks | Ceiling WU | Ceiling blocks |
|---|---|---|---|---|---|---|
| P2PK compressed | 10,278 | 6,536.62 | 4,686,768 | 1.17 | 6,865,704 | 1.72 |
| P2PK uncompressed | 34,328 | 1,709,732.97 | 15,653,568 | 3.91 | 22,931,104 | 5.73 |
| bare multisig 1-of-1 | 1,113 | 0.05 | 511,980 | 0.13 | 747,936 | 0.19 |
| bare multisig 1-of-2 | 204,478 | 19.76 | 94,059,880 | 23.51 | 137,409,216 | 34.35 |
| bare multisig 1-of-3 | 2,328,536 | 41.48 | 1,071,126,560 | 267.78 | 1,564,776,192 | 391.19 |
| bare multisig 2-of-2 | 3,258 | 3.76 | 2,450,016 | 0.61 | 3,140,712 | 0.79 |
| bare multisig 2-of-3 | 511 | 4.86 | 384,272 | 0.10 | 492,604 | 0.12 |
| bare multisig 3-of-3 | 20 | 0.11 | 20,880 | 0.01 | 25,120 | 0.01 |
| P2TR | 39,910,457 | 195,857.90 | 9,179,405,110 | 2,294.85 | 17,720,242,908 | 4,430.06 |
| **Total** | **42,492,979** | **1,912,197.52** | **10,368,299,034** | **2,592.07** | **19,456,631,496** | **4,864.16** |

Thresholds that don't appear (2-of-1, m-of-n with n > 3, and so on) had no coins.

## What it says

**Moving everything takes 3,444 to 6,507 full blocks: about 24 to 45 days of nothing but moves.** Leaving out coins under 546 sats brings it to 2,592 to 4,864 blocks, about 18 to 34 days.

**P2TR is 91% of the weight and 10% of the value.** 54.7 million P2TR coins hold 195,907 BTC; their floor alone is 3,144 blocks. Of those coins, 14.8 million are under 546 sats and hold 49 BTC between them, about 335 sats each: almost all are the 330-sat postage outputs inscriptions and other token protocols create. Dropping them is what moves the total from 24 to 18 days.

**P2PK is 90% of the value and 0.15% of the weight.** 44,615 P2PK coins hold 1,716,270 BTC (most of it mined in 2009 and 2010, in uncompressed-key outputs). Moving all of them fits in about 5 blocks at the floor, 7.5 at the ceiling: under two hours of block space. Whether those coins move is a question of who holds the keys, not of block space.

**Bare multisig is 2.55 million coins holding 70 BTC.** 1-of-3 alone is 2.35 million coins averaging 1,769 sats, and costs 270 floor blocks. This is the shape Counterparty and Stamps use to store data: one real key plus two "keys" that are data bytes. pqweight checks only key lengths, so it counts them; it can't tell which, if any, key in each script anyone holds. Most of them are above 546 sats, so the dust cut-off barely touches them (2,345,243 to 2,328,536). If they are data outputs nobody intends to spend, the real figure for bare multisig is far below the 294 floor blocks shown.

Of the whole UTXO set, Exposed coins are 35% of coins and 9.6% of value.

## Assumptions

These are the "Move layouts" in `docs/migration-templates.md`, and the `assumptions` the command prints:

- Coins move before any soft fork disables ECDSA or Schnorr signatures, spent with today's signatures.
- Each signature is a 72-byte DER ECDSA signature, or a 64-byte Schnorr signature (default sighash) for P2TR.
- P2TR coins move by key path. Coins that can only be spent by script path (such as inscription commit outputs) cost more, so for them this is a lower bound.
- Each move pays into a BIP-360 style PQ output of 43 bytes (172 WU).
- Floor: perfect consolidation, the spends' own weight only, each kind swept on its own. Ceiling: one coin per transaction, 1 input and 1 output.
- **Blocks hold nothing but moves.** In practice moves compete with ordinary traffic, so the calendar time is longer than the block counts suggest.
- **Every number is a lower bound.** Coins exposed only by address reuse (P2PKH, P2SH, P2WPKH, P2WSH whose key or script was revealed by an earlier spend) are not counted: the snapshot can't show reuse. P2PK scripts with an invalid or hybrid-encoded key are stored raw by Core and not counted either.
- **The ≥546-sat tables leave out postage and data outputs** that nobody will pay to move. 546 sats is the P2PKH dust limit; it is a cut-off, not a claim about any one coin.
