use std::sync::Arc;

use tls_api::TlsConnectorBox;

use crate::solicit::HttpScheme;

/// Client TLS setup.
pub enum ClientTlsOption {
    /// Non-TLS connection.
    Plain,
    /// Connection with configured TLS connector.
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
    /// HTTP scheme for the connector.
    pub(crate) fn http_scheme(&self) -> HttpScheme {
        match self {
            &ClientTlsOption::Plain => HttpScheme::Http,
            &ClientTlsOption::Tls(..) => HttpScheme::Https,
        }
    }
}
