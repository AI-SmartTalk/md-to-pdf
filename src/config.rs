use std::env;
use std::sync::OnceLock;
use std::time::Duration;

/// Default wall-clock limit for every external process (pandoc, weasyprint, qpdf, ...)
const DEFAULT_PROCESS_TIMEOUT_SECS: u64 = 60;
const DEFAULT_QUEUE_TIMEOUT_SECS: u64 = 30;
/// Wall-clock ceiling for a whole render, all passes and all processes together. Queue
/// timeout plus this has to stay under the `ProxyTimeout` of `deploy/`, otherwise the
/// proxy answers 503 while the render keeps holding its slot.
const DEFAULT_RENDER_DEADLINE_SECS: u64 = 120;
const DEFAULT_CACHE_MAX_MB: u64 = 512;
const DEFAULT_CACHE_TTL_SECS: u64 = 604_800;
const DEFAULT_MERMAID_TIMEOUT_SECS: u64 = 15;
const DEFAULT_MERMAID_API_URL: &str = "https://mermaid.aismarttalk.tech/api/v1";
const DEFAULT_LOG420_URL: &str = "https://log420.aist-core.fr";
const DEFAULT_LOG420_REGION: &str = "eu";
const DEFAULT_SERVICE_NAME: &str = "md-to-pdf";
const DEFAULT_LAYOUT_MAX_PASSES: u8 = 1;
const DEFAULT_PREVIEW_MAX_PAGES: usize = 20;

/// Upper bound on the render concurrency: past a handful of parallel weasyprint runs the
/// container hits its memory limit long before it gets faster.
const MAX_CONCURRENCY_CEILING: usize = 64;

/// Everything the service reads from the environment, resolved once at startup.
#[derive(Debug, Clone)]
pub struct Config {
    pub max_concurrency: usize,
    pub queue_timeout: Duration,
    pub process_timeout: Duration,
    pub render_deadline: Duration,
    /// Engines the unauthenticated `POST /` may select. Only weasyprint runs behind the
    /// fetcher guard of `deploy/weasyprint-safe.py`; wkhtmltopdf follows redirects on its
    /// own and pdflatex reads files on its own, so neither belongs on an open endpoint.
    pub unauthenticated_engines: Vec<String>,
    pub cache_enabled: bool,
    pub cache_max_mb: u64,
    pub cache_ttl: Duration,
    pub allowed_url_hosts: Vec<String>,
    /// An empty `allowed_url_hosts` means "any public host", not "no host at all": reading
    /// it strictly would break every document that already embeds a remote logo. This flag
    /// restores the strict reading, on both sides of the fence (Rust and the fetcher).
    pub url_strict_hosts: bool,
    pub allow_local_assets: bool,
    pub mermaid_api_url: Option<String>,
    pub mermaid_api_key: Option<String>,
    pub mermaid_timeout: Duration,
    pub log420_url: Option<String>,
    pub log420_token: Option<String>,
    pub log420_region: String,
    pub service_name: String,
    pub layout_max_passes: u8,
    pub preview_max_pages: usize,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// The one and only configuration instance. Cheap enough to call on a request path.
pub fn config() -> &'static Config {
    CONFIG.get_or_init(Config::from_env)
}

impl Config {
    fn from_env() -> Config {
        Config {
            max_concurrency: concurrency(),
            queue_timeout: duration_secs("PDF_QUEUE_TIMEOUT_SECS", DEFAULT_QUEUE_TIMEOUT_SECS),
            process_timeout: duration_secs(
                "PDF_PROCESS_TIMEOUT_SECS",
                DEFAULT_PROCESS_TIMEOUT_SECS,
            ),
            render_deadline: duration_secs(
                "PDF_RENDER_DEADLINE_SECS",
                DEFAULT_RENDER_DEADLINE_SECS,
            ),
            unauthenticated_engines: engine_list("PDF_UNAUTHENTICATED_ENGINES"),
            cache_enabled: boolean("PDF_CACHE_ENABLED", true),
            cache_max_mb: number("PDF_CACHE_MAX_MB", DEFAULT_CACHE_MAX_MB),
            cache_ttl: duration_secs("PDF_CACHE_TTL_SECS", DEFAULT_CACHE_TTL_SECS),
            allowed_url_hosts: host_list("PDF_ALLOWED_URL_HOSTS"),
            url_strict_hosts: boolean("PDF_URL_STRICT_HOSTS", false),
            allow_local_assets: boolean("PDF_ALLOW_LOCAL_ASSETS", true),
            mermaid_api_url: url("MERMAID_API_URL", Some(DEFAULT_MERMAID_API_URL)),
            mermaid_api_key: secret("MERMAID_API_KEY"),
            mermaid_timeout: duration_secs("MERMAID_TIMEOUT_SECS", DEFAULT_MERMAID_TIMEOUT_SECS),
            log420_url: url("LOG420_URL", Some(DEFAULT_LOG420_URL)),
            log420_token: secret("LOG420_INGEST_TOKEN"),
            log420_region: text("LOG420_REGION", DEFAULT_LOG420_REGION),
            service_name: text("SERVICE_NAME", DEFAULT_SERVICE_NAME),
            layout_max_passes: number("PDF_LAYOUT_MAX_PASSES", DEFAULT_LAYOUT_MAX_PASSES).min(3),
            preview_max_pages: positive("PDF_PREVIEW_MAX_PAGES", DEFAULT_PREVIEW_MAX_PAGES),
        }
    }

    /// One-line, secret-free summary for the startup log
    pub fn summary(&self) -> String {
        format!(
            "concurrency={} queue_timeout={}s process_timeout={}s render_deadline={}s cache={} layout_passes={} log420={}",
            self.max_concurrency,
            self.queue_timeout.as_secs(),
            self.process_timeout.as_secs(),
            self.render_deadline.as_secs(),
            if self.cache_enabled { "on" } else { "off" },
            self.layout_max_passes,
            if self.log420_token.is_some() { "on" } else { "off" },
        )
    }
}

/// A deployment typo must never keep the service down: every reader below falls back to the
/// documented default and says so in the log.
fn raw(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

fn number<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr + std::fmt::Display + Copy,
{
    match raw(key) {
        None => default,
        Some(value) => match value.parse::<T>() {
            Ok(parsed) => parsed,
            Err(_) => {
                warn!(
                    "{} = {:?} is not a valid number, using {}",
                    key, value, default
                );
                default
            }
        },
    }
}

fn positive(key: &str, default: usize) -> usize {
    let value = number(key, default);
    if value == 0 {
        warn!("{} must be greater than 0, using {}", key, default);
        return default;
    }
    value
}

fn duration_secs(key: &str, default_secs: u64) -> Duration {
    Duration::from_secs(positive(key, default_secs as usize) as u64)
}

fn boolean(key: &str, default: bool) -> bool {
    match raw(key) {
        None => default,
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => {
                warn!(
                    "{} = {:?} is not a boolean (true/false), using {}",
                    key, value, default
                );
                default
            }
        },
    }
}

fn text(key: &str, default: &str) -> String {
    raw(key).unwrap_or_else(|| default.to_string())
}

fn secret(key: &str) -> Option<String> {
    raw(key)
}

/// An explicitly empty value disables the endpoint, which is how an operator turns off
/// outbound calls without editing the code.
fn url(key: &str, default: Option<&str>) -> Option<String> {
    let value = match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.to_string()
        }
        Err(_) => default?.to_string(),
    };

    Some(value.trim_end_matches('/').to_string())
}

fn host_list(key: &str) -> Vec<String> {
    let mut hosts: Vec<String> = raw(key)
        .map(|value| {
            value
                .split(',')
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .collect()
        })
        .unwrap_or_default();

    hosts.sort();
    hosts.dedup();
    hosts
}

/// Same shape as `host_list`, but an unset variable keeps the guarded engine alone rather
/// than meaning "everything": opening the legacy endpoint has to be a deliberate act.
fn engine_list(key: &str) -> Vec<String> {
    let list = host_list(key);
    if list.is_empty() {
        return vec!["weasyprint".to_string()];
    }
    list
}

fn concurrency() -> usize {
    let default = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 8);

    let value = number("PDF_MAX_CONCURRENCY", default);
    if value == 0 {
        warn!(
            "PDF_MAX_CONCURRENCY must be greater than 0, using {}",
            default
        );
        return default;
    }

    value.min(MAX_CONCURRENCY_CEILING)
}
