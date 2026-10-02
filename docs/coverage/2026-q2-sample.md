# Coverage check: April to June 2026 mainnet sample (31 March to 27 June)

The second Coverage sample, from ticket 08 (`.scratch/pq-migration/issues/08-second-coverage-sample.md`). Every earlier number came from one sample, `2026-09-sample.md`, which had a known skew: 160 of its 189 inscriptions were in one block. This sample checks whether that skew changes any conclusion, and counts the rare shapes that several backlog tickets were waiting on.

Measured with the code as of ticket 07 (slices 1 to 7). The September numbers below are that report's "After slice 7" numbers, which come from the same code.

## Sample

**Rule, fixed before any block was fetched:** 10 blocks at heights 943000 to 955600, every 1,400 blocks. That is roughly 3 months (the first block falls on 31 March, the rest in April to June), ending 2 months before the September sample begins, so a burst in any one week weighs against nine other blocks.

| Height | Hash | Time | Transactions |
|---|---|---|---|
| 943000 | `00000000000000000000b4644da0b48256b69c3c50339c9970bf9e8459072492` | 2026-03-31 01:41 UTC | 4257 |
| 944400 | `000000000000000000006c8c4e2292fc88ce9a68e1b4863110aab5dc12011cc8` | 2026-04-09 23:07 UTC | 3921 |
| 945800 | `00000000000000000001f6d305f8449ca5e4f3d1c735a95089b6a379b48699eb` | 2026-04-19 16:15 UTC | 3149 |
| 947200 | `0000000000000000000110410e55bce6673ae494fc93430d1e900edde53e8380` | 2026-04-29 21:26 UTC | 5654 |
| 948600 | `000000000000000000021c123d619d38860c90d11623b6b5cf2b3cf677f9b80d` | 2026-05-09 10:06 UTC | 5316 |
| 950000 | `000000000000000000010b93c9ea1c29fea277383f0f7d1f26de8b5802e885ff` | 2026-05-18 21:54 UTC | 639 |
| 951400 | `000000000000000000011461eb9d250379c6ee29b69e62bce9880b524498ddb6` | 2026-05-28 10:33 UTC | 4718 |
| 952800 | `00000000000000000000cbc4e840341ab4dd2e34e63dc4467be880a99e640119` | 2026-06-08 04:28 UTC | 3757 |
| 954200 | `000000000000000000005fa7516dc0fe391e7c9c6904525631b6708fda71ff36` | 2026-06-18 07:04 UTC | 4652 |
| 955600 | `0000000000000000000096a4825988f376713b46283185a71b79f0f6db2fff45` | 2026-06-27 03:54 UTC | 6084 |

Block 950000 is only about a quarter full (891,735 WU). The rule keeps it: it's a real block the rule picked.

To reproduce:

```
python scripts/fetch-blocks.py <the ten hashes> | pqweight split-blocks > sample.txt
pqweight aggregate --scheme ml-dsa-44 sample.txt
python scripts/shape-census.py <path to pqweight> sample.txt
```

The hex isn't committed (about 33 MB).

Checks on the sample itself:

- All 42,147 transactions parsed (0 parse errors). Per-block transaction counts match mempool.space's `tx_count`.
- The summed baseline Weight is 36,829,984. The ten blocks' reported weights sum to 36,833,304. The 3,320 WU difference is exactly 10 × (80-byte header + 3-byte transaction count) × 4.

### Verified blocks

Ticket 14 (`.scratch/pq-migration/issues/14-block-parsing.md`). Re-run with the ticket 14 code from the raw blocks, so each block is checked rather than assumed: its transactions hash to the header's merkle root and its coinbase's witness commitment matches. Every block passed, the hashes computed from the blocks' own bytes are the hashes listed above, and the transaction count, baseline and migrated Weight and partially mapped baseline weight equal this report's, as does `aggregate` on the same blocks split one transaction per line.

```
python scripts/fetch-blocks.py <the ten hashes> > blocks.txt
pqweight aggregate --scheme ml-dsa-44 --blocks blocks.txt
```

| Height | Hash computed from the block's bytes | Transactions | Block weight | Migrated Block weight (ML-DSA-44) | × the 4,000,000 WU limit |
|---|---|---|---|---|---|
| 943000 | `00000000000000000000b4644da0b48256b69c3c50339c9970bf9e8459072492` | 4,257 | 3,993,518 | - (2 partially mapped) | - |
| 944400 | `000000000000000000006c8c4e2292fc88ce9a68e1b4863110aab5dc12011cc8` | 3,921 | 3,993,489 | - (7 partially mapped) | - |
| 945800 | `00000000000000000001f6d305f8449ca5e4f3d1c735a95089b6a379b48699eb` | 3,149 | 3,993,541 | - (9 partially mapped) | - |
| 947200 | `0000000000000000000110410e55bce6673ae494fc93430d1e900edde53e8380` | 5,654 | 3,993,678 | 34,030,127 | 8.5× |
| 948600 | `000000000000000000021c123d619d38860c90d11623b6b5cf2b3cf677f9b80d` | 5,316 | 3,993,577 | - (6 partially mapped) | - |
| 950000 | `000000000000000000010b93c9ea1c29fea277383f0f7d1f26de8b5802e885ff` | 639 | 891,735 | 4,603,646 | 1.2× |
| 951400 | `000000000000000000011461eb9d250379c6ee29b69e62bce9880b524498ddb6` | 4,718 | 3,993,893 | - (28 partially mapped) | - |
| 952800 | `00000000000000000000cbc4e840341ab4dd2e34e63dc4467be880a99e640119` | 3,757 | 3,993,800 | - (15 partially mapped) | - |
| 954200 | `000000000000000000005fa7516dc0fe391e7c9c6904525631b6708fda71ff36` | 4,652 | 3,993,414 | - (7 partially mapped) | - |
| 955600 | `0000000000000000000096a4825988f376713b46283185a71b79f0f6db2fff45` | 6,084 | 3,992,659 | - (2 partially mapped) | - |

A block's migrated weight is its header and transaction count plus every transaction's migrated weight. A block with any partially mapped transaction has none (the all-or-nothing rule), which is why most rows show `-`. Over the limit means these transactions wouldn't fit in one block after migration, not that the block is invalid. The Block weights sum to the blocks' reported weights above.

## Thresholds, set before the run

Written into ticket 08 (decision 4) before any block was fetched, so the numbers couldn't move the bar. (The decisions were committed to git in the same session as the results, so history alone doesn't prove the order.)

- **Ticket 18 (positional signatures), ticket 20 (P2SH misfiled as P2PK), the annex rule (ticket 05, decision 6):** any occurrence in either sample reopens the ticket.
- **Ticket 09 (`multi_a` and other multi-key leaves), ticket 16 (bare P2SH non-multisig):** reopen if the shape reaches 0.5% of Input weight in either sample. Below that, it stays in the backlog.

One clarification, made when the census was written: ticket 18 is about the single-key template choosing the signature by size, so its count is only for leaves with **one** signature check. A two-key leaf has two 64-byte signatures on its stack legitimately, and it's Unmapped for its leaf shape, not because of the size rule. Read literally ("script-path stacks with more than one 64 or 65-byte item"), September has 2 such inputs: its two two-key leaves.

## Coverage, side by side

| | September 2026 (3 blocks) | April to June 2026 (10 blocks) |
|---|---|---|
| Transactions | 14,692 | 42,147 |
| Mapped inputs | 25,154 of 25,156 (99.99%) | 69,676 of 69,810 (99.81%) |
| Mapped Input weight | 7,073,104 of 7,074,162 WU (99.985%) | 23,229,215 of 23,296,294 WU (99.71%) |
| Unmapped Input weight | 1,058 WU (0.015%) | 67,079 WU (0.29%) |
| Fully mapped transactions | 14,690 (99.99%) | 42,071 (99.82%) |
| Partially mapped baseline weight | 3,613 WU (0.03%) | 118,182 WU (0.32%) |

Unmapped is P2TR script-path only in both samples: 2 inputs in September, 134 here. No other Unmapped reason occurred. The all-or-nothing migrated-total rule still leaves out almost nothing (0.32% of baseline Weight, well under the 5% the September report set for revisiting it).

### Mapped and Unmapped, by Baseline spend type

Share of each sample's Input weight, largest first in the new sample:

| Baseline spend type | Sept inputs | Sept % of Input weight | Apr-Jun inputs | Apr-Jun % of Input weight | Growth at ML-DSA-44 (Apr-Jun) |
|---|---|---|---|---|---|
| P2WPKH | 18,203 | 69.8% | 50,217 | 58.5% | 14.4× |
| P2TR script-path single-key | 209 | 2.1% | 4,405 | 16.2% | 5.2× |
| P2PKH | 662 | 5.5% | 3,925 | 10.0% | 6.6× |
| P2TR key-path | 2,015 | 6.6% | 6,581 | 6.5% | 17.0× |
| P2SH-P2WPKH | 1,035 | 5.3% | 2,624 | 4.1% | 11.0× |
| All multisig | 489 (10 thresholds) | 3.4% | 1,661 (11 thresholds) | 4.0% | 16.1× |
| P2WSH contract | 297 | 2.0% | 231 | 0.4% | 15.9× |
| P2SH-P2WSH contract | 12 | 0.1% | 22 | 0.1% | 9.8× |
| pay-to-anchor | 2,229 | 5.2% | 0 | 0.0% | - |
| coinbase | 3 | 0.0% | 10 | 0.0% | 1.0× |
| Unmapped: P2TR script-path | 2 | 0.0% | 134 | 0.3% | - |

2-of-3 is again the most common multisig threshold (1,241 of 1,661 inputs across all three wrappers), followed by 2-of-2 (248).

### Fully mapped totals

| Parameter set | Sept ratio | Apr-Jun migrated weight | Apr-Jun baseline weight of the same transactions | Apr-Jun ratio |
|---|---|---|---|---|
| ML-DSA-44 | 8.23× | 297,094,263 | 36,711,802 | 8.09× |
| Falcon-512 | 3.92× | 141,381,600 | 36,711,802 | 3.85× |
| SLH-DSA-128s | 16.40× | 592,008,079 | 36,711,802 | 16.13× |

At ML-DSA-44 the ten blocks would fill about 74. **The headline ratio moved by less than 2% between samples**, even though the mix underneath changed a lot (below). The ratios are the most stable result the project has.

### Key exposure and Added weight

ML-DSA-44:

| Key exposure | Sept share of Added weight | Apr-Jun mapped inputs | Apr-Jun baseline Input weight | Apr-Jun migrated Input weight | Apr-Jun Added weight | Apr-Jun share | Apr-Jun Unmapped inputs (weight) |
|---|---|---|---|---|---|---|---|
| Exposed in output | 9.4% | 10,986 | 5,292,217 | 45,365,465 | 40,073,248 | 15.4% | 134 (67,079) |
| Hashed until spend | 90.6% | 58,680 | 17,931,570 | 238,357,871 | 220,426,301 | 84.6% | 0 |
| No key | 0.0% | 10 | 5,428 | 5,428 | 0 | 0.0% | 0 |
| Undetermined | - | 0 | 0 | 0 | 0 | - | 0 |

Added weight of the Exposed in output row at each parameter set:

| Parameter set | Sept Exposed in output | Sept all inputs | Sept share | Apr-Jun Exposed in output | Apr-Jun all inputs | Apr-Jun share |
|---|---|---|---|---|---|---|
| ML-DSA-44 | 8,155,056 | 86,537,177 | 9.4% | 40,073,248 | 260,499,549 | 15.4% |
| Falcon-512 | 3,331,200 | 34,921,229 | 9.5% | 16,244,614 | 104,715,309 | 15.5% |
| SLH-DSA-128s | 17,393,208 | 184,410,309 | 9.4% | 85,700,348 | 555,550,403 | 15.4% |

**The exposed-first result is not stable.** Migrating only Exposed in output inputs takes the ten blocks from 36.8 million WU to about 76.9 million (19.2 blocks' worth), against 74 if everything migrates. That's about 15% of the full cost here, against 9.4% in September. The reason is P2TR script-path single-key: 22.8% of today's mapped Input weight is Exposed in output here, against 8.7% in September, almost all of it data-envelope leaves. Because those carry their data unchanged, the exposed group grows only 8.6× here (against 13.3× for Hashed until spend), where in September it grew slightly faster than the rest. Treat "migrating exposed keys first costs about a tenth" as one sample's answer. Across these two samples it's between a tenth and a sixth, and it tracks envelope activity.

## Census

`scripts/shape-census.py` on both samples. Bucket membership comes from `pqweight migrate --json`. In both samples its Unmapped P2TR script-path row matches `aggregate` exactly (2 inputs, 1,058 WU; 134 inputs, 67,079 WU), and its total Input weight matches the sum of `aggregate`'s rows.

| Shape (deciding ticket) | Sept inputs | Sept Input weight | Apr-Jun inputs | Apr-Jun Input weight | Apr-Jun % of Input weight |
|---|---|---|---|---|---|
| `multi_a` leaves, `CHECKSIGADD` (09) | 0 | 0 | 109 | 46,933 | 0.20% |
| Other multi-key leaves (09) | 2 | 1,058 | 25 | 20,146 | 0.09% |
| Bare P2SH non-multisig (16) | 0 | 0 | 0 | 0 | 0% |
| One-check leaf with more than one 64 or 65-byte stack item (18) | 0 | 0 | 0 | 0 | 0% |
| Taproot spend with an annex (05, decision 6) | 0 | 0 | 0 | 0 | 0% |
| P2PK whose spent output is P2SH (20) | 0 | 0 | 0 | 0 | 0% |

Every Unmapped P2TR script-path input in both samples is a multi-key leaf. The 18 row counts only leaves with one signature check (see Thresholds). The annex and 18 rows look only at inputs pqweight files as a taproot spend. No input was filed P2PK in either sample, so ticket 20's check had nothing to look up.

## Verdicts

- **Ticket 09: stays in the backlog.** `multi_a` and other multi-key leaves together are 0.29% of Input weight here, below 0.5%. It is the only deciding shape that turned up, and it grew from 2 inputs to 134, so it's the one to watch in a third sample.
- **Ticket 16: stays in the backlog.** No bare P2SH non-multisig spend in either sample.
- **Ticket 18: stays wontfix.** No one-check leaf with more than one 64 or 65-byte item. The only stacks matching the literal wording are multi-key leaves, which ticket 09 covers.
- **Ticket 20: stays wontfix.** No occurrence.
- **Annex rule (ticket 05, decision 6): unchanged.** No annex in either sample.

## What differs from September

- **No pay-to-anchor at all.** September's 2,229 were mostly rune-mint chains. These ten blocks create and spend no anchor outputs. The September report already called pay-to-anchor's share unstable; it swung from 5.2% of Input weight to zero.
- **Data envelopes are bigger, and still bursty.** P2TR script-path single-key is 16.2% of Input weight, against 2.1%. It is not one protocol: block 948600 holds 2,739 spends of a `TACIT` envelope (`<key> CHECKSIG OP_FALSE OP_IF "TACIT" … OP_ENDIF`), block 943000 holds 1,213 `ord` inscriptions, and block 944400 has only 31 script-path spends (19 of them inscriptions) but 1.33 million WU of them. Blocks 944400 and 948600 alone carry 67% of the sample's script-path weight. Spreading the blocks over 3 months didn't avoid bursts; it caught different ones.
- **P2WSH contract is smaller** (0.4% against 2.0%). September's hashlock/CLTV service was one service's traffic, as its report warned.
- **`multi_a` shows up.** The 109 `multi_a` inputs are in 6 of the 10 blocks, with 53 in block 951400 and 25 in 952800. It's in more blocks than any one burst, but still uneven.

## Assumptions

Same as the September report's Assumptions section, plus the census script's own heuristics:

- A leaf with any `OP_CHECKSIGADD` is `multi_a`. A leaf with two or more `CHECKSIG`/`CHECKSIGVERIFY`/`CHECKSIGADD` ops is multi-key.
- An annex is a last witness item starting `0x50` in a witness of 2 or more items.
- The census pre-filter skips transactions whose inputs are all non-taproot segwit or plain P2PKH (two pushes, the second a 33 or 65-byte key). Its Unmapped P2TR script-path totals match `aggregate` in both samples, and `aggregate` reports no P2SH non-multisig and no P2PK in either, so the filter dropped nothing the census counts.
