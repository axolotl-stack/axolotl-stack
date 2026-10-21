//! Per-IP rate limiting for unconnected listener traffic.
//!
//! Prevents amplification and admission-flood attacks where attackers spoof victim IPs
//! or cycle through large numbers of source addresses.
//! Local deviation from upstream `listener.go:security`: upstream blocks malformed
//! peers but has no ping/admission rate cap. Preserve the existing configured cap
//! and reject new identities at capacity rather than evicting live rate windows.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(1);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(10);
const STALE_THRESHOLD: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
struct RateWindow {
    count: u32,
    window_start: Instant,
}

impl RateWindow {
    fn new(now: Instant) -> Self {
        Self {
            count: 1,
            window_start: now,
        }
    }

    fn is_stale(&self, now: Instant) -> bool {
        now.duration_since(self.window_start) >= STALE_THRESHOLD
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct IpRateState {
    ping: Option<RateWindow>,
    request1: Option<RateWindow>,
}

#[derive(Debug, Clone, Copy)]
enum ProbeKind {
    Ping,
    Request1,
}

/// Rate limiter for unconnected traffic per IP address.
pub struct PingRateLimiter {
    per_ip: HashMap<IpAddr, IpRateState>,
    max_per_second: u32,
    max_entries: usize,
    last_cleanup: Instant,
    cookie_key: RandomState,
    previous_cookie_key: RandomState,
    rotated_at: Instant,
    blocks: HashMap<IpAddr, Instant>,
}

impl PingRateLimiter {
    /// Create a new rate limiter.
    ///
    /// `max_pings_per_second` is also applied to `OpenConnectionRequest1` attempts.
    pub fn new(max_pings_per_second: u32) -> Self {
        Self::with_max_entries(max_pings_per_second, 16_384)
    }

    pub fn with_max_entries(max_pings_per_second: u32, max_entries: usize) -> Self {
        Self {
            per_ip: HashMap::new(),
            max_per_second: max_pings_per_second,
            max_entries,
            last_cleanup: Instant::now(),
            cookie_key: RandomState::new(),
            previous_cookie_key: RandomState::new(),
            rotated_at: Instant::now(),
            blocks: HashMap::new(),
        }
    }

    /// Check if a ping from this IP is allowed, and record it if so.
    pub fn check_and_record(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.allow_ping(ip, now)
    }

    pub fn allow_ping(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.check_and_record_kind(ip, now, ProbeKind::Ping)
    }

    pub fn allow_request1(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.check_and_record_kind(ip, now, ProbeKind::Request1)
    }

    /// Upstream `handler.go:cookie` and `listener.go:security.tick`: address-bound
    /// stateless cookies, rotated every two seconds with one previous salt accepted.
    /// Deviation: use the standard library's randomly keyed hash instead of CRC32,
    /// avoiding both predictable clock seeds and an additional hashing dependency.
    pub(super) fn cookie(&mut self, peer: SocketAddr, now: Instant) -> u32 {
        self.rotate_cookies(now);
        Self::hash_cookie(&self.cookie_key, peer)
    }

    pub(super) fn valid_cookie(&mut self, peer: SocketAddr, cookie: u32, now: Instant) -> bool {
        self.rotate_cookies(now);
        cookie == Self::hash_cookie(&self.cookie_key, peer)
            || cookie == Self::hash_cookie(&self.previous_cookie_key, peer)
    }

    fn hash_cookie(key: &RandomState, peer: SocketAddr) -> u32 {
        key.hash_one(peer) as u32
    }

    fn rotate_cookies(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.rotated_at);
        if elapsed >= Duration::from_secs(2) {
            self.previous_cookie_key = if elapsed >= Duration::from_secs(4) {
                RandomState::new()
            } else {
                self.cookie_key.clone()
            };
            self.cookie_key = RandomState::new();
            self.rotated_at = now;
        }
    }

    /// Upstream `listener.go:security.blockFor`: malformed traffic blocks its IP
    /// for ten seconds. Unlike the unbounded upstream map, a full local map fails
    /// closed for unknown addresses until the earliest tracked block expires.
    pub(super) fn block(&mut self, ip: IpAddr, now: Instant) {
        self.blocks.retain(|_, expiry| *expiry > now);
        if self.blocks.contains_key(&ip) || self.blocks.len() < self.max_entries.max(1) {
            self.blocks.insert(ip, now + Duration::from_secs(10));
        }
    }

    pub(super) fn blocked(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.blocks.retain(|_, expiry| *expiry > now);
        self.blocks.contains_key(&ip) || self.blocks.len() >= self.max_entries.max(1)
    }
    fn check_and_record_kind(&mut self, ip: IpAddr, now: Instant, kind: ProbeKind) -> bool {
        if self.max_per_second == 0 {
            return true;
        }

        if now.duration_since(self.last_cleanup) > CLEANUP_INTERVAL {
            self.cleanup(now);
            self.last_cleanup = now;
        }

        if !self.per_ip.contains_key(&ip) && self.per_ip.len() >= self.max_entries {
            if self.max_entries == 0 {
                return false;
            }
            self.cleanup(now);
            if self.per_ip.len() >= self.max_entries {
                return false;
            }
        }

        let state = self.per_ip.entry(ip).or_default();
        let slot = match kind {
            ProbeKind::Ping => &mut state.ping,
            ProbeKind::Request1 => &mut state.request1,
        };

        match slot {
            Some(window) => {
                if now.duration_since(window.window_start) >= WINDOW {
                    *window = RateWindow::new(now);
                    true
                } else if window.count < self.max_per_second {
                    window.count += 1;
                    true
                } else {
                    false
                }
            }
            None => {
                *slot = Some(RateWindow::new(now));
                true
            }
        }
    }

    fn cleanup(&mut self, now: Instant) {
        self.per_ip.retain(|_, state| {
            let ping_live = state.ping.is_some_and(|window| !window.is_stale(now));
            let request1_live = state.request1.is_some_and(|window| !window.is_stale(now));
            ping_live || request1_live
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_address_bound_and_expire_after_two_rotations() {
        let mut limiter = PingRateLimiter::new(10);
        let now = Instant::now();
        let peer = "127.0.0.1:19132".parse().unwrap();
        let cookie = limiter.cookie(peer, now);
        assert!(limiter.valid_cookie(peer, cookie, now));
        assert!(!limiter.valid_cookie("127.0.0.1:19133".parse().unwrap(), cookie, now));
        assert!(limiter.valid_cookie(peer, cookie, now + Duration::from_secs(2)));
        assert!(!limiter.valid_cookie(peer, cookie, now + Duration::from_secs(4)));
    }

    #[test]
    fn blocks_expire_and_are_shared_by_source_ip() {
        let mut limiter = PingRateLimiter::with_max_entries(1, 2);
        let now = Instant::now();
        let ip = "127.0.0.1".parse().unwrap();
        limiter.block(ip, now);
        assert!(limiter.blocked(ip, now));
        assert!(!limiter.blocked(ip, now + Duration::from_secs(10)));
    }
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let mut limiter = PingRateLimiter::new(10);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        for _ in 0..10 {
            assert!(limiter.check_and_record(ip, now));
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let mut limiter = PingRateLimiter::new(5);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        for _ in 0..5 {
            assert!(limiter.check_and_record(ip, now));
        }

        assert!(!limiter.check_and_record(ip, now));
    }

    #[test]
    fn test_rate_limiter_resets_after_window() {
        let mut limiter = PingRateLimiter::new(2);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        assert!(limiter.check_and_record(ip, now));
        assert!(limiter.check_and_record(ip, now));
        assert!(!limiter.check_and_record(ip, now));

        let later = now + Duration::from_secs(1);
        assert!(limiter.check_and_record(ip, later));
    }

    #[test]
    fn test_rate_limiter_per_ip() {
        let mut limiter = PingRateLimiter::new(2);
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        let now = Instant::now();

        assert!(limiter.check_and_record(ip1, now));
        assert!(limiter.check_and_record(ip1, now));
        assert!(!limiter.check_and_record(ip1, now));

        assert!(limiter.check_and_record(ip2, now));
        assert!(limiter.check_and_record(ip2, now));
        assert!(!limiter.check_and_record(ip2, now));
    }

    #[test]
    fn test_rate_limiter_disabled_when_zero() {
        let mut limiter = PingRateLimiter::new(0);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let now = Instant::now();

        for _ in 0..100 {
            assert!(limiter.check_and_record(ip, now));
            assert!(limiter.allow_request1(ip, now));
        }
    }

    #[test]
    fn test_request1_has_its_own_bucket() {
        let mut limiter = PingRateLimiter::new(1);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let now = Instant::now();

        assert!(limiter.allow_ping(ip, now));
        assert!(limiter.allow_request1(ip, now));
        assert!(!limiter.allow_ping(ip, now));
        assert!(!limiter.allow_request1(ip, now));
    }

    #[test]
    fn test_rate_limiter_caps_tracked_ip_cardinality() {
        let mut limiter = PingRateLimiter::with_max_entries(10, 2);
        let now = Instant::now();
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let ip3 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));

        assert!(limiter.allow_ping(ip1, now));
        assert!(limiter.allow_request1(ip2, now + Duration::from_millis(1)));
        assert!(!limiter.allow_ping(ip3, now + Duration::from_millis(2)));

        assert_eq!(limiter.per_ip.len(), 2);
        assert!(limiter.per_ip.contains_key(&ip1));
        assert!(limiter.per_ip.contains_key(&ip2));
        assert!(!limiter.per_ip.contains_key(&ip3));
    }
}
