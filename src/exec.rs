use crate::config::config;
use crate::helpers;
use crate::obs;
use crate::types::AppError;
use rocket::tokio::sync::Semaphore;
use rocket::tokio::task::spawn_blocking;
use rocket::tokio::time::timeout;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

static QUEUE_DEPTH: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT: AtomicU64 = AtomicU64::new(0);

fn semaphore() -> &'static Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(config().max_concurrency)))
}

/// Number of requests waiting for a render slot
pub fn queue_depth() -> u64 {
    QUEUE_DEPTH.load(Ordering::Relaxed)
}

/// Number of renders currently running on a blocking thread
pub fn in_flight() -> u64 {
    IN_FLIGHT.load(Ordering::Relaxed)
}

/// Run a blocking render off the async executor, under a global concurrency limit.
///
/// The rendering pipeline waits on child processes with `try_wait()` loops, so calling it
/// directly from an `async fn` pins a tokio worker for the whole conversion: a burst of
/// conversions could starve `/api/health` and get a healthy service restarted by the
/// production watchdog. Everything that spawns pandoc/weasyprint/qpdf goes through here.
pub async fn offload<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let queue_timeout = config().queue_timeout;

    let permit = {
        // The guard also covers the client hanging up while queued, which drops this future
        let _queued = Counter::enter(&QUEUE_DEPTH);
        match timeout(queue_timeout, semaphore().clone().acquire_owned()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                // The semaphore is a static that is never closed; treat it as a hard failure
                obs::record_render_failure("semaphore_closed");
                return Err(AppError::ProcessFailed {
                    message: "Render queue is unavailable".to_string(),
                    stderr: String::new(),
                });
            }
            Err(_) => {
                obs::record_render_failure("queue_timeout");
                return Err(AppError::TooManyRequests(format!(
                    "The service is saturated: no render slot within {}s",
                    queue_timeout.as_secs()
                )));
            }
        }
    };

    // The permit and the gauge live inside the closure, not around the await: a blocking
    // task cannot be cancelled, so a client that hangs up would otherwise free its slot
    // while its weasyprint keeps running — the concurrency limit would bound nothing.
    let outcome = spawn_blocking(move || {
        let _permit = permit;
        let _running = Counter::enter(&IN_FLIGHT);
        // One deadline for the whole job: per-process limits do not compose, and a job
        // that outlives the proxy timeout keeps its slot for a client that already left.
        let _budget = helpers::Budget::start(config().render_deadline);
        f()
    })
    .await;

    match outcome {
        Ok(result) => result,
        Err(err) => {
            // A panic inside the render thread must surface as a 500, never unwind a worker
            obs::record_render_failure("worker_panic");
            error!("Render task did not complete: {}", err);
            Err(AppError::ProcessFailed {
                message: "Rendering task failed".to_string(),
                stderr: err.to_string(),
            })
        }
    }
}

/// Atomic gauge that decrements even when the future it lives in is cancelled
struct Counter(&'static AtomicU64);

impl Counter {
    fn enter(counter: &'static AtomicU64) -> Counter {
        counter.fetch_add(1, Ordering::Relaxed);
        Counter(counter)
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
