/// Errors from gateway adapters and normalization.
#[derive(Debug)]
pub enum GatewayError {
    /// The inbound payload could not be parsed.
    ParseError(String),
    /// A required field is missing from the payload.
    MissingField(String),
    /// I/O or transport error.
    TransportError(String),
    /// Authentication failed.
    AuthError(String),
    /// Request was rate-limited.
    RateLimited(String),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayError::ParseError(msg) => write!(f, "parse error: {}", msg),
            GatewayError::MissingField(field) => write!(f, "missing field: {}", field),
            GatewayError::TransportError(msg) => write!(f, "transport error: {}", msg),
            GatewayError::AuthError(msg) => write!(f, "auth error: {}", msg),
            GatewayError::RateLimited(msg) => write!(f, "rate limited: {}", msg),
        }
    }
}

impl std::error::Error for GatewayError {}
