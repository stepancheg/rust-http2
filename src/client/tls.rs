use std::sync::Arc;

use tls_api::TlsConnector;

use solicit::HttpScheme;

pub enum ClientTlsOption<C: TlsConnector> {
    Plain,
    Tls(String, Arc<C>), // domain
}

impl<C: TlsConnector> Clone for ClientTlsOption<C> {
    fn clone(&self) -> Self {
        match self {
            &ClientTlsOption::Plain => ClientTlsOption::Plain,
            &ClientTlsOption::Tls(ref d, ref c) => ClientTlsOption::Tls(d.clone(), c.clone()),
        }
    }
}

impl<C: TlsConnector> ClientTlsOption<C> {
    pub fn http_scheme(&self) -> HttpScheme {
        match self {
            &ClientTlsOption::Plain => HttpScheme::Http,
            &ClientTlsOption::Tls(..) => HttpScheme::Https,
        }
    }
}
