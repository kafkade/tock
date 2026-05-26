//! In-memory token-bucket rate limiter and quota enforcement.
//!
//! Each account gets a bucket that refills at the tier's
//! `requests_per_minute` rate. When a request arrives, one token is
//! consumed. If the bucket is empty, the request is rejected with
//! 429 Too Many Requests.
//!
//! Quotas (storage bytes, device count) are checked against the
//! database at request time.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::billing::Tier;

/// Per-account rate limiter state.
#[allow(dead_code)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    rate_per_sec: f64,
    capacity: f64,
}

#[allow(dead_code)]
impl Bucket {
    fn new(tier: Tier) -> Self {
        let rpm = f64::from(tier.requests_per_minute());
        let rate = rpm / 60.0;
        Self {
            tokens: rpm, // start full
            last_refill: Instant::now(),
            rate_per_sec: rate,
            capacity: rpm,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = elapsed
            .mul_add(self.rate_per_sec, self.tokens)
            .min(self.capacity);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Thread-safe rate limiter keyed by account id.
#[allow(dead_code)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

#[allow(dead_code, clippy::significant_drop_tightening)]
impl RateLimiter {
    /// Create a new empty rate limiter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Try to consume a request token for `account_id` at the given
    /// `tier`. Returns `true` if allowed, `false` if rate-limited.
    pub fn check(&self, account_id: &str, tier: Tier) -> bool {
        if tier == Tier::SelfHosted {
            return true;
        }
        let Ok(mut buckets) = self.buckets.lock() else {
            return true; // poisoned → fail open
        };
        let bucket = buckets
            .entry(account_id.to_string())
            .or_insert_with(|| Bucket::new(tier));
        bucket.try_consume()
    }

    /// Remove an account's bucket (e.g., on tier change).
    pub fn reset(&self, account_id: &str) {
        if let Ok(mut buckets) = self.buckets.lock() {
            buckets.remove(account_id);
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn allows_within_limit() {
        let rl = RateLimiter::new();
        // Free tier = 30 rpm → starts with 30 tokens.
        for _ in 0..30 {
            assert!(rl.check("acct1", Tier::Free));
        }
    }

    #[test]
    fn rejects_over_limit() {
        let rl = RateLimiter::new();
        // Drain all 30 tokens.
        for _ in 0..30 {
            rl.check("acct1", Tier::Free);
        }
        // Next should be rejected (no time has passed to refill).
        assert!(!rl.check("acct1", Tier::Free));
    }

    #[test]
    fn self_hosted_always_allowed() {
        let rl = RateLimiter::new();
        for _ in 0..1000 {
            assert!(rl.check("acct", Tier::SelfHosted));
        }
    }

    #[test]
    fn different_accounts_independent() {
        let rl = RateLimiter::new();
        for _ in 0..30 {
            rl.check("acct1", Tier::Free);
        }
        assert!(!rl.check("acct1", Tier::Free));
        // acct2 should still have tokens.
        assert!(rl.check("acct2", Tier::Free));
    }

    #[test]
    fn reset_clears_bucket() {
        let rl = RateLimiter::new();
        for _ in 0..30 {
            rl.check("acct1", Tier::Free);
        }
        assert!(!rl.check("acct1", Tier::Free));
        rl.reset("acct1");
        assert!(rl.check("acct1", Tier::Free));
    }
}
