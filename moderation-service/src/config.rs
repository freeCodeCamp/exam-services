use sentry::types::Dsn;
use std::{env::var, time::Duration};
use tracing::{error, warn};

#[derive(Clone, Debug)]
pub struct EnvVars {
    pub environment: Environment,
    pub moderation_length_in_s: Duration,
    pub moderation_threshold: f64,
    pub mongodb_uri: String,
    pub sentry_dsn: Option<String>,
    pub supabase_key: String,
    pub supabase_url: String,
    pub timeout_secs: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum Environment {
    Development,
    Staging,
    Production,
}

impl From<String> for Environment {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "development" => Environment::Development,
            "staging" => Environment::Staging,
            "production" => Environment::Production,
            other => {
                warn!(
                    "ENVIRONMENT value '{}' is not valid. Defaulting to 'production'.",
                    other
                );
                Environment::Production
            }
        }
    }
}

impl ToString for Environment {
    fn to_string(&self) -> String {
        match self {
            Environment::Development => "development".to_string(),
            Environment::Staging => "staging".to_string(),
            Environment::Production => "production".to_string(),
        }
    }
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
        let moderation_threshold = match var("MODERATION_THRESHOLD") {
            Ok(v) => {
                let num = match v.parse() {
                    Ok(m) => m,
                    Err(e) => {
                        panic!(
                            "MODERATION_THRESHOLD should be between 0.0 and 1.0 inclusive: {:?}",
                            e
                        );
                    }
                };
                num
            }
            Err(_e) => 0.25,
        };

        let environment = match var("ENVIRONMENT") {
            Ok(v) => v.into(),
            Err(_e) => {
                warn!("ENVIRONMENT not set. Defaulting to 'production'.");
                Environment::Production
            }
        };

        let Ok(supabase_url) = var("SUPABASE_URL") else {
            error!("SUPABASE_URL not set");
            panic!("SUPABASE_URL required");
        };
        assert!(!supabase_url.is_empty(), "SUPABASE_URL must not be empty");
        let Ok(supabase_key) = var("SUPABASE_KEY") else {
            error!("SUPABASE_KEY not set");
            panic!("SUPABASE_KEY required");
        };

        // Optional timeout (in seconds) for the task to finish.
        // If TIMEOUT_SECS is not set or invalid, proceed without a timeout.
        let timeout_secs = match std::env::var("TIMEOUT_SECS") {
            Ok(val) => match val.parse::<u64>() {
                Ok(secs) if secs > 0 => Some(secs),
                Ok(_) => {
                    warn!("TIMEOUT_SECS provided but not > 0; ignoring");
                    None
                }
                Err(e) => {
                    warn!("Failed to parse TIMEOUT_SECS ('{val}'): {e}; ignoring");
                    None
                }
            },
            Err(_) => None,
        };

        let env_vars = Self {
            environment,
            moderation_length_in_s,
            moderation_threshold,
            mongodb_uri,
            sentry_dsn,
            supabase_key,
            supabase_url,
            timeout_secs,
        };

        env_vars
    }
}

fn valid_sentry_dsn(url: &str) -> bool {
    url.parse::<Dsn>().is_ok()
}
