//! **Key exposure** of every Baseline spend type and Unmapped reason. Expected
//! values are the table in ticket 07, decision 2: where each shape's public key
//! sits before the spend.

use pqweight::{BaselineSpendType, KeyExposure, MultisigThreshold, UnmappedReason};

const TWO_OF_THREE: MultisigThreshold = MultisigThreshold { m: 2, n: 3 };

#[test]
fn each_baseline_spend_type_has_the_key_exposure_of_its_output() {
    use BaselineSpendType as T;
    use KeyExposure as E;
    let cases = [
        (T::P2trKeyPath, E::ExposedInOutput),
        (T::P2trScriptPathSingleKey, E::ExposedInOutput),
        (T::P2wpkh, E::HashedUntilSpend),
        (T::P2shP2wpkh, E::HashedUntilSpend),
        (T::P2pkh, E::HashedUntilSpend),
        (T::P2wshMultisig(TWO_OF_THREE), E::HashedUntilSpend),
        (T::P2shP2wshMultisig(TWO_OF_THREE), E::HashedUntilSpend),
        (T::P2shMultisig(TWO_OF_THREE), E::HashedUntilSpend),
        (T::P2wshContract, E::HashedUntilSpend),
        (T::P2shP2wshContract, E::HashedUntilSpend),
        (T::PayToAnchor, E::NoKey),
        (T::Coinbase, E::NoKey),
    ];
    for (spend_type, expected) in cases {
        assert_eq!(spend_type.key_exposure(), expected, "{spend_type:?}");
    }
}

#[test]
fn each_unmapped_reason_has_the_key_exposure_of_the_shape_it_names() {
    use KeyExposure as E;
    use UnmappedReason as R;
    let cases = [
        (R::P2trScriptPath, E::ExposedInOutput),
        (R::P2trKeyPathAnnex, E::ExposedInOutput),
        (R::P2pk, E::ExposedInOutput),
        (R::BareMultisig, E::ExposedInOutput),
        (R::P2wshNonMultisig, E::HashedUntilSpend),
        (R::P2shSegwitNonMultisig, E::HashedUntilSpend),
        (R::P2shNonMultisig, E::HashedUntilSpend),
        (R::LegacyOther, E::Undetermined),
        (R::Unknown, E::Undetermined),
    ];
    for (reason, expected) in cases {
        assert_eq!(reason.key_exposure(), expected, "{reason:?}");
    }
}
