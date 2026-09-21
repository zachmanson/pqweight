//! Behavior of the public weight function: bytes in, weight out.
//!
//! Expected values are worked out by hand from the consensus definition
//! (weight = 3 x stripped size + total size), not recomputed by the code under test.

use pqweight::{ParseError, transaction_weight};

/// Smallest structurally valid legacy transaction: one input with an empty
/// scriptSig, one output with an empty scriptPubKey. 60 bytes in total.
fn minimal_legacy_tx() -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 32]); // prevout txid
    tx.extend_from_slice(&0u32.to_le_bytes()); // prevout index
    tx.push(0); // scriptSig length
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // scriptPubKey length
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

/// One-input, one-output P2WPKH-shaped transaction with a 72-byte signature and
/// a 33-byte public key in the witness. Contents are filler; only sizes matter.
fn p2wpkh_tx() -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&2u32.to_le_bytes()); // version
    tx.extend_from_slice(&[0x00, 0x01]); // segwit marker, flag
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 32]); // prevout txid
    tx.extend_from_slice(&0u32.to_le_bytes()); // prevout index
    tx.push(0); // empty scriptSig
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(22); // scriptPubKey length
    tx.extend_from_slice(&[0x00, 0x14]);
    tx.extend_from_slice(&[0u8; 20]); // P2WPKH script
    tx.push(2); // witness item count
    tx.push(72);
    tx.extend_from_slice(&[0u8; 72]); // signature
    tx.push(33);
    tx.extend_from_slice(&[0u8; 33]); // public key
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

#[test]
fn segwit_transaction_discounts_marker_flag_and_witness() {
    let tx = p2wpkh_tx();

    let result = transaction_weight(&tx).expect("valid segwit transaction");

    // Stripped: 4 + 1 + 36 + 1 + 4 + 1 + 8 + 1 + 22 + 4 = 82.
    // Witness: 1 + (1 + 72) + (1 + 33) = 108, plus 2 for marker and flag.
    assert_eq!(result.stripped_size, 82);
    assert_eq!(result.total_size, 192);
    assert_eq!(result.weight, 438); // 3 * 82 + 192
    assert_eq!(result.vsize, 110); // ceil(438 / 4)
}

/// Legacy transaction whose single scriptSig is `script_len` filler bytes,
/// preceded by the given compact-size length prefix.
fn legacy_tx_with_script_sig(length_prefix: &[u8], script_len: usize) -> Vec<u8> {
    let mut tx = Vec::new();
    tx.extend_from_slice(&1u32.to_le_bytes()); // version
    tx.push(1); // input count
    tx.extend_from_slice(&[0u8; 36]); // prevout
    tx.extend_from_slice(length_prefix);
    tx.extend(std::iter::repeat_n(0u8, script_len));
    tx.extend_from_slice(&[0xff; 4]); // sequence
    tx.push(1); // output count
    tx.extend_from_slice(&0u64.to_le_bytes()); // value
    tx.push(0); // scriptPubKey length
    tx.extend_from_slice(&0u32.to_le_bytes()); // locktime
    tx
}

#[test]
fn script_length_of_253_uses_a_three_byte_compact_size() {
    // 0xFD marks a 2-byte little-endian length that follows: 253 = 0x00FD.
    let tx = legacy_tx_with_script_sig(&[0xfd, 0xfd, 0x00], 253);

    let result = transaction_weight(&tx).expect("valid legacy transaction");

    // 60 bytes minimum, minus the 1-byte empty length, plus 3-byte prefix and 253 script bytes.
    assert_eq!(result.total_size, 315);
    assert_eq!(result.weight, 1260);
    assert_eq!(result.vsize, 315);
}

#[test]
fn script_length_of_65536_uses_a_five_byte_compact_size() {
    // 0xFE marks a 4-byte little-endian length that follows: 65536 = 0x0001_0000.
    let tx = legacy_tx_with_script_sig(&[0xfe, 0x00, 0x00, 0x01, 0x00], 65_536);

    let result = transaction_weight(&tx).expect("valid legacy transaction");

    // 60 bytes minimum, minus the 1-byte empty length, plus 5-byte prefix and 65536 script bytes.
    assert_eq!(result.total_size, 65_600);
    assert_eq!(result.weight, 262_400);
    assert_eq!(result.vsize, 65_600);
}

#[test]
fn segwit_marker_with_all_empty_witnesses_is_rejected() {
    let full = p2wpkh_tx();
    // Keep bytes 0..80 (up to the witness), replace the 108-byte witness with a
    // single zero item count for the one input, keep the 4-byte locktime.
    let mut tx = full[..80].to_vec();
    tx.push(0x00);
    tx.extend_from_slice(&full[188..]);

    let err = transaction_weight(&tx).expect_err("empty witnesses must be rejected");

    assert_eq!(err, ParseError::EmptyWitness { offset: 80 });
}

#[test]
fn script_length_larger_than_the_remaining_input_is_rejected() {
    // 0xFF marks an 8-byte little-endian length; this one claims u64::MAX bytes.
    let tx = legacy_tx_with_script_sig(&[0xff; 9], 0);

    let err = transaction_weight(&tx).expect_err("impossible length must be rejected");

    // Version 0..4, input count 4, previous output 5..41, length prefix 41..50.
    assert_eq!(
        err,
        ParseError::Truncated {
            reading: "scriptSig",
            offset: 50
        }
    );
}

#[test]
fn bytes_after_a_complete_transaction_are_rejected() {
    let mut tx = minimal_legacy_tx();
    tx.push(0xab);

    let err = transaction_weight(&tx).expect_err("trailing byte must be rejected");

    // The transaction ends at byte 60, so one byte is left over.
    assert_eq!(
        err,
        ParseError::TrailingBytes {
            offset: 60,
            remaining: 1
        }
    );
}

#[test]
fn truncated_transaction_reports_what_was_being_read_and_where() {
    // (bytes kept, field being read, offset where that read began)
    let cases = [
        (0, "version", 0),
        (3, "version", 0),
        (4, "input count", 4),
        (40, "previous output", 5),
        (59, "locktime", 56),
    ];
    let tx = minimal_legacy_tx();

    for (kept, reading, offset) in cases {
        let err = transaction_weight(&tx[..kept]).expect_err("truncated input must be rejected");

        assert_eq!(
            err,
            ParseError::Truncated { reading, offset },
            "transaction cut to {kept} bytes"
        );
    }
}

#[test]
fn legacy_transaction_weight_is_four_times_its_size() {
    let tx = minimal_legacy_tx();
    assert_eq!(tx.len(), 60);

    let result = transaction_weight(&tx).expect("valid legacy transaction");

    assert_eq!(result.total_size, 60);
    assert_eq!(result.stripped_size, 60);
    assert_eq!(result.weight, 240);
    assert_eq!(result.vsize, 60);
}
