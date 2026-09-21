Status: resolved

# Weight computation validated against the Oracle

## Problem Statement

I want to know exactly how much block space a Bitcoin transaction uses, and I want to trust that number. Today the repo computes nothing. Existing tools either wrap someone else's parser or report sizes without showing they agree with Bitcoin Core. Without a weight calculator that is independently validated, any later model of post-quantum signature costs would be built on numbers I cannot defend.

## Solution

A library function takes a raw transaction and returns its **Weight**, **virtual size (vsize)**, stripped size and total size. The result is checked against three independent sources on every committed **Fixture**: our own parser, `rust-bitcoin` (dev-dependency only) and the **Oracle** (Bitcoin Core's reported `weight` and `vsize`). A thin CLI command exposes the same function for a raw transaction in hex. Fixture Oracle values are recorded once by a committed script run against a local Bitcoin Core node, so CI never needs a node.

## User Stories

1. As a developer, I want to pass raw transaction bytes to a library function and get back the weight, so that I can use pqweight as a dependency.
2. As a developer, I want the result to include vsize, stripped size and total size, so that I can see how weight was derived.
3. As a developer, I want vsize computed as weight divided by 4 rounded up, so that it matches the unit fee rates are quoted in.
4. As a developer, I want weight computed as 3 × stripped size + total size, so that it matches the consensus definition.
5. As a user, I want legacy transactions with no witness data to have weight equal to 4 × size, so that the witness discount is only applied where it exists.
6. As a user, I want native segwit (P2WPKH) transactions measured correctly, so that the most common modern spend type is trustworthy.
7. As a user, I want wrapped segwit (P2SH-P2WPKH) transactions measured correctly, so that the redeem script in the scriptSig is counted at full weight.
8. As a user, I want P2WSH multisig transactions measured correctly, so that spends with many witness items are covered.
9. As a user, I want taproot key-path spends measured correctly, so that the baseline for the long-exposure discussion is right.
10. As a user, I want taproot script-path spends measured correctly, so that control blocks and scripts in the witness are counted.
11. As a user, I want a taproot annex, when present, counted in the witness, so that this edge case does not skew results.
12. As a user, I want the segwit marker and flag bytes counted as witness bytes (1 weight unit each), so that I match Core's accounting.
13. As a user, I want variable-length integers (compact size) decoded correctly at every width boundary, so that large input and output counts and long scripts parse properly.
14. As a user, I want a transaction that uses the segwit marker but has empty witnesses for all inputs to be rejected the way Core rejects it, so that malformed data is not silently accepted.
15. As a user, I want a clear error when the input is truncated, so that I know the data is bad and not that the weight is small.
16. As a user, I want a clear error when there are trailing bytes after a complete transaction, so that concatenated or corrupted data is not silently accepted.
17. As a user, I want a clear error for invalid hex in the CLI, so that I can tell a typo from a parse failure.
18. As a user, I want parse errors to say what was being read and at what offset, so that I can debug bad input.
19. As a developer, I want the parser to never panic on arbitrary bytes, so that the library is safe to call on untrusted input.
20. As a developer, I want the parser to avoid allocating based on unchecked length fields, so that a hostile input cannot exhaust memory.
21. As a maintainer, I want every Fixture stored as raw transaction hex plus the Oracle's weight and vsize, so that expected values do not come from our own code.
22. As a maintainer, I want the script that records Oracle values committed alongside the Fixtures, so that anyone can re-run it and see where the numbers came from.
23. As a maintainer, I want the recording script to record the Bitcoin Core version it used, so that Oracle provenance is traceable.
24. As a maintainer, I want one test that walks every Fixture and checks all three sources agree, so that a disagreement fails loudly and names the Fixture.
25. As a maintainer, I want a failure message that shows all three values side by side, so that I can tell which of the three is wrong.
26. As a maintainer, I want adding a new Fixture to need only dropping files into a directory, so that coverage grows without editing test code.
27. As a maintainer, I want CI to run the Fixture test without a Bitcoin Core node, so that CI stays fast and hermetic.
28. As a CLI user, I want to run `pqweight weight <hex>` and see weight and vsize, so that I can check a single transaction quickly.
29. As a CLI user, I want the CLI to read hex from stdin when no argument is given, so that I can pipe from `bitcoin-cli getrawtransaction`.
30. As a CLI user, I want a non-zero exit code and an error on stderr for bad input, so that I can use it in scripts.
31. As a CLI user, I want machine-readable output available, so that I can feed results into other tools.
32. As a learner, I want the code and docs to use the terms in CONTEXT.md consistently, so that I can follow the domain from code to docs.

## Implementation Decisions

- Two crates stay as they are: a library crate holding all logic, and a CLI crate that only parses arguments, calls the library and prints.
- The library's public surface for this feature is one function taking raw transaction bytes and returning a result struct or a parse error. The struct carries weight, vsize, stripped size and total size, all as unsigned integers.
- The transaction parser is written in this repo and kept private to the library. It decodes version, optional segwit marker and flag, inputs, outputs, per-input witness stacks and locktime. It only needs to measure, so it does not have to interpret scripts or validate signatures. This follows ADR-0001.
- Stripped size is the serialization without marker, flag and witness. Total size is the full serialization. Weight is `3 × stripped + total`, and vsize is `ceil(weight / 4)`.
- Parsing must consume the input exactly. Truncation and trailing bytes are distinct errors, and errors carry a description of what was being read and the byte offset.
- Length-prefixed fields are checked against the remaining input before any allocation or skip. The parser must not panic on any input.
- The error type is a public enum so the CLI and callers can match on it.
- The CLI command `weight` accepts one hex argument, or reads hex from stdin if none is given, and trims surrounding whitespace. Default output is human-readable. A flag selects JSON output. Errors go to stderr with a non-zero exit code.
- Fixtures live in a single directory. Each Fixture is raw transaction hex plus a JSON file holding the Oracle's weight and vsize, the Core version and the script that recorded them. The recording script queries a local Bitcoin Core node and is not run in CI.
- The first Fixture set covers: legacy P2PKH, P2WPKH, P2SH-P2WPKH, P2WSH multisig, P2TR key-path and P2TR script-path, including at least one with an annex if a real one can be found, and at least one large transaction whose counts need a multi-byte compact size.
- `rust-bitcoin` is added as a dev-dependency only, and is used only in the Fixture test as the second independent check. It never appears in the library's public API or normal dependencies.
- No consensus validation is performed. A transaction that parses and measures is reported, even if it would be invalid under consensus rules.
- Where Bitcoin Core rejects a serialization outright, so do we. Two such rules are decided: compact sizes must use the shortest encoding that fits their value (a non-canonical one is an error, `NonCanonicalCompactSize`), and the segwit marker must be followed by the flag `0x01`. Marker plus flag `00 00`, which Core parses as an empty transaction, is rejected as an unknown flag. An empty transaction is not something we measure.

## Testing Decisions

- A good test checks external behavior only: bytes in, weight and vsize out, or a specific error out. It does not test parser internals, so the parser can be restructured without touching tests.
- The primary seam is the library's public weight function. One parameterized integration test walks every Fixture and asserts that our result, `rust-bitcoin`'s weight and the Oracle's recorded weight and vsize all agree. The failure message names the Fixture and prints all three.
- Error behavior (truncation at several offsets, trailing bytes, segwit marker with all-empty witnesses, oversized length fields) is tested through the same public function, using small hand-built byte strings. These cases do not need Oracle values, since they are about rejecting bad input.
- A property-style test feeds arbitrary and truncated-valid bytes to the function and asserts it returns a result or an error without panicking.
- The CLI has one smoke test that runs the binary on a known Fixture, checks the output and exit code, and runs it once with bad input. It has no logic tests, because the CLI holds no logic.
- Prior art: the repo has only a trivial unit test on the version function. There is no earlier Fixture or integration test to copy, so this feature sets the pattern.
- CI runs `cargo test` on the workspace with no Bitcoin Core node available.

## Out of Scope

- Migration templates, PQ signature sizes and any modelling of post-quantum weight or fees.
- Reporting **Unmapped** spends and aggregating over blocks or many transactions.
- Script interpretation, signature checking and any consensus validation.
- Fee-rate calculation and the 400,000-weight relay-limit check from ADR-0002.
- Fetching transactions from a node or the network at runtime.
- Long-exposure and short-exposure attack analysis.

## Further Notes

- Because the recording script needs a local Bitcoin Core node, someone has to run it once by hand to produce the first Fixtures. Fixtures must be committed, since CI cannot regenerate them.
- If the three sources disagree on a Fixture, ADR-0001 says resolving the disagreement is the point of the project, so do not paper over it by adjusting expected values.
- Use the CONTEXT.md terms (Weight, vsize, Witness discount, Oracle, Fixture) in code, comments and docs. Avoid "size" and "byte size" for weight.

## Comments

**Built and merged (PR #1).** Everything in the user stories is implemented and covered by tests. Two deviations from the plan above:

- The first Fixture set was recorded on regtest, not from mainnet. It covers P2PKH, P2WPKH, P2SH-P2WPKH, P2WSH 2-of-3 multisig, P2TR key-path and P2TR script-path, plus a 260-output transaction for the multi-byte output count. The annex Fixture (`p2tr-keypath-annex`) is a real key-path spend with an annex appended after signing, so only its weight is meaningful. Replacing the annex, script-path and many-outputs Fixtures with real mainnet transactions would be stronger evidence.
- The Fixture test compares three sources (ours, rust-bitcoin, the Oracle), as specified. The CLI holds no logic beyond argument handling, hex decoding and printing.

**Decided after the merge:** reject non-canonical compact sizes, and keep rejecting the empty transaction (marker plus flag `00 00`). Both are now covered by tests at the public weight function.
