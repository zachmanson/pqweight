//! Fee calculation: a `sat/vB` rate applied to a **vsize**.
//!
//! Fee rates are parsed from their decimal string, not through a float, so a
//! rate like `1.5` multiplies exactly instead of picking up binary-fraction
//! rounding error.

/// A `sat/vB` fee rate, held as an exact fraction so multiplying it by a vsize
/// never loses precision the way a binary float would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeRate {
    /// The rate's decimal digits with the point removed, e.g. `1.5` becomes 15.
    numerator: u64,
    /// A power of ten: 1 for an integer rate, 10 for one decimal digit, and so on.
    denominator: u64,
}

/// Why a `--fee-rate` string could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeRateError {
    /// The rate parsed but was negative.
    Negative,
    /// The rate was not a plain decimal number.
    NotNumeric,
}

impl FeeRate {
    /// Parses a `sat/vB` rate from a decimal string such as `"1.5"` or `"20"`.
    ///
    /// # Errors
    ///
    /// Returns [`FeeRateError::NotNumeric`] if `input` is not a plain decimal
    /// number, or [`FeeRateError::Negative`] if it is negative.
    pub fn parse(input: &str) -> Result<Self, FeeRateError> {
        let input = input.strip_prefix('+').unwrap_or(input);
        if let Some(rest) = input.strip_prefix('-') {
            // A negative number is numeric, just out of range; check it parses
            // before reporting which error it is.
            parse_unsigned_decimal(rest).ok_or(FeeRateError::NotNumeric)?;
            return Err(FeeRateError::Negative);
        }
        let (numerator, denominator) =
            parse_unsigned_decimal(input).ok_or(FeeRateError::NotNumeric)?;
        Ok(Self {
            numerator,
            denominator,
        })
    }
}

/// Parses an unsigned plain decimal (`"20"`, `"1.5"`, `"0.25"`) into
/// `(numerator, denominator)`, e.g. `"1.5"` into `(15, 10)`. Returns `None` for
/// anything else: empty input, a sign, scientific notation, more than one point.
fn parse_unsigned_decimal(input: &str) -> Option<(u64, u64)> {
    if input.is_empty() {
        return None;
    }
    let (whole, frac) = match input.split_once('.') {
        // A point with nothing after it ("1.") is not a plain decimal number.
        Some((_, "")) => return None,
        Some((whole, frac)) => (whole, frac),
        None => (input, ""),
    };
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let denominator = 10u64.checked_pow(u32::try_from(frac.len()).ok()?)?;
    let frac_value: u64 = if frac.is_empty() {
        0
    } else {
        frac.parse().ok()?
    };
    let numerator = whole.checked_mul(denominator)?.checked_add(frac_value)?;
    Some((numerator, denominator))
}

/// The fee for `vsize` at `fee_rate`, in whole satoshis, rounded up.
///
/// # Panics
///
/// Never in practice: a `u64` vsize and fee rate cannot produce a fee that
/// overflows `u64`.
#[must_use]
pub fn fee(vsize: u64, fee_rate: FeeRate) -> u64 {
    let total = u128::from(vsize) * u128::from(fee_rate.numerator);
    let denominator = u128::from(fee_rate.denominator);
    let sats = total.div_ceil(denominator);
    u64::try_from(sats).expect("fee fits in a u64 for any realistic vsize and rate")
}
