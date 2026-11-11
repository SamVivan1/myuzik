use crate::platform::logging::redact_url;
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
    /// The server asked us to slow down (`429`, or a proxy's equivalent).
    #[error("rate limited")]
    RateLimited,
}

impl NavidromeError {
    /// A plain sentence for the UI: what went wrong, in the user's language.
    ///
    /// The UI must never show `Debug` of this type (`Server("HTTP 500")`,
    /// `Network("timeout")`), which is what this replaces. The payload strings
    /// carry transport detail that is useful in a log line but not on screen, so
    /// they are classified here instead of being printed.
    pub fn user_message(&self) -> &'static str {
        match self {
            NavidromeError::Unauthorized => "The server rejected your username or password.",
            NavidromeError::NotFound => "The server could not find that.",
            NavidromeError::Unsupported => "The server does not support this feature.",
            NavidromeError::Network(message) if is_timeout(message) => {
                "The server took too long to answer."
            }
            NavidromeError::Network(_) => "Myuzik cannot reach the server.",
            NavidromeError::Protocol(_) => "The server answered with something Myuzik cannot read.",
            NavidromeError::Tls(_) => "The server's certificate could not be verified.",
            NavidromeError::RateLimited => "The server is busy and asked Myuzik to slow down.",
            NavidromeError::Server(message) if is_rate_limited(message) => {
                "The server is busy and asked Myuzik to slow down."
            }
            NavidromeError::Server(message) if is_server_fault(message) => {
                "The server ran into a problem handling the request."
            }
            NavidromeError::Server(_) => "The server refused the request.",
        }
    }

    /// Whether trying the same request again could plausibly succeed.
    ///
    /// A wrong password or a missing album never will; a rate limit, a timeout
    /// or a server fault usually will.
    pub fn is_retryable(&self) -> bool {
        match self {
            NavidromeError::RateLimited => true,
            NavidromeError::Network(message) => is_timeout(message) || !message.is_empty(),
            NavidromeError::Server(message) => is_server_fault(message),
            NavidromeError::Unauthorized
            | NavidromeError::NotFound
            | NavidromeError::Unsupported
            | NavidromeError::Protocol(_)
            | NavidromeError::Tls(_) => false,
        }
    }

    /// How long to wait before attempt `attempt` (1-based), with exponential
    /// backoff capped at 30 s. Rate limits wait longer than a plain timeout.
    pub fn retry_delay(&self, attempt: u8) -> std::time::Duration {
        let base_ms: u64 = if matches!(self, NavidromeError::RateLimited) {
            1000
        } else {
            250
        };
        let shift = attempt.saturating_sub(1).min(5) as u32;
        let ms = base_ms.saturating_mul(1u64 << shift);
        std::time::Duration::from_millis(ms.min(30_000))
    }

    /// One sentence of advice that goes with [`Self::user_message`], when there
    /// is something the user can actually do.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            NavidromeError::Unauthorized => {
                Some("Check the username and password, or set them again in Setup.")
            }
            NavidromeError::RateLimited => {
                Some("Wait a moment and try again; Myuzik backs off on its own.")
            }
            NavidromeError::NotFound => Some("It may have been removed or renamed on the server."),
            NavidromeError::Unsupported => {
                Some("Check the server's settings, or sign in with a password instead of a token.")
            }
            NavidromeError::Network(message) if is_timeout(message) => {
                Some("Check that the server is running and reachable, then try again.")
            }
            NavidromeError::Network(_) => Some("Check the server address and that you are online."),
            NavidromeError::Protocol(_) => {
                Some("Make sure the server address points at a Navidrome server.")
            }
            NavidromeError::Tls(_) => {
                Some("If the certificate is self-signed, trust it in the system certificate store.")
            }
            NavidromeError::Server(message) if is_rate_limited(message) => {
                Some("Wait a few seconds and try again.")
            }
            NavidromeError::Server(message) if is_server_fault(message) => {
                Some("Try again in a moment, and check the server's log if it keeps happening.")
            }
            NavidromeError::Server(_) => Some("Try again, or check the server's settings."),
        }
    }
}

/// A refused or unresponsive connection, as `ureq`/`io` report it.
fn is_timeout(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("timeout") || lower.contains("timed out")
}

/// `429` from a proxy or from Navidrome itself (see INT-12: a typed
/// `RateLimited` variant is the proper fix, but the mapping lives in the client).
fn is_rate_limited(message: &str) -> bool {
    message.contains("429")
}

/// A `5xx` answer: the server is up but failed the request.
fn is_server_fault(message: &str) -> bool {
    message
        .split(|c: char| !c.is_ascii_digit())
        .filter(|token| token.len() == 3)
        .any(|token| token.starts_with('5'))
}

impl From<io::Error> for NavidromeError {
    fn from(e: io::Error) -> Self {
        NavidromeError::Network(redact_url(&e.to_string()))
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
                // INT-12: 429 used to look exactly like a 500.
                429 => NavidromeError::RateLimited,
                _ => NavidromeError::Server(format!("HTTP {code}")),
            },
            ureq::Error::Io(io) => {
                let msg = redact_url(&io.to_string());
                if msg.to_lowercase().contains("tls") {
                    NavidromeError::Tls(msg)
                } else {
                    NavidromeError::Network(msg)
                }
            }
            ureq::Error::Timeout(_) => NavidromeError::Network("timeout".to_string()),
            other => NavidromeError::Network(redact_url(&other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_variants() -> Vec<NavidromeError> {
        vec![
            NavidromeError::Unauthorized,
            NavidromeError::NotFound,
            NavidromeError::Unsupported,
            NavidromeError::Network("connection refused".into()),
            NavidromeError::Network("timeout".into()),
            NavidromeError::Protocol("expected value at line 1 column 1".into()),
            NavidromeError::Tls("invalid peer certificate".into()),
            NavidromeError::Server("HTTP 429".into()),
            NavidromeError::Server("HTTP 500".into()),
            NavidromeError::Server("upstream said no".into()),
        ]
    }

    /// INT-12: a 429 must be its own kind, and only the failures that can
    /// plausibly succeed on a second try may be retried.
    #[test]
    fn a_rate_limit_is_its_own_kind_and_only_transient_failures_retry() {
        let limited = NavidromeError::from(ureq::Error::StatusCode(429));
        assert!(
            matches!(limited, NavidromeError::RateLimited),
            "429 mapped to {limited:?}"
        );
        assert!(limited.is_retryable());
        assert!(NavidromeError::Network("timeout".into()).is_retryable());
        assert!(NavidromeError::Server("HTTP 503".into()).is_retryable());
        assert!(!NavidromeError::NotFound.is_retryable());
        assert!(!NavidromeError::Unauthorized.is_retryable());
        assert!(!NavidromeError::Protocol("bad json".into()).is_retryable());
    }

    #[test]
    fn retry_delay_backs_off_and_stays_bounded() {
        let limited = NavidromeError::RateLimited;
        assert!(limited.retry_delay(1) < limited.retry_delay(2));
        assert!(limited.retry_delay(2) < limited.retry_delay(3));
        assert!(limited.retry_delay(20) <= std::time::Duration::from_secs(30));
        // A rate limit waits longer than a plain timeout.
        assert!(
            NavidromeError::RateLimited.retry_delay(1)
                > NavidromeError::Network("timeout".into()).retry_delay(1)
        );
    }

    #[test]
    fn every_variant_has_a_plain_sentence() {
        for error in all_variants() {
            let message = error.user_message();
            assert!(!message.is_empty(), "{error:?} has no user message");
            assert!(
                message.ends_with('.'),
                "{error:?} should be a sentence: {message}"
            );
            let lower = message.to_ascii_lowercase();
            assert!(
                !lower.contains("http"),
                "{error:?} leaks an HTTP status: {message}"
            );
            for jargon in ["some(", "none", "{", "}", "err(", "http 4", "http 5"] {
                assert!(
                    !lower.contains(jargon),
                    "{error:?} leaks Rust/HTTP internals ({jargon}): {message}"
                );
            }
            assert!(
                message.chars().any(|c| c.is_ascii_alphabetic()),
                "{error:?} has no words: {message}"
            );
        }
    }

    // The four failures users hit most must read differently, so they can tell
    // a wrong password from a dead server or a server that asked us to slow down.
    #[test]
    fn common_failures_read_differently() {
        let wrong_password = NavidromeError::Unauthorized.user_message();
        let unreachable = NavidromeError::Network("connection refused".into()).user_message();
        let rate_limited = NavidromeError::Server("HTTP 429".into()).user_message();
        let not_found = NavidromeError::NotFound.user_message();
        let timeout = NavidromeError::Network("timeout".into()).user_message();
        let messages = [
            wrong_password,
            unreachable,
            rate_limited,
            not_found,
            timeout,
        ];
        for (i, left) in messages.iter().enumerate() {
            for right in messages.iter().skip(i + 1) {
                assert_ne!(left, right, "two failures read the same: {left}");
            }
        }
        assert!(
            wrong_password.to_ascii_lowercase().contains("password"),
            "wrong password should say so: {wrong_password}"
        );
        assert!(
            rate_limited.to_ascii_lowercase().contains("busy"),
            "rate limiting should say the server is busy: {rate_limited}"
        );
        assert!(
            timeout.to_ascii_lowercase().contains("too long"),
            "a timeout should say it took too long: {timeout}"
        );
    }

    #[test]
    fn server_errors_are_split_by_status() {
        let rate_limited = NavidromeError::Server("HTTP 429".into());
        let server_error = NavidromeError::Server("HTTP 500".into());
        let refused = NavidromeError::Server("upstream said no".into());
        assert_ne!(rate_limited.user_message(), server_error.user_message());
        assert_ne!(server_error.user_message(), refused.user_message());
        assert!(rate_limited.hint().is_some(), "rate limiting needs a hint");
        assert!(
            rate_limited
                .hint()
                .is_some_and(|hint| hint.to_ascii_lowercase().contains("again")),
            "the rate limit hint should tell the user to try again"
        );
    }

    #[test]
    fn hints_are_short_sentences_or_absent() {
        for error in all_variants() {
            if let Some(hint) = error.hint() {
                assert!(!hint.is_empty(), "{error:?} has an empty hint");
                assert!(
                    hint.ends_with('.'),
                    "{error:?} hint is not a sentence: {hint}"
                );
            }
        }
        // Auth failures must point back at the setup form.
        assert!(
            NavidromeError::Unauthorized
                .hint()
                .is_some_and(|hint| hint.to_ascii_lowercase().contains("setup")),
            "the unauthorized hint should point at Setup"
        );
    }

    #[test]
    fn user_message_never_exposes_the_url_or_secret() {
        let error = NavidromeError::Network(
            "failed to connect to https://music.example.com:4533/rest/ping?u=sam&t=abc&s=def"
                .to_string(),
        );
        let message = error.user_message();
        assert!(!message.contains("example.com"), "{message}");
        assert!(!message.contains("abc"), "{message}");
    }
}
