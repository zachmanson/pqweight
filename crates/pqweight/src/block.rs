//! Block parsing and verification: a block is checked to be intact (its
//! transactions match the header's merkle root and, when it has witness data,
//! the coinbase's witness commitment) before anything is measured from it.

use std::ops::Range;

use sha2::{Digest, Sha256};

use crate::parser::{self, ParseError, ParsedTransaction, Reader};

const HEADER_LEN: usize = 80;

/// The consensus limit on **Block weight**, in WU.
pub const MAX_BLOCK_WEIGHT: u64 = 4_000_000;

/// BIP 141: a witness commitment output's scriptPubKey starts `OP_RETURN`, a
/// 36-byte push, then this 4-byte tag; the 32-byte commitment follows.
const COMMITMENT_PREFIX: [u8; 6] = [0x6a, 0x24, 0xaa, 0x21, 0xa9, 0xed];

/// A double-SHA256 hash as stored in serialized data. Displays reversed, the
/// way Bitcoin Core shows block hashes and txids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hash256(pub [u8; 32]);

impl std::fmt::Display for Hash256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0
            .iter()
            .rev()
            .try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// A block that passed every check in [`parse_block`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Double-SHA256 of the 80-byte header.
    pub hash: Hash256,
    /// The txid merkle root, from the header and checked against the
    /// transactions.
    pub merkle_root: Hash256,
    /// Each transaction's full serialization, in block order.
    pub transactions: Vec<Vec<u8>>,
    /// **Block weight**: 3 x stripped size + total size, header and
    /// transaction count included.
    pub weight: u64,
}

/// Why a byte string is not an intact block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockError {
    /// The input ended inside the 80-byte header.
    TruncatedHeader { len: usize },
    /// The transaction count is zero. Every block has at least its coinbase.
    NoTransactions,
    /// The transaction count could not be read, or bytes follow the last
    /// transaction. Offsets count from the start of the block.
    Framing(ParseError),
    /// Transaction `index` (0 is the coinbase), starting at block byte
    /// `offset`, did not parse. The error's offsets count from that start.
    Transaction {
        index: u64,
        offset: usize,
        error: ParseError,
    },
    /// The txid merkle root computed from the transactions is not the one in
    /// the header: some transaction is not the one the header commits to.
    MerkleRootMismatch { header: Hash256, computed: Hash256 },
    /// Two identical hashes sit side by side at some level of the txid tree
    /// (CVE-2012-2459): a different list of transactions with the same root,
    /// such as one with its last transaction repeated. Core rejects it.
    MutatedMerkleTree,
    /// Some transaction has witness data, but the coinbase has no witness
    /// commitment output, so nothing vouches for that data.
    MissingWitnessCommitment,
    /// The coinbase has a witness commitment, but its witness is not the
    /// single 32-byte witness reserved value the commitment is computed with.
    BadWitnessReservedValue,
    /// The coinbase's witness commitment does not match the one computed from
    /// the transactions' wtxids: some witness is not the one committed to.
    WitnessCommitmentMismatch {
        committed: Hash256,
        computed: Hash256,
    },
}

/// Parses a serialized block and verifies it is intact.
///
/// # Errors
///
/// Returns a [`BlockError`] naming the first check the block fails.
pub fn parse_block(bytes: &[u8]) -> Result<Block, BlockError> {
    let header = bytes
        .get(..HEADER_LEN)
        .ok_or(BlockError::TruncatedHeader { len: bytes.len() })?;
    let mut r = Reader::at(bytes, HEADER_LEN);
    let count = r
        .read_compact_size("transaction count")
        .map_err(BlockError::Framing)?;
    // Where the next transaction starts.
    let mut offset = r.offset();
    let mut stripped_size = offset as u64;
    let mut transactions = Vec::new();
    let mut txids = Vec::new();
    let mut wtxids = Vec::new();
    let mut any_witness = false;
    let mut coinbase = None;
    for index in 0..count {
        let tx =
            parser::parse_prefix(&bytes[offset..]).map_err(|error| BlockError::Transaction {
                index,
                offset,
                error,
            })?;
        stripped_size += tx.stripped_size;
        let raw = &bytes[offset..offset + tx.len];
        offset += raw.len();
        transactions.push(raw.to_vec());
        txids.push(txid(raw, tx.witness_range.clone()));
        any_witness |= tx.witness_range.is_some();
        if index == 0 {
            // BIP 141: the coinbase's wtxid counts as all zeros.
            wtxids.push(Hash256([0; 32]));
            coinbase = Some(tx);
        } else {
            wtxids.push(sha256d(&[raw]));
        }
    }
    if offset != bytes.len() {
        return Err(BlockError::Framing(ParseError::TrailingBytes {
            offset,
            remaining: bytes.len() - offset,
        }));
    }
    let Some(coinbase) = coinbase else {
        return Err(BlockError::NoTransactions);
    };
    let header_root = hash_at(header, 36);
    let (computed, mutated) = merkle_root(txids);
    if computed != header_root {
        return Err(BlockError::MerkleRootMismatch {
            header: header_root,
            computed,
        });
    }
    if mutated {
        return Err(BlockError::MutatedMerkleTree);
    }
    check_witness_commitment(&coinbase, wtxids, any_witness)?;
    Ok(Block {
        hash: sha256d(&[header]),
        merkle_root: header_root,
        transactions,
        weight: 3 * stripped_size + bytes.len() as u64,
    })
}

/// The 32 bytes of `bytes` starting at `start`, which the caller has checked
/// are there.
fn hash_at(bytes: &[u8], start: usize) -> Hash256 {
    let mut hash = [0; 32];
    hash.copy_from_slice(&bytes[start..start + 32]);
    Hash256(hash)
}

/// Double SHA-256 of the concatenation of `parts`.
fn sha256d(parts: &[&[u8]]) -> Hash256 {
    let mut first = Sha256::new();
    for part in parts {
        first.update(part);
    }
    Hash256(Sha256::digest(first.finalize()).into())
}

/// BIP 141: when the coinbase has a witness commitment (the last output that
/// looks like one), it must equal double SHA-256 of the wtxid merkle root and
/// the coinbase's witness reserved value. Without one, no transaction may
/// carry witness data.
fn check_witness_commitment(
    coinbase: &ParsedTransaction<'_>,
    wtxids: Vec<Hash256>,
    any_witness: bool,
) -> Result<(), BlockError> {
    let Some(script) = coinbase
        .output_scripts
        .iter()
        .rev()
        .find(|script| script.len() >= 38 && script.starts_with(&COMMITMENT_PREFIX))
    else {
        return if any_witness {
            Err(BlockError::MissingWitnessCommitment)
        } else {
            Ok(())
        };
    };
    let reserved_value = match coinbase
        .inputs
        .first()
        .and_then(|input| input.witness.as_deref())
    {
        Some([item]) if item.len() == 32 => *item,
        _ => return Err(BlockError::BadWitnessReservedValue),
    };
    let committed = hash_at(script, 6);
    let (wtxid_root, _) = merkle_root(wtxids);
    let computed = sha256d(&[&wtxid_root.0, reserved_value]);
    if computed != committed {
        return Err(BlockError::WitnessCommitmentMismatch {
            committed,
            computed,
        });
    }
    Ok(())
}

/// The txid: double SHA-256 of the transaction without its segwit marker,
/// flag and witness section.
fn txid(raw: &[u8], witness_range: Option<Range<usize>>) -> Hash256 {
    match witness_range {
        None => sha256d(&[raw]),
        Some(witness) => sha256d(&[&raw[..4], &raw[6..witness.start], &raw[witness.end..]]),
    }
}

/// The merkle root of a non-empty `level`: hash pairs level by level until one
/// is left, pairing an odd level's last hash with itself. Also reports whether
/// any real pair (not that self-pairing) held two identical hashes, the way
/// Core's `ComputeMerkleRoot` does.
fn merkle_root(mut level: Vec<Hash256>) -> (Hash256, bool) {
    let mut mutated = false;
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => {
                    mutated |= left == right;
                    sha256d(&[&left.0, &right.0])
                }
                [last] => sha256d(&[&last.0, &last.0]),
                _ => unreachable!("chunks(2) yields one or two hashes"),
            })
            .collect();
    }
    (level[0], mutated)
}

impl std::fmt::Display for BlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedHeader { len } => {
                write!(f, "block ends after {len} bytes, inside the 80-byte header")
            }
            Self::NoTransactions => write!(f, "block has no transactions"),
            Self::Framing(error) => write!(f, "{error}"),
            Self::Transaction {
                index,
                offset,
                error,
            } => write!(f, "transaction {index} (at block byte {offset}): {error}"),
            Self::MerkleRootMismatch { header, computed } => write!(
                f,
                "merkle root mismatch: header has {header}, transactions give {computed}"
            ),
            Self::MutatedMerkleTree => write!(
                f,
                "mutated merkle tree: identical adjacent hashes (CVE-2012-2459)"
            ),
            Self::MissingWitnessCommitment => write!(
                f,
                "a transaction has witness data but the coinbase has no witness commitment"
            ),
            Self::BadWitnessReservedValue => write!(
                f,
                "coinbase witness is not a single 32-byte witness reserved value"
            ),
            Self::WitnessCommitmentMismatch {
                committed,
                computed,
            } => write!(
                f,
                "witness commitment mismatch: coinbase commits to {committed}, transactions give {computed}"
            ),
        }
    }
}

impl std::error::Error for BlockError {}
