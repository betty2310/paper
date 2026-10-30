//! A scripted `HttpClient` for tests. Enabled with the `testing` feature.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::FetchError;
use crate::http::{HttpClient, Response};

/// One recorded request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// Answers GETs from a table of URL to response. Unknown URLs answer 404.
#[derive(Default)]
pub struct FakeHttp {
    routes: Mutex<HashMap<String, Response>>,
    failure: Mutex<Option<String>>,
    requests: Mutex<Vec<Request>>,
}

impl FakeHttp {
    pub fn new() -> FakeHttp {
        FakeHttp::default()
    }

    /// Answers `url` with `status` and a JSON body.
    pub fn route(&self, url: &str, status: u16, body: Vec<u8>) {
        self.routes.lock().unwrap().insert(
            url.to_string(),
            Response {
                status,
                body,
                content_type: Some("application/json".to_string()),
                retry_after: None,
            },
        );
    }

    /// Makes every request fail as if the network were down.
    pub fn fail_with(&self, message: &str) {
        *self.failure.lock().unwrap() = Some(message.to_string());
    }

    /// Stops failing requests.
    pub fn recover(&self) {
        *self.failure.lock().unwrap() = None;
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClient for FakeHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, FetchError> {
        self.requests.lock().unwrap().push(Request {
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        });
        if let Some(message) = self.failure.lock().unwrap().clone() {
            return Err(FetchError::Network { message });
        }
        Ok(self
            .routes
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or(Response {
                status: 404,
                body: Vec::new(),
                content_type: None,
                retry_after: None,
            }))
    }
}
