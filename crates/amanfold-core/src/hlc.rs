//! Hybrid logical clock: timestamps that order edits across devices even when
//! device clocks disagree a little, used for per-field merge.

use serde::{Deserialize, Serialize};

/// A hybrid logical timestamp. Ordering is by `millis`, then `counter`, then
/// `device`, so two different devices never produce equal timestamps.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Hlc {
    /// Physical time, milliseconds since the Unix epoch.
    pub millis: u64,
    /// Logical counter for events within the same millisecond.
    pub counter: u32,
    /// Random per-device id, the final tie-breaker.
    pub device: u64,
}

impl Hlc {
    /// The zero timestamp (older than everything).
    pub const ZERO: Self = Self {
        millis: 0,
        counter: 0,
        device: 0,
    };
}

/// Remote timestamps further than this into the future do not advance our
/// clock, so one device with a broken clock cannot poison everyone else's.
pub const MAX_FORWARD_DRIFT_MS: u64 = 24 * 60 * 60 * 1000;

/// Per-device clock.
#[derive(Debug, Clone)]
pub struct Clock {
    device: u64,
    last: Hlc,
}

impl Clock {
    /// A clock for `device` (store the id with the device's local state).
    pub fn new(device: u64) -> Self {
        Self {
            device,
            last: Hlc {
                device,
                ..Hlc::ZERO
            },
        }
    }

    /// A clock with a fresh random device id.
    pub fn random() -> Self {
        Self::new(rand_core::RngCore::next_u64(&mut rand_core::OsRng))
    }

    /// Timestamp for a local event at physical time `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        self.last = if now_ms > self.last.millis {
            Hlc {
                millis: now_ms,
                counter: 0,
                device: self.device,
            }
        } else {
            Hlc {
                millis: self.last.millis,
                counter: self.last.counter.saturating_add(1),
                device: self.device,
            }
        };
        self.last
    }

    /// Account for a timestamp received from another device.
    pub fn observe(&mut self, remote: Hlc, now_ms: u64) {
        if remote.millis > now_ms.saturating_add(MAX_FORWARD_DRIFT_MS) {
            return;
        }
        if (remote.millis, remote.counter) > (self.last.millis, self.last.counter) {
            self.last = Hlc {
                millis: remote.millis,
                counter: remote.counter,
                device: self.device,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_even_if_wall_clock_goes_back() {
        let mut c = Clock::new(1);
        let a = c.tick(1000);
        let b = c.tick(900);
        let d = c.tick(1000);
        assert!(a < b && b < d);
    }

    #[test]
    fn observe_orders_after_remote() {
        let mut c = Clock::new(1);
        c.observe(
            Hlc {
                millis: 5000,
                counter: 3,
                device: 2,
            },
            1000,
        );
        assert!(
            c.tick(1000)
                > Hlc {
                    millis: 5000,
                    counter: 3,
                    device: 2
                }
        );
    }

    #[test]
    fn far_future_remote_ignored() {
        let mut c = Clock::new(1);
        c.observe(
            Hlc {
                millis: u64::MAX,
                counter: 0,
                device: 2,
            },
            1000,
        );
        assert_eq!(c.tick(1000).millis, 1000);
    }
}
