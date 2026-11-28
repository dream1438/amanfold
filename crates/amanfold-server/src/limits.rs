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
        }
    }
}

const MAX_KEYS: usize = 200_000;

/// One key's window.
#[derive(Clone, Copy)]
struct Window {
    start: u64,
    hits: u32,
    rule: Rule,
}

impl Window {
    fn expired(&self, now: u64) -> bool {
        now >= self.start.saturating_add(self.rule.window)
    }
    fn blocked(&self) -> bool {
        self.hits >= self.rule.max
    }
}

/// The counters.
#[derive(Default)]
pub struct RateLimiter {
    map: Mutex<HashMap<String, Window>>,
}

impl RateLimiter {
    /// Count one hit for `key`; fails once the rule's budget is spent.
    pub fn hit(&self, key: &str, rule: Rule) -> Result<(), ApiError> {
        let now = now_secs();
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= MAX_KEYS && !map.contains_key(key) {
            // Drop windows that are over, each by its own rule.
            map.retain(|_, w| !w.expired(now));
            if map.len() >= MAX_KEYS {
                // Still full (a flood of fresh addresses). Never refuse
                // everyone: forget the keys that are not blocked, and if
                // even that is not enough, start over.
                map.retain(|_, w| w.blocked());
                if map.len() >= MAX_KEYS / 2 {
                    map.clear();
                }
            }
        }
        let w = map.entry(key.to_string()).or_insert(Window {
            start: now,
            hits: 0,
            rule,
        });
        w.rule = rule;
        if w.expired(now) {
            w.start = now;
            w.hits = 0;
        }
        if w.blocked() {
            return Err(ApiError::RateLimited);
        }
        w.hits += 1;
        Ok(())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.lock().unwrap().len()
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

    #[test]
    fn a_full_map_never_locks_everyone_out() {
        let l = RateLimiter::default();
        let r = Rule::new(60, 60);
        for i in 0..MAX_KEYS {
            l.hit(&format!("flood-{i}"), r).unwrap();
        }
        assert!(l.hit("fresh", r).is_ok());
        assert!(l.len() < MAX_KEYS);
        // A blocked key stays blocked through the clean-up.
        let tight = Rule::new(1, 3600);
        let l = RateLimiter::default();
        l.hit("bad", tight).unwrap();
        for i in 0..MAX_KEYS {
            l.hit(&format!("flood-{i}"), r).unwrap();
        }
        assert!(l.hit("bad", tight).is_err());
    }
}
