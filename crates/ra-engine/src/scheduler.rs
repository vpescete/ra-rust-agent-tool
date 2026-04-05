use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore, SemaphorePermit};
use tokio::time::{Duration, Instant};

use ra_core::agent::AgentPriority;
use ra_core::config::RateLimitConfig;
use ra_core::error::{RaError, RaResult};

/// Priority-aware scheduler with rate limiting and backpressure
pub struct PriorityScheduler {
    semaphore: Arc<Semaphore>,
    rate_state: Arc<Mutex<RateState>>,
    _config: RateLimitConfig,
}

struct RateState {
    /// Timestamp until which all requests should be delayed (backpressure)
    backoff_until: Option<Instant>,
    /// Sliding window: timestamps of recent requests
    recent_requests: Vec<Instant>,
    /// Requests per minute limit
    rpm_limit: u32,
}

/// RAII permit — dropping it releases the scheduling slot
pub struct SchedulerPermitGuard<'a> {
    _permit: SemaphorePermit<'a>,
}

impl PriorityScheduler {
    pub fn new(max_concurrency: usize, config: &RateLimitConfig) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            rate_state: Arc::new(Mutex::new(RateState {
                backoff_until: None,
                recent_requests: Vec::new(),
                rpm_limit: config.requests_per_minute,
            })),
            _config: config.clone(),
        }
    }

    /// Acquire a scheduling slot. Blocks until available.
    /// Respects: concurrency limit, rate limit, backpressure.
    pub async fn acquire(&self, _priority: AgentPriority) -> RaResult<SchedulerPermitGuard<'_>> {
        // 1. Wait for backpressure to clear
        self.wait_backpressure().await;

        // 2. Wait for rate limit
        self.wait_rate_limit().await;

        // 3. Acquire semaphore slot
        let permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| RaError::RateLimited { retry_after_ms: 0 })?;

        // 4. Record request
        {
            let mut state = self.rate_state.lock().await;
            state.recent_requests.push(Instant::now());
        }

        Ok(SchedulerPermitGuard { _permit: permit })
    }

    /// Signal global backpressure from an api_retry event
    pub async fn notify_rate_limited(&self, retry_after_ms: u64) {
        let mut state = self.rate_state.lock().await;
        let until = Instant::now() + Duration::from_millis(retry_after_ms);
        state.backoff_until = Some(until);
        tracing::warn!(
            retry_after_ms,
            "Global backpressure engaged for {}ms",
            retry_after_ms
        );
    }

    /// Get current number of available slots
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    async fn wait_backpressure(&self) {
        loop {
            let until = {
                let state = self.rate_state.lock().await;
                state.backoff_until
            };
            match until {
                Some(deadline) if deadline > Instant::now() => {
                    let wait = deadline - Instant::now();
                    tracing::debug!("Waiting {}ms for backpressure", wait.as_millis());
                    tokio::time::sleep(wait).await;
                }
                _ => break,
            }
        }
        // Clear expired backpressure
        let mut state = self.rate_state.lock().await;
        if let Some(until) = state.backoff_until {
            if until <= Instant::now() {
                state.backoff_until = None;
            }
        }
    }

    async fn wait_rate_limit(&self) {
        loop {
            let should_wait = {
                let mut state = self.rate_state.lock().await;
                let window = Duration::from_secs(60);
                let now = Instant::now();

                // Clean old entries
                state
                    .recent_requests
                    .retain(|&t| now.duration_since(t) < window);

                if state.recent_requests.len() >= state.rpm_limit as usize {
                    // Calculate how long to wait
                    if let Some(&oldest) = state.recent_requests.first() {
                        let elapsed = now.duration_since(oldest);
                        if elapsed < window {
                            Some(window - elapsed)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            match should_wait {
                Some(wait) => {
                    tracing::debug!("Rate limit: waiting {}ms", wait.as_millis());
                    tokio::time::sleep(wait).await;
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: 100,
            tokens_per_minute: 400_000,
            burst_multiplier: 1.5,
            backoff_base_ms: 1000,
            backoff_max_ms: 60_000,
        }
    }

    #[tokio::test]
    async fn test_basic_acquire_release() {
        let scheduler = PriorityScheduler::new(2, &test_config());
        assert_eq!(scheduler.available_permits(), 2);

        let _p1 = scheduler.acquire(AgentPriority::Normal).await.unwrap();
        assert_eq!(scheduler.available_permits(), 1);

        let _p2 = scheduler.acquire(AgentPriority::Normal).await.unwrap();
        assert_eq!(scheduler.available_permits(), 0);

        drop(_p1);
        assert_eq!(scheduler.available_permits(), 1);
    }

    #[tokio::test]
    async fn test_backpressure() {
        let scheduler = PriorityScheduler::new(4, &test_config());

        // Set backpressure for 100ms
        scheduler.notify_rate_limited(100).await;

        let start = Instant::now();
        let _p = scheduler.acquire(AgentPriority::Normal).await.unwrap();
        let elapsed = start.elapsed();

        // Should have waited at least ~100ms
        assert!(elapsed.as_millis() >= 90);
    }
}
