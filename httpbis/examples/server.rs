use tls_api::TlsAcceptorBuilder as tls_api_TlsAcceptorBuilder;

use httpbis::ServerHandlerContext;
use httpbis::ServerResponse;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

struct ServiceImpl {
    counter: Arc<AtomicUsize>,
}

impl ServiceImpl {
    fn new() -> ServiceImpl {
        ServiceImpl {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl httpbis::ServerHandler for ServiceImpl {
    fn start_request(
        &self,
        _context: ServerHandlerContext,
        req: httpbis::ServerRequest,
        mut resp: ServerResponse,
    ) -> httpbis::Result<()> {
        println!("starting request: {:?}", req.headers);

        if req.headers.method() == "POST" {
            self.counter.fetch_add(1, Ordering::Relaxed);
            resp.send_redirect_302("/")?;
        } else {
            let mut resp_headers = httpbis::Headers::ok_200();
            resp_headers.add("content-type", "text/html; charset=utf-8");

            let page = format!(
                "
<html>
    <head>
        <title>httpbis demo</title>
    </head>
    <body>
        <h3>httpbis demo</h3>
        <div>Counter: {}</div>
        <div>
            <form method='POST' action='/inc'>
                <button type='submit'>Inc</button>
            </form>
        </div>
    </body>
</html>
        ",
                self.counter.load(Ordering::Relaxed)
            );

            resp.send_headers(resp_headers)?;
            resp.send_data_end_of_stream(page.into())?
        }

        Ok(())
    }
}

fn main() {
    let server_keys = &test_cert_gen::keys().server;
    let mut tls_acceptor = tls_api_openssl::TlsAcceptorBuilder::from_pkcs12(
        &server_keys.pkcs12,
        &server_keys.pkcs12_password,
    )
    .expect("acceptor builder");
    tls_acceptor
        .set_alpn_protocols(&[b"h2"])
        .expect("set_alpn_protocols");

    let mut conf = httpbis::ServerConf::new();
    conf.alpn = Some(httpbis::ServerAlpn::Require);
    let mut server = httpbis::ServerBuilder::new();
    server.set_port(8443);
    server.set_tls(tls_acceptor.build().expect("tls acceptor"));
    server.conf = conf;
    server
        .service
        .set_service("/", Arc::new(ServiceImpl::new()));
    let server = server.build().expect("server");

    println!("started server");
    println!(
        "check it at: https://localhost:{}/",
        server.local_addr().port().unwrap()
    );

    loop {
        thread::park();
    }
}
