# rust-http2

<!-- https://travis-ci.org/stepancheg/rust-http2.png -->
[![Build Status](https://img.shields.io/travis/stepancheg/rust-http2.svg)](https://travis-ci.org/stepancheg/rust-http2)
[![License](https://img.shields.io/crates/l/httpbis.svg)](https://github.com/stepancheg/rust-http2/blob/master/LICENSE.txt)
[![crates.io](https://img.shields.io/crates/v/httpbis.svg)](https://crates.io/crates/httpbis) 

HTTP/2 client and server implementation in Rust based on tokio.

Currently it is used as base for implementation of [grpc-rust](https://github.com/stepancheg/grpc-rust).

## Example server

Checkout the source code and execute command:

```
cargo run --example server
```

Server will be available on https://localhost:8443/.
You need any modern browser with HTTP/2 support to open the page (e. g. Firefox, Safari, Chrome).

Server only works over HTTP/2, if browser doesn't send HTTP/2 preface, server closes the connection.
