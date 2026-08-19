//! Asynchronous work.
//!
//! Everything this service did used to fit under `PDF_RENDER_DEADLINE_SECS`, because
//! everything it did was render a document it had just been handed. OCR on two hundred
//! scanned pages, a LibreOffice conversion and a Ghostscript pass on a hundred-megabyte
//! file do not fit, and no amount of raising the deadline makes them fit — it only moves
//! the failure to the proxy in front.
//!
//! So a caller can ask for `"async": true` and get a job handle instead of a file. The
//! queue and the concurrency limit are unchanged — this is a layer of state on top of
//! `exec::offload`, not a second execution path. The one thing a job does change is the
//! wall-clock budget: `PDF_RENDER_DEADLINE_SECS` is the time a caller holding a socket may
//! be made to wait, and a caller holding a job handle holds no socket. See `JOB_DEADLINE`.
//!
//! Both limits below exist because a registry is memory: without them a loop of `POST
//! /api/jobs` grows a `HashMap` and a pile of tokio tasks that outlive every request that
//! created them.

use crate::assets::{now_unix, rfc3339};
use crate::config::config;
use crate::exec;
use crate::helpers;
use crate::types::{AppError, ToolResponse};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Wall-clock ceiling for one asynchronous job, all passes and all processes together.
///
/// Seven and a half times the synchronous deadline, because the work that was moved here is
/// exactly the work that does not fit in it: `/api/ocr` accepts three hundred pages, and
/// three hundred pages of Tesseract have never taken two minutes. Nothing in front of this
/// budget times out — there is no socket and no proxy on the path of a running job — so the
/// only thing it protects is the render slot the job is holding.
const DEFAULT_JOB_DEADLINE_SECS: u64 = 900;

/// How long we wait for a callback endpoint to acknowledge a finished job.
///
/// Its own setting: the webhook client used to borrow `MERMAID_TIMEOUT_SECS`, so shortening
/// diagram rendering silently shortened webhook delivery. The default matches what that
/// borrowed value was, so no deployment changes behaviour by upgrading.
const DEFAULT_JOB_CALLBACK_TIMEOUT_SECS: u64 = 15;

/// Ceiling on the job records held in memory at once, all keys together.
///
/// Finished records are dropped to make room before a submission is ever refused: history
/// is the only thing here that can be forgotten without losing work.
const DEFAULT_MAX_TRACKED_JOBS: usize = 500;

/// Ceiling on the jobs one key may have queued or running at the same time.
///
/// This is what bounds the tokio tasks: a key that submits faster than the semaphore drains
/// is told to poll what it already has rather than being allowed to fill the queue for
/// everyone else.
const DEFAULT_MAX_JOBS_PER_KEY: usize = 20;

/// The wall-clock budget an asynchronous job runs under
fn job_deadline() -> Duration {
    positive_secs("JOB_DEADLINE_SECS", DEFAULT_JOB_DEADLINE_SECS)
}

fn callback_timeout() -> Duration {
    positive_secs(
        "JOB_CALLBACK_TIMEOUT_SECS",
        DEFAULT_JOB_CALLBACK_TIMEOUT_SECS,
    )
}

fn max_tracked_jobs() -> usize {
    positive("MAX_TRACKED_JOBS", DEFAULT_MAX_TRACKED_JOBS as u64) as usize
}

fn max_jobs_per_key() -> usize {
    positive("MAX_JOBS_PER_KEY", DEFAULT_MAX_JOBS_PER_KEY as u64) as usize
}

/// These four settings are read here rather than in `config.rs` because nothing outside the
/// job layer has any business honouring them — in particular the synchronous path, which
/// must keep the deadline it has today.
fn positive_secs(key: &str, default: u64) -> Duration {
    Duration::from_secs(positive(key, default))
}

fn positive(key: &str, default: u64) -> u64 {
    parse_positive(key, std::env::var(key).ok().as_deref(), default)
}

/// A deployment typo must never take the job layer down: fall back to the documented
/// default and say so in the log.
fn parse_positive(key: &str, raw: Option<&str>, default: u64) -> u64 {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => default,
        Some(value) => match value.parse::<u64>() {
            Ok(parsed) if parsed > 0 => parsed,
            _ => {
                warn!(
                    "{} = {:?} is not a positive number, using {}",
                    key, value, default
                );
                default
            }
        },
    }
}

/// What a caller sees when polling. Also the body posted to a callback URL.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub job_id: String,
    /// `queued`, `running`, `done` or `failed`
    pub status: String,
    /// Which endpoint produced this job, for the caller's own logs
    pub kind: String,
    pub created_at: String,
    pub updated_at: String,
    pub poll_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Unix seconds; the record is dropped past this
    #[serde(skip)]
    expires_unix: u64,
    /// Name of the key that submitted this job, for the per-key ceiling. Not serialised:
    /// it is our bookkeeping, and it names another caller's integration.
    #[serde(skip)]
    owner: String,
}

impl JobView {
    fn new(id: String, kind: &str, owner: &str) -> JobView {
        let now = now_unix();
        JobView {
            poll_url: format!("/api/jobs/{}", id),
            job_id: id,
            status: "queued".to_string(),
            kind: kind.to_string(),
            created_at: rfc3339(now),
            updated_at: rfc3339(now),
            result: None,
            error: None,
            expires_unix: now + config().job_ttl.as_secs(),
            owner: owner.to_string(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.status == "done" || self.status == "failed"
    }
}

fn registry() -> &'static Mutex<HashMap<String, JobView>> {
    static JOBS: OnceLock<Mutex<HashMap<String, JobView>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Update a record in place. A poisoned lock is recovered from rather than propagated: a
/// panic in one job must not make the whole job registry unusable for the process's life.
fn with_registry<T>(f: impl FnOnce(&mut HashMap<String, JobView>) -> T) -> T {
    let mut guard = match registry().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

fn set_status(id: &str, status: &str, result: Option<ToolResponse>, error: Option<String>) {
    with_registry(|jobs| {
        if let Some(job) = jobs.get_mut(id) {
            job.status = status.to_string();
            job.updated_at = rfc3339(now_unix());
            job.result = result;
            job.error = error;
        }
    });
}

/// Drop finished jobs whose retention window has passed
fn purge_expired(jobs: &mut HashMap<String, JobView>) {
    let now = now_unix();
    jobs.retain(|_, job| job.expires_unix > now);
}

/// Decide whether one more job may be tracked, and make room if room can be made.
///
/// The refusal is a 429 rather than a 503 for the same reason `exec::offload` answers 429
/// when the queue saturates: nothing is broken, the caller asked for more than the service
/// is holding at once, and retrying later is the right thing to do.
fn admit(jobs: &mut HashMap<String, JobView>, owner: &str) -> Result<(), AppError> {
    purge_expired(jobs);

    let per_key = max_jobs_per_key();
    let in_flight = jobs
        .values()
        .filter(|job| job.owner == owner && !job.is_terminal())
        .count();
    if in_flight >= per_key {
        return Err(AppError::TooManyRequests(format!(
            "{} already has {} jobs queued or running, which is the limit: poll them before \
             submitting more",
            owner, in_flight
        )));
    }

    let ceiling = max_tracked_jobs();
    if jobs.len() >= ceiling {
        evict_finished(jobs, ceiling);
    }
    if jobs.len() >= ceiling {
        // Everything tracked is still in flight, so there is nothing to forget: the honest
        // answer is that the service is full, not a silently dropped result.
        return Err(AppError::TooManyRequests(format!(
            "The service is already tracking {} jobs, which is its limit: retry once some \
             have finished",
            ceiling
        )));
    }

    Ok(())
}

/// Forget finished jobs, oldest first, until the registry is back under `ceiling`.
///
/// A result nobody has come back for is history; the retention window is a courtesy, not a
/// promise, and `get` already answers `NotFound` once it closes.
fn evict_finished(jobs: &mut HashMap<String, JobView>, ceiling: usize) {
    let mut finished: Vec<(u64, String)> = jobs
        .values()
        .filter(|job| job.is_terminal())
        .map(|job| (job.expires_unix, job.job_id.clone()))
        .collect();
    // Uniform TTL, so ordering by expiry is ordering by submission
    finished.sort();

    for (_, id) in finished {
        if jobs.len() < ceiling {
            break;
        }
        jobs.remove(&id);
    }
}

/// Accept a unit of work and answer immediately with its handle.
///
/// The closure runs through `exec::offload`, so it queues behind the same semaphore as
/// every synchronous render. An async job is not a way around the concurrency limit; it is
/// a way to stop holding a socket open while it applies — and, because no socket is held,
/// a way to run under a longer deadline than a request could ever wait for.
pub fn submit<F>(
    owner: &str,
    kind: &str,
    callback_url: Option<String>,
    work: F,
) -> Result<JobView, AppError>
where
    F: FnOnce() -> Result<ToolResponse, AppError> + Send + 'static,
{
    if let Some(url) = callback_url.as_deref() {
        // Vetted before the job exists: a caller who mistyped their webhook finds out now,
        // not in a log line twenty minutes later.
        crate::urlguard::check_outbound(url)?;
    }

    let id = helpers::random_id("job")?;
    let view = JobView::new(id.clone(), kind, owner);

    with_registry(|jobs| {
        admit(jobs, owner)?;
        jobs.insert(id.clone(), view.clone());
        Ok::<(), AppError>(())
    })?;

    let job_id = id.clone();
    // A spawned task starts with an empty task-local set, so the owner the handler declared
    // does not follow the job across `spawn`: it is carried by value and declared again
    // inside, or every file a job produces would be stored belonging to nobody.
    let job_owner = owner.to_string();
    rocket::tokio::spawn(async move {
        set_status(&job_id, "running", None, None);

        let outcome =
            exec::as_owner(&job_owner, exec::offload(move || under_job_budget(work))).await;

        match outcome {
            Ok(response) => set_status(&job_id, "done", Some(response), None),
            Err(err) => {
                let message = describe(&err);
                warn!("Job {} failed: {}", job_id, message);
                set_status(&job_id, "failed", None, Some(message));
            }
        }

        if let Some(url) = callback_url {
            notify(&job_id, url).await;
        }
    });

    Ok(view)
}

/// Run blocking work under the job deadline instead of the render deadline.
///
/// `exec::offload` opens every unit of work with `Budget::start(render_deadline)` — the two
/// minutes a caller holding a socket may be made to wait. That budget is a thread-local
/// armed on the blocking thread just before the closure runs, so re-arming it here, first
/// thing inside the closure and before any process is spawned, replaces it for this job and
/// this job only. The synchronous path never reaches this line and keeps exactly the budget
/// it has today. Dropping the guard clears the deadline, and `offload`'s own guard clears
/// it again — the thread ends with no budget either way.
///
/// Known cosmetic gap: `helpers::budget_check` names `PDF_RENDER_DEADLINE_SECS` in the
/// timeout message it builds, so a job that exhausts its budget is told the wrong number of
/// seconds. Correcting that means touching `helpers.rs`.
fn under_job_budget<T, F>(work: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError>,
{
    let _budget = helpers::Budget::start(job_deadline());
    work()
}

/// Current state of a job, or `NotFound` once its retention window has closed
pub fn get(id: &str) -> Result<JobView, AppError> {
    with_registry(|jobs| {
        purge_expired(jobs);
        jobs.get(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("Job not found: {}", id)))
    })
}

/// Number of jobs currently held, for the metrics endpoint
pub fn tracked() -> usize {
    with_registry(|jobs| {
        purge_expired(jobs);
        jobs.len()
    })
}

/// POST the finished job to the caller's URL, signed so they can tell it came from us.
///
/// One attempt, no retry: a webhook that has to be delivered exactly once is a queue, and
/// this is not one. The poll URL remains the source of truth, and it says so in the docs.
async fn notify(job_id: &str, url: String) {
    let Ok(job) = get(job_id) else { return };

    let body = match serde_json::to_string(&job) {
        Ok(body) => body,
        Err(e) => {
            warn!(
                "Job {}: could not serialise the callback body: {}",
                job_id, e
            );
            return;
        }
    };

    let signature = crate::sign::sign(body.as_bytes());
    let job_id = job_id.to_string();

    // reqwest's blocking client cannot run on a tokio worker: it builds its own runtime.
    let _ = rocket::tokio::task::spawn_blocking(move || {
        let client = match reqwest::blocking::Client::builder()
            .timeout(callback_timeout())
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                warn!("Job {}: could not build the callback client: {}", job_id, e);
                return;
            }
        };

        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-Signature", signature)
            .header("X-Service", config().service_name.clone())
            .body(body)
            .send()
        {
            Ok(response) if response.status().is_success() => {
                info!("Job {}: callback delivered", job_id);
            }
            Ok(response) => {
                warn!("Job {}: callback answered {}", job_id, response.status());
            }
            Err(e) => warn!("Job {}: callback failed: {}", job_id, e),
        }
    })
    .await;
}

/// Flatten an error into the one line a polling client gets. The full detail stayed in the
/// log, with the request id, at the moment it happened.
fn describe(err: &AppError) -> String {
    match err {
        AppError::ProcessFailed { message, stderr } if !stderr.is_empty() => {
            format!("{}: {}", message, first_line(stderr))
        }
        AppError::ProcessFailed { message, .. } => message.clone(),
        AppError::Io(e) => format!("IO error: {}", e),
        AppError::BadRequest(m)
        | AppError::NotFound(m)
        | AppError::TemplateError(m)
        | AppError::Timeout(m)
        | AppError::Unauthorized(m)
        | AppError::TooManyRequests(m) => m.clone(),
        AppError::Conflict(m) => m.clone(),
        AppError::Upstream { service, details } => format!("{} unavailable: {}", service, details),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, owner: &str, status: &str) -> JobView {
        let mut view = JobView::new(id.to_string(), "ocr", owner);
        view.status = status.to_string();
        view
    }

    fn registry_of(jobs: Vec<JobView>) -> HashMap<String, JobView> {
        jobs.into_iter()
            .map(|job| (job.job_id.clone(), job))
            .collect()
    }

    #[test]
    fn a_new_job_starts_queued_and_knows_where_it_is_polled() {
        let job = JobView::new("job_abc".to_string(), "compress", "public");
        assert_eq!(job.status, "queued");
        assert_eq!(job.poll_url, "/api/jobs/job_abc");
        assert_eq!(job.kind, "compress");
        assert!(!job.is_terminal());
    }

    /// S6: a job that inherits the render deadline is a job the asynchronous surface was
    /// built to escape — a 300-page OCR would fail at 120 s exactly as it does synchronously.
    #[test]
    fn a_job_replaces_the_render_deadline_with_its_own() {
        // What `exec::offload` arms on the blocking thread before calling the closure
        let _render_budget = helpers::Budget::start(Duration::from_secs(5));

        let inside = under_job_budget(|| Ok::<_, AppError>(helpers::Budget::remaining()))
            .unwrap()
            .expect("a job must run under a budget, not without one");

        assert!(
            inside > Duration::from_secs(5),
            "the job kept the render deadline: {:?}",
            inside
        );
        assert!(inside <= job_deadline());
    }

    #[test]
    fn the_job_deadline_leaves_room_for_the_work_that_was_moved_off_the_socket() {
        assert!(DEFAULT_JOB_DEADLINE_SECS > config().render_deadline.as_secs());
    }

    /// M6: the webhook client used to read `MERMAID_TIMEOUT_SECS`
    #[test]
    fn the_callback_timeout_is_read_from_its_own_variable() {
        std::env::set_var("JOB_CALLBACK_TIMEOUT_SECS", "42");
        assert_eq!(callback_timeout(), Duration::from_secs(42));
        std::env::remove_var("JOB_CALLBACK_TIMEOUT_SECS");
        assert_eq!(
            callback_timeout(),
            Duration::from_secs(DEFAULT_JOB_CALLBACK_TIMEOUT_SECS)
        );
    }

    #[test]
    fn a_typo_in_a_job_setting_falls_back_to_the_default() {
        assert_eq!(parse_positive("K", Some(" 900 "), 60), 900);
        assert_eq!(parse_positive("K", Some("0"), 60), 60);
        assert_eq!(parse_positive("K", Some("nine hundred"), 60), 60);
        assert_eq!(parse_positive("K", Some("   "), 60), 60);
        assert_eq!(parse_positive("K", None, 60), 60);
    }

    /// S8: one key must not be able to fill the queue — or the tokio scheduler — for everyone
    #[test]
    fn a_key_may_not_keep_more_jobs_in_flight_than_its_share() {
        let per_key = max_jobs_per_key();
        let mut jobs = registry_of(
            (0..per_key)
                .map(|n| job(&format!("job_{}", n), "client-a", "running"))
                .collect(),
        );

        let refused = admit(&mut jobs, "client-a").unwrap_err();
        assert!(
            matches!(refused, AppError::TooManyRequests(_)),
            "expected a 429, got {:?}",
            refused
        );
        // The ceiling is per key, so nobody else is punished for it
        assert!(admit(&mut jobs, "client-b").is_ok());
    }

    #[test]
    fn a_key_whose_jobs_have_all_finished_may_submit_again() {
        let per_key = max_jobs_per_key();
        let mut jobs = registry_of(
            (0..per_key)
                .map(|n| job(&format!("job_{}", n), "client-a", "done"))
                .collect(),
        );

        assert!(admit(&mut jobs, "client-a").is_ok());
    }

    /// S8: the registry is memory, and memory needs a ceiling
    #[test]
    fn a_full_registry_forgets_finished_jobs_before_refusing_a_new_one() {
        let ceiling = max_tracked_jobs();
        let mut jobs = registry_of(
            (0..ceiling)
                .map(|n| job(&format!("job_{}", n), &format!("key-{}", n), "done"))
                .collect(),
        );

        assert!(admit(&mut jobs, "key-new").is_ok());
        assert!(jobs.len() < ceiling, "no room was made: {}", jobs.len());
    }

    #[test]
    fn a_registry_full_of_running_jobs_refuses_rather_than_grows() {
        let ceiling = max_tracked_jobs();
        let mut jobs = registry_of(
            (0..ceiling)
                .map(|n| job(&format!("job_{}", n), &format!("key-{}", n), "running"))
                .collect(),
        );

        let refused = admit(&mut jobs, "key-new").unwrap_err();
        assert!(
            matches!(refused, AppError::TooManyRequests(_)),
            "expected a 429, got {:?}",
            refused
        );
        assert_eq!(jobs.len(), ceiling, "running work must never be evicted");
    }

    #[test]
    fn eviction_forgets_the_oldest_finished_job_first() {
        let mut oldest = job("job_old", "key", "done");
        oldest.expires_unix = now_unix() + 10;
        let mut recent = job("job_recent", "key", "done");
        recent.expires_unix = now_unix() + 3_000;
        let running = job("job_running", "key", "running");
        let mut jobs = registry_of(vec![oldest, recent, running]);

        // A ceiling of three over three records: one slot has to be freed, and exactly one
        evict_finished(&mut jobs, 3);

        assert!(!jobs.contains_key("job_old"));
        assert!(jobs.contains_key("job_recent"));
        assert!(jobs.contains_key("job_running"));
    }

    #[test]
    fn terminal_states_are_the_two_a_client_may_stop_polling_on() {
        let mut job = JobView::new("job_abc".to_string(), "ocr", "public");
        for (status, terminal) in [
            ("queued", false),
            ("running", false),
            ("done", true),
            ("failed", true),
        ] {
            job.status = status.to_string();
            assert_eq!(job.is_terminal(), terminal, "status {}", status);
        }
    }

    #[test]
    fn expired_records_are_dropped() {
        let mut jobs = HashMap::new();
        let mut old = JobView::new("job_old".to_string(), "ocr", "public");
        old.expires_unix = now_unix() - 1;
        jobs.insert("job_old".to_string(), old);
        jobs.insert(
            "job_new".to_string(),
            JobView::new("job_new".to_string(), "ocr", "public"),
        );

        purge_expired(&mut jobs);

        assert!(!jobs.contains_key("job_old"));
        assert!(jobs.contains_key("job_new"));
    }

    #[test]
    fn errors_become_one_readable_line() {
        assert_eq!(
            describe(&AppError::BadRequest("bad page range".to_string())),
            "bad page range"
        );
        assert_eq!(
            describe(&AppError::ProcessFailed {
                message: "Ghostscript failed".to_string(),
                stderr: "\n\nsyntax error near line 4\nmore".to_string(),
            }),
            "Ghostscript failed: syntax error near line 4"
        );
    }
}
