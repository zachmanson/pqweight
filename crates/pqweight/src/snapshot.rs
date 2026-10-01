//! Streaming reader for Bitcoin Core's UTXO snapshot format (`dumptxoutset`,
//! the file `loadtxoutset` reads). Coins are read one at a time, so a snapshot
//! of any size is never held in memory (ADR 0003).
//!
//! Layout (Core v31: `node/utxo_snapshot.h`, `rpc/blockchain.cpp`
//! `WriteUTXOSnapshot`): a 51-byte header, then coins grouped by txid.

use std::fmt;
use std::io::{self, BufReader, Read};

/// The 5 bytes every snapshot starts with: `utxo` and `0xff`.
const SNAPSHOT_MAGIC: [u8; 5] = *b"utxo\xff";

/// The only snapshot format version Core v31 writes and reads.
const SNAPSHOT_VERSION: u16 = 2;

/// What the snapshot header says about the coins that follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotHeader {
    /// The network's message start bytes, such as `f9 be b4 d9` for mainnet.
    pub network_magic: [u8; 4],
    /// Hash of the block the snapshot was taken at, in serialized byte order
    /// (the reverse of how Core displays it).
    pub base_block_hash: [u8; 32],
    /// How many coins follow the header.
    pub coins_count: u64,
}

/// Core's `MAX_SCRIPT_SIZE`. A longer script is unspendable, so it is never
/// in the UTXO set; a snapshot claiming one is rejected rather than allocated.
const MAX_SCRIPT_SIZE: u64 = 10_000;

/// Compressed-script codes 0 to 5 are the special forms; a larger code is a
/// raw script of `code - 6` bytes (`compressor.h` `nSpecialScripts`).
const SPECIAL_SCRIPTS: u64 = 6;

/// One unspent output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coin {
    /// The creating transaction's id, in serialized byte order (the reverse of
    /// how Core displays it).
    pub txid: [u8; 32],
    pub vout: u32,
    /// Height of the block that created it.
    pub height: u32,
    /// Whether it is a coinbase output.
    pub coinbase: bool,
    /// Value in satoshis.
    pub value: u64,
    pub script: CoinScript,
}

/// A coin's scriptPubKey, in the form Core's script compressor stored it
/// (`compressor.cpp`). Only standard P2PKH, P2SH and valid-key P2PK are stored
/// compressed; every other script, including P2PK whose key is not on the
/// curve, is stored raw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoinScript {
    /// `OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG` (code 0).
    P2pkh([u8; 20]),
    /// `OP_HASH160 <20 bytes> OP_EQUAL` (code 1).
    P2sh([u8; 20]),
    /// `<33-byte key> OP_CHECKSIG` (codes 2 and 3, the key's first byte).
    P2pkCompressed([u8; 33]),
    /// `<65-byte key> OP_CHECKSIG` (codes 4 and 5), stored as the
    /// x-coordinate and y's parity. Rebuilding the key needs point
    /// decompression, which pqweight does not do (ADR 0003).
    P2pkUncompressed { x: [u8; 32], odd_y: bool },
    /// Any other script, byte for byte.
    Raw(Vec<u8>),
}

/// Why a snapshot could not be read.
#[derive(Debug)]
pub enum SnapshotError {
    /// The file does not start with `utxo` and `0xff`.
    BadMagic,
    /// A format version other than 2.
    UnsupportedVersion(u16),
    /// The file ended while reading `reading`.
    Truncated { reading: &'static str },
    /// A VARINT read for `reading` did not fit in 64 bits.
    VarIntOverflow { reading: &'static str },
    /// A coin's height, vout or amount is out of range.
    InvalidCoin { reading: &'static str },
    /// A raw script longer than Core's 10,000-byte limit.
    ScriptTooLong { len: u64 },
    /// Reading the file failed.
    Io(io::Error),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not a UTXO snapshot (bad magic bytes)"),
            Self::UnsupportedVersion(version) => {
                write!(
                    f,
                    "unsupported snapshot version {version}, expected {SNAPSHOT_VERSION}"
                )
            }
            Self::Truncated { reading } => write!(f, "snapshot ended early, reading {reading}"),
            Self::VarIntOverflow { reading } => write!(f, "VARINT too large, reading {reading}"),
            Self::InvalidCoin { reading } => write!(f, "coin {reading} out of range"),
            Self::ScriptTooLong { len } => {
                write!(
                    f,
                    "script of {len} bytes is over the {MAX_SCRIPT_SIZE}-byte limit"
                )
            }
            Self::Io(err) => write!(f, "could not read snapshot: {err}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// An open snapshot: its header, then its coins read on demand as an iterator.
/// After the first error the iterator ends.
pub struct Snapshot<R> {
    pub header: SnapshotHeader,
    reader: BufReader<R>,
    /// Coins still to read, from the header's count.
    coins_left: u64,
    /// The txid of the group being read and how many of its coins are left.
    group: Option<([u8; 32], u64)>,
    failed: bool,
}

impl<R: Read> Iterator for Snapshot<R> {
    type Item = Result<Coin, SnapshotError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.coins_left == 0 {
            return None;
        }
        let coin = self.read_coin();
        match coin {
            Ok(_) => self.coins_left -= 1,
            Err(_) => self.failed = true,
        }
        Some(coin)
    }
}

impl<R: Read> Snapshot<R> {
    /// The body is grouped by txid: the txid, a compact-size count, then that
    /// many coins, each a compact-size vout and a serialized `Coin`. There is
    /// no terminator; the header's count says when to stop.
    fn read_coin(&mut self) -> Result<Coin, SnapshotError> {
        let (txid, left_in_group) = match self.group {
            Some(group) if group.1 > 0 => group,
            _ => {
                let txid = read_array(&mut self.reader, "txid")?;
                (txid, read_compact_size(&mut self.reader, "coins per txid")?)
            }
        };
        if left_in_group == 0 {
            return Err(SnapshotError::InvalidCoin {
                reading: "coins per txid",
            });
        }
        self.group = Some((txid, left_in_group - 1));

        let vout = u32::try_from(read_compact_size(&mut self.reader, "vout")?)
            .map_err(|_| SnapshotError::InvalidCoin { reading: "vout" })?;
        // Coin::Serialize: VARINT(height * 2 + coinbase), then the compressed
        // amount and script (`coins.h`, `TxOutCompression`).
        let code = read_varint(&mut self.reader, "height and coinbase flag")?;
        let height = u32::try_from(code >> 1)
            .map_err(|_| SnapshotError::InvalidCoin { reading: "height" })?;
        let value = decompress_amount(read_varint(&mut self.reader, "amount")?)
            .ok_or(SnapshotError::InvalidCoin { reading: "amount" })?;
        let script = read_script(&mut self.reader)?;
        Ok(Coin {
            txid,
            vout,
            height,
            coinbase: code & 1 == 1,
            value,
            script,
        })
    }
}

/// Reads a snapshot's header from `reader`.
///
/// # Errors
///
/// Returns a [`SnapshotError`] if the header is not a version 2 snapshot header.
pub fn read_snapshot<R: Read>(reader: R) -> Result<Snapshot<R>, SnapshotError> {
    let mut reader = BufReader::new(reader);
    let magic: [u8; 5] = read_array(&mut reader, "magic")?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SnapshotError::BadMagic);
    }
    let version = u16::from_le_bytes(read_array(&mut reader, "version")?);
    if version != SNAPSHOT_VERSION {
        return Err(SnapshotError::UnsupportedVersion(version));
    }
    let header = SnapshotHeader {
        network_magic: read_array(&mut reader, "network magic")?,
        base_block_hash: read_array(&mut reader, "base block hash")?,
        coins_count: u64::from_le_bytes(read_array(&mut reader, "coin count")?),
    };
    Ok(Snapshot {
        coins_left: header.coins_count,
        header,
        reader,
        group: None,
        failed: false,
    })
}

fn read_array<const N: usize>(
    reader: &mut impl Read,
    reading: &'static str,
) -> Result<[u8; N], SnapshotError> {
    let mut bytes = [0; N];
    reader
        .read_exact(&mut bytes)
        .map_err(|err| eof_as_truncated(err, reading))?;
    Ok(bytes)
}

fn eof_as_truncated(err: io::Error, reading: &'static str) -> SnapshotError {
    match err.kind() {
        io::ErrorKind::UnexpectedEof => SnapshotError::Truncated { reading },
        _ => SnapshotError::Io(err),
    }
}

/// Reads a compressed script (`compressor.h` `ScriptCompression::Unser`): a
/// VARINT code, then 20 bytes (codes 0, 1), 32 bytes (codes 2 to 5) or a raw
/// script of `code - 6` bytes.
fn read_script(reader: &mut impl Read) -> Result<CoinScript, SnapshotError> {
    let code = read_varint(reader, "script code")?;
    Ok(match code {
        0 => CoinScript::P2pkh(read_array(reader, "P2PKH hash")?),
        1 => CoinScript::P2sh(read_array(reader, "P2SH hash")?),
        2 | 3 => {
            let x: [u8; 32] = read_array(reader, "P2PK key")?;
            let mut key = [0; 33];
            key[0] = u8::try_from(code).expect("code is 2 or 3");
            key[1..].copy_from_slice(&x);
            CoinScript::P2pkCompressed(key)
        }
        4 | 5 => CoinScript::P2pkUncompressed {
            x: read_array(reader, "P2PK key")?,
            odd_y: code == 5,
        },
        _ => {
            let len = code - SPECIAL_SCRIPTS;
            if len > MAX_SCRIPT_SIZE {
                return Err(SnapshotError::ScriptTooLong { len });
            }
            let mut script = vec![0; usize::try_from(len).expect("10,000 fits in usize")];
            reader
                .read_exact(&mut script)
                .map_err(|err| eof_as_truncated(err, "script"))?;
            CoinScript::Raw(script)
        }
    })
}

/// Reads a Bitcoin compact size (1, 3, 5 or 9 bytes, little-endian).
fn read_compact_size(reader: &mut impl Read, reading: &'static str) -> Result<u64, SnapshotError> {
    let [first] = read_array(reader, reading)?;
    Ok(match first {
        0xfd => u64::from(u16::from_le_bytes(read_array(reader, reading)?)),
        0xfe => u64::from(u32::from_le_bytes(read_array(reader, reading)?)),
        0xff => u64::from_le_bytes(read_array(reader, reading)?),
        small => u64::from(small),
    })
}

/// Reads one of Core's VARINTs (`serialize.h` `ReadVarInt`): base-128, most
/// significant group first, high bit set on every byte but the last. Unlike
/// LEB128, each continuation adds one, so every value has exactly one encoding
/// (`0x80 0x00` is 128, not 0).
fn read_varint(reader: &mut impl Read, reading: &'static str) -> Result<u64, SnapshotError> {
    let mut value: u64 = 0;
    loop {
        let [byte] = read_array(reader, reading)?;
        if value > u64::MAX >> 7 {
            return Err(SnapshotError::VarIntOverflow { reading });
        }
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        value = value
            .checked_add(1)
            .ok_or(SnapshotError::VarIntOverflow { reading })?;
    }
}

/// Undoes Core's `CompressAmount` (`compressor.cpp` `DecompressAmount`), which
/// stores trailing decimal zeros as an exponent. `None` if the result would not
/// fit in 64 bits, which a Core-written snapshot never contains.
fn decompress_amount(compressed: u64) -> Option<u64> {
    // compressed = 0, or 1 + 10 * (9 * n + d - 1) + e, or 1 + 10 * (n - 1) + 9
    if compressed == 0 {
        return Some(0);
    }
    let mut x = compressed - 1;
    let exponent = x % 10;
    x /= 10;
    let mantissa = if exponent < 9 {
        let last_digit = x % 9 + 1;
        (x / 9).checked_mul(10)?.checked_add(last_digit)?
    } else {
        x + 1
    };
    let exponent = u32::try_from(exponent).expect("a remainder of 10 fits in u32");
    mantissa.checked_mul(10u64.checked_pow(exponent)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table in the comment above `WriteVarInt` in Core's `serialize.h`.
    #[test]
    fn varint_reads_cores_documented_encodings() {
        let cases: [(&[u8], u64); 11] = [
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x7f], 127),
            (&[0x80, 0x00], 128),
            (&[0x80, 0x7f], 255),
            (&[0x81, 0x00], 256),
            (&[0xfe, 0x7f], 16383),
            (&[0xff, 0x00], 16384),
            (&[0xff, 0x7f], 16511),
            (&[0x82, 0xfe, 0x7f], 65535),
            (&[0x8e, 0xfe, 0xfe, 0xff, 0x00], 1 << 32),
        ];
        for (bytes, expected) in cases {
            let mut reader = bytes;
            assert_eq!(
                read_varint(&mut reader, "test").unwrap(),
                expected,
                "{bytes:02x?}"
            );
            assert!(reader.is_empty(), "{bytes:02x?} left bytes unread");
        }
    }

    #[test]
    fn varint_that_overflows_u64_is_an_error() {
        let mut reader: &[u8] = &[0xff; 11];
        assert!(matches!(
            read_varint(&mut reader, "test"),
            Err(SnapshotError::VarIntOverflow { .. })
        ));
    }

    #[test]
    fn varint_cut_short_is_truncated() {
        let mut reader: &[u8] = &[0x80];
        assert!(matches!(
            read_varint(&mut reader, "test"),
            Err(SnapshotError::Truncated { .. })
        ));
    }

    /// The `TestPair` cases in Core's `src/test/compress_tests.cpp`.
    #[test]
    fn amount_decompresses_cores_documented_pairs() {
        const COIN: u64 = 100_000_000;
        const CENT: u64 = 1_000_000;
        let cases = [
            (0x0, 0),
            (0x1, 1),
            (0x7, CENT),
            (0x9, COIN),
            (0x32, 50 * COIN),
            (0x0140_6f40, 21_000_000 * COIN),
        ];
        for (compressed, expected) in cases {
            assert_eq!(
                decompress_amount(compressed),
                Some(expected),
                "{compressed:#x}"
            );
        }
    }

    #[test]
    fn amount_that_overflows_u64_is_none() {
        assert_eq!(decompress_amount(u64::MAX), None);
    }
}
