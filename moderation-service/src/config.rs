use sentry::types::Dsn;
use std::env::var;
use tracing::{error, warn};

#[derive(Clone, Debug)]
pub struct EnvVars {
    pub sentry_dsn: Option<String>,
    pub mongodb_uri: String,
}

impl EnvVars {
    pub fn new() -> Self {
        let Ok(mongodb_uri) = var("MONGODB_URI") else {
            error!("MONGODB_URI not set");
            panic!("MONGODB_URI required");
        };
        assert!(!mongodb_uri.is_empty(), "MONGODB_URI must not be empty");

        let sentry_dsn = match var("SENTRY_DSN") {
            Ok(dsn_string) => {
                assert!(
                    valid_sentry_dsn(&dsn_string),
                    "SENTRY_DSN is not valid DSN."
                );
                Some(dsn_string)
            }
            Err(_e) => {
                if cfg!(not(debug_assertions)) {
                    panic!("SENTRY_DSN is not allowed to be unset outside of a debug build");
                }
                warn!("SENTRY_DSN not set.");
                None
            }
        };

        let env_vars = Self {
            mongodb_uri,
            sentry_dsn,
        };

        env_vars
    }
}

fn valid_sentry_dsn(url: &str) -> bool {
    url.parse::<Dsn>().is_ok()
}
