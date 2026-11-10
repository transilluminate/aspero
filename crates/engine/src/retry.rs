//! Retrying one provider operation under its [`ResiliencePolicy`].

use std::future::Future;

use aspero_core::{Error, ResiliencePolicy};
use tokio::time::Instant;

/// Returns a random number from 0 to 1 that scales each backoff sleep: 0
/// means retry immediately, 1 means sleep the full ceiling. Swappable so
/// tests can fix it and get exact timings.
pub type Jitter = fn() -> f64;

/// Runs `op` until it succeeds, fails with a non-retryable error, exhausts
/// `max_retries`, or would overrun `deadline`.
///
/// Sleeps use full-jitter backoff, stretched to a server's `Retry-After` when
/// that is longer. The whole call, sleeps included, is bounded by the
/// deadline; an attempt still running at the deadline is abandoned with
/// [`Error::Timeout`].
pub async fn with_retries<T, F, Fut>(
    policy: &ResiliencePolicy,
    jitter: Jitter,
    mut op: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let deadline = Instant::now() + policy.deadline;
    let mut retry = 0;
    loop {
        let error = match tokio::time::timeout_at(deadline, op()).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(e)) => e,
            Err(_elapsed) => return Err(Error::Timeout),
        };
        if !error.is_retryable() || retry >= policy.max_retries {
            return Err(error);
        }
        let mut delay = policy.backoff(retry, jitter());
        if let Error::RateLimited {
            retry_after: Some(after),
        } = &error
        {
            delay = delay.max(*after);
        }
        if Instant::now() + delay >= deadline {
            return Err(error);
        }
        tracing::info!(
            target: "aspero::retry",
            retry = retry + 1, delay_ms = delay.as_millis() as u64, error = %error,
            "backing off"
        );
        tokio::time::sleep(delay).await;
        retry += 1;
    }
}

/// Jitter from a thread-local fast RNG.
pub fn random_jitter() -> f64 {
    fastrand::f64()
}

/// Always the full ceiling; for tests.
#[cfg(test)]
pub fn full_jitter() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;

    fn policy() -> ResiliencePolicy {
        let mut p = ResiliencePolicy::default();
        p.max_retries = 3;
        p.backoff_base = Duration::from_millis(100);
        p.backoff_cap = Duration::from_secs(10);
        p.deadline = Duration::from_secs(60);
        p
    }

    /// An operation failing with `errors` in turn, then succeeding.
    fn scripted(
        errors: Vec<Error>,
    ) -> (
        impl FnMut() -> std::future::Ready<Result<u32, Error>>,
        std::sync::Arc<AtomicU32>,
    ) {
        let calls = std::sync::Arc::new(AtomicU32::new(0));
        let counter = calls.clone();
        let op = move || {
            let n = counter.fetch_add(1, Ordering::SeqCst) as usize;
            std::future::ready(match errors.get(n) {
                Some(e) => Err(e.clone()),
                None => Ok(n as u32),
            })
        };
        (op, calls)
    }

    fn server_error() -> Error {
        Error::Server {
            status: 503,
            message: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retries_transient_errors_with_doubling_backoff() {
        let (op, calls) = scripted(vec![server_error(), Error::Timeout]);
        let start = Instant::now();
        assert_eq!(with_retries(&policy(), full_jitter, op).await.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(start.elapsed(), Duration::from_millis(100 + 200));
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_stretches_the_backoff() {
        let limited = Error::RateLimited {
            retry_after: Some(Duration::from_secs(2)),
        };
        let (op, _) = scripted(vec![limited]);
        let start = Instant::now();
        with_retries(&policy(), full_jitter, op).await.unwrap();
        assert_eq!(start.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn non_retryable_errors_return_at_once() {
        let (op, calls) = scripted(vec![Error::QuotaExhausted { resets_at: None }]);
        let err = with_retries(&policy(), full_jitter, op).await.unwrap_err();
        assert!(matches!(err, Error::QuotaExhausted { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_retries_with_the_last_error() {
        let (op, calls) = scripted(vec![server_error(); 10]);
        let err = with_retries(&policy(), full_jitter, op).await.unwrap_err();
        assert!(matches!(err, Error::Server { status: 503, .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_before_a_sleep_would_pass_the_deadline() {
        let mut p = policy();
        p.deadline = Duration::from_millis(250);
        let (op, calls) = scripted(vec![server_error(); 10]);
        let start = Instant::now();
        with_retries(&p, full_jitter, op).await.unwrap_err();
        // Sleeps 100ms; the next 200ms sleep would end at 300ms > 250ms.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(start.elapsed(), Duration::from_millis(100));
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_attempt_times_out_at_the_deadline() {
        let mut p = policy();
        p.deadline = Duration::from_secs(1);
        let err = with_retries(&p, full_jitter, || {
            std::future::pending::<Result<(), Error>>()
        })
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Timeout));
    }
}
