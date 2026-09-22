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

**Migration template**:
A hypothetical post-quantum spend layout for one baseline spend type, stating exactly what its witness would contain. Bitcoin has no PQ opcode today, so templates are the model's stated assumptions.
_Avoid_: Scenario, PQ transaction type

**Unmapped**:
The result for a real spend that no Migration template covers. Reported explicitly so unmodeled transactions never silently skew totals.

**Long-exposure attack**:
Breaking a public key that is already visible on-chain before the coins are spent, such as a taproot output key, with unlimited time.

**Short-exposure attack**:
Breaking a public key revealed at spend time and forging a competing spend before the original confirms.

**Signature scheme**:
A post-quantum signing algorithm family, such as ML-DSA, SLH-DSA or Falcon.
_Avoid_: Algorithm, cipher

**Parameter set**:
A specific configuration of a Signature scheme with fixed signature and public key sizes, such as ML-DSA-44.
_Avoid_: Variant, level

**Baseline spend type**:
The kind of spend a real input performs today, such as P2WPKH or P2TR key-path. Each Migration template covers exactly one.
_Avoid_: Script type, address type

**Input result**:
The outcome for one input of a transaction: either its Migration template weight, or Unmapped. A transaction has a migrated total only when every input is mapped.
