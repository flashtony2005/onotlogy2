//! 控制平面限流：执行、审批、WebSocket 三条通道独立计数，超限 fail-closed。

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Channel {
    Execution,
    Approval,
    WebSocket,
}

#[derive(Debug, Clone, Copy)]
struct Limit {
    capacity: f64,
    refill_per_second: f64,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    updated_at: Instant,
}

/// 内存 token bucket。每个 `(channel, principal)` 独立计数；锁中毒时拒绝请求，
/// 不恢复为放行，保证限流故障不会退化成无限制执行。
pub(crate) struct RateLimiter {
    buckets: Mutex<HashMap<(Channel, String), Bucket>>,
    limits: [Limit; 3],
    ws_connections: Mutex<HashMap<String, usize>>,
    max_ws_connections: usize,
}

impl RateLimiter {
    pub(crate) fn new() -> Self {
        Self::with_config(10, 60, 30, 60, 20, 60, 4)
    }

    #[cfg(test)]
    pub(crate) fn with_limits(
        execution_capacity: u32,
        execution_window_secs: u64,
        approval_capacity: u32,
        approval_window_secs: u64,
        ws_capacity: u32,
        ws_window_secs: u64,
        max_ws_connections: usize,
    ) -> Self {
        Self::with_config(
            execution_capacity,
            execution_window_secs,
            approval_capacity,
            approval_window_secs,
            ws_capacity,
            ws_window_secs,
            max_ws_connections,
        )
    }

    fn with_config(
        execution_capacity: u32,
        execution_window_secs: u64,
        approval_capacity: u32,
        approval_window_secs: u64,
        ws_capacity: u32,
        ws_window_secs: u64,
        max_ws_connections: usize,
    ) -> Self {
        let limit = |capacity: u32, window_secs: u64| Limit {
            capacity: f64::from(capacity),
            refill_per_second: f64::from(capacity) / window_secs.max(1) as f64,
        };
        Self {
            buckets: Mutex::new(HashMap::new()),
            limits: [
                limit(execution_capacity, execution_window_secs),
                limit(approval_capacity, approval_window_secs),
                limit(ws_capacity, ws_window_secs),
            ],
            ws_connections: Mutex::new(HashMap::new()),
            max_ws_connections: max_ws_connections.max(1),
        }
    }

    fn limit(&self, channel: Channel) -> Limit {
        match channel {
            Channel::Execution => self.limits[0],
            Channel::Approval => self.limits[1],
            Channel::WebSocket => self.limits[2],
        }
    }

    pub(crate) fn allow(&self, channel: Channel, principal: &str) -> bool {
        let Ok(mut buckets) = self.buckets.lock() else {
            return false;
        };
        let limit = self.limit(channel);
        let now = Instant::now();
        let bucket = buckets
            .entry((channel, principal.to_string()))
            .or_insert(Bucket {
                tokens: limit.capacity,
                updated_at: now,
            });
        let elapsed = now.duration_since(bucket.updated_at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * limit.refill_per_second).min(limit.capacity);
        bucket.updated_at = now;
        if bucket.tokens < 1.0 {
            return false;
        }
        bucket.tokens -= 1.0;
        true
    }

    /// 建连同时受 token bucket 与并发连接数限制。调用方必须在 WS 结束时 release。
    pub(crate) fn acquire_ws(self: &Arc<Self>, principal: &str) -> Option<WsLease> {
        if !self.allow(Channel::WebSocket, principal) {
            return None;
        }
        let Ok(mut connections) = self.ws_connections.lock() else {
            return None;
        };
        let active = connections.entry(principal.to_string()).or_default();
        if *active >= self.max_ws_connections {
            return None;
        }
        *active += 1;
        Some(WsLease {
            limiter: Arc::clone(self),
            principal: principal.to_string(),
        })
    }

    pub(crate) fn release_ws(&self, principal: &str) {
        let Ok(mut connections) = self.ws_connections.lock() else {
            return;
        };
        if let Some(active) = connections.get_mut(principal) {
            *active = active.saturating_sub(1);
            if *active == 0 {
                connections.remove(principal);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn retry_after(&self, channel: Channel) -> Duration {
        let limit = self.limit(channel);
        Duration::from_secs((1.0 / limit.refill_per_second.max(f64::MIN_POSITIVE)).ceil() as u64)
    }
}

/// WS 连接租约：无论客户端如何断开，都在 Drop 时释放并发槽位。
pub(crate) struct WsLease {
    limiter: Arc<RateLimiter>,
    principal: String,
}

impl Drop for WsLease {
    fn drop(&mut self) {
        self.limiter.release_ws(&self.principal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_are_independent_and_fail_closed() {
        let limiter = RateLimiter::with_limits(1, 60, 1, 60, 1, 60, 1);
        assert!(limiter.allow(Channel::Execution, "u"));
        assert!(!limiter.allow(Channel::Execution, "u"));
        assert!(limiter.allow(Channel::Approval, "u"));
        assert!(!limiter.allow(Channel::Approval, "u"));
    }

    #[test]
    fn websocket_concurrency_is_released() {
        let limiter = Arc::new(RateLimiter::with_limits(10, 60, 10, 60, 10, 60, 1));
        let lease = limiter.acquire_ws("u");
        assert!(lease.is_some());
        let second = limiter.acquire_ws("u");
        assert!(second.is_none());
        drop(lease);
        assert!(limiter.acquire_ws("u").is_some());
    }
}
