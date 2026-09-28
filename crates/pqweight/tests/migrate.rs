//! Behavior of the public migration function: raw transaction in, Input results out.
//!
//! No **Oracle** exists for post-quantum weight, so expected values are worked out
//! by hand from the **Migration template** and the FIPS 204 sizes, with the
//! arithmetic written next to each assertion.

mod common;

use common::{decode_hex, der_signature, fixtures_dir};
use pqweight::{
    BaselineSpendType, InputResult, MultisigThreshold, ParameterSet, UnmappedReason, migrate,
    transaction_weight,
};
use proptest::prelude::*;

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(format!("{name}.hex"));
    let hex = std::fs::read_to_string(&path).expect("fixture exists");
    decode_hex(hex.trim())
}

/// A segwit transaction with one input per `(script_sig, witness)` pair given,
/// and one output. Contents are filler; only sizes matter. scriptSigs and item
/// counts must be below 253; witness items may be longer.
fn segwit_tx_with_inputs(inputs: &[(&[u8], &[&[u8]])]) -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&2u32.to_le_bytes()); // version
    tx.extend_from_slice(&[0x00, 0x01]); // segwit marker, flag
    tx.push(u8::try_from(inputs.len()).expect("few inputs"));
    for (script_sig, _) in inputs {
        tx.extend_from_slice(&[0u8; 36]); // prevout
        tx.push(u8::try_from(script_sig.len()).expect("short scriptSig"));
        tx.extend_from_slice(script_sig);
        tx.extend_from_slice(&[0xff; 4]); // sequence
    }
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    for (_, witness) in inputs {
        tx.push(u8::try_from(witness.len()).expect("few items"));
        for item in *witness {
            push_compact_size(&mut tx, item.len());
            tx.extend_from_slice(item);
        }
    }
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

/// Appends `len` as a Bitcoin compact-size integer (1 or 3 bytes here).
fn push_compact_size(tx: &mut Vec<u8>, len: usize) {
    match u8::try_from(len) {
        Ok(short) if short < 0xfd => tx.push(short),
        _ => {
            tx.push(0xfd);
            tx.extend_from_slice(&u16::try_from(len).expect("item under 64 KiB").to_le_bytes());
        }
    }
}

/// `<m> <n keys> <n> OP_CHECKMULTISIG`, each key a direct push. `m` and `n`
/// are raw script bytes so tests can build malformed scripts.
fn multisig_script(m: &[u8], keys: &[&[u8]], n: &[u8]) -> Vec<u8> {
    let mut script = m.to_vec();
    for key in keys {
        script.push(u8::try_from(key.len()).expect("direct-push length"));
        script.extend_from_slice(key);
    }
    script.extend_from_slice(n);
    script.push(0xae);
    script
}

/// A script number as a minimal push: `OP_1` to `OP_16`, or a 1-byte push of
/// the value above 16.
fn op_n(n: u8) -> Vec<u8> {
    match n {
        1..=16 => vec![0x50 + n],
        _ => vec![0x01, n],
    }
}

/// A P2SH multisig scriptSig: `OP_0`, each signature as a direct push, then
/// `script` as a direct push or, above 75 bytes, `OP_PUSHDATA1`.
fn p2sh_multisig_script_sig(signatures: &[&[u8]], script: &[u8]) -> Vec<u8> {
    let mut script_sig = vec![0x00];
    for signature in signatures {
        script_sig.push(u8::try_from(signature.len()).expect("direct-push length"));
        script_sig.extend_from_slice(signature);
    }
    let script_len = u8::try_from(script.len()).expect("script under 256 bytes");
    if script_len > 0x4b {
        script_sig.push(0x4c); // OP_PUSHDATA1
    }
    script_sig.push(script_len);
    script_sig.extend_from_slice(script);
    script_sig
}

/// A P2WSH m-of-n multisig spend with 33-byte keys and 72-byte signatures.
fn p2wsh_multisig_tx(m: u8, n: u8) -> Vec<u8> {
    multisig_witness_tx(&[], m, n)
}

/// The P2SH-P2WSH redeem script push: `0x22` then `0020<32-byte script hash>`.
fn p2sh_p2wsh_redeem_script_push() -> Vec<u8> {
    let mut push = vec![0x22, 0x00, 0x20];
    push.extend_from_slice(&[0u8; 32]);
    push
}

/// A spend with the given scriptSig and an m-of-n multisig witness with
/// 33-byte keys and 72-byte signatures.
fn multisig_witness_tx(script_sig: &[u8], m: u8, n: u8) -> Vec<u8> {
    let key = [2u8; 33];
    let signature = der_signature(72);
    let script = multisig_script(&op_n(m), &vec![&key[..]; usize::from(n)], &op_n(n));
    let mut witness: Vec<&[u8]> = vec![&[]];
    witness.extend(vec![&signature[..]; usize::from(m)]);
    witness.push(&script);
    segwit_tx(script_sig, &witness)
}

/// One-input, one-output segwit transaction with the given scriptSig and witness
/// items. Contents are filler; only sizes matter. All lengths must be below 253.
fn segwit_tx(script_sig: &[u8], witness: &[&[u8]]) -> Vec<u8> {
    segwit_tx_with_inputs(&[(script_sig, witness)])
}

/// One-input, one-output legacy (non-segwit) transaction whose scriptSig is
/// `pushes`, each encoded as a direct push (a length byte, 1 to 75, followed by
/// that many bytes). Contents are filler; only sizes matter.
fn legacy_tx(pushes: &[&[u8]]) -> Vec<u8> {
    let mut script_sig = Vec::new();
    for push in pushes {
        script_sig.push(u8::try_from(push.len()).expect("direct-push length"));
        script_sig.extend_from_slice(push);
    }

    legacy_tx_with_script_sig(&script_sig)
}

/// One-input, one-output legacy (non-segwit) transaction with the given raw
/// scriptSig. Contents are filler; only sizes matter.
fn legacy_tx_with_script_sig(script_sig: &[u8]) -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 36]); // prevout
    push_compact_size(&mut tx, script_sig.len());
    tx.extend_from_slice(script_sig);
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

/// The parts of an Input result that classification and template tests check:
/// Mapped spend type and template weight, or just "Unmapped".
#[derive(Debug, PartialEq, Eq)]
enum Summary {
    Mapped(BaselineSpendType, u64),
    Unmapped,
}

fn summaries(inputs: &[InputResult]) -> Vec<Summary> {
    inputs
        .iter()
        .map(|input| match *input {
            InputResult::Mapped {
                spend_type,
                template_weight,
                ..
            } => Summary::Mapped(spend_type, template_weight),
            InputResult::Unmapped { .. } => Summary::Unmapped,
        })
        .collect()
}

fn is_mapped_as(tx: &[u8], expected: BaselineSpendType) -> bool {
    let migration = migrate(tx, ParameterSet::MlDsa44).expect("valid transaction");
    matches!(
        migration.inputs[0],
        InputResult::Mapped { spend_type, .. } if spend_type == expected
    )
}

/// Whether the first input is Unmapped or mapped as a P2WSH contract: what a
/// near miss of standard multisig may become, since most still have keys and a
/// signature check (ticket 06, decision 6).
fn is_contract_or_unmapped(tx: &[u8]) -> bool {
    let migration = migrate(tx, ParameterSet::MlDsa44).expect("valid transaction");
    matches!(
        migration.inputs[0],
        InputResult::Unmapped { .. }
            | InputResult::Mapped {
                spend_type: BaselineSpendType::P2wshContract,
                ..
            }
    )
}

/// The Unmapped reason of the first input, or `None` if it is mapped.
fn unmapped_reason(tx: &[u8]) -> Option<UnmappedReason> {
    let migration = migrate(tx, ParameterSet::MlDsa44).expect("valid transaction");
    match migration.inputs[0] {
        InputResult::Unmapped { reason, .. } => Some(reason),
        InputResult::Mapped { .. } => None,
    }
}

#[test]
fn p2tr_single_key_leaf_is_migrated_with_the_pq_key_in_the_leaf_and_no_internal_key() {
    let tx = load_fixture("p2tr-scriptpath");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Fixture witness: [64-byte signature, 34-byte leaf `20 <32-byte key> ac`,
    // 33-byte control block `c1 <internal key>`] = 1 + 65 + 35 + 34 = 135 bytes.
    // ML-DSA-44 witness [PQ signature, leaf, control block]:
    //   item count 1
    //   + signature: 3-byte length prefix + 2420 = 2423
    //   + leaf: OP_PUSHDATA2 3 + 1312 key + OP_CHECKSIG 1 = 1316, plus its
    //     3-byte length prefix = 1319
    //   + control block: leaf-version byte only (no internal key, no Merkle
    //     path) = 1, plus its 1-byte length prefix = 2
    //   = 1 + 2423 + 1319 + 2 = 3745 WU.
    // Non-witness part: empty scriptSig, 32 + 4 + 1 + 4 = 41 bytes = 164 WU.
    // Input template weight: 164 + 3745 = 3909 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2trScriptPathSingleKey,
            3909
        )]
    );

    // Stripped size and marker are unchanged, so migrated weight
    // = 465 (Oracle) - 135 + 3745 = 4075 WU, vsize ceil(4075 / 4) = 1019.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4075);
    assert_eq!(total.vsize, 1019);
}

#[test]
fn p2tr_single_key_leaf_pushes_the_pq_key_with_the_shortest_push() {
    let tx = load_fixture("p2tr-scriptpath");

    // Same fixture as above: 164 WU non-witness, control block migrates to
    // 1 byte + 1-byte prefix = 2, item count 1.
    // SLH-DSA-128s: 32-byte key, so the leaf keeps its direct push and stays
    // 34 bytes (+1 prefix = 35). Signature 3 + 7856 = 7859.
    //   1 + 7859 + 35 + 2 = 7897; 164 + 7897 = 8061 WU.
    // Falcon-512: 897-byte key needs OP_PUSHDATA2: leaf 3 + 897 + 1 = 901
    // (+3 prefix = 904). Signature 3 + 666 = 669.
    //   1 + 669 + 904 + 2 = 1576; 164 + 1576 = 1740 WU.
    let cases = [
        (ParameterSet::SlhDsa128s, 8061),
        (ParameterSet::Falcon512, 1740),
    ];
    for (parameter_set, expected) in cases {
        let migration = migrate(&tx, parameter_set).expect("valid transaction");
        assert_eq!(
            summaries(&migration.inputs),
            vec![Summary::Mapped(
                BaselineSpendType::P2trScriptPathSingleKey,
                expected
            )],
            "{parameter_set:?}"
        );
    }
}

#[test]
fn p2tr_inscription_envelope_keeps_its_data_bytes_unchanged() {
    let tx = load_fixture("p2tr-scriptpath-envelope");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Mainnet witness: [64-byte signature, 83-byte leaf `20 <key> ac 00 63
    // 03 "ord" 51 18 "text/plain;charset=utf-8" 00 0a "968210.bitmap" 68`,
    // 33-byte control block] = 1 + 65 + 84 + 34 = 184 bytes.
    // ML-DSA-44: the 50 envelope bytes after the key push stay as they are.
    //   item count 1 + signature 2423
    //   + leaf: 83 - 33 (direct key push) + 1315 (OP_PUSHDATA2 key push) = 1365,
    //     plus its 3-byte prefix = 1368
    //   + control block 1 + 1 = 2
    //   = 3794; 164 non-witness + 3794 = 3958 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2trScriptPathSingleKey,
            3958
        )]
    );
    // 562 (Oracle) - 184 + 3794 = 4172 WU, vsize ceil(4172 / 4) = 1043.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4172);
    assert_eq!(total.vsize, 1043);
}

#[test]
fn p2tr_dropped_tag_leaf_is_migrated_alongside_a_key_path_input() {
    let tx = load_fixture("p2tr-scriptpath-dropped-tag");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Input 0, mainnet witness [64-byte signature, 39-byte leaf
    // `03 <3-byte tag> 75 20 <key> ac`, 33-byte control block]
    // = 1 + 65 + 40 + 34 = 140 bytes.
    //   item count 1 + signature 2423
    //   + leaf: 39 - 33 + 1315 = 1321, plus its 3-byte prefix = 1324
    //   + control block 2
    //   = 3750; 164 + 3750 = 3914 WU.
    // Input 1 is a key-path spend (witness 1 + 65 = 66 bytes), migrated to
    // [PQ signature, PQ key]: 1 + 2423 + 1315 = 3739; 164 + 3739 = 3903 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![
            Summary::Mapped(BaselineSpendType::P2trScriptPathSingleKey, 3914),
            Summary::Mapped(BaselineSpendType::P2trKeyPath, 3903),
        ]
    );
    // 872 (Oracle) - 140 - 66 + 3750 + 3739 = 8155 WU, vsize ceil(8155 / 4) = 2039.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 8155);
    assert_eq!(total.vsize, 2039);
}

/// A leaf `<32-byte key> OP_CHECKSIG` (34 bytes).
fn single_key_leaf() -> Vec<u8> {
    let mut leaf = vec![0x20];
    leaf.extend_from_slice(&[7u8; 32]);
    leaf.push(0xac);
    leaf
}

/// A control block with tapscript leaf version `0xc0` and `path_hashes` Merkle
/// path hashes after the internal key.
fn control_block(path_hashes: usize) -> Vec<u8> {
    let mut block = vec![0xc0];
    block.extend(vec![9u8; 32 * (1 + path_hashes)]);
    block
}

#[test]
fn p2tr_single_key_leaf_keeps_each_merkle_path_hash_at_32_bytes() {
    let signature = [1u8; 64];
    let leaf = single_key_leaf();
    let control_block = control_block(1);
    let tx = segwit_tx(&[], &[&signature, &leaf, &control_block]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // As the fixture at ML-DSA-44 (signature 2423, leaf 1319, item count 1),
    // but the 65-byte control block keeps its one path hash: 1 + 32 = 33
    // (+1 prefix = 34). 1 + 2423 + 1319 + 34 = 3777; 164 + 3777 = 3941 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2trScriptPathSingleKey,
            3941
        )]
    );
}

#[test]
fn p2tr_script_path_near_misses_of_a_single_key_leaf_are_unmapped() {
    let key = [7u8; 32];
    let signature = [1u8; 64];
    let control_block = control_block(0);
    let leaf = |parts: &[&[u8]]| parts.concat();
    let push_key: &[u8] = &[&[0x20][..], &key].concat();
    let checksig: &[u8] = &[0xac];
    let checksigverify: &[u8] = &[0xad];

    let two_key_chain = leaf(&[push_key, checksigverify, push_key, checksig]);
    let key_from_stack = leaf(&[checksig]);
    let compressed_key = leaf(&[&[0x21, 0x02], &[7u8; 32], checksig]);
    let pushdata1_key = leaf(&[&[0x4c, 0x20], &key, checksig]);
    // multi_a 1-of-2: <key> CHECKSIG <key> CHECKSIGADD OP_1 OP_NUMEQUAL.
    let multi_a = leaf(&[push_key, checksig, push_key, &[0xba, 0x51, 0x9c]]);
    let no_check = leaf(&[&[0x51]]);
    let truncated_push = leaf(&[push_key, checksig, &[0x20, 0x00]]);
    // OP_SUCCESS80 anywhere in a tapscript leaf makes it succeed without a signature.
    let op_success = leaf(&[push_key, checksig, &[0x50]]);
    let single_key = single_key_leaf();
    let mut wrong_leaf_version = control_block.clone();
    wrong_leaf_version[0] = 0xc2;

    // (name, witness, P2TR script-path is the Unmapped reason)
    let cases: [(&str, Vec<&[u8]>, bool); 12] = [
        (
            "two-key CHECKSIGVERIFY chain",
            vec![&[], &signature, &two_key_chain, &control_block],
            true,
        ),
        (
            "key taken from the stack",
            vec![&signature, &key, &key_from_stack, &control_block],
            true,
        ),
        (
            "33-byte key push",
            vec![&signature, &compressed_key, &control_block],
            true,
        ),
        (
            "32-byte key pushed with OP_PUSHDATA1",
            vec![&signature, &pushdata1_key, &control_block],
            true,
        ),
        (
            "multi_a leaf with one signer",
            vec![&[], &signature, &multi_a, &control_block],
            true,
        ),
        (
            "no signature check",
            vec![&signature, &no_check, &control_block],
            true,
        ),
        (
            "truncated push",
            vec![&signature, &truncated_push, &control_block],
            true,
        ),
        (
            "OP_SUCCESS opcode",
            vec![&signature, &op_success, &control_block],
            true,
        ),
        (
            "two 64-byte stack items",
            vec![&signature, &signature, &single_key, &control_block],
            true,
        ),
        (
            "empty signature, no 64 or 65-byte item",
            vec![&[], &single_key, &control_block],
            true,
        ),
        (
            "63-byte signature",
            vec![&signature[..63], &single_key, &control_block],
            true,
        ),
        (
            "leaf version 0xc2",
            vec![&signature, &single_key, &wrong_leaf_version],
            false,
        ),
    ];
    for (name, witness, is_script_path_reason) in cases {
        let reason = unmapped_reason(&segwit_tx(&[], &witness));
        assert!(reason.is_some(), "{name}: must be Unmapped");
        assert_eq!(
            reason == Some(UnmappedReason::P2trScriptPath),
            is_script_path_reason,
            "{name}: reason {reason:?}"
        );
    }
}

#[test]
fn p2tr_single_key_leaf_with_an_annex_is_unmapped_as_p2tr_script_path() {
    let signature = [1u8; 64];
    let leaf = single_key_leaf();
    let control_block = control_block(0);
    let annex = [0x50u8, 0x01];
    let tx = segwit_tx(&[], &[&signature, &leaf, &control_block, &annex]);

    assert_eq!(unmapped_reason(&tx), Some(UnmappedReason::P2trScriptPath));
}

#[test]
fn p2tr_script_path_needs_a_control_block_and_a_script_before_it() {
    let script = [0x51u8; 34];
    let annex = [0x50u8, 0x01];
    let control_block = |len: usize, first: u8| {
        let mut block = vec![0u8; len];
        block[0] = first;
        block
    };
    let internal_key_only = control_block(33, 0xc0);
    let one_path_hash_odd_parity = control_block(65, 0xc1);
    let wrong_length = control_block(34, 0xc0);
    let wrong_leaf_version = control_block(33, 0xc2);
    // A key-path signature with a sighash byte is 65 bytes (= 33 + 32) and can
    // start with 0xc0; with an annex it must not pass for a script-path spend.
    let signature_like_control_block = control_block(65, 0xc0);

    // (name, witness, is P2TR script-path)
    let cases: [(&str, Vec<&[u8]>, bool); 6] = [
        (
            "33-byte control block",
            vec![&script, &internal_key_only],
            true,
        ),
        (
            "65-byte control block, odd parity",
            vec![&script, &one_path_hash_odd_parity],
            true,
        ),
        (
            "script path with annex",
            vec![&script, &internal_key_only, &annex],
            true,
        ),
        (
            "control block of 34 bytes",
            vec![&script, &wrong_length],
            false,
        ),
        (
            "leaf version 0xc2",
            vec![&script, &wrong_leaf_version],
            false,
        ),
        (
            "key path with annex",
            vec![&signature_like_control_block, &annex],
            false,
        ),
    ];
    for (name, witness, expected) in cases {
        let reason = unmapped_reason(&segwit_tx(&[], &witness));
        assert_eq!(
            reason == Some(UnmappedReason::P2trScriptPath),
            expected,
            "{name}"
        );
    }
}

#[test]
fn p2tr_key_path_spend_with_an_annex_is_unmapped_as_key_path_with_annex() {
    assert_eq!(
        unmapped_reason(&load_fixture("p2tr-keypath-annex")),
        Some(UnmappedReason::P2trKeyPathAnnex)
    );
}

#[test]
fn p2wsh_single_key_script_is_migrated_as_a_contract_with_the_pq_key_in_the_script() {
    let tx = load_fixture("p2wsh-pk");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Fixture witness: [71-byte DER signature, 35-byte script `21 <33-byte key> ac`]
    // = 1 + 72 + 36 = 109 bytes.
    // ML-DSA-44 witness [PQ signature, script]:
    //   item count 1
    //   + signature: 3-byte length prefix + 2420 = 2423
    //   + script: 35 - 34 (direct key push) + 1315 (OP_PUSHDATA2 3 + 1312 key)
    //     = 1316, plus its 3-byte length prefix = 1319
    //   = 1 + 2423 + 1319 = 3743 bytes.
    // Non-witness part: empty scriptSig, 41 bytes = 164 WU.
    // Input template weight: 164 + 3743 = 3907 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 3907)]
    );
    // 439 (Oracle) - 109 + 3743 = 4073 WU, vsize ceil(4073 / 4) = 1019.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4073);
    assert_eq!(total.vsize, 1019);
}

/// The two-key hashlock/CLTV contract that dominates P2WSH in the September 2026
/// sample (130 bytes, two 33-byte keys pushed directly):
/// `<key A> CHECKSIG NOTIF DUP HASH160 <20> EQUALVERIFY CHECKSIGVERIFY <locktime>
/// CLTV ELSE <key B> CHECKSIGVERIFY SIZE 32 EQUALVERIFY HASH160 <20> EQUAL ENDIF`.
/// Migrated at ML-DSA-44: 130 - 2 × 34 + 2 × 1315 = 2692 bytes, plus its 3-byte
/// length prefix = 2695.
const HASHLOCK_CONTRACT_MIGRATED_ITEM: u64 = 2695;

#[test]
fn p2wsh_contract_claim_swaps_both_keys_and_signatures_and_keeps_the_preimage() {
    let tx = load_fixture("p2wsh-contract-claim");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Mainnet witness: [32-byte preimage, 72 and 71-byte signatures, 130-byte
    // script] = 1 + 33 + 73 + 72 + 131 = 310 bytes.
    // ML-DSA-44: item count 1 + preimage 33 (unchanged) + 2 × 2423 signatures
    //   + script 2695 = 7575; 164 non-witness + 7575 = 7739 WU.
    assert_eq!(1 + 33 + 2 * 2423 + HASHLOCK_CONTRACT_MIGRATED_ITEM, 7575);
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 7739)]
    );
    // 640 (Oracle) - 310 + 7575 = 7905 WU, vsize ceil(7905 / 4) = 1977.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 7905);
    assert_eq!(total.vsize, 1977);
}

#[test]
fn p2wsh_contract_refund_swaps_the_stack_key_and_keeps_its_hash_and_the_empty_item() {
    let tx = load_fixture("p2wsh-contract-refund");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Mainnet witness: [71-byte signature, 33-byte key, empty, 130-byte script]
    // = 1 + 72 + 34 + 1 + 131 = 239 bytes. The script checks the key against a
    // 20-byte HASH160, which stays 20 bytes.
    // ML-DSA-44: item count 1 + signature 2423 + key 3 + 1312 = 1315 + empty 1
    //   + script 2695 = 6435; 164 + 6435 = 6599 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 6599)]
    );
    // 569 (Oracle) - 239 + 6435 = 6765 WU, vsize ceil(6765 / 4) = 1692.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 6765);
    assert_eq!(total.vsize, 1692);
}

#[test]
fn p2wsh_contract_preimage_starting_with_the_der_tag_is_not_a_signature() {
    let tx = load_fixture("p2wsh-contract-der-like-preimage");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Input 2: [32-byte preimage starting 0x30, 71 and 71-byte signatures,
    // 130-byte script]. The preimage stays 33 bytes with its prefix, so the
    // input migrates exactly like the claim above: 164 + 7575 = 7739 WU.
    assert_eq!(
        summaries(&migration.inputs)[2],
        Summary::Mapped(BaselineSpendType::P2wshContract, 7739)
    );
}

#[test]
fn p2wsh_contract_with_an_empty_signature_is_mapped_because_its_script_key_grows() {
    let tx = load_fixture("p2wsh-contract-anchor");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Input 0 is a key-path spend (witness 1 + 65 = 66 bytes), migrated to
    // 1 + 2423 + 1315 = 3739; 164 + 3739 = 3903 WU.
    // Input 1 is a Lightning anchor swept with an empty signature: [empty,
    // 40-byte script `21 <key> ac 73 64 60 b2 68`] = 1 + 1 + 41 = 43 bytes.
    // ML-DSA-44: item count 1 + empty 1 + script 40 - 34 + 1315 = 1321 (+3
    // prefix = 1324) = 1326; 164 + 1326 = 1490 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![
            Summary::Mapped(BaselineSpendType::P2trKeyPath, 3903),
            Summary::Mapped(BaselineSpendType::P2wshContract, 1490),
        ]
    );
    // 651 (Oracle) - 66 - 43 + 3739 + 1326 = 5607 WU, vsize ceil(5607 / 4) = 1402.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 5607);
    assert_eq!(total.vsize, 1402);
}

#[test]
fn p2wsh_contract_with_checkmultisig_inside_swaps_every_key_in_the_script() {
    let tx = load_fixture("p2wsh-contract-checkmultisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Mainnet witness: [72-byte signature, empty, 71-byte signature, `01`,
    // 189-byte script `OP_12 CSV VERIFY IF 1 <K1> <K2> 2 CHECKMULTISIGVERIFY <K3>
    // CHECKSIG ELSE <3-byte> CSV VERIFY 2 <K4> <K5> 2 CHECKMULTISIG ENDIF`]
    // = 1 + 73 + 1 + 72 + 2 + 190 = 339 bytes. Five keys, all swapped, though
    // this spend's branch uses only three.
    // ML-DSA-44: script 189 - 5 × 34 + 5 × 1315 = 6594 (+3 prefix = 6597).
    //   item count 1 + 2 × 2423 + empty 1 + `01` 2 + 6597 = 11447;
    //   164 + 11447 = 11611 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 11611)]
    );
    // 669 (Oracle) - 339 + 11447 = 11777 WU, vsize ceil(11777 / 4) = 2945.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 11777);
    assert_eq!(total.vsize, 2945);
}

/// `len` bytes starting with the DER sequence tag `0x30`, then zeros: the tag
/// and a signature's length, but `r` is 0 bytes long and `s` is missing, so
/// the structure isn't DER.
fn der_lookalike(len: usize) -> Vec<u8> {
    let mut item = vec![0u8; len];
    item[0] = 0x30;
    item
}

#[test]
fn p2wsh_contract_swaps_every_key_shaped_push_and_keeps_der_lookalikes() {
    let compressed: &[u8] = &[&[0x21, 0x02][..], &[5u8; 32]].concat();
    let uncompressed: &[u8] = &[&[0x41, 0x04][..], &[6u8; 64]].concat();
    // `<33-byte key> SWAP <65-byte key> CHECKSIG`: the first key isn't next to
    // the signature check, and the second is uncompressed. 34 + 1 + 66 + 1 = 102 bytes.
    let script: &[u8] = &[compressed, &[0x7c], uncompressed, &[0xac]].concat();
    // Starts with the DER tag and is 72 bytes, but its structure is not DER.
    let der_lookalike = der_lookalike(72);
    let tx = segwit_tx(&[], &[&der_signature(72), &der_lookalike, script]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44: script 102 - 34 - 66 + 2 × 1315 = 2632 (+3 prefix = 2635).
    //   item count 1 + signature 2423 + lookalike 73 (unchanged) + 2635 = 5132;
    //   164 + 5132 = 5296 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 5296)]
    );
}

#[test]
fn p2wsh_contract_carries_items_with_a_broken_der_structure_as_data() {
    let compressed: &[u8] = &[&[0x21, 0x02][..], &[5u8; 32]].concat();
    let script: &[u8] = &[compressed, &[0xac]].concat();
    let valid = der_signature(72);
    let with_byte = |index: usize, value: u8| {
        let mut item = valid.clone();
        item[index] = value;
        item
    };
    let empty_r = [
        &[0x30, 0x24, 0x02, 0x00, 0x02, 0x20][..],
        &[1u8; 32],
        &[0x01],
    ]
    .concat();
    let empty_s = [
        &[0x30, 0x25, 0x02, 0x21][..],
        &[1u8; 33],
        &[0x02, 0x00, 0x01],
    ]
    .concat();
    // Consistent lengths, but a 34-byte r makes it 74 bytes, over DER's 73.
    let over_73_bytes = [
        &[0x30, 0x47, 0x02, 0x22][..],
        &[1u8; 34],
        &[0x02, 0x21],
        &[1u8; 33],
        &[0x01],
    ]
    .concat();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("wrong sequence length", with_byte(1, 0x44)),
        ("r not tagged 0x02", with_byte(2, 0x03)),
        ("s not tagged 0x02", with_byte(37, 0x03)),
        ("no sighash byte", valid[..valid.len() - 1].to_vec()),
        ("empty r", empty_r),
        ("empty s", empty_s),
        ("74 bytes", over_73_bytes),
    ];

    // `21 <key> ac` migrates to 35 - 34 + 1315 = 1316 (+3 prefix = 1319), so
    // with the item carried unchanged: 1 + (1 + len) + 1319; plus 164 WU
    // non-witness. A valid signature would be 1 + 2423 + 1319 + 164 = 3907.
    let control = segwit_tx(&[], &[&valid, script]);
    assert_eq!(
        summaries(
            &migrate(&control, ParameterSet::MlDsa44)
                .expect("valid")
                .inputs
        ),
        vec![Summary::Mapped(BaselineSpendType::P2wshContract, 3907)]
    );
    for (name, item) in cases {
        let tx = segwit_tx(&[], &[&item, script]);
        let expected = 164 + 1 + (1 + item.len() as u64) + 1319;
        assert_eq!(
            summaries(&migrate(&tx, ParameterSet::MlDsa44).expect("valid").inputs),
            vec![Summary::Mapped(BaselineSpendType::P2wshContract, expected)],
            "{name}"
        );
    }
}

#[test]
fn p2wsh_contract_pushes_each_pq_key_with_the_shortest_push() {
    let tx = load_fixture("p2wsh-contract-claim");

    // Claim fixture: [32-byte preimage, two signatures, 130-byte script with
    // two 34-byte key pushes]; 164 WU non-witness, item count 1, preimage 33.
    // SLH-DSA-128s: 32-byte key, a direct push of 33 bytes, so the script
    // shrinks to 130 - 68 + 66 = 128 (+1 prefix = 129). Signatures 2 × (3 +
    // 7856) = 15718. 1 + 33 + 15718 + 129 = 15881; 164 + 15881 = 16045 WU.
    // Falcon-512: 897-byte key needs OP_PUSHDATA2, 900 bytes per push: script
    // 130 - 68 + 1800 = 1862 (+3 prefix = 1865). Signatures 2 × (3 + 666) =
    // 1338. 1 + 33 + 1338 + 1865 = 3237; 164 + 3237 = 3401 WU.
    let cases = [
        (ParameterSet::SlhDsa128s, 16045),
        (ParameterSet::Falcon512, 3401),
    ];
    for (parameter_set, expected) in cases {
        let migration = migrate(&tx, parameter_set).expect("valid transaction");
        assert_eq!(
            summaries(&migration.inputs),
            vec![Summary::Mapped(BaselineSpendType::P2wshContract, expected)],
            "{parameter_set:?}"
        );
    }
}

#[test]
fn p2wsh_contract_near_misses_stay_unmapped_with_their_reason() {
    let key = [2u8; 33];
    let signature = der_signature(72);
    let push_key: &[u8] = &[&[0x21][..], &key].concat();
    // `21 <key> ac` with 29 OP_0s after it: 65 bytes starting 0xc0, so it is
    // also a tapscript control block with one Merkle path hash.
    let control_block_like_contract: &[u8] = &[&[0xc0][..], push_key, &[0xac], &[0; 29]].concat();
    // `50 21 <key> ac`: starts with the annex tag 0x50 (OP_RESERVED as an opcode).
    let annex_like_contract: &[u8] = &[&[0x50][..], push_key, &[0xac]].concat();
    let key_then_drop_1: &[u8] = &[push_key, &[0x75, 0x51]].concat();

    let cases: Vec<(&str, Vec<&[u8]>, UnmappedReason)> = vec![
        (
            "truncated push",
            vec![&signature, &[0x21, 0x02, 0x03, 0xac]],
            UnmappedReason::P2wshNonMultisig,
        ),
        (
            "no signature check: <key> DROP 1",
            vec![&signature, key_then_drop_1],
            UnmappedReason::P2wshNonMultisig,
        ),
        (
            "no key: bare CHECKSIG with a 20-byte stack item",
            vec![&signature, &[9u8; 20], &[0xac]],
            UnmappedReason::P2wshNonMultisig,
        ),
        (
            "P2TR script-path whose control block parses as a contract",
            vec![&[1u8; 64], &[0xac], control_block_like_contract],
            UnmappedReason::P2trScriptPath,
        ),
        (
            "P2TR key path whose annex parses as a contract",
            vec![&[1u8; 64], annex_like_contract],
            UnmappedReason::P2trKeyPathAnnex,
        ),
    ];
    for (name, witness, expected) in cases {
        assert_eq!(
            unmapped_reason(&segwit_tx(&[], &witness)),
            Some(expected),
            "{name}"
        );
    }

    let wrapped_truncated = segwit_tx(
        &p2sh_p2wsh_redeem_script_push(),
        &[&signature, &[0x21, 0x02, 0x03, 0xac]],
    );
    assert_eq!(
        unmapped_reason(&wrapped_truncated),
        Some(UnmappedReason::P2shSegwitNonMultisig)
    );
}

#[test]
fn p2sh_p2wsh_single_key_script_is_migrated_as_a_contract_keeping_the_redeem_script_scriptsig() {
    let tx = load_fixture("p2sh-p2wsh-pk");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Witness as the P2WSH single-key fixture: 109 bytes, migrated to 3743.
    // Non-witness part: scriptSig unchanged, 0x22 push plus the 34-byte
    // 0020<32-byte hash> redeem script = 35 bytes, with a 1-byte length prefix:
    // 36 + 1 + 35 + 4 = 76 bytes = 304 WU.
    // Input template weight: 304 + 3743 = 4047 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2shP2wshContract, 4047)]
    );
    // 579 (Oracle) - 109 + 3743 = 4213 WU, vsize ceil(4213 / 4) = 1054.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4213);
    assert_eq!(total.vsize, 1054);
}

#[test]
fn p2sh_p2wsh_contract_is_migrated_like_its_p2wsh_form() {
    let tx = load_fixture("p2sh-p2wsh-contract");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Mainnet witness: [empty, 71-byte signature, 78-byte script `<K1>
    // CHECKSIGVERIFY <K2> CHECKSIG IFDUP NOTIF <3-byte> CSV ENDIF`]
    // = 1 + 1 + 72 + 79 = 153 bytes.
    // ML-DSA-44: script 78 - 2 × 34 + 2 × 1315 = 2640 (+3 prefix = 2643).
    //   item count 1 + empty 1 + signature 2423 + 2643 = 5068;
    //   304 non-witness (redeem script scriptSig) + 5068 = 5372 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2shP2wshContract, 5372)]
    );
    // 751 (Oracle) - 153 + 5068 = 5666 WU, vsize ceil(5666 / 4) = 1417.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 5666);
    assert_eq!(total.vsize, 1417);
}

#[test]
fn p2sh_p2wpkh_shape_with_an_uncompressed_key_is_unmapped_as_p2sh_wrapped_segwit() {
    let mut redeem_script_push = vec![0x16, 0x00, 0x14];
    redeem_script_push.extend_from_slice(&[0u8; 20]);
    let tx = segwit_tx(&redeem_script_push, &[&der_signature(72), &[4u8; 65]]);

    assert_eq!(
        unmapped_reason(&tx),
        Some(UnmappedReason::P2shSegwitNonMultisig)
    );
}

#[test]
fn p2sh_single_key_script_spend_is_unmapped_as_p2sh_non_multisig() {
    assert_eq!(
        unmapped_reason(&load_fixture("p2sh-pk")),
        Some(UnmappedReason::P2shNonMultisig)
    );
}

#[test]
fn legacy_script_sigs_ending_in_a_signature_or_key_are_unmapped_as_legacy_other() {
    // Filler signatures: the DER tag and a length, but no consistent DER
    // structure, so they are not P2PK or bare multisig.
    let signature = der_lookalike(72);
    let mut uncompressed_key = [0u8; 65];
    uncompressed_key[0] = 0x04;

    // (name, scriptSig)
    let cases: [(&str, Vec<u8>); 4] = [
        (
            "P2PK shape with a non-DER signature",
            [&[72][..], &signature].concat(),
        ),
        ("bare multisig shape with non-DER signatures", {
            let mut script_sig = vec![0x00];
            for _ in 0..2 {
                script_sig.push(72);
                script_sig.extend_from_slice(&signature);
            }
            script_sig
        }),
        ("OP_0 then a public key", {
            let mut script_sig = vec![0x00, 65];
            script_sig.extend_from_slice(&uncompressed_key);
            script_sig
        }),
        ("P2PKH with a 74-byte signature", {
            let mut script_sig = vec![74, 0x30];
            script_sig.extend_from_slice(&[0u8; 73]);
            script_sig.push(65);
            script_sig.extend_from_slice(&uncompressed_key);
            script_sig
        }),
    ];
    for (name, script_sig) in cases {
        let reason = unmapped_reason(&legacy_tx_with_script_sig(&script_sig));
        assert_eq!(reason, Some(UnmappedReason::LegacyOther), "{name}");
    }
}

#[test]
fn legacy_script_sig_of_one_strict_der_signature_is_unmapped_as_p2pk() {
    assert_eq!(
        unmapped_reason(&legacy_tx(&[&der_signature(72)])),
        Some(UnmappedReason::P2pk)
    );
}

#[test]
fn legacy_script_sig_of_op_0_then_strict_der_signatures_is_unmapped_as_bare_multisig() {
    let signature = der_signature(72);
    // (name, pushes after OP_0)
    let cases: [(&str, Vec<&[u8]>); 2] = [
        ("one signature (1-of-n)", vec![&signature]),
        ("two signatures", vec![&signature, &signature]),
    ];
    for (name, signatures) in cases {
        let pushes: Vec<&[u8]> = [&[][..]].into_iter().chain(signatures).collect();
        assert_eq!(
            unmapped_reason(&legacy_tx(&pushes)),
            Some(UnmappedReason::BareMultisig),
            "{name}"
        );
    }
}

#[test]
fn mainnet_p2pk_and_bare_multisig_spends_are_unmapped_with_their_reasons() {
    let p2pk = migrate(&load_fixture("p2pk"), ParameterSet::MlDsa44).expect("valid transaction");
    let bare =
        migrate(&load_fixture("bare-multisig"), ParameterSet::MlDsa44).expect("valid transaction");

    let reasons = |migration: &pqweight::Migration| -> Vec<Option<UnmappedReason>> {
        migration
            .inputs
            .iter()
            .map(|input| match *input {
                InputResult::Unmapped { reason, .. } => Some(reason),
                InputResult::Mapped { .. } => None,
            })
            .collect()
    };
    assert_eq!(reasons(&p2pk), vec![Some(UnmappedReason::P2pk)]);
    // Input 0 is an ordinary P2PKH spend.
    assert_eq!(
        reasons(&bare),
        vec![None, Some(UnmappedReason::BareMultisig)]
    );
}

#[test]
fn shapes_matching_no_reason_are_unmapped_as_unknown() {
    // (name, transaction)
    let cases = [
        (
            "non-P2SH scriptSig alongside a witness",
            segwit_tx(&[0x01, 0x51], &[&der_signature(72), &[2u8; 33]]),
        ),
        ("one short witness item", segwit_tx(&[], &[&[0u8; 10]])),
    ];
    for (name, tx) in cases {
        assert_eq!(
            unmapped_reason(&tx),
            Some(UnmappedReason::Unknown),
            "{name}"
        );
    }
}

#[test]
fn input_weight_counts_non_witness_bytes_at_4_wu_and_witness_bytes_at_1_wu() {
    // p2pkh.hex: scriptSig length byte 0x6a (106). Outpoint 36 + length prefix 1 +
    // scriptSig 106 + sequence 4 = 147 non-witness bytes x 4 = 588. No witness.
    let legacy = migrate(&load_fixture("p2pkh"), ParameterSet::MlDsa44).expect("valid");
    // p2wpkh.hex: empty scriptSig, so 36 + 1 + 0 + 4 = 41 bytes x 4 = 164. Witness:
    // item count 1 + (1 + 70-byte signature) + (1 + 33-byte pubkey) = 106 x 1.
    let segwit = migrate(&load_fixture("p2wpkh"), ParameterSet::MlDsa44).expect("valid");

    assert_eq!(legacy.inputs[0].baseline_weight(), 588);
    assert_eq!(segwit.inputs[0].baseline_weight(), 164 + 106);
}

#[test]
fn p2wpkh_accepts_der_signatures_of_9_to_73_bytes_and_rejects_others() {
    let pubkey = [0u8; 33];
    // (signature length, expected to be recognized as P2WPKH). 68 and 69 bytes
    // are real: DER drops leading zero bytes of r and s, and the September 2026
    // coverage sample had 26 such P2WPKH spends.
    let cases = [
        (8, false),
        (9, true),
        (68, true),
        (69, true),
        (70, true),
        (73, true),
        (74, false),
    ];

    for (sig_len, mapped) in cases {
        let signature = der_signature(sig_len);
        let tx = segwit_tx(&[], &[&signature, &pubkey]);

        assert_eq!(
            is_mapped_as(&tx, BaselineSpendType::P2wpkh),
            mapped,
            "signature of {sig_len} bytes"
        );
    }
}

/// A 72-byte item with the DER tag and a signature's length whose `r` length
/// byte says 34 instead of 33, so `s` doesn't start where the tags say.
fn der_with_inconsistent_r_length() -> Vec<u8> {
    let mut item = der_signature(72);
    item[3] = 0x22;
    item
}

#[test]
fn p2wpkh_signature_must_have_consistent_der_lengths() {
    let tx = segwit_tx(&[], &[&der_with_inconsistent_r_length(), &[2u8; 33]]);

    assert!(!is_mapped_as(&tx, BaselineSpendType::P2wpkh));
}

#[test]
fn p2wpkh_signature_must_start_with_the_der_sequence_tag() {
    let not_der = [0u8; 72];
    let tx = segwit_tx(&[], &[&not_der, &[0u8; 33]]);

    assert!(!is_mapped_as(&tx, BaselineSpendType::P2wpkh));
}

#[test]
fn p2wpkh_requires_a_33_byte_public_key() {
    let signature = der_signature(72);

    for pubkey_len in [32, 34, 65] {
        let pubkey = vec![0u8; pubkey_len];
        let tx = segwit_tx(&[], &[&signature, &pubkey]);

        assert!(
            !is_mapped_as(&tx, BaselineSpendType::P2wpkh),
            "public key of {pubkey_len} bytes"
        );
    }
}

#[test]
fn p2wpkh_shape_with_an_extra_witness_item_or_a_script_sig_is_unmapped() {
    let signature = der_signature(72);
    let pubkey = [0u8; 33];

    let extra_item = segwit_tx(&[], &[&signature, &pubkey, &[1u8]]);
    let with_script_sig = segwit_tx(&[0x51], &[&signature, &pubkey]);

    for tx in [extra_item, with_script_sig] {
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(summaries(&migration.inputs), vec![Summary::Unmapped]);
        assert_eq!(migration.migrated, None);
    }
}

#[test]
fn p2tr_key_path_accepts_a_64_or_65_byte_schnorr_signature_and_rejects_others() {
    // (signature length, expected to be recognized as P2TR key-path)
    let cases = [(63, false), (64, true), (65, true), (66, false)];

    for (sig_len, mapped) in cases {
        let signature = vec![0u8; sig_len];
        let tx = segwit_tx(&[], &[&signature]);

        assert_eq!(
            is_mapped_as(&tx, BaselineSpendType::P2trKeyPath),
            mapped,
            "signature of {sig_len} bytes"
        );
    }
}

#[test]
fn p2tr_key_path_with_an_annex_is_unmapped() {
    let signature = [0u8; 64];
    // An annex is any witness item, present as the last one when there are at
    // least two, whose first byte is 0x50.
    let annex = [0x50u8, 0x00, 0x00];
    let tx = segwit_tx(&[], &[&signature, &annex]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert_eq!(summaries(&migration.inputs), vec![Summary::Unmapped]);
    assert_eq!(migration.migrated, None);
}

#[test]
fn p2sh_p2wpkh_requires_the_0014_redeem_script_push_and_a_p2wpkh_witness() {
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let mut redeem_script_push = vec![0x16, 0x00, 0x14];
    redeem_script_push.extend_from_slice(&[0u8; 20]);

    let matching = segwit_tx(&redeem_script_push, &[&signature, &pubkey]);
    assert!(is_mapped_as(&matching, BaselineSpendType::P2shP2wpkh));

    // Near-misses: wrong-length redeem script, and a redeem script that isn't
    // the 0x0014 witness-program push.
    let wrong_push_len = vec![0x15, 0x00, 0x14, 0u8, 0u8];
    let wrong_program = {
        let mut v = vec![0x16, 0x00, 0x15];
        v.extend_from_slice(&[0u8; 20]);
        v
    };
    for script_sig in [wrong_push_len, wrong_program] {
        let tx = segwit_tx(&script_sig, &[&signature, &pubkey]);
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(summaries(&migration.inputs), vec![Summary::Unmapped]);
    }
}

#[test]
fn p2sh_p2wpkh_accepts_a_short_der_signature() {
    let mut redeem_script_push = vec![0x16, 0x00, 0x14];
    redeem_script_push.extend_from_slice(&[0u8; 20]);
    let tx = segwit_tx(&redeem_script_push, &[&der_signature(69), &[0u8; 33]]);

    assert!(is_mapped_as(&tx, BaselineSpendType::P2shP2wpkh));
}

#[test]
fn p2sh_p2wpkh_signature_must_have_consistent_der_lengths() {
    let mut redeem_script_push = vec![0x16, 0x00, 0x14];
    redeem_script_push.extend_from_slice(&[0u8; 20]);
    let tx = segwit_tx(
        &redeem_script_push,
        &[&der_with_inconsistent_r_length(), &[2u8; 33]],
    );

    assert!(!is_mapped_as(&tx, BaselineSpendType::P2shP2wpkh));
}

#[test]
fn p2sh_p2wpkh_input_is_migrated_keeping_the_redeem_script_scriptsig() {
    let tx = load_fixture("p2sh-p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 witness: 3739 WU, as in the P2WPKH case.
    // Non-witness part of the input: scriptSig is unchanged, a push opcode (0x16,
    // meaning 22 bytes follow) plus the 22-byte 0014<20-byte hash> redeem script,
    // 23 bytes in total, with its own 1-byte compact-size length prefix.
    // 32 + 4 + 1 (length prefix) + 23 (scriptSig) + 4 (sequence) = 64 bytes = 256 WU.
    // Input template weight: 256 + 3739 = 3995 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2shP2wpkh, 3995,)]
    );
}

#[test]
fn p2pkh_accepts_a_der_signature_and_a_33_or_65_byte_pubkey() {
    for pubkey_len in [33, 65] {
        let signature = der_signature(72);
        let pubkey = vec![0u8; pubkey_len];
        let tx = legacy_tx(&[&signature, &pubkey]);

        assert!(
            is_mapped_as(&tx, BaselineSpendType::P2pkh),
            "pubkey of {pubkey_len} bytes"
        );
    }
}

#[test]
fn p2pkh_accepts_a_short_der_signature_and_rejects_a_non_der_one() {
    let pubkey = [0u8; 33];
    let short = legacy_tx(&[&der_signature(69), &pubkey]);
    let not_der = legacy_tx(&[&[0u8; 72], &pubkey]);

    assert!(is_mapped_as(&short, BaselineSpendType::P2pkh));
    assert!(!is_mapped_as(&not_der, BaselineSpendType::P2pkh));
}

#[test]
fn p2pkh_signature_must_have_consistent_der_lengths() {
    let tx = legacy_tx(&[&der_with_inconsistent_r_length(), &[2u8; 33]]);

    assert!(!is_mapped_as(&tx, BaselineSpendType::P2pkh));
}

#[test]
fn p2pkh_near_misses_are_unmapped() {
    let signature = der_signature(72);
    let pubkey = [0u8; 33];

    // Wrong pubkey length, one push short of the pair, and an extra third push.
    let wrong_pubkey_len = legacy_tx(&[&signature, &[0u8; 34]]);
    let missing_pubkey = legacy_tx(&[&signature]);
    let extra_push = legacy_tx(&[&signature, &pubkey, &[1u8]]);

    for tx in [wrong_pubkey_len, missing_pubkey, extra_push] {
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(summaries(&migration.inputs), vec![Summary::Unmapped]);
        assert_eq!(migration.migrated, None);
    }
}

#[test]
fn p2pkh_input_is_migrated_to_an_ml_dsa_44_witness() {
    let tx = load_fixture("p2pkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 witness: 3739 WU, as in the P2WPKH case.
    // Non-witness part of the input: scriptSig becomes empty, like the other
    // witness-only templates: 32 + 4 + 1 (empty length) + 4 = 41 bytes = 164 WU.
    // Input template weight: 164 + 3739 = 3903 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2pkh, 3903,)]
    );

    // The transaction had no witness before migration, so it gains the 2-byte
    // segwit marker and flag. Rest of the transaction (legacy, one input, one
    // output): version 4 + input count 1 + output count 1 + output (8 + 1 + 22,
    // matching the P2PKH fixture's scriptPubKey) + locktime 4 = 41 bytes = 164 WU.
    // Total: 164 + 2 + 3903 = 4069 WU, and vsize = ceil(4069 / 4) = 1018.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4069);
    assert_eq!(total.vsize, 1018);
}

#[test]
fn p2wsh_multisig_input_is_migrated_with_every_public_key_in_the_script() {
    let tx = load_fixture("p2wsh-multisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 2-of-3 witness [dummy, sig, sig, script]:
    //   item count 1 + dummy 1 (empty item, just its 0x00 length)
    //   + 2 signatures x (3-byte length prefix + 2420) = 4846
    //   + script: OP_2 1 + 3 keys x (OP_PUSHDATA2 3 + 1312) + OP_3 1
    //     + OP_CHECKMULTISIG 1 = 3948, plus its 3-byte length prefix = 3951
    //   = 1 + 1 + 4846 + 3951 = 8799 bytes = 8799 WU.
    // Non-witness part: empty scriptSig, 32 + 4 + 1 + 4 = 41 bytes = 164 WU.
    // Input template weight: 164 + 8799 = 8963 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2wshMultisig(MultisigThreshold { m: 2, n: 3 }),
            8963,
        )]
    );

    // Baseline witness from the fixture: count 1 + dummy 1 + two 71-byte
    // signatures with 1-byte prefixes (72 each) + 105-byte script with a 1-byte
    // prefix (106) = 252 bytes. Stripped size (82) and marker are unchanged, so
    // migrated weight = 582 (Oracle) - 252 + 8799 = 9129 WU, vsize ceil(9129 / 4) = 2283.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 9129);
    assert_eq!(total.vsize, 2283);
}

#[test]
fn p2wsh_multisig_with_slh_dsa_pushes_its_small_keys_directly() {
    let tx = load_fixture("p2wsh-multisig");

    let migration = migrate(&tx, ParameterSet::SlhDsa128s).expect("valid transaction");

    // SLH-DSA-128s 2-of-3 witness: signatures 7856 bytes, keys 32 bytes.
    //   item count 1 + dummy 1
    //   + 2 signatures x (3-byte length prefix + 7856) = 15718
    //   + script: OP_2 1 + 3 keys x (direct push 1 + 32) + OP_3 1
    //     + OP_CHECKMULTISIG 1 = 102, plus its 1-byte length prefix = 103
    //   = 1 + 1 + 15718 + 103 = 15823 WU.
    // Input template weight: 164 (non-witness) + 15823 = 15987 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2wshMultisig(MultisigThreshold { m: 2, n: 3 }),
            15987,
        )]
    );
}

#[test]
fn p2wsh_multisig_is_recognised_for_thresholds_from_1_of_1_to_20_of_20() {
    for (m, n) in [
        (1, 1),
        (1, 2),
        (3, 5),
        (15, 15),
        (16, 16),
        (1, 17),
        (17, 20),
        (20, 20),
    ] {
        let tx = p2wsh_multisig_tx(m, n);

        assert!(
            is_mapped_as(
                &tx,
                BaselineSpendType::P2wshMultisig(MultisigThreshold { m, n })
            ),
            "{m}-of-{n}"
        );
    }
}

#[test]
fn p2wsh_multisig_3_of_5_template_weight() {
    let tx = p2wsh_multisig_tx(3, 5);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 3-of-5 witness:
    //   item count 1 + dummy 1 + 3 x (3 + 2420) = 7269
    //   + script: 1 + 5 x (3 + 1312) + 1 + 1 = 6578, plus 3-byte prefix = 6581
    //   = 1 + 1 + 7269 + 6581 = 13852 WU.
    // Input template weight: 164 + 13852 = 14016 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2wshMultisig(MultisigThreshold { m: 3, n: 5 }),
            14016,
        )]
    );
}

#[test]
fn p2wsh_multisig_above_16_keys_keeps_its_2_byte_number_push() {
    let tx = p2wsh_multisig_tx(1, 20);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 1-of-20 witness. 20 is above OP_16, so the script pushes it as
    // 0x01 0x14 (2 bytes) and the literal swap keeps that encoding.
    //   item count 1 + dummy 1 + 1 x (3 + 2420) = 2423
    //   + script: OP_1 1 + 20 x (3 + 1312) + 2 + 1 = 26304, plus 3-byte prefix = 26307
    //   = 1 + 1 + 2423 + 26307 = 28732 WU.
    // Input template weight: 164 + 28732 = 28896 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2wshMultisig(MultisigThreshold { m: 1, n: 20 }),
            28896,
        )]
    );
}

#[test]
fn p2wsh_multisig_numbers_must_be_minimally_encoded_and_at_most_20() {
    // Each near miss is a P2WSH contract or Unmapped, never standard multisig.
    let key = [2u8; 33];
    let signature = der_signature(72);
    let sigs_and_script = |m: &[u8], n_keys: usize, n: &[u8], n_sigs: usize| {
        let script = multisig_script(m, &vec![&key[..]; n_keys], n);
        let mut witness: Vec<Vec<u8>> = vec![vec![]];
        witness.extend(vec![signature.clone(); n_sigs]);
        witness.push(script);
        witness
    };

    let non_minimal_m = sigs_and_script(&[0x01, 0x02], 3, &op_n(3), 2);
    let twenty_one_keys = sigs_and_script(&op_n(1), 21, &[0x01, 21], 1);
    let zero_of_one = sigs_and_script(&[0x00], 1, &op_n(1), 0);

    for (name, witness) in [
        ("m pushed as 0x01 0x02", non_minimal_m),
        ("21 keys", twenty_one_keys),
        ("0-of-1", zero_of_one),
    ] {
        let items: Vec<&[u8]> = witness.iter().map(Vec::as_slice).collect();
        let tx = segwit_tx(&[], &items);
        assert!(is_contract_or_unmapped(&tx), "{name}");
    }
}

#[test]
fn p2wsh_multisig_accepts_a_short_der_signature() {
    let key = [2u8; 33];
    let script = multisig_script(&op_n(2), &[&key, &key, &key], &op_n(3));
    let tx = segwit_tx(&[], &[&[], &der_signature(72), &der_signature(69), &script]);

    assert!(is_mapped_as(
        &tx,
        BaselineSpendType::P2wshMultisig(MultisigThreshold { m: 2, n: 3 })
    ));
}

#[test]
fn multisig_signatures_must_have_consistent_der_lengths() {
    let key = [2u8; 33];
    let script = multisig_script(&op_n(2), &[&key, &key, &key], &op_n(3));
    let signature = der_signature(72);
    let inconsistent = der_with_inconsistent_r_length();

    let native_segwit = segwit_tx(&[], &[&[], &signature, &inconsistent, &script]);
    let legacy_p2sh = legacy_tx_with_script_sig(&p2sh_multisig_script_sig(
        &[&signature, &inconsistent],
        &script,
    ));

    let threshold = MultisigThreshold { m: 2, n: 3 };
    assert!(!is_mapped_as(
        &native_segwit,
        BaselineSpendType::P2wshMultisig(threshold)
    ));
    assert!(!is_mapped_as(
        &legacy_p2sh,
        BaselineSpendType::P2shMultisig(threshold)
    ));
}

#[test]
fn p2wsh_multisig_near_misses_are_contracts_or_unmapped() {
    // Each near miss is a P2WSH contract or Unmapped, never standard multisig.
    let key = [2u8; 33];
    let keys: &[&[u8]] = &[&key, &key, &key];
    let signature = der_signature(72);
    let script_2_of_3 = multisig_script(&op_n(2), keys, &op_n(3));

    let too_few_signatures = segwit_tx(&[], &[&[], &signature, &script_2_of_3]);
    let too_many_signatures = segwit_tx(
        &[],
        &[&[], &signature, &signature, &signature, &script_2_of_3],
    );
    let non_empty_dummy = segwit_tx(&[], &[&[0u8], &signature, &signature, &script_2_of_3]);
    let empty_signature = segwit_tx(&[], &[&[], &signature, &[], &script_2_of_3]);
    let short_signature = segwit_tx(&[], &[&[], &signature, &der_signature(8), &script_2_of_3]);
    let extra_opcode = {
        let mut script = script_2_of_3.clone();
        script.push(0x75); // OP_DROP after OP_CHECKMULTISIG
        segwit_tx(&[], &[&[], &signature, &signature, &script])
    };
    let m_greater_than_n = {
        let script = multisig_script(&op_n(4), keys, &op_n(3));
        segwit_tx(
            &[],
            &[&[], &signature, &signature, &signature, &signature, &script],
        )
    };
    let n_does_not_match_key_count = {
        let script = multisig_script(&op_n(2), keys, &op_n(4));
        segwit_tx(&[], &[&[], &signature, &signature, &script])
    };
    let uncompressed_key = {
        let big = [4u8; 65];
        let script = multisig_script(&op_n(2), &[&key, &key, &big], &op_n(3));
        segwit_tx(&[], &[&[], &signature, &signature, &script])
    };
    let not_checkmultisig = {
        // OP_2 <keys> OP_3 OP_CHECKMULTISIGVERIFY (0xaf): a different opcode.
        let mut script = script_2_of_3.clone();
        *script.last_mut().expect("non-empty") = 0xaf;
        segwit_tx(&[], &[&[], &signature, &signature, &script])
    };
    let with_script_sig = segwit_tx(&[0x51], &[&[], &signature, &signature, &script_2_of_3]);
    let timelock_script = {
        // <1> OP_CHECKSEQUENCEVERIFY OP_DROP <key> OP_CHECKSIG: a single-key
        // timelocked script, not multisig.
        let mut script = vec![0x51, 0xb2, 0x75, 0x21];
        script.extend_from_slice(&key);
        script.push(0xac);
        segwit_tx(&[], &[&signature, &script])
    };

    for (name, tx) in [
        ("too few signatures", too_few_signatures),
        ("too many signatures", too_many_signatures),
        ("non-empty dummy", non_empty_dummy),
        ("0-byte signature", empty_signature),
        ("8-byte signature", short_signature),
        ("extra opcode", extra_opcode),
        ("m > n", m_greater_than_n),
        ("n does not match key count", n_does_not_match_key_count),
        ("uncompressed key", uncompressed_key),
        ("OP_CHECKMULTISIGVERIFY", not_checkmultisig),
        ("non-empty scriptSig", with_script_sig),
        ("timelock script", timelock_script),
    ] {
        assert!(is_contract_or_unmapped(&tx), "{name}");
    }
}

#[test]
fn p2sh_p2wsh_multisig_is_migrated_keeping_the_redeem_script_scriptsig() {
    let tx = load_fixture("p2sh-p2wsh-multisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Witness: 8799 WU, the same 2-of-3 ML-DSA-44 witness as the P2WSH case.
    // Non-witness part: scriptSig is unchanged, a 0x22 push opcode plus the
    // 34-byte 0020<32-byte hash> redeem script = 35 bytes, with a 1-byte length
    // prefix: 32 + 4 + 1 + 35 + 4 = 76 bytes = 304 WU.
    // Input template weight: 304 + 8799 = 9103 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2shP2wshMultisig(MultisigThreshold { m: 2, n: 3 }),
            9103,
        )]
    );

    // Oracle: weight 722, size 371, so stripped size (722 - 371) / 3 = 117 and
    // the baseline witness is 371 - 117 - 2 (marker, flag) = 252 bytes.
    // Migrated: stripped 117 unchanged, total 117 + 2 + 8799 = 8918,
    // weight 3 x 117 + 8918 = 9269 WU, vsize ceil(9269 / 4) = 2318.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 9269);
    assert_eq!(total.vsize, 2318);
}

#[test]
fn p2sh_p2wsh_multisig_requires_the_0020_redeem_script_push() {
    // A 0014 (P2WPKH) program, and a 0020 program one byte short.
    let wrong_program = {
        let mut push = vec![0x16, 0x00, 0x14];
        push.extend_from_slice(&[0u8; 20]);
        push
    };
    let short_hash = {
        let mut push = vec![0x21, 0x00, 0x20];
        push.extend_from_slice(&[0u8; 31]);
        push
    };

    for script_sig in [wrong_program, short_hash] {
        let tx = multisig_witness_tx(&script_sig, 2, 3);
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(summaries(&migration.inputs), vec![Summary::Unmapped]);
    }
}

#[test]
fn p2sh_multisig_moves_signatures_and_script_into_the_witness() {
    let tx = load_fixture("p2sh-multisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Witness: 8799 WU, the same 2-of-3 ML-DSA-44 witness as the P2WSH case.
    // Non-witness part: scriptSig becomes empty, 41 bytes = 164 WU.
    // Input template weight: 164 + 8799 = 8963 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(
            BaselineSpendType::P2shMultisig(MultisigThreshold { m: 2, n: 3 }),
            8963,
        )]
    );

    // Oracle: size 334, all stripped (no witness). The fixture's scriptSig is
    // 0xfc = 252 bytes (OP_0, two 71-byte signatures with 1-byte pushes, and the
    // 105-byte script behind OP_PUSHDATA1 0x69), after a 1-byte length prefix
    // that stays as 0x00. Migrated stripped size: 334 - 252 = 82. The
    // transaction gains the 2-byte marker and flag: total 82 + 2 + 8799 = 8883,
    // weight 3 x 82 + 8883 = 9129 WU, vsize ceil(9129 / 4) = 2283.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 9129);
    assert_eq!(total.vsize, 2283);
}

#[test]
fn p2sh_multisig_accepts_uncompressed_and_mixed_key_sizes() {
    let compressed = [2u8; 33];
    let uncompressed = [4u8; 65];
    let signature = der_signature(72);
    let script = multisig_script(&op_n(1), &[&compressed, &uncompressed], &op_n(2));
    let tx = legacy_tx_with_script_sig(&p2sh_multisig_script_sig(&[&signature], &script));

    assert!(is_mapped_as(
        &tx,
        BaselineSpendType::P2shMultisig(MultisigThreshold { m: 1, n: 2 })
    ));
}

#[test]
fn p2sh_multisig_near_misses_are_unmapped() {
    let key = [2u8; 33];
    let signature = der_signature(72);
    let script = multisig_script(&op_n(2), &[&key, &key, &key], &op_n(3));
    let valid = p2sh_multisig_script_sig(&[&signature, &signature], &script);

    // Dummy pushed as one byte instead of OP_0.
    let non_empty_dummy = {
        let mut script_sig = vec![0x01, 0x00];
        script_sig.extend_from_slice(&valid[1..]);
        script_sig
    };
    // A non-push opcode (OP_DUP) before the signatures.
    let non_push_opcode = {
        let mut script_sig = vec![0x76];
        script_sig.extend_from_slice(&valid);
        script_sig
    };
    // One signature short of m.
    let too_few_signatures = p2sh_multisig_script_sig(&[&signature], &script);

    for (name, script_sig) in [
        ("non-empty dummy", non_empty_dummy),
        ("non-push opcode", non_push_opcode),
        ("too few signatures", too_few_signatures),
    ] {
        let tx = legacy_tx_with_script_sig(&script_sig);
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(
            summaries(&migration.inputs),
            vec![Summary::Unmapped],
            "{name}"
        );
    }

    // A P2SH-shaped scriptSig alongside a non-empty witness is not a legacy
    // P2SH spend. Items are kept under 253 bytes for the segwit builder.
    let short_script = multisig_script(&op_n(1), &[&key], &op_n(1));
    let short_script_sig = p2sh_multisig_script_sig(&[&signature], &short_script);
    let with_witness = segwit_tx(&short_script_sig, &[&signature]);
    let migration = migrate(&with_witness, ParameterSet::MlDsa44).expect("valid transaction");
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Unmapped],
        "with witness"
    );
}

fn mentions(assumptions: &[&str], needles: &[&str]) -> bool {
    assumptions
        .iter()
        .any(|a| needles.iter().all(|needle| a.contains(needle)))
}

#[test]
fn multisig_inputs_state_the_script_limit_and_checkmultisig_layout_assumptions() {
    let tx = load_fixture("p2wsh-multisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    let assumptions = &migration.assumptions;
    assert!(mentions(assumptions, &["10,000-byte script", "soft fork"]));
    assert!(mentions(assumptions, &["OP_CHECKMULTISIG", "dummy"]));
    // P2WSH has no scriptSig to move, so the P2SH assumption doesn't apply.
    assert!(!mentions(assumptions, &["P2SH"]));
}

#[test]
fn p2sh_multisig_inputs_state_that_the_script_moves_into_the_witness() {
    let key = [2u8; 33];
    let signature = der_signature(72);
    let script = multisig_script(&op_n(1), &[&key], &op_n(1));
    let tx = legacy_tx_with_script_sig(&p2sh_multisig_script_sig(&[&signature], &script));

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(mentions(&migration.assumptions, &["P2SH", "witness"]));
}

#[test]
fn p2tr_single_key_leaf_inputs_state_that_outputs_commit_to_the_merkle_root_without_an_internal_key()
 {
    let tx = load_fixture("p2tr-scriptpath");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    let assumptions = &migration.assumptions;
    assert!(mentions(
        assumptions,
        &["Merkle root", "no internal key", "BIP-360"]
    ));
    assert!(mentions(
        assumptions,
        &["control block", "leaf-version byte"]
    ));
    assert!(!mentions(assumptions, &["OP_CHECKMULTISIG"]));
}

#[test]
fn p2wsh_contract_inputs_state_the_script_limit_and_how_keys_and_signatures_are_recognized() {
    let tx = load_fixture("p2wsh-contract-claim");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    let assumptions = &migration.assumptions;
    assert!(mentions(assumptions, &["10,000-byte script", "soft fork"]));
    assert!(mentions(
        assumptions,
        &["P2WSH contract", "key-shaped push", "strict-DER"]
    ));
    // No OP_CHECKMULTISIG in this contract's script.
    assert!(!mentions(assumptions, &["OP_CHECKMULTISIG"]));
}

#[test]
fn p2wsh_contract_with_checkmultisig_inside_also_states_the_checkmultisig_layout() {
    let tx = load_fixture("p2wsh-contract-checkmultisig");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(mentions(
        &migration.assumptions,
        &["OP_CHECKMULTISIG", "dummy"]
    ));
}

#[test]
fn the_520_byte_limit_is_stated_for_stack_elements_so_it_covers_pushes_in_a_leaf() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(mentions(
        &migration.assumptions,
        &["520-byte stack element limit"]
    ));
}

#[test]
fn single_key_migrations_do_not_state_multisig_assumptions() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(!mentions(&migration.assumptions, &["OP_CHECKMULTISIG"]));
    assert!(!mentions(&migration.assumptions, &["10,000-byte script"]));
    assert!(!mentions(&migration.assumptions, &["Merkle root"]));
}

#[test]
fn pay_to_anchor_is_a_no_op_not_unmapped() {
    // Second input carries the only non-empty witness in the transaction, so the
    // segwit marker is legal (a transaction can't have every witness empty).
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let tx = segwit_tx_with_inputs(&[(&[], &[]), (&[], &[&signature, &pubkey])]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert_eq!(migration.inputs.len(), 2);
    // Nothing to migrate: no witness bytes before, none after.
    assert_eq!(
        summaries(&migration.inputs)[0],
        Summary::Mapped(
            BaselineSpendType::PayToAnchor,
            // Non-witness 41 bytes x 4 WU, plus the 1-byte empty item count x 1 WU.
            4 * 41 + 1,
        )
    );
    assert!(migration.migrated.is_some(), "both inputs are mapped");
}

#[test]
fn coinbase_input_is_a_no_op_keeping_the_oracle_weight() {
    let tx = load_fixture("coinbase");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(is_mapped_as(&tx, BaselineSpendType::Coinbase));
    // Nothing to migrate, so the migrated total is the Oracle's weight (564) and
    // vsize (141) from coinbase.json.
    let total = migration.migrated.expect("the only input is mapped");
    assert_eq!((total.weight, total.vsize), (564, 141));
}

/// Overwrites the outpoint of the first input with `txid` and `index`. Works for
/// transactions built by `legacy_tx*` (first input at byte 5) and `segwit_tx*`
/// (byte 7, after the marker and flag).
fn with_first_outpoint(mut tx: Vec<u8>, txid: [u8; 32], index: u32) -> Vec<u8> {
    let start = if tx[4] == 0x00 { 7 } else { 5 };
    tx[start..start + 32].copy_from_slice(&txid);
    tx[start + 32..start + 36].copy_from_slice(&index.to_le_bytes());
    tx
}

#[test]
fn pre_segwit_coinbase_stays_legacy_with_its_weight_unchanged() {
    // A block with no segwit spends needs no witness commitment, so its coinbase
    // has no witness section. Migration must not add a marker, flag or witness.
    let tx = with_first_outpoint(
        legacy_tx_with_script_sig(&[0x03, 0x01, 0x02, 0x03]),
        [0; 32],
        0xffff_ffff,
    );
    let baseline = transaction_weight(&tx).expect("valid transaction");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(is_mapped_as(&tx, BaselineSpendType::Coinbase));
    assert_eq!(migration.migrated, Some(baseline));
}

#[test]
fn null_outpoint_near_misses_are_not_coinbase() {
    let script_sig = [0x03, 0x01, 0x02, 0x03];
    // Index 0 instead of 0xffffffff.
    let wrong_index = with_first_outpoint(legacy_tx_with_script_sig(&script_sig), [0; 32], 0);
    // Index right, but one txid byte is not zero.
    let mut txid = [0; 32];
    txid[31] = 1;
    let wrong_txid = with_first_outpoint(legacy_tx_with_script_sig(&script_sig), txid, 0xffff_ffff);
    // A null outpoint on one input of a two-input transaction: a coinbase has
    // exactly one input.
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let two_inputs = with_first_outpoint(
        segwit_tx_with_inputs(&[(&script_sig, &[]), (&[], &[&signature, &pubkey])]),
        [0; 32],
        0xffff_ffff,
    );

    for tx in [wrong_index, wrong_txid, two_inputs] {
        assert!(!is_mapped_as(&tx, BaselineSpendType::Coinbase));
    }
}

#[test]
fn legacy_form_empty_spend_is_pay_to_anchor_and_stays_legacy() {
    // Bitcoin Core rejects a segwit marker when every witness is empty, so a
    // transaction whose only inputs are anchor spends must be serialized in
    // legacy form. That is the common real shape (one-input anchor chains), so
    // it is a no-op like any other anchor spend, and gains no marker or witness.
    let tx = legacy_tx(&[]);
    let baseline = transaction_weight(&tx).expect("valid transaction");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(is_mapped_as(&tx, BaselineSpendType::PayToAnchor));
    assert_eq!(migration.migrated, Some(baseline));
}

#[test]
fn anchor_spend_gains_an_empty_witness_when_another_input_makes_the_migrated_transaction_segwit() {
    // Legacy transaction: an anchor spend (empty scriptSig), then a P2PKH spend.
    let mut p2pkh_script_sig = vec![72];
    p2pkh_script_sig.extend_from_slice(&der_signature(72));
    p2pkh_script_sig.push(33);
    p2pkh_script_sig.extend_from_slice(&[2u8; 33]);
    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(2); // input count
    for script_sig in [&[][..], &p2pkh_script_sig] {
        tx.extend_from_slice(&[0u8; 36]); // prevout
        tx.push(u8::try_from(script_sig.len()).expect("short scriptSig"));
        tx.extend_from_slice(script_sig);
        tx.extend_from_slice(&[0xff; 4]); // sequence
    }
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // The P2PKH input's PQ witness makes the migrated transaction segwit, and a
    // segwit transaction has a witness item count for every input: the anchor
    // spend's is an empty one, 1 byte. 41 non-witness bytes x 4 + 1 = 165.
    assert_eq!(
        summaries(&migration.inputs)[0],
        Summary::Mapped(BaselineSpendType::PayToAnchor, 4 * 41 + 1)
    );
}

#[test]
fn assumptions_list_the_stated_facts_the_model_relies_on() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Per ADR-0002 and the slice 1 spec: the three assumptions every migration
    // states, regardless of parameter set.
    assert!(
        migration
            .assumptions
            .iter()
            .any(|a| a.contains("commit to a hash of the"))
    );
    assert!(
        migration
            .assumptions
            .iter()
            .any(|a| a.contains("520-byte") && a.contains("soft fork"))
    );
    assert!(
        migration
            .assumptions
            .iter()
            .any(|a| a.contains("outputs") && a.contains("unchanged"))
    );
}

#[test]
fn falcon_512_assumptions_include_the_fixed_padded_signature_size() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::Falcon512).expect("valid transaction");

    assert!(
        migration
            .assumptions
            .iter()
            .any(|a| a.contains("Falcon") && a.contains("padded"))
    );
}

#[test]
fn non_falcon_parameter_sets_do_not_mention_falcon_padding() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert!(!migration.assumptions.iter().any(|a| a.contains("Falcon")));
}

#[test]
fn a_migrated_total_at_or_below_400_000_weight_does_not_exceed_the_relay_limit() {
    let tx = load_fixture("p2wpkh");

    // ML-DSA-44 gives this fixture a migrated weight of 4069 WU, well under the
    // 400,000 WU relay limit (Bitcoin Core's MAX_STANDARD_TX_WEIGHT), which is
    // policy, not consensus (ADR-0002).
    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert_eq!(migration.exceeds_relay_limit, Some(false));
}

#[test]
fn a_migrated_total_over_400_000_weight_exceeds_the_relay_limit() {
    // Each P2WPKH input migrated to SLH-DSA-128s (8057 WU template weight, see
    // p2wpkh_input_is_migrated_to_an_slh_dsa_128s_witness) costs 8057 WU; 50 of
    // them push the transaction's migrated weight past 400,000 WU.
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let witness: &[&[u8]] = &[&signature, &pubkey];
    let inputs = vec![(&[][..], witness); 50];
    let tx = segwit_tx_with_inputs(&inputs);

    let migration = migrate(&tx, ParameterSet::SlhDsa128s).expect("valid transaction");

    let total = migration.migrated.expect("every input is mapped");
    assert!(total.weight > 400_000, "weight was {}", total.weight);
    assert_eq!(migration.exceeds_relay_limit, Some(true));
}

#[test]
fn a_transaction_with_one_unmapped_input_has_no_migrated_total() {
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    // First input is a clean P2WPKH match; second has an extra witness item,
    // so its shape matches nothing in slice 1.
    let tx = segwit_tx_with_inputs(&[
        (&[], &[&signature, &pubkey]),
        (&[], &[&signature, &pubkey, &[1u8]]),
    ]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // Every input still gets its own reported result...
    assert_eq!(migration.inputs.len(), 2);
    assert!(matches!(migration.inputs[0], InputResult::Mapped { .. }));
    assert_eq!(summaries(&migration.inputs)[1], Summary::Unmapped);
    // ...but there is no migrated total, because not every input is mapped,
    // so there is nothing to check the relay limit against either.
    assert_eq!(migration.migrated, None);
    assert_eq!(migration.exceeds_relay_limit, None);
}

#[test]
fn p2wpkh_input_is_migrated_to_a_falcon_512_witness() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::Falcon512).expect("valid transaction");

    // Falcon-512 (fixed padded signature per ADR-0002 assumption): 666-byte
    // signature, 897-byte public key.
    // Witness: 1 (item count) + (3 + 666) + (3 + 897) = 1570 WU.
    // Non-witness part of the input (empty scriptSig): 41 bytes = 164 WU.
    // Input template weight: 164 + 1570 = 1734 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wpkh, 1734,)]
    );
}

#[test]
fn p2wpkh_input_is_migrated_to_an_slh_dsa_128s_witness() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::SlhDsa128s).expect("valid transaction");

    // SLH-DSA-128s (FIPS 205, "small" parameter set): 7856-byte signature,
    // 32-byte public key.
    // Witness: 1 (item count) + (3 + 7856) + (1 + 32) = 7893 WU.
    // Non-witness part of the input (empty scriptSig): 41 bytes = 164 WU.
    // Input template weight: 164 + 7893 = 8057 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wpkh, 8057,)]
    );
}

#[test]
fn p2wpkh_input_is_migrated_to_an_ml_dsa_44_witness() {
    let tx = load_fixture("p2wpkh");

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    // ML-DSA-44 (FIPS 204): 2420-byte signature, 1312-byte public key.
    // Witness: 1 (item count) + (3 + 2420) + (3 + 1312) = 3739 WU, 1 WU per byte.
    // Non-witness part of the input: 32 + 4 + 1 (empty scriptSig) + 4 = 41 bytes = 164 WU.
    // Input template weight: 164 + 3739 = 3903 WU.
    assert_eq!(
        summaries(&migration.inputs),
        vec![Summary::Mapped(BaselineSpendType::P2wpkh, 3903,)]
    );

    // Rest of the transaction: version 4 + counts 2 + output (8 + 1 + 22) + locktime 4 = 41 bytes
    // = 164 WU, plus 2 WU for the segwit marker and flag.
    // Total: 164 + 2 + 3903 = 4069 WU, and vsize = ceil(4069 / 4) = 1018.
    let total = migration.migrated.expect("every input is mapped");
    assert_eq!(total.weight, 4069);
    assert_eq!(total.vsize, 1018);
}

/// Weight of a compact size for `value`, for the small ranges these strategies use.
fn compact_size_weight(value: u64) -> u64 {
    if value <= 0xfc { 1 } else { 3 }
}

/// A single-input transaction of one of the mapped Baseline spend types,
/// paired with an independently computed upper bound on how much weight
/// migration could possibly remove: what the original signature and public key
/// cost today, in the unit (witness at 1 WU/byte, scriptSig at 4 WU/byte) they
/// were actually stored in.
fn mapped_tx_with_removable_weight() -> impl Strategy<Value = (Vec<u8>, u64)> {
    let native_segwit_strategy =
        (70u8..=73, prop::sample::select(vec![33u8])).prop_map(|(sig_len, pk_len)| {
            let signature = der_signature(sig_len as usize);
            let pubkey = vec![0u8; pk_len as usize];
            let tx = segwit_tx(&[], &[&signature, &pubkey]);
            // Witness: item count + each item's compact size and bytes, all 1 WU/byte.
            let removable = 1
                + compact_size_weight(u64::from(sig_len))
                + u64::from(sig_len)
                + compact_size_weight(u64::from(pk_len))
                + u64::from(pk_len);
            (tx, removable)
        });

    let taproot_key_path_strategy = (64u8..=65).prop_map(|sig_len| {
        let signature = vec![0u8; sig_len as usize];
        let tx = segwit_tx(&[], &[&signature]);
        // Witness: item count + the signature's compact size and bytes.
        let removable = 1 + compact_size_weight(u64::from(sig_len)) + u64::from(sig_len);
        (tx, removable)
    });

    let wrapped_segwit_strategy =
        (70u8..=73, prop::sample::select(vec![33u8])).prop_map(|(sig_len, pk_len)| {
            let signature = der_signature(sig_len as usize);
            let pubkey = vec![0u8; pk_len as usize];
            let mut redeem_script_push = vec![0x16, 0x00, 0x14];
            redeem_script_push.extend_from_slice(&[0u8; 20]);
            let tx = segwit_tx(&redeem_script_push, &[&signature, &pubkey]);
            // The scriptSig is unchanged by migration, so nothing there is removable.
            let removable = 1
                + compact_size_weight(u64::from(sig_len))
                + u64::from(sig_len)
                + compact_size_weight(u64::from(pk_len))
                + u64::from(pk_len);
            (tx, removable)
        });

    let legacy_strategy =
        (70u8..=73, prop::sample::select(vec![33u8, 65u8])).prop_map(|(sig_len, pk_len)| {
            let signature = der_signature(sig_len as usize);
            let pubkey = vec![0u8; pk_len as usize];
            let tx = legacy_tx(&[&signature, &pubkey]);
            // scriptSig: a 1-byte direct-push opcode plus the bytes, for each
            // push, all at 4 WU/byte.
            let removable = 4 * (1 + u64::from(sig_len) + 1 + u64::from(pk_len));
            (tx, removable)
        });

    // m-of-n with 1 <= m <= n <= 16; the whole baseline witness is removable.
    let witness_multisig_strategy =
        (1u8..=16, 0u8..16, any::<bool>()).prop_map(|(n, m_offset, wrapped)| {
            let m = 1 + m_offset % n;
            let script_sig = if wrapped {
                p2sh_p2wsh_redeem_script_push()
            } else {
                Vec::new()
            };
            let tx = multisig_witness_tx(&script_sig, m, n);
            // Items as built by multisig_witness_tx: empty dummy, m 72-byte
            // signatures, and the script of n 34-byte key pushes plus 3 opcodes.
            let script_len = 3 + 34 * u64::from(n);
            let removable = compact_size_weight(u64::from(m) + 2)
                + 1
                + u64::from(m) * (1 + 72)
                + compact_size_weight(script_len)
                + script_len;
            (tx, removable)
        });

    // Legacy P2SH m-of-n with n <= 7 so the script fits one OP_PUSHDATA1; the
    // whole scriptSig and its length prefix are removable, at 4 WU/byte.
    let p2sh_multisig_strategy = (1u8..=7, 0u8..7).prop_map(|(n, m_offset)| {
        let m = 1 + m_offset % n;
        let key = [2u8; 33];
        let signature = der_signature(72);
        let script = multisig_script(&op_n(m), &vec![&key[..]; usize::from(n)], &op_n(n));
        let script_sig = p2sh_multisig_script_sig(&vec![&signature[..]; usize::from(m)], &script);
        let script_sig_len = script_sig.len() as u64;
        let tx = legacy_tx_with_script_sig(&script_sig);
        let removable = 4 * (compact_size_weight(script_sig_len) + script_sig_len);
        (tx, removable)
    });

    prop_oneof![
        native_segwit_strategy,
        taproot_key_path_strategy,
        wrapped_segwit_strategy,
        legacy_strategy,
        witness_multisig_strategy,
        p2sh_multisig_strategy
    ]
}

proptest! {
    #[test]
    fn migrated_weight_is_at_least_baseline_weight_minus_what_could_be_removed(
        (tx, removable) in mapped_tx_with_removable_weight()
    ) {
        let baseline = transaction_weight(&tx).expect("valid transaction");
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        let migrated = migration.migrated.expect("every generated input is mapped");

        // Migration only ever adds a much larger PQ witness in place of the
        // removed bytes, so this bound has a wide margin; it exists to catch a
        // sign error (for example, subtracting the PQ witness instead of adding
        // it) rather than to pin an exact value.
        prop_assert!(migrated.weight + removable >= baseline.weight);
    }
}
