use std::time::Duration;

#[derive(Default, Debug, Clone)]
pub struct ClientConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    pub thread_name: Option<String>,
    pub connection_timeout: Option<Duration>
}

impl ClientConf {
    pub fn new() -> ClientConf {
        Default::default()
    }
}
