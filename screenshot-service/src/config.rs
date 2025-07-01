use tracing::warn;

#[derive(Debug, Clone)]
pub struct AppState {
    pub client: aws_sdk_s3::Client,
    pub env_vars: EnvVars,
}

#[derive(Debug, Clone)]
pub struct EnvVars {
    pub bucket_name: String,
    pub port: u16,
    pub request_body_size_limit: usize,
    pub request_timeout_in_ms: u64,
}

impl EnvVars {
    pub fn new() -> Self {
        let default_bucket_name = "EXAM_ENVIRONMENT_SCREENSHOTS".to_string();
        let bucket_name = match std::env::var("S3_BUCKET_NAME") {
            Ok(s) => {
                if s.is_empty() {
                    warn!("S3_BUCKET_NAME not set. Defaulting to {default_bucket_name}");
                    default_bucket_name
                } else {
                    s
                }
            }
            Err(_e) => {
                warn!("S3_BUCKET_NAME not set. Defaulting to {default_bucket_name}");
                default_bucket_name
            }
        };

        let port = match std::env::var("PORT") {
            Ok(port_string) => port_string.parse().expect("PORT to be parseable as u16"),
            Err(_e) => {
                let default_port = 3002;
                warn!("PORT not set. Defaulting to {default_port}");
                default_port
            }
        };

        let request_timeout_in_ms = match std::env::var("REQUEST_TIMEOUT_IN_MS") {
            Ok(s) => s
                .parse()
                .expect("REQUEST_TIMEOUT_IN_MS to be valid unsigned integer"),
            Err(_e) => {
                let default_request_timeout = 30_000;
                warn!("REQUEST_TIMEOUT_IN_MS not set. Defaulting to {default_request_timeout}");
                default_request_timeout
            }
        };

        let request_body_size_limit = match std::env::var("REQUEST_BODY_SIZE_LIMIT") {
            Ok(s) => s
                .parse()
                .expect("REQUEST_BODY_SIZE_LIMIT to be valid unsigned integer"),
            Err(_e) => {
                let base: usize = 2;
                let exp = 20;
                let default_request_body_size_limit = 5 * base.pow(exp);
                warn!(
                    "REQUEST_BODY_SIZE_LIMIT not set. Defaulting to {default_request_body_size_limit}"
                );
                default_request_body_size_limit
            }
        };
        EnvVars {
            bucket_name,
            port,
            request_body_size_limit,
            request_timeout_in_ms,
        }
    }
}
