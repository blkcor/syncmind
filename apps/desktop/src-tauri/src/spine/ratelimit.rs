use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

pub const MAX_REQUESTS: usize = 30;
pub const WINDOW_SECONDS: u64 = 60;

pub type SharedRateLimiter = Arc<Mutex<SlidingWindowRateLimiter>>;

static SEARCH_RATE_LIMITER: OnceLock<SharedRateLimiter> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct SlidingWindowRateLimiter {
    max_requests: usize,
    window: Duration,
    requests: HashMap<String, VecDeque<Instant>>,
}

impl SlidingWindowRateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            requests: HashMap::new(),
        }
    }

    pub fn default_search_limit() -> Self {
        Self::new(MAX_REQUESTS, Duration::from_secs(WINDOW_SECONDS))
    }

    pub fn check_and_record(&mut self, peer_fingerprint: &str) -> bool {
        self.check_and_record_at(peer_fingerprint, Instant::now())
    }

    #[cfg(test)]
    pub(crate) fn check_and_record_at(&mut self, peer_fingerprint: &str, now: Instant) -> bool {
        self.check_and_record_at_inner(peer_fingerprint, now)
    }

    #[cfg(not(test))]
    fn check_and_record_at(&mut self, peer_fingerprint: &str, now: Instant) -> bool {
        self.check_and_record_at_inner(peer_fingerprint, now)
    }

    fn check_and_record_at_inner(&mut self, peer_fingerprint: &str, now: Instant) -> bool {
        let window = self.window;
        let entries = self
            .requests
            .entry(peer_fingerprint.to_string())
            .or_default();
        while entries
            .front()
            .is_some_and(|timestamp| now.duration_since(*timestamp) >= window)
        {
            entries.pop_front();
        }

        if entries.len() >= self.max_requests {
            return false;
        }

        entries.push_back(now);
        true
    }
}

pub fn search_rate_limiter() -> SharedRateLimiter {
    SEARCH_RATE_LIMITER
        .get_or_init(|| Arc::new(Mutex::new(SlidingWindowRateLimiter::default_search_limit())))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_requests_within_limit_and_blocks_over_limit() {
        let mut limiter = SlidingWindowRateLimiter::new(2, Duration::from_secs(60));

        assert!(limiter.check_and_record("peer-a"));
        assert!(limiter.check_and_record("peer-a"));
        assert!(!limiter.check_and_record("peer-a"));
    }

    #[test]
    fn resets_after_window_elapses() {
        let mut limiter = SlidingWindowRateLimiter::new(1, Duration::from_secs(60));
        let start = Instant::now();

        assert!(limiter.check_and_record_at("peer-a", start));
        assert!(!limiter.check_and_record_at("peer-a", start + Duration::from_secs(59)));
        assert!(limiter.check_and_record_at("peer-a", start + Duration::from_secs(60)));
    }

    #[test]
    fn limits_peers_independently() {
        let mut limiter = SlidingWindowRateLimiter::new(1, Duration::from_secs(60));

        assert!(limiter.check_and_record("peer-a"));
        assert!(limiter.check_and_record("peer-b"));
        assert!(!limiter.check_and_record("peer-a"));
        assert!(!limiter.check_and_record("peer-b"));
    }
}
