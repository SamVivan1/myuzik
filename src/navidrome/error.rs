use std::io;

#[derive(Debug, thiserror::Error)]
pub enum NavidromeError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("unsupported")]
    Unsupported,
    #[error("network: {0}")]
    Network(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("tls: {0}")]
    Tls(String),
    #[error("server: {0}")]
    Server(String),
}

impl From<io::Error> for NavidromeError {
    fn from(e: io::Error) -> Self {
        NavidromeError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for NavidromeError {
    fn from(e: serde_json::Error) -> Self {
        NavidromeError::Protocol(e.to_string())
    }
}

impl From<ureq::Error> for NavidromeError {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::StatusCode(code) => match code {
                401 | 403 => NavidromeError::Unauthorized,
                404 => NavidromeError::NotFound,
                _ => NavidromeError::Server(format!("HTTP {code}")),
            },
            ureq::Error::Io(io) => {
                let msg = io.to_string();
                if msg.to_lowercase().contains("tls") {
                    NavidromeError::Tls(msg)
                } else {
                    NavidromeError::Network(msg)
                }
            }
            ureq::Error::Timeout(_) => NavidromeError::Network("timeout".to_string()),
            other => NavidromeError::Network(other.to_string()),
        }
    }
}
