use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Copy)]
pub struct RateLimit {
    max_events: u32,
    window: Duration,
}

impl RateLimit {
    pub const fn new(max_events: u32, window: Duration) -> Self {
        Self { max_events, window }
    }
}

pub struct RateBucket {
    window_started_at: Instant,
    events: u32,
}

impl RateBucket {
    pub fn new(now: Instant) -> Self {
        Self {
            window_started_at: now,
            events: 0,
        }
    }
}

pub async fn allow_event(
    buckets: &Mutex<HashMap<IpAddr, RateBucket>>,
    ip: IpAddr,
    limit: RateLimit,
) -> bool {
    let now = Instant::now();
    let mut buckets = buckets.lock().await;
    buckets.retain(|_, bucket| now.duration_since(bucket.window_started_at) <= limit.window * 2);
    let bucket = buckets.entry(ip).or_insert(RateBucket {
        window_started_at: now,
        events: 0,
    });

    if now.duration_since(bucket.window_started_at) > limit.window {
        bucket.window_started_at = now;
        bucket.events = 0;
    }

    if bucket.events >= limit.max_events {
        return false;
    }

    bucket.events += 1;
    true
}

pub async fn allow_global_event(bucket: &Mutex<RateBucket>, limit: RateLimit) -> bool {
    let now = Instant::now();
    let mut bucket = bucket.lock().await;

    if now.duration_since(bucket.window_started_at) > limit.window {
        bucket.window_started_at = now;
        bucket.events = 0;
    }

    if bucket.events >= limit.max_events {
        return false;
    }

    bucket.events += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_after_limit() {
        let buckets = Mutex::new(HashMap::new());
        let ip = "127.0.0.1".parse().unwrap();
        let limit = RateLimit::new(2, Duration::from_secs(60));

        assert!(allow_event(&buckets, ip, limit).await);
        assert!(allow_event(&buckets, ip, limit).await);
        assert!(!allow_event(&buckets, ip, limit).await);
    }
}
