//! `parse_block` against the block Fixtures: a regtest block whose Oracle is
//! Bitcoin Core's `getblock` (recorded by `scripts/record-block-fixture.ps1`),
//! and mainnet block 170, whose hash and merkle root are well known. Weight is
//! also checked against `rust-bitcoin`, the second implementation (ADR 0001).

mod common;

use common::{block_fixture, block_fixture_names};

use pqweight::{BlockError, ParseError, parse_block};

#[test]
fn block_170_hash_and_merkle_root_are_the_known_ones() {
    let fixture = block_fixture("block-170");

    let block = parse_block(&fixture.bytes).unwrap();

    assert_eq!(
        block.hash.to_string(),
        fixture.oracle["hash"].as_str().unwrap()
    );
    assert_eq!(
        block.merkle_root.to_string(),
        fixture.oracle["merkleroot"].as_str().unwrap()
    );
}

#[test]
fn regtest_block_matches_getblock() {
    let fixture = block_fixture("regtest-block");
    let oracle = &fixture.oracle;

    let block = parse_block(&fixture.bytes).unwrap();

    assert_eq!(block.hash.to_string(), oracle["hash"].as_str().unwrap());
    assert_eq!(
        block.merkle_root.to_string(),
        oracle["merkleroot"].as_str().unwrap()
    );
    assert_eq!(
        block.transactions.len() as u64,
        oracle["nTx"].as_u64().unwrap()
    );
    assert_eq!(block.weight, oracle["weight"].as_u64().unwrap());
    // Weight is 3 x stripped size + size, so with Core's size it pins the
    // stripped size too.
    let size = fixture.bytes.len() as u64;
    assert_eq!(size, oracle["size"].as_u64().unwrap());
    assert_eq!(
        (block.weight - size) / 3,
        oracle["strippedsize"].as_u64().unwrap()
    );
}

#[test]
fn rust_bitcoin_agrees_on_every_block_fixture() {
    use bitcoin::consensus::deserialize;

    for name in block_fixture_names() {
        let fixture = block_fixture(&name);
        let theirs: bitcoin::Block = deserialize(&fixture.bytes).unwrap();
        assert!(theirs.check_merkle_root(), "{name}");
        assert!(theirs.check_witness_commitment(), "{name}");

        let ours = parse_block(&fixture.bytes).unwrap();

        assert_eq!(
            ours.hash.to_string(),
            theirs.block_hash().to_string(),
            "{name}"
        );
        assert_eq!(ours.weight, theirs.weight().to_wu(), "{name}");
        let our_txs: Vec<bitcoin::Transaction> = ours
            .transactions
            .iter()
            .map(|tx| deserialize(tx).unwrap())
            .collect();
        assert_eq!(our_txs, theirs.txdata, "{name}");
    }
}

#[test]
fn regtest_block_splits_into_the_transactions_core_lists() {
    let fixture = block_fixture("regtest-block");

    let block = parse_block(&fixture.bytes).unwrap();

    let txids: Vec<String> = block
        .transactions
        .iter()
        .map(|tx| {
            bitcoin::consensus::deserialize::<bitcoin::Transaction>(tx)
                .unwrap()
                .compute_txid()
                .to_string()
        })
        .collect();
    let core: Vec<&str> = fixture.oracle["tx"]
        .as_array()
        .unwrap()
        .iter()
        .map(|txid| txid.as_str().unwrap())
        .collect();
    assert_eq!(txids, core);
}

#[test]
fn a_flipped_non_witness_byte_fails_the_merkle_check() {
    let mut bytes = block_fixture("regtest-block").bytes;
    // The last transaction's locktime: in its txid, not its witness.
    *bytes.last_mut().unwrap() ^= 1;

    let error = parse_block(&bytes).unwrap_err();

    assert!(
        matches!(error, BlockError::MerkleRootMismatch { .. }),
        "{error:?}"
    );
}

#[test]
fn a_block_without_transactions_is_rejected() {
    let mut bytes = block_fixture("regtest-block").bytes[..80].to_vec();
    bytes.push(0);

    assert_eq!(parse_block(&bytes), Err(BlockError::NoTransactions));
}

#[test]
fn a_mutated_merkle_tree_is_rejected_even_though_its_root_matches() {
    let fixture = block_fixture("regtest-block");
    let block = parse_block(&fixture.bytes).unwrap();
    assert_eq!(block.transactions.len(), 3);
    // Three transactions: the first level pairs the last with itself, so
    // appending a copy of it leaves the merkle root unchanged.
    let mut mutated = fixture.bytes[..80].to_vec();
    mutated.push(4);
    for tx in block.transactions.iter().chain(block.transactions.last()) {
        mutated.extend_from_slice(tx);
    }

    assert_eq!(parse_block(&mutated), Err(BlockError::MutatedMerkleTree));
}

/// Where `needle` starts in `haystack`; it must occur exactly once.
fn find_once(haystack: &[u8], needle: &[u8]) -> usize {
    let starts: Vec<usize> = haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(start, _)| start)
        .collect();
    assert_eq!(starts.len(), 1, "needle must occur exactly once");
    starts[0]
}

/// The first witness item of the regtest block's one non-coinbase segwit spend.
fn segwit_spend_signature(block: &bitcoin::Block) -> Vec<u8> {
    let spends: Vec<_> = block.txdata[1..]
        .iter()
        .filter(|tx| tx.input.iter().any(|input| !input.witness.is_empty()))
        .collect();
    assert_eq!(spends.len(), 1);
    spends[0].input[0].witness.nth(0).unwrap().to_vec()
}

#[test]
fn a_flipped_witness_byte_fails_the_witness_commitment() {
    let mut bytes = block_fixture("regtest-block").bytes;
    let theirs: bitcoin::Block = bitcoin::consensus::deserialize(&bytes).unwrap();
    let signature = segwit_spend_signature(&theirs);
    // A byte inside the signature's r value: witness data, so not in any txid.
    let start = find_once(&bytes, &signature);
    bytes[start + 10] ^= 1;

    let error = parse_block(&bytes).unwrap_err();

    assert!(
        matches!(error, BlockError::WitnessCommitmentMismatch { .. }),
        "{error:?}"
    );
}

/// The regtest block decoded by `rust-bitcoin`, changed by `edit`, given the
/// merkle root of its edited transactions, and serialized again: a block whose
/// txid tree is intact, so only the later checks can reject it.
fn edited_regtest_block(edit: impl FnOnce(&mut bitcoin::Block)) -> Vec<u8> {
    let mut block: bitcoin::Block =
        bitcoin::consensus::deserialize(&block_fixture("regtest-block").bytes).unwrap();
    edit(&mut block);
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    bitcoin::consensus::serialize(&block)
}

#[test]
fn witness_data_without_a_commitment_is_rejected() {
    let bytes = edited_regtest_block(|block| {
        let outputs = &mut block.txdata[0].output;
        let before = outputs.len();
        outputs.retain(|output| !output.script_pubkey.as_bytes().starts_with(&[0x6a, 0x24]));
        assert_eq!(outputs.len(), before - 1, "removed the commitment output");
    });

    assert_eq!(
        parse_block(&bytes),
        Err(BlockError::MissingWitnessCommitment)
    );
}

#[test]
fn a_coinbase_witness_that_is_not_one_32_byte_item_is_rejected() {
    let bytes = edited_regtest_block(|block| {
        block.txdata[0].input[0].witness.push([0u8; 32]);
    });

    assert_eq!(
        parse_block(&bytes),
        Err(BlockError::BadWitnessReservedValue)
    );
}

#[test]
fn bytes_after_the_last_transaction_are_rejected() {
    let mut bytes = block_fixture("regtest-block").bytes;
    let end = bytes.len();
    bytes.push(0);

    assert_eq!(
        parse_block(&bytes),
        Err(BlockError::Framing(ParseError::TrailingBytes {
            offset: end,
            remaining: 1
        }))
    );
}

#[test]
fn a_count_larger_than_the_transactions_present_is_truncated() {
    let mut bytes = block_fixture("regtest-block").bytes;
    assert_eq!(bytes[80], 3);
    bytes[80] = 4;
    let end = bytes.len();

    assert_eq!(
        parse_block(&bytes),
        Err(BlockError::Transaction {
            index: 3,
            offset: end,
            error: ParseError::Truncated {
                reading: "version",
                offset: 0
            }
        })
    );
}
