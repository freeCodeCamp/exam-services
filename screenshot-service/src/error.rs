use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{1}")]
    Server(StatusCode, String),
    // Froms
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let msg = format!("{}", self.to_string());
        let status: StatusCode = self.into();

        (status, msg).into_response()
    }
}

impl From<Error> for StatusCode {
    fn from(error: Error) -> Self {
        match error {
            Error::Server(c, _) => c,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
