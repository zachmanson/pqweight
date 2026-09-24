Status: done

# Key exposure tagging and added weight by exposure (slice 7)

Blocked by: 06 (done)

## Problem Statement

After slice 6, 99.985% of the sample's Input weight is mapped, so more templates barely move the results. The spec has deferred Long/Short-exposure analysis since slice 1. The question this slice answers is: **if only the coins whose keys are already on-chain migrate first (the BIP-360 idea, P2TR first), how much block space does that cost compared with migrating everything?**

A tag on its own is just a label. The finding comes from splitting the migrated weight by tag.

## Solution

Tag every input with its **Key exposure** (glossary term, already in `CONTEXT.md`): where its public key sat before this spend, as seen from the spending side. Group `aggregate`'s per-input sums by tag and report the **Added weight** (glossary term, already in `CONTEXT.md`) for each group. Show the tag on each `migrate` input line too, and report the finding in the coverage report.

## Decisions (grilled 2026-09-23)

1. **Slice 7 is exposure tagging with split totals**, not a second coverage sample, multi_a leaves, `aggregate --json` or a P2PK template.
2. **The tag is fully determined by the Baseline spend type or Unmapped reason.** No new parsing, except for decision 5. Unmapped inputs are tagged too.

   | Key exposure | Baseline spend types | Unmapped reasons |
   |---|---|---|
   | Exposed in output | P2TR key-path, P2TR script-path single-key | P2TR script-path, P2TR key-path with annex, P2PK, bare multisig |
   | Hashed until spend | P2WPKH, P2SH-P2WPKH, P2PKH, P2WSH / P2SH-P2WSH / P2SH multisig, P2WSH / P2SH-P2WSH contract | P2WSH non-multisig, P2SH-wrapped segwit non-multisig, P2SH non-multisig |
   | No key | pay-to-anchor, coinbase | |
   | Undetermined | | legacy other, unknown |

   - P2TR script-path counts as Exposed in output: the output key is the internal key tweaked, so breaking it opens the key path whatever the leaves say.
3. **Name what was seen: Key exposure values are `Exposed in output`, `Hashed until spend`, `No key`, `Undetermined`.** Never "long-exposed" or "short-exposed". A Hashed until spend key may already be public through address reuse, which the spending side can't show. So the Exposed in output share is a **lower bound** on what's open to a Long-exposure attack, and the report says so.
4. **Split by input, not by transaction.** Use the Input weights `aggregate` already sums per breakdown row. The Added weight of a group is the block space migrating just that group adds, because outputs and transaction overhead don't change. The one exception is the 2-byte segwit marker and flag a legacy-only transaction gains. Exposed in output inputs are all segwit already, so it can't affect that row. No partial-migration mode for transactions.
5. **Split `LegacyOther` into two new Unmapped reasons**, both tagged Exposed in output, with no templates. Both apply only to an empty witness where the scriptSig matched no template and no earlier reason (today they fall through to `LegacyOther`, because `P2shNonMultisig` requires the last push not to be a signature):
   - **`P2pk`** (label `P2PK`): scriptSig is exactly one push, and it passes ticket 06's strict DER parse.
   - **`BareMultisig`** (label `bare multisig`): scriptSig is `OP_0` followed by 1 or more pushes, every one passing strict DER, and no trailing redeem script. `OP_0 <sig>` is bare 1-of-n multisig, not P2PK.
   - Everything else in `LegacyOther` stays there, Undetermined.
   - Why no P2PK template: it would need a new assumption about where the PQ key goes when the key sits in the output, and the sample has 0 such inputs.
6. **Derived, not stored.** Add `key_exposure()` to `BaselineSpendType` and to `UnmappedReason`, each an exhaustive `match` with no `_` arm, like `threshold()`. `InputResult` gets no new field: the tag is a pure function of data it already holds, and a stored copy could disagree. The exhaustive match makes the compiler force every future spend type or reason to pick a tag. A `KeyExposure` enum with the four values lives in `migration.rs`.
7. **The `aggregate` table: one row per Key exposure value**, after the existing breakdown. Columns:
   - mapped inputs, baseline Input weight, migrated Input weight;
   - **Added weight** and its share of the total Added weight;
   - Unmapped inputs and their baseline Input weight. They have no Added weight, so a row's Added weight is a lower bound whenever this column isn't zero.
   - No fee column: fees are rounded up per transaction, and splitting them by input would need a new rounding rule.
   - Rows with zero inputs still print (`0`), so the table always has four rows.
8. **Where the tag appears:**
   - the `aggregate` table (decision 7);
   - each input line of `migrate`, human (`key exposure: Exposed in output`) and `--json` (`"key_exposure": "Exposed in output"`, same labels as the human output, as reasons are today), for Mapped and Unmapped inputs;
   - the coverage report (Doc edits).
9. **No ADR.** The reuse blind spot is surprising, but cheap to change, so it fails the "hard to reverse" test. The glossary definition records it.

## Doc edits (part of this ticket)

- **`CONTEXT.md`**: already done during grilling (**Key exposure**, **Added weight**).
- **`spec.md`**: remove "Long-exposure and Short-exposure tagging or analysis" from Out of Scope and point to this ticket. Change the P2PK row of "Other Baseline spend types" to "Recognized as an Unmapped reason and tagged Exposed in output (slice 7); no template". Add the bare multisig recognition to the Bare multisig row.
- **Coverage report** (`docs/coverage/2026-09-sample.md`): new section "After slice 7: Key exposure (2026-09-23)", re-running the same sample:
  - the Key exposure table at ML-DSA-44, and the Added weight of the Exposed in output row at all three parameter sets;
  - the headline: migrating only Exposed in output inputs adds X WU out of the Y WU that migrating everything adds;
  - the lower-bound caveat from decision 3 (address reuse);
  - add the P2PK and bare multisig rules to the Assumptions list. From the spending side, a P2SH spend whose redeem script is itself a single DER-shaped push would be misfiled as P2PK.
- **`UnmappedReason::LegacyOther` doc comment**: no longer covers P2PK or bare multisig.

## Testing Decisions

- **`key_exposure()` unit tests:** one assertion per Baseline spend type and per Unmapped reason, matching the table in decision 2. A mismatch should name the variant.
- **Recognition unit tests (hand-built scriptSigs):**
  - one strict-DER push → `P2pk`;
  - `OP_0 <sig>` and `OP_0 <sig> <sig>` → `BareMultisig`;
  - one push that fails strict DER (such as `0x30` with inconsistent lengths) → `LegacyOther`;
  - `OP_0` followed by a non-signature push → `LegacyOther`;
  - a P2SH multisig scriptSig still maps to the slice 3 template, and a P2SH non-multisig one keeps its reason.
- **Real-byte fixtures from mainnet**, oracle values from `bitcoin-tx -json` as in slice 5: one early P2PK spend and one bare multisig spend. Both come out Unmapped with the new reason and Exposed in output.
- **Aggregate tests:** a small batch that mixes P2TR key-path, P2WPKH, pay-to-anchor and one Unmapped input. Check that each row's Added weight equals the sum of its inputs' (migrated − baseline) Input weight, that the Unmapped input shows up in its row's Unmapped column, and that all four rows print.
- **CLI tests:** `migrate` human and `--json` output include the tag on every input.
