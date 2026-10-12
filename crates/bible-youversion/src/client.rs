//! Blocking HTTP client for the YouVersion Platform API.
//!
//! Call from a background thread. Every request carries the app key in
//! `X-YVP-App-Key`, plus a bearer token when the user is signed in.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use url::Url;

pub const DEFAULT_HOST: &str = "api.youversion.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No app key is configured.
    MissingKey,
    /// 401 / 403: the key isn't licensed for this Bible or endpoint.
    NotPermitted,
    /// 429, with the server's `Retry-After` in seconds when given.
    RateLimited { retry_after: Option<u64> },
    /// The network or server could not be reached.
    Offline(String),
    /// Any other non-success status.
    Status(u16),
    /// The response wasn't what we expected.
    Invalid(String),
    /// Sign in was refused or failed.
    SignIn(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingKey => {
                write!(
                    f,
                    "Add your YouVersion app key in Settings ▸ Bible sources."
                )
            }
            Error::NotPermitted => write!(
                f,
                "YouVersion hasn't licensed this Bible to this app. Accept the publisher's license in the YouVersion Platform portal."
            ),
            Error::RateLimited {
                retry_after: Some(secs),
            } => write!(
                f,
                "YouVersion's request limit was reached. Try again in {}.",
                human_wait(*secs)
            ),
            Error::RateLimited { retry_after: None } => {
                write!(
                    f,
                    "YouVersion's request limit was reached. Try again later."
                )
            }
            Error::Offline(_) => write!(f, "Couldn't reach YouVersion. Check your connection."),
            Error::Status(code) => write!(f, "YouVersion returned an error ({code})."),
            Error::Invalid(what) => write!(f, "Unexpected response from YouVersion: {what}"),
            Error::SignIn(why) => write!(f, "Sign in failed: {why}"),
        }
    }
}

impl std::error::Error for Error {}

fn human_wait(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} seconds"),
        60..=3599 => format!("{} minutes", secs.div_ceil(60)),
        _ => format!("{} hours", secs.div_ceil(3600)),
    }
}

/// A successful response: its `Location` header (redirects) and body.
pub(crate) struct Raw {
    pub location: Option<String>,
    pub body: String,
}

struct Inner {
    agent: ureq::Agent,
    /// Same agent settings but never follows redirects (sign-in callback).
    no_redirect: ureq::Agent,
    host: String,
    app_key: String,
    token: RwLock<Option<String>>,
    requests: AtomicU64,
}

/// Cheap to clone; clones share the token and the request counter.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

fn agent(redirects: u32) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(redirects)
        .max_redirects_will_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent("BibleReader-macOS")
        .build()
        .into()
}

impl Client {
    pub fn new(app_key: impl Into<String>) -> Self {
        Self::with_host(app_key, DEFAULT_HOST)
    }

    pub fn with_host(app_key: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                agent: agent(5),
                no_redirect: agent(0),
                host: host.into(),
                app_key: app_key.into(),
                token: RwLock::new(None),
                requests: AtomicU64::new(0),
            }),
        }
    }

    pub fn app_key(&self) -> &str {
        &self.inner.app_key
    }

    pub fn host(&self) -> &str {
        &self.inner.host
    }

    /// Send this bearer token with every request (`None` signs out).
    pub fn set_token(&self, token: Option<String>) {
        if let Ok(mut slot) = self.inner.token.write() {
            *slot = token;
        }
    }

    fn token(&self) -> Option<String> {
        self.inner.token.read().ok().and_then(|t| t.clone())
    }

    /// Requests that actually went to the network since launch.
    pub fn requests_made(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
    }

    pub(crate) fn url(&self, path: &str, query: &[(&str, String)]) -> Result<Url, Error> {
        let mut url = Url::parse(&format!("https://{}{path}", self.inner.host))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(url)
    }

    fn check_key(&self) -> Result<(), Error> {
        if self.inner.app_key.trim().is_empty() {
            Err(Error::MissingKey)
        } else {
            Ok(())
        }
    }

    fn raw(&self, resp: ureq::http::Response<ureq::Body>) -> Result<Raw, Error> {
        let (parts, mut body) = resp.into_parts();
        let retry_after = parts
            .headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let location = parts
            .headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let status = parts.status.as_u16();
        let text = body
            .read_to_string()
            .map_err(|e| Error::Offline(e.to_string()))?;
        match status {
            429 => Err(Error::RateLimited { retry_after }),
            401 | 403 => Err(Error::NotPermitted),
            s if s >= 400 => Err(Error::Status(s)),
            _ => Ok(Raw {
                location,
                body: text,
            }),
        }
    }

    fn count(&self) {
        self.inner.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn get(&self, path: &str, query: &[(&str, String)]) -> Result<String, Error> {
        self.check_key()?;
        let url = self.url(path, query)?;
        let mut req = self
            .inner
            .agent
            .get(url.as_str())
            .header("X-YVP-App-Key", &self.inner.app_key);
        if let Some(token) = self.token() {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        self.count();
        let resp = req.call().map_err(|e| Error::Offline(e.to_string()))?;
        Ok(self.raw(resp)?.body)
    }

    /// GET without following redirects; returns the `Location` header of a 3xx.
    pub(crate) fn get_location(&self, url: &Url) -> Result<Option<String>, Error> {
        self.check_key()?;
        self.count();
        let resp = self
            .inner
            .no_redirect
            .get(url.as_str())
            .header("X-YVP-App-Key", &self.inner.app_key)
            .call()
            .map_err(|e| Error::Offline(e.to_string()))?;
        Ok(self.raw(resp)?.location)
    }

    pub(crate) fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<String, Error> {
        self.check_key()?;
        let url = self.url(path, &[])?;
        self.count();
        let resp = self
            .inner
            .agent
            .post(url.as_str())
            .header("X-YVP-App-Key", &self.inner.app_key)
            .send_form(form.iter().copied())
            .map_err(|e| Error::Offline(e.to_string()))?;
        Ok(self.raw(resp)?.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn builds_urls_with_bracketed_query_keys() {
        let client = Client::new("key");
        let url = client
            .url(
                "/v1/bibles",
                &[
                    ("language_ranges[]", "en".into()),
                    ("page_size", "99".into()),
                ],
            )
            .unwrap();
        assert_eq!(url.host_str(), Some(DEFAULT_HOST));
        assert_eq!(url.path(), "/v1/bibles");
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs[0].0, "language_ranges[]");
        assert_eq!(pairs[0].1, "en");
    }

    #[test]
    fn empty_key_fails_before_any_request() {
        let client = Client::new("  ");
        assert_eq!(client.get("/v1/bibles", &[]), Err(Error::MissingKey));
        assert_eq!(client.requests_made(), 0);
    }

    #[test]
    fn error_messages_are_user_facing() {
        assert!(
            Error::RateLimited {
                retry_after: Some(90)
            }
            .to_string()
            .contains("2 minutes")
        );
        assert!(Error::NotPermitted.to_string().contains("licensed"));
        assert!(
            Error::Offline("dns".into())
                .to_string()
                .contains("connection")
        );
    }
}
