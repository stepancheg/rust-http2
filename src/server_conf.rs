use common::CommonConf;

#[derive(Default, Debug, Clone)]
pub struct ServerConf {
    /// TCP_NODELAY
    pub no_delay: Option<bool>,
    pub thread_name: Option<String>,
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
