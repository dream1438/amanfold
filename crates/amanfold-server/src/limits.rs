//! Fixed-window rate limits kept in memory (one server process). A
//! multi-instance deployment needs a shared store; see docs/SERVER.md.

use std::{collections::HashMap, sync::Mutex};

use crate::{error::ApiError, now_secs};

/// A rule: at most `max` hits per `window` seconds for one key.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// Hits allowed per window.
    pub max: u32,
    /// Window length in seconds.
    pub window: u64,
}

impl Rule {
    /// `max` hits per `window` seconds.
    pub const fn new(max: u32, window: u64) -> Self {
        Self { max, window }
    }
}

/// Rate limits used by the API.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Pre-login lookups per client address.
    pub prelogin_ip: Rule,
    /// Login attempts per client address.
    pub login_ip: Rule,
    /// Login attempts per email address (protects one account from guessing).
    pub login_email: Rule,
    /// Emails sent (sign-up, recovery) per address.
    pub mail_email: Rule,
    /// Emails requested per client address.
    pub mail_ip: Rule,
    /// Code and proof checks per client address.
    pub verify_ip: Rule,
    /// Invitations sent per account.
    pub invites_account: Rule,
    /// Waitlist confirmation emails per day across all addresses.
    pub waitlist_global: Rule,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            prelogin_ip: Rule::new(60, 60),
            login_ip: Rule::new(30, 300),
            login_email: Rule::new(10, 900),
            mail_email: Rule::new(5, 3600),
            mail_ip: Rule::new(20, 3600),
            verify_ip: Rule::new(30, 300),
            invites_account: Rule::new(50, 86_400),
            waitlist_global: Rule::new(1000, 86_400),
        }
    }
}

const MAX_KEYS: usize = 200_000;

/// The counters.
#[derive(Default)]
pub struct RateLimiter {
    map: Mutex<HashMap<String, (u64, u32)>>,
}

impl RateLimiter {
    /// Count one hit for `key`; fails once the rule's budget is spent.
    pub fn hit(&self, key: &str, rule: Rule) -> Result<(), ApiError> {
        let now = now_secs();
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= MAX_KEYS {
            // Drop expired windows; windows are at most a day long.
            map.retain(|_, (start, _)| now < start.saturating_add(86_400));
            if map.len() >= MAX_KEYS {
                return Err(ApiError::RateLimited);
            }
        }
        let e = map.entry(key.to_string()).or_insert((now, 0));
        if now >= e.0.saturating_add(rule.window) {
            *e = (now, 0);
        }
        if e.1 >= rule.max {
            return Err(ApiError::RateLimited);
        }
        e.1 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_is_enforced_per_key() {
        let l = RateLimiter::default();
        let r = Rule::new(2, 60);
        assert!(l.hit("a", r).is_ok());
        assert!(l.hit("a", r).is_ok());
        assert!(l.hit("a", r).is_err());
        assert!(l.hit("b", r).is_ok());
    }
}
