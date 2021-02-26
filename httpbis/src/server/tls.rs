use std::sync::Arc;

use tls_api::TlsAcceptorBox;

pub enum ServerTlsOption {
    Plain,
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
