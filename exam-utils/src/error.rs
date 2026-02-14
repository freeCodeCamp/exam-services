#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Generation(String),
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    ModerationScore(String),
    // Froms
    #[error("{0}")]
    MongoDB(#[from] mongodb::error::Error),
    #[error("{0}")]
    SystemTimeError(#[from] std::time::SystemTimeError),
    #[error("{0}")]
    BsonAccess(#[from] mongodb::bson::error::ValueAccessErrorKind),
    #[error("{0}")]
    BsonSerialization(#[from] mongodb::bson::error::Error),
    // #[error("{0}")]
    // TimeConversion(#[from] time::error::ConversionRange),
}
