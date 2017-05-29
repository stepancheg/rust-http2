use std::time::Duration;

use common::CommonConf;

#[derive(Default, Debug, Clone)]
pub struct ClientConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    pub thread_name: Option<String>,
    pub connection_timeout: Option<Duration>,

    pub common: CommonConf,
}

impl ClientConf {
    pub fn new() -> ClientConf {
        Default::default()
    }
}
