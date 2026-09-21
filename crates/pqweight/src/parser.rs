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
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn skip(&mut self, len: u64, reading: &'static str) -> Result<(), ParseError> {
        let remaining = self.bytes.len() - self.offset;
        match usize::try_from(len) {
            Ok(len) if len <= remaining => {
                self.offset += len;
                Ok(())
            }
            _ => Err(ParseError::Truncated {
                reading,
                offset: self.offset,
            }),
        }
    }

    /// Reads a variable-length integer (compact size).
    fn read_compact_size(&mut self, reading: &'static str) -> Result<u64, ParseError> {
        let first = self.read_u8(reading)?;
        if first == 0xfd {
            let start = self.offset;
            self.skip(2, reading)?;
            let raw = &self.bytes[start..start + 2];
            return Ok(u64::from(u16::from_le_bytes([raw[0], raw[1]])));
        }
        if first == 0xfe {
            let start = self.offset;
            self.skip(4, reading)?;
            let raw = &self.bytes[start..start + 4];
            return Ok(u64::from(u32::from_le_bytes([
                raw[0], raw[1], raw[2], raw[3],
            ])));
        }
        if first == 0xff {
            let start = self.offset;
            self.skip(8, reading)?;
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&self.bytes[start..start + 8]);
            return Ok(u64::from_le_bytes(raw));
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

/// Returns the stripped size: the serialization without marker, flag and witnesses.
pub(crate) fn measure(bytes: &[u8]) -> Result<u64, ParseError> {
    let mut r = Reader { bytes, offset: 0 };
    r.skip(4, "version")?;
    let mut inputs = r.read_compact_size("input count")?;
    // An input count of zero is the segwit marker; a flag byte follows it.
    let segwit = inputs == 0;
    if segwit {
        r.skip(1, "segwit flag")?;
        inputs = r.read_compact_size("input count")?;
    }
    for _ in 0..inputs {
        r.skip(36, "previous output")?;
        let script_len = r.read_compact_size("scriptSig length")?;
        r.skip(script_len, "scriptSig")?;
        r.skip(4, "sequence")?;
    }
    let outputs = r.read_compact_size("output count")?;
    for _ in 0..outputs {
        r.skip(8, "output value")?;
        let script_len = r.read_compact_size("scriptPubKey length")?;
        r.skip(script_len, "scriptPubKey")?;
    }
    let mut discounted = 0;
    if segwit {
        let witness_start = r.offset;
        let mut any_witness = false;
        for _ in 0..inputs {
            let items = r.read_compact_size("witness item count")?;
            any_witness |= items > 0;
            for _ in 0..items {
                let item_len = r.read_compact_size("witness item length")?;
                r.skip(item_len, "witness item")?;
            }
        }
        if !any_witness {
            return Err(ParseError::EmptyWitness {
                offset: witness_start,
            });
        }
        // Marker and flag, plus everything just read.
        discounted = 2 + (r.offset - witness_start);
    }
    r.skip(4, "locktime")?;
    if r.offset != bytes.len() {
        return Err(ParseError::TrailingBytes {
            offset: r.offset,
            remaining: bytes.len() - r.offset,
        });
    }
    Ok((r.offset - discounted) as u64)
}
