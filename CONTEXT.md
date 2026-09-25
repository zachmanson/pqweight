# pqweight

Computes Bitcoin transaction weight from raw transactions, validated against Bitcoin Core, and models how block space and fees change if signatures move to post-quantum schemes.

## Language

**Weight**:
The consensus size measure of a transaction: 3 × stripped size + total size, in weight units.
_Avoid_: Size, byte size

**Virtual size (vsize)**:
Weight divided by 4, rounded up. The unit fee rates are quoted against.
_Avoid_: vbytes (as a synonym for weight)

**Witness discount**:
The rule that witness bytes count 1 weight unit each while non-witness bytes count 4.

**Oracle**:
Bitcoin Core's reported `weight` and `vsize` for a transaction, the independent source of truth expected values are checked against.
_Avoid_: Reference implementation, ground truth

**Fixture**:
A raw transaction committed to the repo together with its Oracle values and the script that recorded them.
_Avoid_: Test data, sample

**Coverage sample**:
A set of mainnet blocks, identified by hash, run through `aggregate` to measure how much real traffic the Migration templates cover. Unlike a Fixture it has no Oracle values, and its raw transactions aren't committed.
_Avoid_: Test set, dataset

**Migration template**:
A hypothetical post-quantum spend layout for one baseline spend type, stating exactly what its witness would contain. Bitcoin has no PQ opcode today, so templates are the model's stated assumptions.
_Avoid_: Scenario, PQ transaction type

**Unmapped**:
The result for a real spend that no Migration template covers. Reported explicitly so unmodeled transactions never silently skew totals.

**Unmapped reason**:
The spend shape observed on an Unmapped input, such as P2TR script-path or non-multisig P2WSH, recognized from the spending side only. It names what was seen, never what a Migration template would cost.
_Avoid_: Unmapped type, failure reason

**Coinbase input**:
The single input of a block's first transaction, which spends no previous output and carries no signature. It has nothing to migrate, so it is mapped with its weight unchanged, never Unmapped.

**Long-exposure attack**:
Breaking a public key that is already visible on-chain before the coins are spent, such as a taproot output key, with unlimited time.

**Short-exposure attack**:
Breaking a public key revealed at spend time and forging a competing spend before the original confirms.

**Key exposure**:
Where an input's public key sat before this spend, as seen from the spending side: Exposed in output (the key is in the output itself, such as P2TR or P2PK, so open to a Long-exposure attack), Hashed until spend (only a hash was on-chain, so open to a Short-exposure attack unless the key was revealed earlier through address reuse, which the spending side can't show), No key (nothing to attack), or Undetermined. It names what was seen, never what an attacker could actually do.
_Avoid_: Long-exposed, short-exposed, vulnerable

**Signature scheme**:
A post-quantum signing algorithm family, such as ML-DSA, SLH-DSA or Falcon.
_Avoid_: Algorithm, cipher

**Parameter set**:
A specific configuration of a Signature scheme with fixed signature and public key sizes, such as ML-DSA-44.
_Avoid_: Variant, level

**Baseline spend type**:
The kind of spend a real input performs today, such as P2WPKH, P2TR key-path, P2TR script-path single-key or P2WSH contract. Each Migration template covers exactly one.
_Avoid_: Script type, address type

**Single-key leaf**:
A tapscript leaf (version `0xc0`) whose only signature check is one 32-byte key pushed right before `OP_CHECKSIG` or `OP_CHECKSIGVERIFY`, spent with exactly one 64 or 65-byte stack item. Every other op and data push, such as an inscription envelope, is carried unchanged by its Migration template.
_Avoid_: inscription leaf, pk leaf

**Contract script**:
A P2WSH or P2SH-P2WSH witnessScript other than standard multisig, such as a hashlock, timelock or Lightning script. Its Migration template swaps every key and signature for a PQ one, whether in the script or on the stack, and carries every other byte unchanged.
_Avoid_: HTLC, custom script

**Multisig threshold**:
The m-of-n shape of a multisig spend: n public keys in its script, m signatures in its witness. It is part of the Baseline spend type, so a 2-of-3 and a 3-of-5 P2WSH multisig spend migrate to different Migration templates.
_Avoid_: Quorum, k-of-n, "multisig" alone as a spend type name

**Input result**:
The outcome for one input of a transaction: either its Migration template weight, or Unmapped. A transaction has a migrated total only when every input is mapped.

**Input weight**:
The weight of one input's own bytes: outpoint, scriptSig and sequence at 4 weight units each, plus its witness at 1 each. Transaction overhead and outputs belong to no input, so a transaction's Input weights do not sum to its Weight.

**Added weight**:
An input's Input weight after migration minus its Input weight today. Summed over any set of inputs, it is the block space migrating just those inputs would add, since outputs and transaction overhead don't change (except the 2-byte segwit marker and flag a legacy-only transaction gains). Unmapped inputs have none.

**Fully mapped**:
A transaction whose Input result is Mapped for every input. Only fully mapped transactions contribute to a migrated total, individually or summed across a batch.

**Partially mapped**:
A transaction with at least one Unmapped input, including one where every input is Unmapped. Its baseline weight still counts; it never contributes to a migrated total.
