#!/bin/sh -e

dir=$(cd ../rust-tls-api; pwd)

find . -name Cargo.toml | xargs gsed -E -i -e '
s;(tokio-)?tls-api([^ ]*) *= *(.*);\1tls-api\2 = { path = "'"$dir/\1impl\2"'", version = \3 };
s;impl";api";
s;tokio-api";tokio-tls-api";
'

# vim: set ts=4 sw=4 et:
