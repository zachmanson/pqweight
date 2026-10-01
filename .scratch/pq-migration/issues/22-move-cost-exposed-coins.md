Status: done

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

**2026-09-30, grilled: ready-for-agent.** New glossary terms Exposed coin and Move cost (`CONTEXT.md`); data source in ADR 0003. Decisions:

1. **Result.** For each output type: coin count, BTC value, and Move cost floor and ceiling in WU and in full blocks (÷ 4,000,000 WU). "Blocks hold nothing but moves" is a stated assumption. Every row also has a second pair of numbers counting only coins of at least 546 sats (inscription postage and data outputs nobody will pay to move).
2. **When.** Moves happen before any ECDSA/Schnorr-disabling fork: today's signatures, into a PQ output. Post-fork rescue spends are Out of Scope; reopen if a rescue proposal with a byte layout appears (same objection as ticket 15).
3. **Which coins.** Exposed coins only: P2PK (compressed and uncompressed), bare multisig, P2TR. Reuse exposure (P2PKH, P2SH, P2WSH with an earlier revealed key or script) is left out; the report says the numbers are a lower bound.
4. **Data source.** The 935k assumeutxo snapshot, verified once with `loadtxoutset` on a throwaway datadir (headers-only sync). No `hash_serialized_3` recompute, no secp256k1 dependency: uncompressed P2PK (compressed-script codes 4/5) is recognized by code alone. See ADR 0003.
5. **Consolidation.** No guess at N. Floor = sum of spend weights (perfect consolidation). Ceiling = one coin per transaction (1 in, 1 out: spend weight + transaction overhead + one PQ output).
6. **Spend layouts** (stated assumptions, written in `docs/migration-templates.md` so the Second calculation checks them): every input has a 36-byte outpoint and 4-byte sequence; P2PK scriptSig = one 72-byte DER signature push; bare multisig m-of-n = `OP_0` + m × 72-byte signature pushes, m read from the script; P2TR = key-path, one 64-byte Schnorr signature (a lower bound for script-path-only coins such as inscriptions, said in the report). PQ output = BIP-360 style 34-byte scriptPubKey, 43 bytes = 172 WU.
7. **Code.** A streaming snapshot parser module (bytes to coins, never holds the file in memory) separate from a move-cost module (coins to floor/ceiling). CLI `pqweight move-cost <snapshot>`, text and `--json`.
8. **Snapshot format facts** (Core v31 source): 51-byte header (magic `utxoÿ`, u16 version 2, network magic, base blockhash, u64 coin count); body grouped per txid (txid, CompactSize count, then per coin CompactSize vout + `Coin`); no terminator, stop after coin count. `Coin` = `VARINT(height*2+coinbase)`, `VARINT(CompressAmount(value))`, compressed script (`VARINT nSize`: 0 P2PKH, 1 P2SH, 2/3 compressed P2PK, 4/5 uncompressed P2PK x-only, >=6 raw script of nSize-6). Core VARINT is MSB-first base-128 with -1 per continuation byte, not LEB128. Sources: `node/utxo_snapshot.h`, `rpc/blockchain.cpp` `WriteUTXOSnapshot`, `coins.h`, `serialize.h`, `compressor.{h,cpp}`. Mirrors with direct HTTP links: bitcoin-snapshots.jaonoctus.dev.

**Slices:**

1. Snapshot parser + a regtest snapshot Fixture (P2PK compressed and uncompressed, bare multisig 1-of-1 to 3-of-3, P2TR, P2PKH, P2WPKH, P2WSH) recorded by a new script in the `record-fixtures.ps1` style; Oracle values are `gettxoutsetinfo` totals and per-coin `gettxout`. Unit tests for VARINT and amount decompression on Core's documented values.
2. Move-cost arithmetic + move layouts in `docs/migration-templates.md` + Second calculation extended to them.
3. `pqweight move-cost` CLI, text and `--json`.
4. Download the 935k snapshot, verify with `loadtxoutset`, run it, write `docs/move-cost/935k-snapshot.md`.

**2026-09-30, slices 1-3 done (c108df9, 13afd8b + review fixes).** Slice 4 (mainnet run and report) split into ticket 23 by the user's decision, so this ticket closes on the tool.

- `read_snapshot` (`src/snapshot.rs`) streams coins; snapshot Fixture `tests/fixtures/snapshot/regtest-utxo.{dat,json}` (118 coins, Oracle = `gettxoutsetinfo` + `gettxout` for every coin), recorded by `scripts/record-snapshot-fixture.ps1` (needs `-permitbaremultisig=1`: off by default since Core v28).
- `move_cost` / `MoveCost::add` (`src/move_cost.rs`); bare multisig gets one row per threshold. Layouts in `docs/migration-templates.md` "Move layouts"; Second calculation checks them on the snapshot Fixture in `--fixtures` mode (no CI change needed).
- Decisions made while implementing: raw-stored P2PK (key not on the curve, or hybrid `06`/`07` prefix) is not counted; P2TR ceiling includes the 2 WU marker and flag; blocks are reported as a decimal (weight ÷ 4,000,000); `--json` values are sats; the CLI also reports scanned totals and a total row.
