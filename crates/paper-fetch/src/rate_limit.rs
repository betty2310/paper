//! A per host minimum interval between requests, shared by every lookup. The
//! limits are the ones in `docs/research/doi-and-pdf-sources.md`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    last_request: Mutex<HashMap<String, Instant>>,
}

impl RateLimiter {
    pub fn new() -> RateLimiter {
        RateLimiter {
            last_request: Mutex::new(HashMap::new()),
        }
    }

    /// Blocks until a request to `url`'s host is allowed, then records it.
    pub fn wait_for(&self, url: &str) {
        let host = host_of(url);
        let interval = min_interval(&host);
        let wait = {
            let mut last = self.last_request.lock().unwrap();
            let now = Instant::now();
            let allowed_at = last
                .get(&host)
                .map(|previous| *previous + interval)
                .unwrap_or(now)
                .max(now);
            last.insert(host, allowed_at);
            allowed_at.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
}

fn min_interval(host: &str) -> Duration {
    match host {
        // Polite pool: 10 per second.
        "api.crossref.org" => Duration::from_millis(100),
        // One request per three seconds, single connection.
        "export.arxiv.org" | "arxiv.org" => Duration::from_secs(3),
        // Content negotiation has no documented limit; redirects go elsewhere.
        "doi.org" | "dx.doi.org" => Duration::ZERO,
        // Everyone else: 5 per second.
        _ => Duration::from_millis(200),
    }
}

fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_extracted_from_urls() {
        assert_eq!(
            host_of("https://api.crossref.org/works/10.1/x?mailto=a"),
            "api.crossref.org"
        );
        assert_eq!(
            host_of("http://Export.arXiv.org:80/api/query"),
            "export.arxiv.org"
        );
        assert_eq!(host_of("https://doi.org/10.1/x"), "doi.org");
    }

    #[test]
    fn second_request_to_the_same_host_waits_for_the_interval() {
        let limiter = RateLimiter::new();
        let start = Instant::now();
        limiter.wait_for("https://api.crossref.org/works/a");
        limiter.wait_for("https://api.crossref.org/works/b");
        limiter.wait_for("https://api.crossref.org/works/c");
        assert!(
            start.elapsed() >= Duration::from_millis(200),
            "{:?}",
            start.elapsed()
        );

        let start = Instant::now();
        limiter.wait_for("https://doi.org/a");
        limiter.wait_for("https://doi.org/b");
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
