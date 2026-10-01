# Migration templates

The exact migrated bytes of every **Migration template**: what each input's scriptSig and witness become, and when the transaction gains the segwit marker and flag. These are the stated assumptions of ADR 0002 written out byte by byte.

This doc is the spec both implementations are written from: `crates/pqweight/src/migration.rs` and the **Second calculation** (`scripts/second-calculation.py`, ticket 21). When they disagree, this doc decides which one is wrong. It was written from the tickets (01, 02, 05, 06 and the pq-migration spec), not from the Rust.

## Sizes

| Parameter set | `--scheme` | PQ signature | PQ public key |
|---|---|---|---|
| ML-DSA-44 | `ml-dsa-44` | 2,420 | 1,312 |
| Falcon-512 | `falcon-512` | 666 (fixed padded size) | 897 |
| SLH-DSA-128s | `slh-dsa-128s` | 7,856 | 32 |

Below, `SIG` is a PQ signature and `PK` a PQ public key: that many bytes, contents irrelevant. A PQ signature is `SIG` bytes whatever the signature it replaces was (64, 65 or a DER length). There is no separate sighash byte.

## Encoding rules

- **Witness**: a compact-size item count, then each item as a compact-size length and its bytes. Compact size is 1 byte below 253, 3 bytes (`fd` + 2) up to 65,535, 5 bytes (`fe` + 4) above.
- **Push inside a script** (the shortest push): 1 to 75 bytes is one opcode byte; up to 255 is `OP_PUSHDATA1` + 1 length byte; up to 65,535 is `OP_PUSHDATA2` + 2 length bytes. So a `PK` push is `20 <32>` at SLH-DSA-128s and `4d <2-byte length> <PK>` at ML-DSA-44 and Falcon-512.
- **A script that grows** (multisig, contract, single-key leaf) is rebuilt in full first; its length prefix, as a witness item, comes from its new length.
- **Only the listed bytes change.** Outpoint, sequence, outputs, version and locktime are unchanged.

## Marker and flag

The migrated transaction is serialized as segwit (with the `00 01` marker and flag, and one witness per input) exactly when at least one input's migrated witness has at least one item. Otherwise it is serialized without them. An input whose witness stays empty in a segwit transaction is written as `00` (zero items).

So a transaction with no witness today gains 2 bytes of marker and flag, plus one `00` byte for every input left without witness items, as soon as one input migrates into the witness. Weight = 3 × stripped size + total size, vsize = weight ÷ 4 rounded up.

## Templates

`m` is the multisig threshold's m, `n` its n.

### Coinbase

scriptSig and witness unchanged.

### Pay-to-anchor

scriptSig (empty) and witness (empty or absent) unchanged.

### P2WPKH

- scriptSig: unchanged (empty).
- Witness: `[SIG, PK]`.

### P2SH-P2WPKH

- scriptSig: unchanged (the one push of `0014<20 bytes>`).
- Witness: `[SIG, PK]`.

### P2PKH

- scriptSig: becomes empty.
- Witness: `[SIG, PK]`.

### P2TR key-path

- scriptSig: unchanged (empty).
- Witness: `[SIG, PK]`. The original single 64 or 65-byte item is replaced. PQ outputs commit to a hash of the key, so the key is revealed at spend time.

### P2WSH multisig

Today: witness `[<empty>, sig_1 … sig_m, script]`, script `OP_m <key_1> … <key_n> OP_n OP_CHECKMULTISIG`.

- scriptSig: unchanged (empty).
- Witness: `[<empty>, SIG × m, script']`, where `script'` is `OP_m`, then n pushes of `PK`, then `OP_n`, then `OP_CHECKMULTISIG`. `OP_m` and `OP_n` keep their original encoding (1 byte for 1 to 16, `01 xx` for 17 to 20).

### P2SH-P2WSH multisig

- scriptSig: unchanged (the one push of `0020<32 bytes>`).
- Witness: as P2WSH multisig.

### P2SH multisig

Today: no witness; scriptSig `OP_0 <sig_1> … <sig_m> <script push>`, with 33 or 65-byte keys in the script.

- scriptSig: becomes empty.
- Witness: as P2WSH multisig, built from the script in the last scriptSig push. Every key, 33 or 65 bytes, becomes `PK`.

### P2TR script-path single-key

Today: witness `[stack items…, leaf, control block]`, no annex.

- scriptSig: unchanged (empty).
- Stack items: the one item of 64 or 65 bytes becomes `SIG`. Every other item is unchanged.
- Leaf: the 32-byte push right before the one `OP_CHECKSIG` or `OP_CHECKSIGVERIFY` becomes a push of `PK`. Every other op and push is unchanged, including other 32-byte pushes.
- Control block: `33 + 32k` bytes becomes `1 + 32k` bytes (the leaf-version byte plus the unchanged k-level Merkle path; the 32-byte internal key is dropped).
- Witness: `[stack items', leaf', control block']`.

### P2WSH contract

Today: witness `[stack items…, script]`.

- scriptSig: unchanged (empty).
- Script: every direct push (opcode `0x01` to `0x4b`) that is key-shaped becomes a push of `PK`. Every other byte is unchanged, including keys pushed with `OP_PUSHDATA1`/`2`.
- Stack items (every witness item before the script):
  - key-shaped: becomes `PK` bytes;
  - otherwise a strict-DER signature: becomes `SIG` bytes;
  - otherwise unchanged (preimages, hashes, empty items, selectors).
- Witness: `[stack items', script']`.

**Key-shaped**: 33 bytes starting `02` or `03`, or 65 bytes starting `04`.

**Strict-DER signature**: the item is `30 L 02 Lr <r> 02 Ls <s> <sighash>`, where the item is 9 to 73 bytes long, `Lr` and `Ls` are each at least 1, `L = Lr + Ls + 4`, and the item is `L + 3` bytes: the lengths account for every byte, with the sighash byte last. This is BIP66's layout only: its rules against negative or zero-padded integers are not checked.

### P2SH-P2WSH contract

- scriptSig: unchanged (the one push of `0020<32 bytes>`).
- Witness: as P2WSH contract.

## Worked checks

Two figures from the tickets, re-derived from the rules above.

- **P2WSH multisig 2-of-3, ML-DSA-44** (Fixture `p2wsh-multisig`, empty scriptSig, so 41 non-witness bytes): `script'` = 1 + 3 × (3 + 1,312) + 1 + 1 = 3,948 bytes, a 3-byte prefix. Witness = 1 (count) + 1 (empty dummy, length 0) + 2 × (3 + 2,420) + (3 + 3,948) = 8,799. Template weight = 4 × 41 + 8,799 = 8,963.
- **P2TR script-path single-key, ML-DSA-44** (Fixture `p2tr-scriptpath`, leaf `20<key>ac`, control block 33 bytes): leaf' = 3 + 1,312 + 1 = 1,316. Witness = 1 + (3 + 2,420) + (3 + 1,316) + (1 + 1) = 3,745.

## Move layouts

The transactions **Move cost** counts (ticket 22): an unspent **Exposed coin** spent with today's signature, before any soft fork disables it, into a PQ output. Unlike the templates above, nothing here is post-quantum except the output; these are stated assumptions in the same sense as ADR 0002.

### Which coins

Read from the coin's scriptPubKey as the UTXO snapshot stores it:

- **P2PK compressed**: `21 <33-byte key> ac` (snapshot compressed-script codes 2 and 3).
- **P2PK uncompressed**: `41 04 <64 bytes> ac` with a valid key (codes 4 and 5). A P2PK script whose key is not on the curve is stored raw by Core and is not counted: nobody can spend it, so nothing can move it or steal it. Hybrid-encoded keys (prefix `06` or `07`) are also stored raw and not counted; they are rare enough not to matter, and leaving them out keeps the result a lower bound.
- **Bare multisig m-of-n**: exactly `OP_m <n keys> OP_n OP_CHECKMULTISIG`, 1 ≤ m ≤ n ≤ 20, every key a direct push of 33 or 65 bytes, as the P2SH multisig template reads its script. One row per threshold.
- **P2TR**: exactly `51 20 <32 bytes>`.

Every other coin is not an Exposed coin, including P2PKH, P2SH, P2WPKH and P2WSH whose key was revealed by address reuse (the snapshot can't show reuse, so Move cost is a lower bound).

### Spends

Every input has a 36-byte outpoint and a 4-byte sequence. A signature is a 72-byte DER signature including its sighash byte, or a 64-byte Schnorr signature (default sighash, no sighash byte).

| Exposed coin | scriptSig | Witness | Spend weight |
|---|---|---|---|
| P2PK (either) | `48 <72>`: 73 bytes | none | 4 × (36 + 1 + 73 + 4) = 456 |
| Bare multisig m-of-n | `00`, then m × `48 <72>`: 1 + 73m bytes | none | 4 × (36 + len(1 + 73m) + 1 + 73m + 4) |
| P2TR | empty | `[<64>]`: 1 + 1 + 64 = 66 bytes | 4 × (36 + 1 + 4) + 66 = 230 |

`len(x)` is the compact size of the scriptSig length: 1 byte up to 252, 3 bytes above. So bare multisig is 460 at m = 1, 752 at m = 2, 1,044 at m = 3, and 4 × (36 + 3 + 1,169 + 4) = 4,848 at m = 16.

P2TR is the key-path spend. Coins that can only be spent by script path (such as inscription commit outputs) cost more, so for them this is a lower bound.

### PQ output

A BIP-360 style output: 8-byte value, 1-byte script length, 34-byte scriptPubKey (a version opcode and a 32-byte push). 43 bytes, 172 WU.

### Floor and ceiling

- **Floor** (perfect consolidation): the sum of the coins' spend weights. Transaction overhead and outputs are shared by so many coins that they round to nothing.
- **Ceiling** (one coin per transaction, 1 input, 1 output): spend weight + 40 (version 4, input count 1, output count 1, locktime 4 bytes, at 4 WU each) + 172 (the PQ output), + 2 for a P2TR coin (the segwit marker and flag at 1 WU each).

So the ceiling is 668 per P2PK coin, 672 for bare multisig 1-of-n, 964 for 2-of-n, 1,256 for 3-of-n, and 444 per P2TR coin.

Blocks = weight ÷ 4,000,000, assuming blocks hold nothing but moves. Every result is also reported for coins of at least 546 sats only, leaving out inscription postage and data outputs nobody will pay to move.
