//! Polite feed download: conditional GET, a size cap, a timeout, and retries
//! on failures that asking again can fix.

use std::time::Duration;

use chrono_machines::{AsyncRetryable, ExponentialBackoff};
use rama::error::BoxError;
use rama::http::body::CollectOptions;
use rama::http::client::DefaultHttpWebClient;
use rama::http::service::client::HttpClientExt;
use rama::http::{BodyExtractExt, HeaderMap, StatusCode};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Feeds are a few hundred kilobytes; anything past this is not a feed.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const ATTEMPTS: u8 = 3;
const ACCEPT: &str =
    "application/atom+xml, application/rss+xml, application/xml;q=0.9, text/xml;q=0.8";

/// What the server said identifies this version of the feed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug)]
pub enum Fetched {
    /// 304: the copy from the last run is still current.
    NotModified,
    Fresh {
        body: String,
        validators: Validators,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("request failed: {0}")]
    Transport(#[source] BoxError),
    #[error("HTTP {0}")]
    Status(u16),
    #[error("reading body: {0}")]
    Body(String),
}

impl FetchError {
    /// Network, TLS and timeout failures, rate limits and server errors.
    pub const fn is_transient(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Status(status) => *status == 429 || *status >= 500,
            Self::Body(_) => false,
        }
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}

/// One GET, sending `known` so an unchanged feed costs a 304 and no body.
///
/// # Errors
/// [`FetchError::Transport`] when no response arrives, [`FetchError::Status`]
/// on anything but 200/304, [`FetchError::Body`] when the body is not UTF-8 or
/// exceeds the size cap.
pub async fn fetch(
    client: &DefaultHttpWebClient,
    user_agent: &str,
    url: &str,
    known: &Validators,
) -> Result<Fetched, FetchError> {
    let conditional = [("if-none-match", &known.etag), ("if-modified-since", &known.last_modified)];
    let response = conditional
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.as_deref()?)))
        .fold(
            client.get(url).header("user-agent", user_agent).header("accept", ACCEPT),
            |request, (name, value)| request.header(name, value),
        )
        .send_with_timeout(REQUEST_TIMEOUT)
        .await
        .map_err(FetchError::Transport)?;
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if status != StatusCode::OK {
        return Err(FetchError::Status(status.as_u16()));
    }

    let validators = Validators {
        etag: header(response.headers(), "etag"),
        last_modified: header(response.headers(), "last-modified"),
    };
    let options = CollectOptions::new().with_max_size(MAX_BODY_BYTES).with_timeout(REQUEST_TIMEOUT);
    let body = response
        .try_into_string_with(options)
        .await
        .map_err(|error| FetchError::Body(error.to_string()))?;
    Ok(Fetched::Fresh { body, validators })
}

/// [`fetch`], retried with jittered exponential backoff while the failure is
/// transient.
///
/// # Errors
/// The last attempt's error, or the first permanent one.
pub async fn fetch_with_retry(
    client: &DefaultHttpWebClient,
    user_agent: &str,
    url: &str,
    known: &Validators,
) -> Result<Fetched, FetchError> {
    (|| fetch(client, user_agent, url, known))
        .retry_async(ExponentialBackoff::default().max_attempts(ATTEMPTS).base_delay_ms(500))
        .when(FetchError::is_transient)
        .call_async(|ms| tokio::time::sleep(Duration::from_millis(ms)))
        .await
        .map(chrono_machines::RetryOutcome::into_inner)
        .map_err(chrono_machines::RetryError::into_cause)
}
