use crate::common::conf::CommonConf;
use std::time::Duration;

/// Client configuration.
#[derive(Default, Debug, Clone)]
pub struct ClientConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    /// Thread name.
    pub thread_name: Option<String>,
    /// Connection timeout.
    pub connection_timeout: Option<Duration>,

    /// Common client/server conf.
    pub common: CommonConf,
}

impl ClientConf {
    /// Default configuration.
    pub fn new() -> ClientConf {
        Default::default()
    }
}
