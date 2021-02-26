use crate::common::conf::CommonConf;

/// Server ALPN negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerAlpn {
    // Ignore negotiated ALPN
    Ignore,
    // Return error is ALPN is not "h2"
    Require,
}

/// Server configuration.
#[derive(Default, Debug, Clone)]
pub struct ServerConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    /// Thread name, used when runtime is not provided.
    pub thread_name: Option<String>,

    /// Is ALPN required?
    pub alpn: Option<ServerAlpn>,

    // Bind on both IPv4 and IPv6 addresses when addr is IPv6
    pub only_v6: Option<bool>,

    /// Ignored on Windows
    pub reuse_port: Option<bool>,

    /// Socket option.
    pub backlog: Option<i32>,

    /// Common client and server configuration.
    pub common: CommonConf,
}

impl ServerConf {
    pub fn new() -> ServerConf {
        Default::default()
    }
}
