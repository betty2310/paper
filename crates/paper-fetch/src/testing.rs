//! A scripted `HttpClient` for tests. Enabled with the `testing` feature.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Condvar, Mutex};

use crate::FetchError;
use crate::http::{HttpClient, Response};

/// One recorded request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    /// The body of a POST; empty for a GET.
    pub body: Vec<u8>,
}

/// Answers requests from a table of URL to response, whatever the method.
/// Unknown URLs answer 404.
#[derive(Default)]
pub struct FakeHttp {
    /// The answers for a URL in order; the last one repeats.
    routes: Mutex<HashMap<String, VecDeque<Response>>>,
    failure: Mutex<Option<String>>,
    requests: Mutex<Vec<Request>>,
    gate: Mutex<Gate>,
    gate_changed: Condvar,
}

/// While `closed`, requests wait inside `get`, so a test can act in the
/// middle of a lookup. `waiting` counts the requests parked there.
#[derive(Default)]
struct Gate {
    closed: bool,
    waiting: usize,
}

impl FakeHttp {
    pub fn new() -> FakeHttp {
        FakeHttp::default()
    }

    /// Answers `url` with `status` and a JSON body, replacing earlier answers.
    pub fn route(&self, url: &str, status: u16, body: Vec<u8>) {
        self.route_responses(url, vec![json_response(status, body, None)]);
    }

    /// Answers `url` with each response in turn; the last one repeats.
    pub fn route_responses(&self, url: &str, responses: Vec<Response>) {
        self.routes
            .lock()
            .unwrap()
            .insert(url.to_string(), responses.into_iter().collect());
    }

    /// Makes every request fail as if the network were down.
    pub fn fail_with(&self, message: &str) {
        *self.failure.lock().unwrap() = Some(message.to_string());
    }

    /// Stops failing requests.
    pub fn recover(&self) {
        *self.failure.lock().unwrap() = None;
    }

    /// Parks every request inside `get` until `release`.
    pub fn hold(&self) {
        self.gate.lock().unwrap().closed = true;
    }

    /// Blocks until at least one request is parked by `hold`.
    pub fn wait_for_held_request(&self) {
        let mut gate = self.gate.lock().unwrap();
        while gate.waiting == 0 {
            gate = self.gate_changed.wait(gate).unwrap();
        }
    }

    /// Lets parked and later requests through.
    pub fn release(&self) {
        self.gate.lock().unwrap().closed = false;
        self.gate_changed.notify_all();
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn pass_gate(&self) {
        let mut gate = self.gate.lock().unwrap();
        if gate.closed {
            gate.waiting += 1;
            self.gate_changed.notify_all();
            while gate.closed {
                gate = self.gate_changed.wait(gate).unwrap();
            }
            gate.waiting -= 1;
        }
    }
}

/// A JSON response with `retry_after` seconds, for scripting 429 and 5xx answers.
pub fn json_response(status: u16, body: Vec<u8>, retry_after: Option<u64>) -> Response {
    Response {
        status,
        body,
        content_type: Some("application/json".to_string()),
        retry_after,
    }
}

impl FakeHttp {
    fn answer(&self, request: Request) -> Result<Response, FetchError> {
        let url = request.url.clone();
        self.requests.lock().unwrap().push(request);
        self.pass_gate();
        if let Some(message) = self.failure.lock().unwrap().clone() {
            return Err(FetchError::Network { message });
        }
        let mut routes = self.routes.lock().unwrap();
        let response = match routes.get_mut(&url) {
            Some(responses) if responses.len() > 1 => responses.pop_front(),
            Some(responses) => responses.front().cloned(),
            None => None,
        };
        Ok(response.unwrap_or(Response {
            status: 404,
            body: Vec::new(),
            content_type: None,
            retry_after: None,
        }))
    }
}

fn header_map(headers: &[(&str, &str)]) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

impl HttpClient for FakeHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, FetchError> {
        self.answer(Request {
            method: "GET",
            url: url.to_string(),
            headers: header_map(headers),
            body: Vec::new(),
        })
    }

    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<Response, FetchError> {
        self.answer(Request {
            method: "POST",
            url: url.to_string(),
            headers: header_map(headers),
            body: body.to_vec(),
        })
    }
}
