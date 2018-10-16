use std::sync::Arc;

use tls_api::TlsAcceptor;

pub enum ServerTlsOption<A: TlsAcceptor> {
    Plain,
    Tls(Arc<A>),
}

impl<A: TlsAcceptor> Clone for ServerTlsOption<A> {
    fn clone(&self) -> Self {
        match self {
            &ServerTlsOption::Plain => ServerTlsOption::Plain,
            &ServerTlsOption::Tls(ref a) => ServerTlsOption::Tls(a.clone()),
        }
    }
}
