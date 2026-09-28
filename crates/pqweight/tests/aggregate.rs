//! Behavior of the public aggregate function: many raw transactions in,
//! summed totals out. Expected sums are hand-derived from per-transaction
//! numbers already established (and hand-derived themselves) in `migrate.rs`
//! and each fixture's Oracle, not by calling `migrate`/`transaction_weight`
//! again inside the test.

mod common;

use common::{der_signature, fixtures_dir};
use pqweight::{
    BaselineSpendType, BreakdownKind, BreakdownRow, ExposureRow, FeeRate, KeyExposure,
    ParameterSet, UnmappedReason, aggregate,
};

/// A one-input, one-output segwit transaction with the given scriptSig and
/// witness items, built the same way as `migrate.rs`'s `segwit_tx` helper.
/// Contents are filler; only sizes matter.
fn segwit_tx(script_sig: &[u8], witness: &[&[u8]]) -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&2u32.to_le_bytes()); // version
    tx.extend_from_slice(&[0x00, 0x01]); // segwit marker, flag
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 36]); // prevout
    tx.push(u8::try_from(script_sig.len()).expect("short scriptSig"));
    tx.extend_from_slice(script_sig);
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    tx.push(u8::try_from(witness.len()).expect("few items"));
    for item in witness {
        tx.push(u8::try_from(item.len()).expect("short item"));
        tx.extend_from_slice(item);
    }
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

/// A one-input, one-output legacy transaction with an empty scriptSig: a
/// pay-to-anchor spend in legacy form. Its input is 41 non-witness bytes.
fn legacy_anchor_tx() -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 36]); // prevout
    tx.push(0); // empty scriptSig
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // empty scriptPubKey
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").expect("writing to a String never fails");
        hex
    })
}

fn fixture_hex(name: &str) -> String {
    let path = fixtures_dir().join(format!("{name}.hex"));
    std::fs::read_to_string(&path)
        .expect("fixture exists")
        .trim()
        .to_string()
}

#[test]
fn baseline_and_migrated_totals_are_summed_across_two_fixtures() {
    // p2wpkh: Oracle baseline weight 436, vsize 109. Migrated (ML-DSA-44):
    // weight 4069, vsize 1018 (see migrate.rs::p2wpkh_input_is_migrated_to_an_ml_dsa_44_witness).
    //
    // p2sh-p2wpkh: Oracle baseline weight 529, vsize 133. Migrated
    // (ML-DSA-44): input template weight 3995 WU (see
    // migrate.rs::p2sh_p2wpkh_input_is_migrated_keeping_the_redeem_script_scriptsig),
    // and since this scriptSig is unchanged by migration, the transaction's
    // stripped_size is unchanged at 105 bytes (weight 529 = 3*105 + 214).
    // Migrated total_size = 105 (stripped) + 2 (marker/flag) + 3739 (ML-DSA-44
    // witness, same as the P2WPKH case since there's one input) = 3846.
    // Migrated weight = 3*105 + 3846 = 4161, vsize = ceil(4161/4) = 1041.
    let lines = vec![fixture_hex("p2wpkh"), fixture_hex("p2sh-p2wpkh")];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.baseline.weight, 436 + 529);
    assert_eq!(result.baseline.vsize, 109 + 133);
    let migrated = result.migrated.expect("both fixtures fully mapped");
    assert_eq!(migrated.weight, 4069 + 4161);
    assert_eq!(migrated.vsize, 1018 + 1041);
    assert_eq!(result.counts.parsed, 2);
    assert_eq!(result.counts.fully_mapped, 2);
    assert_eq!(result.counts.partially_mapped, 0);
    assert_eq!(result.counts.unmapped_inputs, 0);
    assert_eq!(result.counts.parse_errors, 0);
    assert!(result.errors.is_empty());
}

#[test]
fn a_malformed_line_is_recorded_and_does_not_abort_the_batch() {
    let lines = vec![
        fixture_hex("p2wpkh"),
        "zz".to_string(), // invalid hex
        fixture_hex("p2sh-p2wpkh"),
    ];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.counts.parsed, 2);
    assert_eq!(result.counts.parse_errors, 1);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].line, 2);
    assert!(result.errors[0].message.contains("invalid hex"));
    // The two valid lines are still aggregated.
    assert_eq!(result.baseline.weight, 436 + 529);
    let migrated = result.migrated.expect("both valid fixtures fully mapped");
    assert_eq!(migrated.weight, 4069 + 4161);
}

#[test]
fn an_unmapped_input_counts_toward_baseline_but_is_excluded_from_the_migrated_total() {
    // A P2WPKH-shaped witness plus an extra item is Unmapped (see
    // migrate.rs::p2wpkh_shape_with_an_extra_witness_item_or_a_script_sig_is_unmapped).
    // Hand-derived sizes: stripped_size = version 4 + input count 1 + input
    // (36 prevout + 1 empty scriptSig length + 4 sequence = 41) + output count
    // 1 + output (8 value + 1 empty scriptPubKey length = 9) + locktime 4 = 60
    // bytes. Witness: item count 1 + sig (1 + 72) + pubkey (1 + 33) + extra
    // (1 + 1) = 110 bytes. total_size = 60 + 2 (marker/flag) + 110 = 172.
    // weight = 3*60 + 172 = 352, vsize = ceil(352/4) = 88.
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let extra = [0u8; 1];
    let unmapped_tx = segwit_tx(&[], &[&signature, &pubkey, &extra]);
    let lines = vec![fixture_hex("p2wpkh"), to_hex(&unmapped_tx)];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.counts.parsed, 2);
    assert_eq!(result.counts.fully_mapped, 1);
    assert_eq!(result.counts.partially_mapped, 1);
    assert_eq!(result.counts.unmapped_inputs, 1);
    // Baseline weight/vsize don't depend on migration, so both transactions count.
    assert_eq!(result.baseline.weight, 436 + 352);
    assert_eq!(result.baseline.vsize, 109 + 88);
    // Migrated total is present (p2wpkh is fully mapped) and excludes the
    // unmapped transaction's weight entirely.
    let migrated = result.migrated.expect("one fully mapped transaction");
    assert_eq!(migrated.weight, 4069);
    assert_eq!(migrated.vsize, 1018);
}

#[test]
fn when_every_transaction_has_an_unmapped_input_the_migrated_total_is_none() {
    let signature = der_signature(72);
    let pubkey = [0u8; 33];
    let extra = [0u8; 1];
    let unmapped_tx = segwit_tx(&[], &[&signature, &pubkey, &extra]);
    let lines = vec![to_hex(&unmapped_tx), to_hex(&unmapped_tx)];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.counts.parsed, 2);
    assert_eq!(result.counts.fully_mapped, 0);
    assert_eq!(result.counts.partially_mapped, 2);
    // Baseline is still summed; nothing to do with migration coverage.
    assert_eq!(result.baseline.weight, 352 * 2);
    assert_eq!(result.migrated, None);
}

#[test]
fn fee_totals_sum_per_transaction_fees_not_the_fee_of_a_summed_vsize() {
    // p2wpkh baseline vsize 109, p2sh-p2wpkh baseline vsize 133, at 1.5 sat/vB.
    // fee(109, 1.5) = ceil(163.5) = 164; fee(133, 1.5) = ceil(199.5) = 200.
    // Per-transaction sum: 364. Summing the vsizes first (242) and rounding
    // once gives ceil(363.0) = 363 -- the undercount this test guards against.
    let rate = FeeRate::parse("1.5").expect("valid rate");
    let lines = vec![fixture_hex("p2wpkh"), fixture_hex("p2sh-p2wpkh")];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, Some(rate));

    let fees = result.fees.expect("a fee rate was given");
    assert_eq!(fees.baseline, 164 + 200);
}

#[test]
fn breakdown_has_one_row_per_spend_type_and_unmapped_reason_in_order_of_first_appearance() {
    // p2wpkh: Input weight 270 (41 non-witness bytes x 4 + 106 witness bytes),
    // ML-DSA-44 template weight 3903 (see migrate.rs).
    // p2tr-keypath-annex: 41 non-witness bytes x 4 = 164, witness: item count 1
    // + (1 + 64 signature) + (1 + 3 annex) = 70.
    let lines = vec![
        fixture_hex("p2wpkh"),
        fixture_hex("p2tr-keypath-annex"),
        fixture_hex("p2wpkh"),
    ];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(
        result.breakdown,
        vec![
            BreakdownRow {
                kind: BreakdownKind::Mapped(BaselineSpendType::P2wpkh),
                inputs: 2,
                baseline_weight: 2 * 270,
                migrated_weight: Some(2 * 3903),
            },
            BreakdownRow {
                kind: BreakdownKind::Unmapped(UnmappedReason::P2trKeyPathAnnex),
                inputs: 1,
                baseline_weight: 164 + 70,
                migrated_weight: None,
            },
        ]
    );
}

#[test]
fn partially_mapped_transactions_have_their_own_baseline_totals() {
    // Only p2tr-keypath-annex is partially mapped: Oracle weight 400, vsize 100
    // (p2tr-keypath-annex.json). The report needs this to weigh the all-or-nothing
    // migrated-total rule by weight, not only by transaction count.
    let lines = vec![fixture_hex("p2wpkh"), fixture_hex("p2tr-keypath-annex")];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    assert_eq!(result.partially_mapped.weight, 400);
    assert_eq!(result.partially_mapped.vsize, 100);
}

#[test]
fn exposure_has_one_row_per_key_exposure_summing_its_inputs_and_counting_unmapped_ones_apart() {
    // Input weights at ML-DSA-44, each 41 non-witness bytes x 4 = 164 WU plus witness:
    // - P2TR key-path, built: witness 1 + (1 + 64) = 66, so 230. Template 3903
    //   (same [signature, key] witness as P2WPKH, see migrate.rs).
    // - p2tr-keypath-annex fixture: Unmapped, 164 + 70 = 234 (see the breakdown test).
    // - p2wpkh fixture: 270, template 3903.
    // - legacy anchor: 164, no witness before or after, so 164.
    let lines = vec![
        to_hex(&segwit_tx(&[], &[&[0u8; 64]])),
        fixture_hex("p2tr-keypath-annex"),
        fixture_hex("p2wpkh"),
        to_hex(&legacy_anchor_tx()),
    ];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    let row = |key_exposure, mapped: (usize, u64, u64), unmapped: (usize, u64)| ExposureRow {
        key_exposure,
        mapped_inputs: mapped.0,
        baseline_weight: mapped.1,
        migrated_weight: mapped.2,
        unmapped_inputs: unmapped.0,
        unmapped_baseline_weight: unmapped.1,
    };
    assert_eq!(
        result.exposure,
        vec![
            row(KeyExposure::ExposedInOutput, (1, 230, 3903), (1, 234)),
            row(KeyExposure::HashedUntilSpend, (1, 270, 3903), (0, 0)),
            row(KeyExposure::NoKey, (1, 164, 164), (0, 0)),
            row(KeyExposure::Undetermined, (0, 0, 0), (0, 0)),
        ]
    );
    let added: Vec<i64> = result
        .exposure
        .iter()
        .map(ExposureRow::added_weight)
        .collect();
    assert_eq!(added, vec![3903 - 230, 3903 - 270, 0, 0]);
}

#[test]
fn a_mapped_breakdown_row_has_added_weight_and_an_unmapped_one_has_none() {
    // Same batch and Input weights as the exposure test above: P2TR key-path 230
    // -> 3903, p2tr-keypath-annex Unmapped at 234, P2WPKH 270 -> 3903, legacy
    // anchor 164 -> 164.
    let lines = vec![
        to_hex(&segwit_tx(&[], &[&[0u8; 64]])),
        fixture_hex("p2tr-keypath-annex"),
        fixture_hex("p2wpkh"),
        to_hex(&legacy_anchor_tx()),
    ];

    let result = aggregate(lines.into_iter(), ParameterSet::MlDsa44, None);

    let added: Vec<(BreakdownKind, Option<i64>)> = result
        .breakdown
        .iter()
        .map(|row| (row.kind, row.added_weight()))
        .collect();
    assert_eq!(
        added,
        vec![
            (
                BreakdownKind::Mapped(BaselineSpendType::P2trKeyPath),
                Some(3903 - 230)
            ),
            (
                BreakdownKind::Unmapped(UnmappedReason::P2trKeyPathAnnex),
                None
            ),
            (
                BreakdownKind::Mapped(BaselineSpendType::P2wpkh),
                Some(3903 - 270)
            ),
            (
                BreakdownKind::Mapped(BaselineSpendType::PayToAnchor),
                Some(0)
            ),
        ]
    );
    // The mapped rows' Added weight is the same block space as the Key exposure
    // rows', just grouped differently.
    let by_spend_type: i64 = result
        .breakdown
        .iter()
        .filter_map(BreakdownRow::added_weight)
        .sum();
    let by_exposure: i64 = result.exposure.iter().map(ExposureRow::added_weight).sum();
    assert_eq!(by_spend_type, by_exposure);
}
