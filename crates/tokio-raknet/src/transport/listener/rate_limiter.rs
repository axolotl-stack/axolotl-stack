//! Per-IP rate limiting for UnconnectedPing responses.
//!
//! Prevents amplification attacks where attackers spoof victim IPs to flood them
//! with large pong responses.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Rate limiter for ping responses per IP address.
pub struct PingRateLimiter {
    /// Map of IP -> (count, window_start)
    pings_per_ip: HashMap<IpAddr, (u32, Instant)>,
    /// Maximum pings allowed per second per IP
    max_pings_per_second: u32,
    /// Last time we cleaned up stale entries
    last_cleanup: Instant,
}

impl PingRateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Arguments
    /// * `max_pings_per_second` - Maximum pings allowed per second per IP. Set to 0 to disable.
    pub fn new(max_pings_per_second: u32) -> Self {
        Self {
            pings_per_ip: HashMap::new(),
            max_pings_per_second,
            last_cleanup: Instant::now(),
        }
    }

    /// Check if a ping from this IP is allowed, and record it if so.
    ///
    /// Returns `true` if the ping should be responded to, `false` if rate limited.
    pub fn check_and_record(&mut self, ip: IpAddr, now: Instant) -> bool {
        // Rate limiting disabled
        if self.max_pings_per_second == 0 {
            return true;
        }

        // Periodic cleanup of stale entries (every 10 seconds)
        if now.duration_since(self.last_cleanup) > Duration::from_secs(10) {
            self.cleanup(now);
            self.last_cleanup = now;
        }

        let window = Duration::from_secs(1);

        match self.pings_per_ip.get_mut(&ip) {
            Some((count, window_start)) => {
                // Check if we're in a new window
                if now.duration_since(*window_start) >= window {
                    // Reset window
                    *window_start = now;
                    *count = 1;
                    true
                } else if *count < self.max_pings_per_second {
                    // Within window and under limit
                    *count += 1;
                    true
                } else {
                    // Rate limited
                    false
                }
            }
            None => {
                // First ping from this IP
                self.pings_per_ip.insert(ip, (1, now));
                true
            }
        }
    }

    /// Remove entries older than 60 seconds to prevent memory growth.
    fn cleanup(&mut self, now: Instant) {
        let stale_threshold = Duration::from_secs(60);
        self.pings_per_ip
            .retain(|_, (_, window_start)| now.duration_since(*window_start) < stale_threshold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let mut limiter = PingRateLimiter::new(10);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        // Should allow 10 pings
        for _ in 0..10 {
            assert!(limiter.check_and_record(ip, now));
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let mut limiter = PingRateLimiter::new(5);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        // First 5 should be allowed
        for _ in 0..5 {
            assert!(limiter.check_and_record(ip, now));
        }

        // 6th should be blocked
        assert!(!limiter.check_and_record(ip, now));
    }

    #[test]
    fn test_rate_limiter_resets_after_window() {
        let mut limiter = PingRateLimiter::new(2);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        // Use up the limit
        assert!(limiter.check_and_record(ip, now));
        assert!(limiter.check_and_record(ip, now));
        assert!(!limiter.check_and_record(ip, now));

        // After 1 second, should reset
        let later = now + Duration::from_secs(1);
        assert!(limiter.check_and_record(ip, later));
    }

    #[test]
    fn test_rate_limiter_per_ip() {
        let mut limiter = PingRateLimiter::new(2);
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        let now = Instant::now();

        // Each IP gets its own limit
        assert!(limiter.check_and_record(ip1, now));
        assert!(limiter.check_and_record(ip1, now));
        assert!(!limiter.check_and_record(ip1, now)); // ip1 blocked

        assert!(limiter.check_and_record(ip2, now)); // ip2 still allowed
        assert!(limiter.check_and_record(ip2, now));
        assert!(!limiter.check_and_record(ip2, now)); // ip2 blocked
    }

    #[test]
    fn test_rate_limiter_disabled_when_zero() {
        let mut limiter = PingRateLimiter::new(0);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        // Should always allow when disabled
        for _ in 0..100 {
            assert!(limiter.check_and_record(ip, now));
        }
    }
}
