use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, StrategyError>;

#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    #[error("failed to read strategy file `{path}`: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read current directory: {0}")]
    CurrentDir(#[source] std::io::Error),
    #[error("strategy parse error: {0}")]
    Parse(String),
    #[error("strategy compile error: {0}")]
    Compile(String),
}
