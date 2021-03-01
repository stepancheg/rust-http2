use std::sync::Arc;

use tls_api::TlsAcceptorBox;

/// Server TLS configuration.
pub enum ServerTlsOption {
    /// Non-TLS server.
    Plain,
    /// TLS server configuration.
    Tls(Arc<TlsAcceptorBox>),
}

impl Clone for ServerTlsOption {
    fn clone(&self) -> Self {
        match self {
            &ServerTlsOption::Plain => ServerTlsOption::Plain,
            &ServerTlsOption::Tls(ref a) => ServerTlsOption::Tls(a.clone()),
        }
    }
}
