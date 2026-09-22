//! Behavior of the public migration function: raw transaction in, Input results out.
//!
//! No **Oracle** exists for post-quantum weight, so expected values are worked out
//! by hand from the **Migration template** and the FIPS 204 sizes, with the
//! arithmetic written next to each assertion.

mod common;

use common::{decode_hex, fixtures_dir};
use pqweight::{BaselineSpendType, InputResult, ParameterSet, migrate, transaction_weight};
use proptest::prelude::*;

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(format!("{name}.hex"));
    let hex = std::fs::read_to_string(&path).expect("fixture exists");
    decode_hex(hex.trim())
}

/// A segwit transaction with one input per `(script_sig, witness)` pair given,
/// and one output. Contents are filler; only sizes matter. All lengths must be
/// below 253.
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
            tx.push(u8::try_from(item.len()).expect("short item"));
            tx.extend_from_slice(item);
        }
    }
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
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

    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 36]); // prevout
    tx.push(u8::try_from(script_sig.len()).expect("short scriptSig"));
    tx.extend_from_slice(&script_sig);
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

fn is_mapped_as(tx: &[u8], expected: BaselineSpendType) -> bool {
    let migration = migrate(tx, ParameterSet::MlDsa44).expect("valid transaction");
    matches!(
        migration.inputs[0],
        InputResult::Mapped { spend_type, .. } if spend_type == expected
    )
}

#[test]
fn p2wpkh_accepts_der_signatures_of_70_to_73_bytes_and_rejects_others() {
    let pubkey = [0u8; 33];
    // (signature length, expected to be recognized as P2WPKH)
    let cases = [(69, false), (70, true), (73, true), (74, false)];

    for (sig_len, mapped) in cases {
        let signature = vec![0u8; sig_len];
        let tx = segwit_tx(&[], &[&signature, &pubkey]);

        assert_eq!(
            is_mapped_as(&tx, BaselineSpendType::P2wpkh),
            mapped,
            "signature of {sig_len} bytes"
        );
    }
}

#[test]
fn p2wpkh_requires_a_33_byte_public_key() {
    let signature = [0u8; 72];

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
    let signature = [0u8; 72];
    let pubkey = [0u8; 33];

    let extra_item = segwit_tx(&[], &[&signature, &pubkey, &[1u8]]);
    let with_script_sig = segwit_tx(&[0x51], &[&signature, &pubkey]);

    for tx in [extra_item, with_script_sig] {
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(migration.inputs, vec![InputResult::Unmapped]);
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

    assert_eq!(migration.inputs, vec![InputResult::Unmapped]);
    assert_eq!(migration.migrated, None);
}

#[test]
fn p2sh_p2wpkh_requires_the_0014_redeem_script_push_and_a_p2wpkh_witness() {
    let signature = [0u8; 72];
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
        assert_eq!(migration.inputs, vec![InputResult::Unmapped]);
    }
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
        migration.inputs,
        vec![InputResult::Mapped {
            spend_type: BaselineSpendType::P2shP2wpkh,
            template_weight: 3995,
        }]
    );
}

#[test]
fn p2pkh_accepts_a_der_signature_and_a_33_or_65_byte_pubkey() {
    for pubkey_len in [33, 65] {
        let signature = [0u8; 72];
        let pubkey = vec![0u8; pubkey_len];
        let tx = legacy_tx(&[&signature, &pubkey]);

        assert!(
            is_mapped_as(&tx, BaselineSpendType::P2pkh),
            "pubkey of {pubkey_len} bytes"
        );
    }
}

#[test]
fn p2pkh_near_misses_are_unmapped() {
    let signature = [0u8; 72];
    let pubkey = [0u8; 33];

    // Wrong pubkey length, one push short of the pair, and an extra third push.
    let wrong_pubkey_len = legacy_tx(&[&signature, &[0u8; 34]]);
    let missing_pubkey = legacy_tx(&[&signature]);
    let extra_push = legacy_tx(&[&signature, &pubkey, &[1u8]]);

    for tx in [wrong_pubkey_len, missing_pubkey, extra_push] {
        let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");
        assert_eq!(migration.inputs, vec![InputResult::Unmapped]);
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
        migration.inputs,
        vec![InputResult::Mapped {
            spend_type: BaselineSpendType::P2pkh,
            template_weight: 3903,
        }]
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
fn pay_to_anchor_is_a_no_op_not_unmapped() {
    // Second input carries the only non-empty witness in the transaction, so the
    // segwit marker is legal (a transaction can't have every witness empty).
    let signature = [0u8; 72];
    let pubkey = [0u8; 33];
    let tx = segwit_tx_with_inputs(&[(&[], &[]), (&[], &[&signature, &pubkey])]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert_eq!(migration.inputs.len(), 2);
    // Nothing to migrate: no witness bytes before, none after.
    assert_eq!(
        migration.inputs[0],
        InputResult::Mapped {
            spend_type: BaselineSpendType::PayToAnchor,
            // Non-witness 41 bytes x 4 WU, plus the 1-byte empty item count x 1 WU.
            template_weight: 4 * 41 + 1,
        }
    );
    assert!(migration.migrated.is_some(), "both inputs are mapped");
}

#[test]
fn empty_script_sig_in_a_legacy_transaction_is_not_pay_to_anchor() {
    // No segwit marker at all: this shape is Unmapped, not a no-op, because the
    // spending side never asserted "empty witness" the way a real anchor spend does.
    let tx = legacy_tx(&[]);

    let migration = migrate(&tx, ParameterSet::MlDsa44).expect("valid transaction");

    assert_eq!(migration.inputs, vec![InputResult::Unmapped]);
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
    let signature = [0u8; 72];
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
    let signature = [0u8; 72];
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
    assert_eq!(migration.inputs[1], InputResult::Unmapped);
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
        migration.inputs,
        vec![InputResult::Mapped {
            spend_type: BaselineSpendType::P2wpkh,
            template_weight: 1734,
        }]
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
        migration.inputs,
        vec![InputResult::Mapped {
            spend_type: BaselineSpendType::P2wpkh,
            template_weight: 8057,
        }]
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
        migration.inputs,
        vec![InputResult::Mapped {
            spend_type: BaselineSpendType::P2wpkh,
            template_weight: 3903,
        }]
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

/// A single-input transaction of one of the four mapped Baseline spend types,
/// paired with an independently computed upper bound on how much weight
/// migration could possibly remove: what the original signature and public key
/// cost today, in the unit (witness at 1 WU/byte, scriptSig at 4 WU/byte) they
/// were actually stored in.
fn mapped_tx_with_removable_weight() -> impl Strategy<Value = (Vec<u8>, u64)> {
    let native_segwit_strategy =
        (70u8..=73, prop::sample::select(vec![33u8])).prop_map(|(sig_len, pk_len)| {
            let signature = vec![0u8; sig_len as usize];
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
            let signature = vec![0u8; sig_len as usize];
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
            let signature = vec![0u8; sig_len as usize];
            let pubkey = vec![0u8; pk_len as usize];
            let tx = legacy_tx(&[&signature, &pubkey]);
            // scriptSig: a 1-byte direct-push opcode plus the bytes, for each
            // push, all at 4 WU/byte.
            let removable = 4 * (1 + u64::from(sig_len) + 1 + u64::from(pk_len));
            (tx, removable)
        });

    prop_oneof![
        native_segwit_strategy,
        taproot_key_path_strategy,
        wrapped_segwit_strategy,
        legacy_strategy
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
