# Write our own transaction parser and check it against two independent oracles

We parse raw transactions with our own code instead of using `rust-bitcoin` for weight. Every fixture is checked three ways: our parser, `rust-bitcoin` (dev-dependency only) and Bitcoin Core's reported weight and vsize. A disagreement means a bug in one of the three, and resolving it is the point. Wrapping `rust-bitcoin` would have been faster, but then "validated against Core" would only validate someone else's parser, and the project would not show that we understand the serialization format.
