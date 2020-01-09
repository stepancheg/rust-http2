#!/bin/sh -e

export RUST_BACKTRACE=1

rustc --version

if test "$ACTION" = "h2spec"; then
    ci/install-h2spec.sh
    export PATH="$PATH:$(pwd)"
    cargo run --manifest-path h2spec-test/Cargo.toml --bin the_test
else
    cargo test -- --test-threads=1
    cargo test --benches
fi

# vim: set ts=4 sw=4 et:
