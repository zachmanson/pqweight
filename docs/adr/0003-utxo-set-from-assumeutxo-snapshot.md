# Read the UTXO set from an assumeutxo snapshot, not a synced node

Move cost (ticket 22) needs every unspent Exposed coin, and there is no mainnet node here. We download the assumeutxo snapshot at height 935,000 (about 9.4 GB, from public mirrors; there is no canonical source) and parse it with our own Rust code. The trust anchor is the snapshot's `hash_serialized_3`, which Bitcoin Core v31 hardcodes in `chainparams.cpp`: the file is checked once by `loadtxoutset` on a throwaway datadir, which needs only the headers chain and fails with "Bad snapshot content hash" on any mismatch.

Syncing a pruned node and running `dumptxoutset latest` would give a fresher set in the same file format, but costs a ~650 GB download and one to two days of initial sync to gain a few months, and the coins this question is about (early P2PK above all) barely move. An outside dataset or published counts would have been fastest, but would break ADR 0001: every number pqweight reports comes from bytes it parsed itself.

We do not recompute `hash_serialized_3` ourselves. That needs every script rebuilt, including secp256k1 point decompression for uncompressed P2PK keys (stored as x-coordinate only), and Move cost only needs each coin's script type and length. Recomputing it would make the check independent of Core; it can be added later if a reason appears.
