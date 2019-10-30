#!/bin/sh -e

export RUST_BACKTRACE=1

rustc --version

cargo test -- --test-threads=1
cargo test --benches
cargo run --manifest-path h2spec-test/Cargo.toml --bin the_test

# vim: set ts=4 sw=4 et:
