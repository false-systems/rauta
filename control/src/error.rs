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

/// Proxy request errors with structured discrimination (replaces string-based error matching)
#[allow(dead_code)]
#[derive(Debug)]
pub enum ProxyError {
    /// Backend request or overall request timeout exceeded
    Timeout { message: String },
    /// Backend connection or protocol error
    BackendError { message: String },
    /// Request body too large
    BodyTooLarge { size: usize, max: usize },
    /// Filter application failed
    FilterError { message: String },
}

#[allow(dead_code)]
impl ProxyError {
    /// HTTP status code for this error
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
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyError::Timeout { message } => write!(f, "TIMEOUT: {}", message),
            ProxyError::BackendError { message } => write!(f, "{}", message),
            ProxyError::BodyTooLarge { size, max } => {
                write!(f, "Request body too large: {} bytes (max {})", size, max)
            }
            ProxyError::FilterError { message } => write!(f, "Filter error: {}", message),
        }
    }
}
