use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::GatewayError;

/// Basic auth + per-user token bucket rate limiting.
pub struct AuthManager {
    bearer_token: String,
    limiter: RateLimiter,
}

impl AuthManager {
    pub fn new(bearer_token: impl Into<String>, requests_per_minute: u32) -> Self {
        Self {
            bearer_token: bearer_token.into(),
            limiter: RateLimiter::new(requests_per_minute, Duration::from_secs(60)),
        }
    }

    pub fn validate(&mut self, provided_token: &str, user_id: &str) -> Result<(), GatewayError> {
        if provided_token != self.bearer_token {
            return Err(GatewayError::AuthError("invalid bearer token".into()));
        }
        self.limiter.check(user_id)
    }
}

pub struct RateLimiter {
    capacity: u32,
    refill_window: Duration,
    buckets: HashMap<String, Bucket>,
}

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(capacity: u32, refill_window: Duration) -> Self {
        Self {
            capacity,
            refill_window,
            buckets: HashMap::new(),
        }
    }

    pub fn check(&mut self, key: &str) -> Result<(), GatewayError> {
        let now = Instant::now();
        let entry = self.buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity as f64,
            last_refill: now,
        });

        let elapsed = now.duration_since(entry.last_refill);
        let refill_rate_per_sec = self.capacity as f64 / self.refill_window.as_secs_f64();
        entry.tokens =
            (entry.tokens + elapsed.as_secs_f64() * refill_rate_per_sec).min(self.capacity as f64);
        entry.last_refill = now;

        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            Ok(())
        } else {
            Err(GatewayError::RateLimited(format!(
                "rate limit exceeded for '{}'",
                key
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_rejects_bad_token() {
        let mut auth = AuthManager::new("secret", 5);
        let res = auth.validate("wrong", "u1");
        assert!(matches!(res, Err(GatewayError::AuthError(_))));
    }

    #[test]
    fn rate_limit_blocks_when_capacity_exhausted() {
        let mut auth = AuthManager::new("secret", 1);
        assert!(auth.validate("secret", "u1").is_ok());
        let res = auth.validate("secret", "u1");
        assert!(matches!(res, Err(GatewayError::RateLimited(_))));
    }
}
