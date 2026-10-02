//! Private transaction parser. It only measures; it does not interpret scripts.

/// Why a byte string could not be measured as a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The input ended while reading `reading`, at byte `offset`.
    Truncated {
        reading: &'static str,
        offset: usize,
    },
    /// A complete transaction ended at `offset`, but `remaining` bytes follow it.
    TrailingBytes { offset: usize, remaining: usize },
    /// The segwit marker was present but every input's witness was empty, which
    /// Bitcoin Core rejects. `offset` is where the witness section starts.
    EmptyWitness { offset: usize },
    /// The byte after the segwit marker was not `0x01`, the only flag Bitcoin Core
    /// accepts for a transaction with witness data.
    UnknownSegwitFlag { flag: u8, offset: usize },
    /// A compact size (count or length) used a wider encoding than its value needs,
    /// which Bitcoin Core rejects. `offset` is where the compact size begins.
    NonCanonicalCompactSize {
        reading: &'static str,
        offset: usize,
    },
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    /// A reader positioned at `offset`, which counts from the start of `bytes`.
    pub(crate) fn at(bytes: &'a [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }

    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    fn skip(&mut self, len: u64, reading: &'static str) -> Result<(), ParseError> {
        self.take(len, reading).map(|_| ())
    }

    /// Reads the next `len` bytes and returns them.
    fn take(&mut self, len: u64, reading: &'static str) -> Result<&'a [u8], ParseError> {
        let remaining = self.bytes.len() - self.offset;
        match usize::try_from(len) {
            Ok(len) if len <= remaining => {
                let taken = &self.bytes[self.offset..self.offset + len];
                self.offset += len;
                Ok(taken)
            }
            _ => Err(ParseError::Truncated {
                reading,
                offset: self.offset,
            }),
        }
    }

    /// Reads a variable-length integer (compact size).
    pub(crate) fn read_compact_size(&mut self, reading: &'static str) -> Result<u64, ParseError> {
        let field_offset = self.offset;
        let first = self.read_u8(reading)?;
        if first == 0xfd {
            let start = self.offset;
            self.skip(2, reading)?;
            let raw = &self.bytes[start..start + 2];
            let value = u64::from(u16::from_le_bytes([raw[0], raw[1]]));
            // Values below 0xFD must use the one-byte form; Core rejects the wider one.
            if value < 0xfd {
                return Err(ParseError::NonCanonicalCompactSize {
                    reading,
                    offset: field_offset,
                });
            }
            return Ok(value);
        }
        if first == 0xfe {
            let start = self.offset;
            self.skip(4, reading)?;
            let raw = &self.bytes[start..start + 4];
            let value = u64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]));
            if value < 0x1_0000 {
                return Err(ParseError::NonCanonicalCompactSize {
                    reading,
                    offset: field_offset,
                });
            }
            return Ok(value);
        }
        if first == 0xff {
            let start = self.offset;
            self.skip(8, reading)?;
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&self.bytes[start..start + 8]);
            let value = u64::from_le_bytes(raw);
            if value < 0x1_0000_0000 {
                return Err(ParseError::NonCanonicalCompactSize {
                    reading,
                    offset: field_offset,
                });
            }
            return Ok(value);
        }
        Ok(u64::from(first))
    }

    fn read_u8(&mut self, reading: &'static str) -> Result<u8, ParseError> {
        let byte = *self.bytes.get(self.offset).ok_or(ParseError::Truncated {
            reading,
            offset: self.offset,
        })?;
        self.offset += 1;
        Ok(byte)
    }
}

/// One input's spending data: what its scriptSig and witness contain.
pub(crate) struct ParsedInput<'a> {
    /// The 36-byte outpoint: previous txid, then its output index (little-endian).
    pub previous_output: &'a [u8],
    pub script_sig: &'a [u8],
    /// `None` when the transaction has no witness section at all (a legacy,
    /// non-segwit transaction). `Some` when it does, even if this particular
    /// input's own witness is empty.
    pub witness: Option<Vec<&'a [u8]>>,
}

/// A transaction split into the parts that migration needs.
pub(crate) struct ParsedTransaction<'a> {
    /// Full serialized length, including any witness data.
    pub len: usize,
    /// Where the witness section sits, for a transaction that has one. The
    /// segwit marker and flag are always bytes 4 and 5.
    pub witness_range: Option<std::ops::Range<usize>>,
    /// Serialization without marker, flag and witnesses.
    pub stripped_size: u64,
    pub inputs: Vec<ParsedInput<'a>>,
    /// Each output's scriptPubKey.
    pub output_scripts: Vec<&'a [u8]>,
}

/// Returns the stripped size: the serialization without marker, flag and witnesses.
pub(crate) fn measure(bytes: &[u8]) -> Result<u64, ParseError> {
    parse(bytes).map(|tx| tx.stripped_size)
}

pub(crate) fn parse(bytes: &[u8]) -> Result<ParsedTransaction<'_>, ParseError> {
    let tx = parse_prefix(bytes)?;
    if tx.len != bytes.len() {
        return Err(ParseError::TrailingBytes {
            offset: tx.len,
            remaining: bytes.len() - tx.len,
        });
    }
    Ok(tx)
}

/// Parses the transaction at the start of `bytes`, which may continue past
/// its end (as in a block). Offsets in errors count from the start of `bytes`.
pub(crate) fn parse_prefix(bytes: &[u8]) -> Result<ParsedTransaction<'_>, ParseError> {
    let mut r = Reader::at(bytes, 0);
    r.skip(4, "version")?;
    let mut inputs = r.read_compact_size("input count")?;
    // An input count of zero is the segwit marker; a flag byte follows it.
    let segwit = inputs == 0;
    if segwit {
        let flag_offset = r.offset;
        let flag = r.read_u8("segwit flag")?;
        if flag != 1 {
            return Err(ParseError::UnknownSegwitFlag {
                flag,
                offset: flag_offset,
            });
        }
        inputs = r.read_compact_size("input count")?;
    }
    let mut parsed_inputs = Vec::new();
    for _ in 0..inputs {
        let previous_output = r.take(36, "previous output")?;
        let script_len = r.read_compact_size("scriptSig length")?;
        let script_sig = r.take(script_len, "scriptSig")?;
        r.skip(4, "sequence")?;
        parsed_inputs.push(ParsedInput {
            previous_output,
            script_sig,
            witness: None,
        });
    }
    let outputs = r.read_compact_size("output count")?;
    let mut output_scripts = Vec::new();
    for _ in 0..outputs {
        r.skip(8, "output value")?;
        let script_len = r.read_compact_size("scriptPubKey length")?;
        output_scripts.push(r.take(script_len, "scriptPubKey")?);
    }
    let mut discounted = 0;
    let mut witness_range = None;
    if segwit {
        let witness_start = r.offset;
        let mut any_witness = false;
        for input in &mut parsed_inputs {
            let items = r.read_compact_size("witness item count")?;
            any_witness |= items > 0;
            let mut witness = Vec::new();
            for _ in 0..items {
                let item_len = r.read_compact_size("witness item length")?;
                witness.push(r.take(item_len, "witness item")?);
            }
            input.witness = Some(witness);
        }
        if !any_witness {
            return Err(ParseError::EmptyWitness {
                offset: witness_start,
            });
        }
        // Marker and flag, plus everything just read.
        discounted = 2 + (r.offset - witness_start);
        witness_range = Some(witness_start..r.offset);
    }
    r.skip(4, "locktime")?;
    Ok(ParsedTransaction {
        len: r.offset,
        witness_range,
        stripped_size: (r.offset - discounted) as u64,
        inputs: parsed_inputs,
        output_scripts,
    })
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { reading, offset } => {
                write!(
                    f,
                    "unexpected end of input while reading {reading} at byte {offset}"
                )
            }
            Self::TrailingBytes { offset, remaining } => {
                write!(
                    f,
                    "{remaining} unexpected byte(s) after the end of the transaction at byte {offset}"
                )
            }
            Self::EmptyWitness { offset } => write!(
                f,
                "segwit marker present but every witness is empty (witness section starts at byte {offset})"
            ),
            Self::UnknownSegwitFlag { flag, offset } => write!(
                f,
                "unknown segwit flag 0x{flag:02x} at byte {offset}, expected 0x01"
            ),
            Self::NonCanonicalCompactSize { reading, offset } => write!(
                f,
                "non-canonical compact size while reading {reading} at byte {offset}: a shorter encoding fits the value"
            ),
        }
    }
}

impl std::error::Error for ParseError {}
