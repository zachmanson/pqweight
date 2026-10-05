# pqweight

Computes Bitcoin transaction weight from raw transactions, checked against Bitcoin Core, and models how block space and fees would change if signatures moved to post-quantum schemes (ML-DSA-44, Falcon-512, SLH-DSA-128s).

Bitcoin has no post-quantum opcode today, so every migrated number comes from a **Migration template**: a stated guess at what the PQ version of a spend would put in its witness. Each result prints the assumptions it rests on. Spends no template covers are reported as **Unmapped** instead of being guessed at. See [CONTEXT.md](CONTEXT.md) for the vocabulary and [docs/adr/](docs/adr/) for the design decisions.

## Install

Needs Rust 1.98.1 (pinned in `rust-toolchain.toml`, so `rustup` picks it up automatically).

```sh
cargo install --path crates/pqweight-cli
```

Or run it without installing: `cargo run -p pqweight-cli -- <command> ...`

## Usage

There are five commands. `weight`, `migrate` and `aggregate` take raw transactions as hex, either as an argument or on stdin (`aggregate --blocks` takes raw blocks instead); `split-blocks` takes raw blocks; `move-cost` reads a UTXO snapshot file.

```
pqweight weight [--json] [<hex>]
pqweight migrate --scheme <scheme> [--fee-rate <rate>] [--json] [<hex>]
pqweight migrate --scheme <scheme> [--fee-rate <rate>] --json-lines [<path>]
pqweight aggregate --scheme <scheme> [--fee-rate <rate>] [--json] [--blocks] [<path>]
pqweight move-cost [--json] <snapshot>
pqweight split-blocks [<path>]
```

`<scheme>` is one of `ml-dsa-44`, `falcon-512`, `slh-dsa-128s`. `<rate>` is in sat/vB and can be a decimal like `1.5`.

### Weight of a transaction

```sh
$ pqweight weight 02000000000101...
weight: 436
vsize: 109
stripped size: 82
total size: 190
```

`--json` gives one line you can pipe into `jq`:

```sh
$ pqweight weight --json 02000000000101...
{"weight":436,"vsize":109,"stripped_size":82,"total_size":190}
```

Getting the hex from a node:

```sh
bitcoin-cli getrawtransaction <txid> | pqweight weight
```

### What a transaction would weigh with PQ signatures

```sh
$ pqweight migrate --scheme ml-dsa-44 --fee-rate 10 02000000000101...
input 0: mapped (P2WPKH), weight: 3903, key exposure: Hashed until spend
migrated weight: 4069
migrated vsize: 1018
exceeds relay limit: no
baseline fee: 1090 sat
pq fee: 10180 sat
fee difference: 9090 sat
fee ratio: 9.34
assumptions:
- PQ outputs commit to a hash of the public key, as today's outputs do
- the 520-byte stack element limit (witness items and pushes inside a script) and sigop accounting are changed by a soft fork
- outputs are unchanged by migration
```

Each input is either mapped to a template (with its migrated Input weight and Key exposure) or unmapped with the reason. A transaction only gets a migrated total when every input is mapped:

```sh
$ pqweight migrate --scheme slh-dsa-128s <a P2PK spend>
input 0: unmapped (P2PK), key exposure: Exposed in output
migrated total: unavailable (not every input is mapped)
...
```

`--json` returns the same thing as one object, plus each input's Input weight today (`baseline_weight`) and the transaction's weight today (`baseline`), with a `fee` field when `--fee-rate` is given:

```sh
$ pqweight migrate --scheme falcon-512 --json <a P2TR key-path spend>
{"inputs":[{"status":"mapped","spend_type":"P2TR key-path","baseline_weight":230,"template_weight":1734,"key_exposure":"Exposed in output"}],"baseline":{"weight":396,"vsize":99,"stripped_size":82,"total_size":150},"migrated":{"weight":1900,"vsize":475,"stripped_size":82,"total_size":1654},"exceeds_relay_limit":false,"assumptions":[...]}
```

`--json-lines` does the same for a file (or stdin) of one hex per line, printing one object per transaction with its line number first (`{"line":1,"inputs":...}`). A line that doesn't parse prints `{"line":N,"error":"..."}` and the rest still run. This is the per-transaction view behind `aggregate`'s totals, so "which transaction produced this?" is a `jq` filter away:

```sh
pqweight migrate --scheme ml-dsa-44 --json-lines sample.txt | jq -c 'select(.migrated.weight > 100000) | .line'
```

### Totals over many transactions (whole blocks)

`aggregate` reads one transaction hex per line (blank lines skipped) from a file or stdin. Lines that don't parse are listed as errors and the rest still count.

The easiest way to get real input is to pull whole mainnet blocks from mempool.space. `fetch-blocks.py` writes one raw block per line (what `bitcoin-cli getblock <hash> 0` prints), and `--blocks` reads that:

```sh
python scripts/fetch-blocks.py <block hash> [<block hash>...] > blocks.txt
pqweight aggregate --scheme ml-dsa-44 --fee-rate 5 --blocks blocks.txt
```

With `--blocks`, each block is verified before it counts: it must parse with no bytes left over, its transactions must hash to the header's merkle root (a mutated tree, CVE-2012-2459, is rejected), and if any transaction has witness data the coinbase's BIP 141 witness commitment must match. A block that fails is listed as an error and dropped whole. Proof of work isn't checked. Instead, each block's row shows the hash computed from its own header, so you can compare it with the hash you asked for. The totals are the same as for the block's transactions one per line. The extra rows give each block's Block weight today and after migration, and that as a multiple of the 4,000,000 WU limit. Over the limit means those transactions wouldn't fit in one block, not that the block is invalid. A block with a partially mapped transaction shows `-` and the count, since it has no migrated weight.

`pqweight split-blocks blocks.txt > sample.txt` verifies the same way and prints one transaction hex per line, for tools that take transactions (`aggregate` without `--blocks`, `migrate --json-lines` and the Python scripts). Each block's hash goes to stderr, and a block that fails stops the command.

Output (from a small four-transaction file):

```
transactions parsed: 4
fully mapped: 4
partially mapped: 0
unmapped inputs: 0
parse errors: 0
baseline weight: 2286
baseline vsize: 572
migrated weight: 25422
migrated vsize: 6358
baseline fee: 2860 sat
migrated fee: 31790 sat
partially mapped baseline weight: 0 (0.0% of baseline)
breakdown (inputs, % of inputs, baseline input weight, % of it, migrated input weight, added input weight, % of it):
mapped:
  P2WSH multisig 2-of-3               1   20.0%           416   28.7%          8963          8547   36.9%
  P2TR key-path                       2   40.0%           460   31.7%          7806          7346   31.8%
  P2WPKH                              1   20.0%           270   18.6%          3903          3633   15.7%
  P2TR script-path single-key         1   20.0%           304   21.0%          3914          3610   15.6%
unmapped:
  (none)
key exposure (mapped inputs, baseline input weight, migrated input weight, added weight, % of it, unmapped inputs, their baseline input weight):
  Exposed in output          3           764         11720         10956   47.4%         0             0
  Hashed until spend         2           686         12866         12180   52.6%         0             0
  No key                     0             0             0             0    0.0%         0             0
  Undetermined               0             0             0             0    0.0%         0             0
```

How to read it:

- **Migrated totals only include fully mapped transactions.** A transaction with any unmapped input still counts toward the baseline but never the migrated total, and `partially mapped baseline weight` tells you how much of the baseline that left out.
- **Breakdown** splits inputs by spend type (and by Unmapped reason), so you can see which spend types drive the growth. Mapped rows are sorted by `added input weight`, the block space migrating that spend type would add; its `% of it` is of the total Added weight, the same total as the key exposure table's. Unmapped rows have no Added weight and are sorted by baseline Input weight.
- **Key exposure** splits the same inputs by where their public key sat before the spend. `Exposed in output` (P2TR, P2PK) is open to a long-exposure quantum attack; `Hashed until spend` (P2WPKH, P2PKH, ...) only once the key is revealed. `added weight` is the block space migrating just that group would add.

`--json` prints the same report as one object (`scheme`, `fee_rate`, `counts`, `baseline`, `migrated`, `partially_mapped`, `fee`, `breakdown`, `key_exposure`, `errors`), using `migrate --json`'s names for spend types, thresholds and Unmapped reasons. `migrated` is `null` when no transaction is fully mapped, and `fee` is left out without `--fee-rate`:

```sh
pqweight aggregate --scheme ml-dsa-44 --json sample.txt | jq '.breakdown[] | select(.status == "mapped") | [.spend_type, .added_weight]'
```

[docs/coverage/2026-09-sample.md](docs/coverage/2026-09-sample.md) is a worked example over three mainnet blocks, with the block hashes to reproduce it.

### Move cost of Exposed coins (a UTXO snapshot)

`move-cost` reads a Bitcoin Core UTXO snapshot (the file `dumptxoutset` writes and `loadtxoutset` reads; see ADR 0003) one coin at a time, finds every **Exposed coin** (P2PK, bare multisig, P2TR: the key sits in the output itself) and reports how much weight it would take to move them into PQ outputs with today's signatures. Each row gives a floor (perfect consolidation) and a ceiling (one coin per transaction), in WU and in full blocks, once for all coins and once for coins of at least 546 sats. In `--json`, every `value` is in satoshis. The layouts are in [docs/migration-templates.md](docs/migration-templates.md), "Move layouts".

```sh
pqweight move-cost crates/pqweight/tests/fixtures/snapshot/regtest-utxo.dat
pqweight move-cost --json <snapshot> | jq '.total.above_dust'
```

[docs/move-cost/935k-snapshot.md](docs/move-cost/935k-snapshot.md) runs it on the mainnet snapshot at height 935,000: moving every Exposed coin takes 3,444 to 6,507 full blocks, with the steps to download, verify and reproduce it.

## What's covered

Mapped spend types: P2WPKH, P2SH-P2WPKH, P2PKH, P2TR key-path, P2TR script-path single-key leaves, P2WSH / P2SH-P2WSH / P2SH multisig (any m-of-n), P2WSH / P2SH-P2WSH contract scripts (hashlocks, timelocks, Lightning), pay-to-anchor, and coinbase.

Unmapped (reported, not guessed at): P2PK, bare multisig, other P2TR script-path leaves, P2TR key-path with an annex, other legacy and P2SH scripts, and anything unrecognized.

## Using the library

The CLI is a thin wrapper over the `pqweight` crate:

```rust
use pqweight::{ParameterSet, migrate, transaction_weight};

let bytes: Vec<u8> = /* raw transaction bytes */;

let baseline = transaction_weight(&bytes)?;
let migration = migrate(&bytes, ParameterSet::MlDsa44)?;

println!("baseline vsize: {}", baseline.vsize);
if let Some(total) = migration.migrated {
    println!("migrated vsize: {}", total.vsize);
}
```

`aggregate(lines, parameter_set, fee_rate)` does the batch version (`aggregate_blocks` for one raw block per line, `parse_block(bytes)` for one verified block), and `FeeRate::parse("1.5")` / `fee(vsize, rate)` handle fees without float rounding.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI runs the same three on every push and PR, then the **Second calculation**:

```sh
cargo build -p pqweight-cli
python scripts/second-calculation.py target/debug/pqweight --fixtures crates/pqweight/tests/fixtures
```

It rebuilds every mapped input's migrated bytes from [docs/migration-templates.md](docs/migration-templates.md) alone (not from the Rust) and checks pqweight's migrated weights on every Fixture at all three parameter sets. Agreement means the code matches the stated templates, not that the templates are right. Stdlib-only Python 3.

Weight tests run against **fixtures** in `crates/pqweight/tests/fixtures/`: signed transactions (mostly built on a throwaway regtest node, a few taken from mainnet) with the `weight` and `vsize` Bitcoin Core reported for them. Fixtures are committed, so CI never needs a node. To re-record them you need a local Bitcoin Core; see `scripts/record-fixtures.ps1`. The snapshot Fixture (`tests/fixtures/snapshot/`, a regtest UTXO snapshot with `gettxout` for every coin) is recorded by `scripts/record-snapshot-fixture.ps1`.

## License

MIT or Apache-2.0, at your option.
