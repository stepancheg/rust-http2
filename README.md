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

## Example client:

```
cargo run --example client https://google.com/
```

Result is:

```
:status: 302
cache-control: private
content-type: text/html; charset=UTF-8
referrer-policy: no-referrer
location: https://www.google.ru/?gfe_rd=cr&ei=mZQ4WZfaGISDZOzOktgO
content-length: 257
date: Thu, 08 Jun 2017 00:04:41 GMT
alt-svc: quic=":443"; ma=2592000; v="38,37,36,35"

<HTML><HEAD><meta http-equiv="content-type" content="text/html;charset=utf-8">
<TITLE>302 Moved</TITLE></HEAD><BODY>
<H1>302 Moved</H1>
The document has moved
<A HREF="https://www.google.ru/?gfe_rd=cr&amp;ei=mZQ4WZfaGISDZOzOktgO">here</A>.
</BODY></HTML>
```
