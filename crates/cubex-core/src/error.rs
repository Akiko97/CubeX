#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(#[from] toml::de::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(#[from] cubex_protocol::ProtocolError),
    #[error("store error: {0}")]
    Store(#[from] cubex_store::StoreError),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("plugin `{0}` is not configured")]
    MissingPlugin(String),
    #[error("plugin `{0}` is configured more than once")]
    DuplicatePlugin(String),
    #[error("route `{0}` has no targets")]
    EmptyRouteTargets(String),
    #[error("route `{route}` targets unknown plugin `{target}`")]
    UnknownRouteTarget { route: String, target: String },
    #[error("route `{route}` matches unknown source `{source_name}`")]
    UnknownRouteSource { route: String, source_name: String },
    #[error("plugin `{name}` exited before it returned a response")]
    PluginExited { name: String },
    #[error("plugin `{plugin}` state error: {reason}")]
    PluginState { plugin: String, reason: String },
    #[error("plugin `{plugin}` error: {reason}")]
    PluginError { plugin: String, reason: String },
    #[error("plugin `{plugin}` emitted invalid message: {reason}")]
    InvalidPluginMessage { plugin: String, reason: String },
    #[error("stored message is invalid: {0}")]
    InvalidStoredMessage(String),
    #[error("message limit {0} reached")]
    MessageLimit(usize),
}

pub type Result<T> = std::result::Result<T, Error>;
