use http::HeaderMap;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct StatisticsSnapshot {
    pub requests: u64,
    pub pending: usize,
    pub blocked: u64,
    pub throttled: u64,
    pub timeouts: u64,
    pub responses_2xx: u64,
    pub responses_4xx: u64,
    pub responses_5xx: u64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub websocket_connections: usize,
}

#[derive(Default)]
pub(crate) struct RequestStatistics {
    pub(crate) requests: AtomicU64,
    pub(crate) pending: AtomicUsize,
    pub(crate) blocked: AtomicU64,
    pub(crate) throttled: AtomicU64,
    pub(crate) timeouts: AtomicU64,
    pub(crate) responses_2xx: AtomicU64,
    pub(crate) responses_4xx: AtomicU64,
    pub(crate) responses_5xx: AtomicU64,
    pub(crate) downloaded_bytes: AtomicU64,
    pub(crate) uploaded_bytes: AtomicU64,
    pub(crate) websocket_connections: AtomicUsize,
    limits: Mutex<HashMap<String, (Instant, u64)>>,
}

impl RequestStatistics {
    pub(crate) fn snapshot(&self) -> StatisticsSnapshot {
        StatisticsSnapshot {
            requests: self.requests.load(Ordering::Relaxed),
            pending: self.pending.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            throttled: self.throttled.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            responses_2xx: self.responses_2xx.load(Ordering::Relaxed),
            responses_4xx: self.responses_4xx.load(Ordering::Relaxed),
            responses_5xx: self.responses_5xx.load(Ordering::Relaxed),
            downloaded_bytes: self.downloaded_bytes.load(Ordering::Relaxed),
            uploaded_bytes: self.uploaded_bytes.load(Ordering::Relaxed),
            websocket_connections: self.websocket_connections.load(Ordering::Relaxed),
        }
    }
}

pub(crate) struct PendingRequest(pub(crate) Arc<RequestStatistics>);

impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) fn request_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
        })
        .unwrap_or("unknown")
        .to_string()
}

pub(crate) fn ip_is_blacklisted(configured: &str, ip: &str) -> bool {
    configured
        .split([',', ';', '|'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .any(|value| {
            value == "*"
                || value == ip
                || value
                    .strip_suffix('*')
                    .is_some_and(|prefix| ip.starts_with(prefix))
        })
}

pub(crate) fn request_limit_exceeded(stats: &RequestStatistics, ip: &str, limit: u64) -> bool {
    let mut limits = stats
        .limits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    limits.retain(|_, (started, _)| now.duration_since(*started) < Duration::from_secs(60));
    let entry = limits.entry(ip.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0) >= Duration::from_secs(60) {
        *entry = (now, 0);
    }
    entry.1 += 1;
    entry.1 > limit
}
