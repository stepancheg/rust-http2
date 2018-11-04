use common::conf::CommonConf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerAlpn {
    // Ignore negotiated ALPN
    Ignore,
    // Return error is ALPN is not "h2"
    Require,
}

#[derive(Default, Debug, Clone)]
pub struct ServerConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    pub thread_name: Option<String>,

    pub alpn: Option<ServerAlpn>,

    // Bind on both IPv4 and IPv6 addresses when addr is IPv6
    pub only_v6: Option<bool>,

    /// Ignored on Windows
    pub reuse_port: Option<bool>,
    pub backlog: Option<i32>,

    pub common: CommonConf,
}

impl ServerConf {
    pub fn new() -> ServerConf {
        Default::default()
    }
}
