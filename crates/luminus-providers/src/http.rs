use luminus_core::provider::{ProviderError, ProviderErrorCategory};
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub async fn bounded_body(response: Response) -> Result<Vec<u8>, ProviderError> {
    let bytes = response.bytes().await.map_err(|_| {
        ProviderError::new(
            ProviderErrorCategory::UpstreamUnavailable,
            "Failed to read Blackbox response body",
            true,
        )
    })?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(ProviderError::new(
            ProviderErrorCategory::ProviderFailure,
            "Blackbox response body exceeded the 8 MiB limit",
            false,
        ));
    }
    Ok(bytes.to_vec())
}

pub fn bounded_error_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct HttpTransport {
    client: Client,
}

impl HttpTransport {
    pub fn new() -> Result<Self, ProviderError> {
        Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map(|client| Self { client })
            .map_err(|error| {
                ProviderError::new(
                    ProviderErrorCategory::ProviderFailure,
                    error.to_string(),
                    false,
                )
            })
    }

    pub fn post(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    pub async fn send(
        &self,
        response: Result<Response, reqwest::Error>,
    ) -> Result<Response, ProviderError> {
        response.map_err(|error| {
            let category = if error.is_timeout() {
                ProviderErrorCategory::Timeout
            } else {
                ProviderErrorCategory::UpstreamUnavailable
            };
            ProviderError::new(category, "Blackbox transport request failed", true)
        })
    }

    pub fn status_error(status: StatusCode, retry_after: Option<u64>) -> ProviderError {
        let (category, retryable) = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                (ProviderErrorCategory::Authentication, false)
            }
            StatusCode::TOO_MANY_REQUESTS => (ProviderErrorCategory::RateLimit, true),
            StatusCode::REQUEST_TIMEOUT => (ProviderErrorCategory::Timeout, true),
            s if s.is_server_error() => (ProviderErrorCategory::UpstreamUnavailable, true),
            s if s.is_client_error() => (ProviderErrorCategory::InvalidRequest, false),
            _ => (ProviderErrorCategory::ProviderFailure, false),
        };
        let mut error = ProviderError::new(
            category,
            format!("Blackbox upstream returned HTTP {status}"),
            retryable,
        );
        error.cooldown_seconds = retry_after;
        error
    }
}

impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpTransport")
            .finish_non_exhaustive()
    }
}

pub fn parse_retry_after(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.trim().parse().ok())
}

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn error_text_is_bounded_and_single_line() {
        let text = bounded_error_text(b"a\nb\r\nc");
        assert_eq!(text, "a b c");
        assert!(bounded_error_text(&vec![b'x'; 5000]).len() <= 4096);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_mapping_is_semantic() {
        assert_eq!(
            HttpTransport::status_error(StatusCode::UNAUTHORIZED, None).category,
            ProviderErrorCategory::Authentication
        );
        assert_eq!(
            HttpTransport::status_error(StatusCode::TOO_MANY_REQUESTS, Some(4)).cooldown_seconds,
            Some(4)
        );
        assert!(HttpTransport::status_error(StatusCode::INTERNAL_SERVER_ERROR, None).retryable);
    }
}
