use std::sync::Arc;

use tls_api::TlsConnectorBox;

use crate::solicit::HttpScheme;

pub enum ClientTlsOption {
    Plain,
    Tls(String, Arc<TlsConnectorBox>), // domain, connector
}

impl Clone for ClientTlsOption {
    fn clone(&self) -> Self {
        match self {
            &ClientTlsOption::Plain => ClientTlsOption::Plain,
            &ClientTlsOption::Tls(ref d, ref c) => ClientTlsOption::Tls(d.clone(), c.clone()),
        }
    }
}

impl ClientTlsOption {
    pub fn http_scheme(&self) -> HttpScheme {
        match self {
            &ClientTlsOption::Plain => HttpScheme::Http,
            &ClientTlsOption::Tls(..) => HttpScheme::Https,
        }
    }
}
