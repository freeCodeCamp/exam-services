use sentry::types::Dsn;
use std::{env::var, time::Duration};
use tracing::{error, warn};

#[derive(Clone, Debug)]
pub struct EnvVars {
    pub sentry_dsn: Option<String>,
    pub mongodb_uri: String,
    pub moderation_length_in_s: Duration,
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

        let moderation_length_in_s = match var("MODERATION_LENGTH_IN_S") {
            Ok(v) => {
                let seconds = match v.parse() {
                    Ok(m) => m,
                    Err(e) => {
                        panic!(
                            "MODERATION_LENGTH_IN_S is not a valid whole number of seconds: {:?}",
                            e
                        );
                    }
                };
                let duration = Duration::from_secs(seconds);
                duration
            }
            Err(_e) => {
                let seven_days_in_s = 7 * 24 * 60 * 60;
                let duration = Duration::from_secs(seven_days_in_s);
                duration
            }
        };

        let env_vars = Self {
            mongodb_uri,
            sentry_dsn,
            moderation_length_in_s,
        };

        env_vars
    }
}

fn valid_sentry_dsn(url: &str) -> bool {
    url.parse::<Dsn>().is_ok()
}
