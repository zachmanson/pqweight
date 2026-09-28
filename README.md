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

There are three commands. Each takes a raw transaction as hex, either as an argument or on stdin.

```
pqweight weight [--json] [<hex>]
pqweight migrate --scheme <scheme> [--fee-rate <rate>] [--json] [<hex>]
pqweight migrate --scheme <scheme> [--fee-rate <rate>] --json-lines [<path>]
pqweight aggregate --scheme <scheme> [--fee-rate <rate>] [--json] [<path>]
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

The easiest way to get real input is to pull whole mainnet blocks from mempool.space:

```sh
python scripts/fetch-blocks.py <block hash> [<block hash>...] > sample.txt
pqweight aggregate --scheme ml-dsa-44 --fee-rate 5 sample.txt
```

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

`aggregate(lines, parameter_set, fee_rate)` does the batch version, and `FeeRate::parse("1.5")` / `fee(vsize, rate)` handle fees without float rounding.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

CI runs the same three on every push and PR.

Weight tests run against **fixtures** in `crates/pqweight/tests/fixtures/`: signed transactions (mostly built on a throwaway regtest node, a few taken from mainnet) with the `weight` and `vsize` Bitcoin Core reported for them. Fixtures are committed, so CI never needs a node. To re-record them you need a local Bitcoin Core; see `scripts/record-fixtures.ps1`.

## License

MIT or Apache-2.0, at your option.
