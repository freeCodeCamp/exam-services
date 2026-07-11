#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Generation(String),
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    ModerationScore(String),
}
