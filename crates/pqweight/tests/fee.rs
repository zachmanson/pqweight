//! Behavior of the public fee function: a `sat/vB` rate and a vsize in, a whole
//! satoshi fee out, rounded up.

use pqweight::{FeeRate, FeeRateError, fee};

#[test]
fn a_whole_sat_per_vbyte_rate_multiplies_exactly() {
    let rate = FeeRate::parse("2").expect("valid rate");

    assert_eq!(fee(100, rate), 200);
}

#[test]
fn a_fractional_rate_is_rounded_up_to_the_next_satoshi() {
    let rate = FeeRate::parse("1.5").expect("valid rate");

    // 1.5 sat/vB x 101 vB = 151.5 sat, rounded up to 152.
    assert_eq!(fee(101, rate), 152);

    // 1.5 sat/vB x 100 vB = 150 sat exactly: rounding up must not add a satoshi
    // it doesn't owe.
    assert_eq!(fee(100, rate), 150);
}

#[test]
fn a_negative_rate_is_rejected() {
    let err = FeeRate::parse("-1.5").expect_err("negative rate must be rejected");

    assert_eq!(err, FeeRateError::Negative);
}

#[test]
fn a_non_numeric_rate_is_rejected() {
    let err = FeeRate::parse("abc").expect_err("non-numeric rate must be rejected");

    assert_eq!(err, FeeRateError::NotNumeric);
}

#[test]
fn malformed_decimal_shapes_are_rejected_as_non_numeric() {
    for input in ["", "1.2.3", "1.", ".", "1,5", "1e5", "--1"] {
        let err = FeeRate::parse(input).expect_err(input);
        assert_eq!(err, FeeRateError::NotNumeric, "{input:?}");
    }
}

#[test]
fn a_leading_point_and_a_leading_plus_are_accepted() {
    assert_eq!(FeeRate::parse(".5"), FeeRate::parse("0.5"));
    assert_eq!(FeeRate::parse("+2"), FeeRate::parse("2"));
}

#[test]
fn a_zero_rate_produces_a_zero_fee() {
    let rate = FeeRate::parse("0").expect("valid rate");

    assert_eq!(fee(1000, rate), 0);
}

#[test]
fn a_fee_rate_displays_as_a_plain_decimal_keeping_its_given_digits() {
    let shown = |input: &str| FeeRate::parse(input).expect("valid rate").to_string();

    assert_eq!(shown("20"), "20");
    assert_eq!(shown("1.5"), "1.5");
    assert_eq!(shown("1.50"), "1.50");
    assert_eq!(shown("0.05"), "0.05");
    // Forms `parse` accepts but a JSON number doesn't.
    assert_eq!(shown(".5"), "0.5");
    assert_eq!(shown("+007"), "7");
}
