//! The parser must be safe on untrusted input: any bytes give a result or an error,
//! never a panic. Real Fixtures are used as seeds so the inputs reach deep into the parser.

mod common;

use common::{decode_hex, fixture_hex_paths};
use pqweight::{ParseError, transaction_weight};
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
}
