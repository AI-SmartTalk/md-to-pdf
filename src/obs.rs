use crate::config::config;
use crate::exec;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::request::{FromRequest, Outcome};
use rocket::{Data, Request, Response};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use log::Level;

/// Records waiting to be shipped. Beyond this the oldest are dropped: observability is
/// allowed to lose data, never to slow a request down.
const QUEUE_CAPACITY: usize = 4096;
const BATCH_MAX_LINES: usize = 500;
const BATCH_MAX_WAIT: Duration = Duration::from_secs(2);
const SHIP_TIMEOUT: Duration = Duration::from_secs(10);
/// A dead aggregator must not flood the local log with one line per batch
const SHIP_WARN_INTERVAL: Duration = Duration::from_secs(60);

/// Ceiling on distinct label values per metric, so a route explosion cannot grow the
/// exposition unbounded
const MAX_LABEL_SETS: usize = 256;
const MAX_LABEL_LEN: usize = 64;

// ------------ Request id ------------

/// Correlation id for one request: the caller's `X-Request-Id` when it is usable, ours
/// otherwise. Reused by every event emitted while serving that request.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RequestId {
    type Error = Infallible;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        Outcome::Success(request_id(req).clone())
    }
}

/// Resolve (once per request) the id every event for this request must carry
pub fn request_id<'r>(req: &'r Request<'_>) -> &'r RequestId {
    req.local_cache(|| {
        req.headers()
            .get_one("X-Request-Id")
            .filter(|value| plausible_request_id(value))
            .map(|value| RequestId(value.to_string()))
            .unwrap_or_else(|| RequestId(generated_id()))
    })
}

/// The id is echoed into logs and response headers, so anything but short printable ASCII
/// is refused and replaced by one of ours.
fn plausible_request_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_graphic())
}

/// Unique enough without pulling a uuid dependency in: process id, clock, and a counter
/// that makes ids minted in the same nanosecond distinct.
fn generated_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);

    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        nanos,
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

// ------------ Events ------------

/// Emit one structured event: always to the local log, and to log420 when it is configured.
pub fn event(level: Level, msg: &str, fields: Vec<(&str, Value)>) {
    let mut payload = serde_json::Map::with_capacity(fields.len() + 4);
    for (key, value) in fields {
        payload.insert(key.to_string(), value);
    }

    if payload.is_empty() {
        log::log!(level, "{}", msg);
    } else {
        // Display on a Value is compact JSON, so the local line stays greppable
        log::log!(level, "{} {}", msg, Value::Object(payload.clone()));
    }

    let sink = match sink() {
        Some(sink) => sink,
        None => return,
    };

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0);

    payload.insert("level".to_string(), json!(pino_level(level)));
    payload.insert("time".to_string(), json!(now_ms));
    payload.insert("msg".to_string(), json!(msg));
    payload.insert("service".to_string(), json!(config().service_name.as_str()));

    if let Ok(line) = serde_json::to_string(&Value::Object(payload)) {
        sink.push(line);
    }
}

/// log420 groups records with `level >= 50` into issues from an `err.type` fingerprint:
/// every error event should carry this field so the grouping is stable.
pub fn err_field(kind: &str, message: &str) -> (&'static str, Value) {
    ("err", json!({ "type": kind, "message": message }))
}

fn pino_level(level: Level) -> u16 {
    match level {
        Level::Trace => 10,
        Level::Debug => 20,
        Level::Info => 30,
        Level::Warn => 40,
        Level::Error => 50,
    }
}

// ------------ log420 shipping ------------

struct Sink {
    queue: Mutex<VecDeque<String>>,
    ready: Condvar,
}

static SINK: OnceLock<Option<Sink>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);

fn sink() -> Option<&'static Sink> {
    match SINK.get() {
        Some(Some(sink)) => Some(sink),
        _ => None,
    }
}

impl Sink {
    fn push(&self, line: String) {
        // A poisoned lock would mean a panic while holding it, which cannot happen here;
        // if it somehow did, the record is dropped rather than propagated to the caller.
        let mut queue = match self.queue.lock() {
            Ok(queue) => queue,
            Err(_) => return,
        };

        while queue.len() >= QUEUE_CAPACITY {
            queue.pop_front();
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }

        queue.push_back(line);
        self.ready.notify_one();
    }

    fn take_batch(&self) -> Vec<String> {
        let mut queue = lock(&self.queue);

        if queue.is_empty() {
            let (waited, _) = self
                .ready
                .wait_timeout(queue, BATCH_MAX_WAIT)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue = waited;
        }

        let take = queue.len().min(BATCH_MAX_LINES);
        queue.drain(..take).collect()
    }
}

/// Start the shipping thread. Safe to call once; without an ingest token the service runs
/// exactly as before, with local logs only.
pub fn init() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let cfg = config();
    let endpoint = match (cfg.log420_url.as_deref(), cfg.log420_token.as_deref()) {
        (Some(url), Some(_)) => format!("{}/api/ingest", url),
        _ => {
            let _ = SINK.set(None);
            info!("log420 shipping is off (no LOG420_URL/LOG420_INGEST_TOKEN): local logs only");
            return;
        }
    };

    let _ = SINK.set(Some(Sink {
        queue: Mutex::new(VecDeque::with_capacity(64)),
        ready: Condvar::new(),
    }));

    let sink = match sink() {
        Some(sink) => sink,
        None => return,
    };

    let token = cfg.log420_token.clone().unwrap_or_default();
    let region = cfg.log420_region.clone();

    // A dedicated OS thread, not a tokio task: the shipping client is blocking and must
    // never share a worker with request handling.
    let spawned = std::thread::Builder::new()
        .name("log420".to_string())
        .spawn(move || ship_forever(sink, endpoint, token, region));

    if let Err(e) = spawned {
        warn!("Could not start the log420 shipping thread: {}", e);
    }
}

fn ship_forever(sink: &'static Sink, endpoint: String, token: String, region: String) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(SHIP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            warn!("log420 client unavailable, events stay local: {}", e);
            return;
        }
    };

    let authorization = format!("Bearer {}", token);
    let mut last_warn: Option<Instant> = None;

    loop {
        let batch = sink.take_batch();
        if batch.is_empty() {
            continue;
        }

        let mut body = batch.join("\n");
        body.push('\n');

        let sent = client
            .post(&endpoint)
            .header("authorization", &authorization)
            .header("content-type", "application/x-ndjson")
            .header("x-log420-region", &region)
            .body(body)
            .send();

        let failure = match sent {
            Ok(response) if response.status().is_success() => None,
            Ok(response) => Some(format!("HTTP {}", response.status())),
            Err(e) => Some(e.to_string()),
        };

        if let Some(reason) = failure {
            DROPPED.fetch_add(batch.len() as u64, Ordering::Relaxed);
            let now = Instant::now();
            if last_warn.is_none_or(|at| now.duration_since(at) >= SHIP_WARN_INTERVAL) {
                warn!("log420 rejected {} records: {}", batch.len(), reason);
                last_warn = Some(now);
            }
        }
    }
}

// ------------ Metrics ------------

const RENDER_BUCKETS: [f64; 10] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0];
static RENDER_BUCKET_COUNTS: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];
static RENDER_MICROS_TOTAL: AtomicU64 = AtomicU64::new(0);
static RENDER_COUNT: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

type Requests = Mutex<HashMap<(String, u16), u64>>;
type Failures = Mutex<HashMap<String, u64>>;

fn requests() -> &'static Requests {
    static REQUESTS: OnceLock<Requests> = OnceLock::new();
    REQUESTS.get_or_init(Requests::default)
}

fn failures() -> &'static Failures {
    static FAILURES: OnceLock<Failures> = OnceLock::new();
    FAILURES.get_or_init(Failures::default)
}

/// Never panic on a poisoned lock: metrics are best effort
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn record_request(route: &str, status: u16) {
    let mut map = lock(requests());
    let key = (label(route), status);
    if map.len() >= MAX_LABEL_SETS && !map.contains_key(&key) {
        return;
    }
    *map.entry(key).or_insert(0) += 1;
}

/// `reason` must stay a short, closed set of identifiers (never a message or a filename)
pub fn record_render_failure(reason: &str) {
    let mut map = lock(failures());
    let key = label(reason);
    if map.len() >= MAX_LABEL_SETS && !map.contains_key(&key) {
        return;
    }
    *map.entry(key).or_insert(0) += 1;
}

pub fn observe_render(elapsed: Duration) {
    let seconds = elapsed.as_secs_f64();
    let index = RENDER_BUCKETS
        .iter()
        .position(|bound| seconds <= *bound)
        .unwrap_or(RENDER_BUCKETS.len());

    RENDER_BUCKET_COUNTS[index].fetch_add(1, Ordering::Relaxed);
    RENDER_MICROS_TOTAL.fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
    RENDER_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cache_hit() {
    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_cache_miss() {
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

/// Prometheus text exposition (format 0.0.4)
pub fn metrics_text() -> String {
    let mut out = String::with_capacity(2048);

    out.push_str("# HELP mdtopdf_requests_total Requests handled, by matched route and status.\n");
    out.push_str("# TYPE mdtopdf_requests_total counter\n");
    for ((route, status), count) in lock(requests()).iter() {
        let _ = writeln!(
            out,
            "mdtopdf_requests_total{{route=\"{}\",status=\"{}\"}} {}",
            escape(route),
            status,
            count
        );
    }

    out.push_str(
        "# HELP mdtopdf_render_duration_seconds Time spent producing a document, counted \
         only when a document came out: a cache hit is not a render, a failed render has \
         no duration to compare. Pair it with mdtopdf_render_failures_total.\n",
    );
    out.push_str("# TYPE mdtopdf_render_duration_seconds histogram\n");
    let mut cumulative = 0u64;
    for (index, bound) in RENDER_BUCKETS.iter().enumerate() {
        cumulative += RENDER_BUCKET_COUNTS[index].load(Ordering::Relaxed);
        let _ = writeln!(
            out,
            "mdtopdf_render_duration_seconds_bucket{{le=\"{}\"}} {}",
            bound, cumulative
        );
    }
    cumulative += RENDER_BUCKET_COUNTS[RENDER_BUCKETS.len()].load(Ordering::Relaxed);
    let _ = writeln!(
        out,
        "mdtopdf_render_duration_seconds_bucket{{le=\"+Inf\"}} {}",
        cumulative
    );
    let _ = writeln!(
        out,
        "mdtopdf_render_duration_seconds_sum {:.6}",
        RENDER_MICROS_TOTAL.load(Ordering::Relaxed) as f64 / 1_000_000.0
    );
    let _ = writeln!(
        out,
        "mdtopdf_render_duration_seconds_count {}",
        RENDER_COUNT.load(Ordering::Relaxed)
    );

    out.push_str(
        "# HELP mdtopdf_render_failures_total Renders that produced no PDF, by reason. \
         `bad_request` and `not_found` are refused documents; alert on the others.\n",
    );
    out.push_str("# TYPE mdtopdf_render_failures_total counter\n");
    for (reason, count) in lock(failures()).iter() {
        let _ = writeln!(
            out,
            "mdtopdf_render_failures_total{{reason=\"{}\"}} {}",
            escape(reason),
            count
        );
    }

    for (name, help, value) in [
        (
            "mdtopdf_cache_hits_total",
            "Renders served from the PDF cache.",
            CACHE_HITS.load(Ordering::Relaxed),
        ),
        (
            "mdtopdf_cache_misses_total",
            "Renders that had to be produced.",
            CACHE_MISSES.load(Ordering::Relaxed),
        ),
        (
            "mdtopdf_log420_dropped_total",
            "Log records dropped instead of shipped.",
            DROPPED.load(Ordering::Relaxed),
        ),
    ] {
        let _ = writeln!(out, "# HELP {} {}", name, help);
        let _ = writeln!(out, "# TYPE {} counter", name);
        let _ = writeln!(out, "{} {}", name, value);
    }

    for (name, help, value) in [
        (
            "mdtopdf_queue_depth",
            "Requests waiting for a render slot.",
            exec::queue_depth(),
        ),
        (
            "mdtopdf_in_flight",
            "Renders currently running.",
            exec::in_flight(),
        ),
    ] {
        let _ = writeln!(out, "# HELP {} {}", name, help);
        let _ = writeln!(out, "# TYPE {} gauge", name);
        let _ = writeln!(out, "{} {}", name, value);
    }

    out
}

/// Keep label values short and free of anything user-controlled in length
fn label(value: &str) -> String {
    if value.chars().count() <= MAX_LABEL_LEN {
        return value.to_string();
    }
    value.chars().take(MAX_LABEL_LEN).collect()
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

// ------------ Fairing ------------

/// Instant the request was received, kept in the request-local state
struct Received(Instant);

/// Per-response metrics and one structured access event
pub struct Observer;

#[rocket::async_trait]
impl Fairing for Observer {
    fn info(&self) -> Info {
        Info {
            name: "Observability",
            kind: Kind::Request | Kind::Response,
        }
    }

    async fn on_request(&self, req: &mut Request<'_>, _: &mut Data<'_>) {
        req.local_cache(|| Received(Instant::now()));
        // Fix the id before any handler runs so guards and events agree on it
        let _ = request_id(req);
    }

    async fn on_response<'r>(&self, req: &'r Request<'_>, res: &mut Response<'r>) {
        let elapsed = req.local_cache(|| Received(Instant::now())).0.elapsed();
        let route = match req.route() {
            Some(route) => label(&route.uri.to_string()),
            None => "unmatched".to_string(),
        };
        let status = res.status().code;
        let bytes = res.body().preset_size().unwrap_or(0);
        let trace_id = request_id(req).0.clone();

        record_request(&route, status);

        // Support tickets quote this header; it is the only way back to the request's events
        res.set_header(Header::new("X-Request-Id", trace_id.clone()));

        let mut fields = vec![
            ("route", json!(route)),
            ("method", json!(req.method().as_str())),
            ("status", json!(status)),
            ("duration_ms", json!(elapsed.as_millis())),
            ("trace_id", json!(trace_id)),
            ("bytes", json!(bytes)),
        ];

        let level = if status >= 500 {
            fields.push(err_field(
                &format!("http_{}", status),
                res.status().reason().unwrap_or("error"),
            ));
            Level::Error
        } else if status >= 400 {
            Level::Warn
        } else {
            Level::Info
        };

        event(level, "request", fields);
    }
}
