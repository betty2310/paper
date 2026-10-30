//! The blocking HTTP client behind every lookup, and the polite identification
//! each metadata API asks for.

use std::time::Duration;

use crate::FetchError;

/// The product part of the user agent. The contact email is appended per request.
pub const USER_AGENT_PRODUCT: &str = concat!(
    "Paper/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/betty2310/paper"
);

const TIMEOUT: Duration = Duration::from_secs(20);

/// Who is asking. Crossref serves requests that carry an email from its faster
/// polite pool, and Unpaywall requires one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contact {
    pub email: Option<String>,
}

impl Contact {
    pub fn new(email: impl Into<String>) -> Contact {
        let email = email.into();
        Contact {
            email: (!email.trim().is_empty()).then(|| email.trim().to_string()),
        }
    }

    /// `Paper/0.1.0 (https://github.com/betty2310/paper; mailto:me@example.org)`.
    pub fn user_agent(&self) -> String {
        match &self.email {
            Some(email) => format!("{USER_AGENT_PRODUCT}; mailto:{email})"),
            None => format!("{USER_AGENT_PRODUCT})"),
        }
    }
}

/// A finished HTTP response. Status errors are values, not `Err`, so the
/// pipeline can decide what a 404 means for each source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    /// Seconds from a `Retry-After` header, when the server sent one.
    pub retry_after: Option<u64>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self, source: &str) -> Result<serde_json::Value, FetchError> {
        serde_json::from_slice(&self.body).map_err(|error| FetchError::Parse {
            api: source.to_string(),
            message: error.to_string(),
        })
    }
}

/// One GET at a time. Implemented by `UreqClient` and by test fakes.
pub trait HttpClient: Send + Sync {
    /// Performs a GET with the given extra headers, following redirects.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, FetchError>;
}

/// The real client: `ureq` with rustls and the platform's certificate store,
/// so campus and corporate proxies keep working.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub fn new() -> UreqClient {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(10)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        UreqClient {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        UreqClient::new()
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, FetchError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.call().map_err(|error| FetchError::Network {
            message: error.to_string(),
        })?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let content_type = header("content-type");
        let retry_after = header("retry-after").and_then(|value| value.trim().parse().ok());
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| FetchError::Network {
                message: error.to_string(),
            })?;
        Ok(Response {
            status,
            body,
            content_type,
            retry_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_follows_the_polite_format() {
        assert_eq!(
            Contact::new(" me@example.org ").user_agent(),
            format!(
                "Paper/{} (https://github.com/betty2310/paper; mailto:me@example.org)",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(
            Contact::new("").user_agent(),
            format!(
                "Paper/{} (https://github.com/betty2310/paper)",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(Contact::new("  ").email, None);
    }
}
