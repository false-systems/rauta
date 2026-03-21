use thiserror::Error;

/// RAUTA Control Plane Errors
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum RautaError {
    #[error("Route configuration error: {0}")]
    RouteConfig(String),

    #[error("Kubernetes error: {0}")]
    Kubernetes(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Proxy request errors with structured discrimination
///
/// Replaces string-based error matching (`e.starts_with("TIMEOUT:")`) with
/// proper enum variants. Each variant maps to a specific HTTP status code.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ProxyError {
    /// Backend request or overall request timeout exceeded → 504
    Timeout { message: String },
    /// Backend connection or protocol error → 502
    BackendError { message: String },
    /// Request body too large → 413
    BodyTooLarge { size: usize, max: usize },
    /// Filter application failed → 500
    FilterError { message: String },
}

#[allow(dead_code)]
impl ProxyError {
    pub fn status_code(&self) -> u16 {
        match self {
            ProxyError::Timeout { .. } => 504,
            ProxyError::BackendError { .. } => 502,
            ProxyError::BodyTooLarge { .. } => 413,
            ProxyError::FilterError { .. } => 500,
        }
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, ProxyError::Timeout { .. })
    }

    pub fn status_str(&self) -> &'static str {
        match self {
            ProxyError::Timeout { .. } => "504",
            ProxyError::BackendError { .. } => "502",
            ProxyError::BodyTooLarge { .. } => "413",
            ProxyError::FilterError { .. } => "500",
        }
    }
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyError::Timeout { message } => write!(f, "{}", message),
            ProxyError::BackendError { message } => write!(f, "{}", message),
            ProxyError::BodyTooLarge { size, max } => {
                write!(f, "Request body too large: {} bytes (max {})", size, max)
            }
            ProxyError::FilterError { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for ProxyError {}

/// Allow `?` on String errors (legacy compatibility during migration).
///
/// Maps all string errors to `BackendError` (502). This is a transitional shim —
/// callers should construct specific ProxyError variants directly. Will be removed
/// once all error sites are migrated.
impl From<String> for ProxyError {
    fn from(s: String) -> Self {
        ProxyError::BackendError { message: s }
    }
}
