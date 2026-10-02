//! The parser must be safe on untrusted input: any bytes give a result or an error,
//! never a panic. Real Fixtures are used as seeds so the inputs reach deep into the parser.

mod common;

use common::{block_fixture, block_fixture_names, decode_hex, fixture_hex_paths};
use pqweight::{ParseError, parse_block, transaction_weight};
use proptest::prelude::*;

fn fixture_transactions() -> Vec<Vec<u8>> {
    fixture_hex_paths()
        .iter()
        .map(|path| decode_hex(std::fs::read_to_string(path).unwrap().trim()))
        .collect()
}

#[test]
fn every_strict_prefix_of_a_real_transaction_is_reported_as_truncated() {
    for tx in fixture_transactions() {
        assert!(transaction_weight(&tx).is_ok(), "full fixture must parse");
        for len in 0..tx.len() {
            let result = transaction_weight(&tx[..len]);
            assert!(
                matches!(result, Err(ParseError::Truncated { .. })),
                "prefix of {len}/{} bytes gave {result:?}",
                tx.len()
            );
        }
    }
}

fn fixture_blocks() -> Vec<Vec<u8>> {
    block_fixture_names()
        .iter()
        .map(|name| block_fixture(name).bytes)
        .collect()
}

#[test]
fn every_strict_prefix_of_a_real_block_is_an_error() {
    for block in fixture_blocks() {
        assert!(parse_block(&block).is_ok(), "full fixture must parse");
        for len in 0..block.len() {
            assert!(parse_block(&block[..len]).is_err(), "prefix of {len} bytes");
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let _ = transaction_weight(&bytes);
    }

    #[test]
    fn corrupting_a_real_transaction_never_panics(
        which in any::<prop::sample::Index>(),
        position in any::<prop::sample::Index>(),
        replacement in any::<u8>(),
    ) {
        let txs = fixture_transactions();
        let mut tx = txs[which.index(txs.len())].clone();
        let at = position.index(tx.len());
        tx[at] = replacement;
        let _ = transaction_weight(&tx);
    }

    #[test]
    fn corrupting_a_real_block_never_panics(
        which in any::<prop::sample::Index>(),
        position in any::<prop::sample::Index>(),
        replacement in any::<u8>(),
    ) {
        let blocks = fixture_blocks();
        let mut block = blocks[which.index(blocks.len())].clone();
        let at = position.index(block.len());
        block[at] = replacement;
        let _ = parse_block(&block);
    }

    #[test]
    fn arbitrary_bytes_never_panic_as_a_block(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let _ = parse_block(&bytes);
    }
}
