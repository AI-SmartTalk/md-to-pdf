use crate::types::AppError;
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use std::collections::HashMap;
use std::env;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Name given to the key of a deployment that never named one
const ANONYMOUS: &str = "anonymous";
/// Name reported when `API_KEY` is unset and the guard lets everything through
const OPEN: &str = "open";

/// Request guard protecting the `/api` routes.
///
/// `API_KEY` is read once at startup and understood in two ways, so that no deployment has
/// to change to keep working:
///
/// - a bare value — `API_KEY=s3cr3t` — is the single anonymous key it has always been;
/// - a list — `API_KEY=core=s3cr3t,zapier=t0k3n` — mints one named key per integration.
///
/// The name is what makes the rest possible: it lands in the logs, in the metrics and in
/// the quota counter, which is the difference between "the service is busy" and "the
/// Zapier integration is looping". When the variable is unset or empty the guard is a
/// no-op, exactly as before.
pub struct ApiKey(pub &'static str);

/// Name of the key a request presented, resolved by the guard and cached for the
/// observability fairing — which runs after the handler and cannot re-read the header
/// without repeating the comparison.
pub struct KeyName(pub &'static str);

/// Attribution of a request that never reached a guard, or was refused by one
const UNATTRIBUTED: &str = "none";

/// What the access log and the metrics should attribute this request to.
///
/// The fallback is `none`, not `open`: a rejected request has no key, and labelling it
/// `open` would make a wall of 401s read as "the service is unprotected" — the opposite of
/// what happened.
pub fn key_name<'r>(req: &'r Request<'_>) -> &'r KeyName {
    req.local_cache(|| KeyName(UNATTRIBUTED))
}

// ------------ The key table ------------

struct Keys {
    /// (name, secret) pairs, in declaration order
    entries: Vec<(&'static str, String)>,
    /// No key configured: the guard lets every request through
    open: bool,
}

fn keys() -> &'static Keys {
    static KEYS: OnceLock<Keys> = OnceLock::new();
    KEYS.get_or_init(|| parse(env::var("API_KEY").unwrap_or_default().as_str()))
}

fn parse(raw: &str) -> Keys {
    let raw = raw.trim();
    if raw.is_empty() {
        return Keys {
            entries: Vec::new(),
            open: true,
        };
    }

    // A bare secret stays a bare secret. Splitting it on ',' would quietly break a
    // deployment whose key happens to contain one.
    if !raw.contains('=') {
        return Keys {
            entries: vec![(ANONYMOUS, raw.to_string())],
            open: false,
        };
    }

    let mut entries = Vec::new();
    for item in raw.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        match item.split_once('=') {
            Some((name, secret)) => {
                let name = name.trim();
                let secret = secret.trim();
                if name.is_empty() || secret.is_empty() {
                    warn!("API_KEY: ignoring an entry with an empty name or secret");
                    continue;
                }
                // Leaked into logs and metric labels, so it stays to a safe alphabet
                let clean: String = name
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .take(48)
                    .collect();
                if clean.is_empty() {
                    warn!("API_KEY: ignoring an entry whose name has no usable characters");
                    continue;
                }
                entries.push((
                    Box::leak(clean.into_boxed_str()) as &'static str,
                    secret.to_string(),
                ));
            }
            // Mixed forms happen during a migration; accepting the bare one keeps the old
            // integration alive while the named ones are rolled out.
            None => entries.push((ANONYMOUS, item.to_string())),
        }
    }

    if entries.is_empty() {
        warn!("API_KEY was set but no usable key could be read from it: the API stays closed");
    }

    Keys {
        entries,
        open: false,
    }
}

/// Names of the configured keys, for the startup log and `/api/health`
pub fn configured_names() -> Vec<&'static str> {
    keys().entries.iter().map(|(name, _)| *name).collect()
}

pub fn is_open() -> bool {
    keys().open
}

// ------------ Quotas ------------

/// Requests per minute a single key may make. `0` — the default — means no limit, which is
/// what every existing deployment gets.
fn quota_per_minute() -> u32 {
    static QUOTA: OnceLock<u32> = OnceLock::new();
    *QUOTA.get_or_init(|| {
        env::var("API_QUOTA_PER_MINUTE")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    })
}

struct Window {
    started: Instant,
    count: u32,
}

fn counters() -> &'static Mutex<HashMap<&'static str, Window>> {
    static COUNTERS: OnceLock<Mutex<HashMap<&'static str, Window>>> = OnceLock::new();
    COUNTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Count this request against its key, and say whether it may proceed.
///
/// A fixed window rather than a sliding one: the failure mode of a fixed window is letting
/// through twice the quota across a boundary, and the failure mode of getting clever here
/// is a lock held on the request path of every call.
fn within_quota(name: &'static str) -> bool {
    let quota = quota_per_minute();
    if quota == 0 {
        return true;
    }

    let mut guard = match counters().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let window = guard.entry(name).or_insert_with(|| Window {
        started: Instant::now(),
        count: 0,
    });

    if window.started.elapsed() >= Duration::from_secs(60) {
        window.started = Instant::now();
        window.count = 0;
    }

    window.count += 1;
    window.count <= quota
}

// ------------ The guard ------------

#[rocket::async_trait]
impl<'r> FromRequest<'r> for ApiKey {
    type Error = AppError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let keys = keys();

        if keys.open {
            req.local_cache(|| KeyName(OPEN));
            return Outcome::Success(ApiKey(OPEN));
        }

        let provided = req
            .headers()
            .get_one("X-API-Key")
            .or_else(|| {
                req.headers()
                    .get_one("Authorization")
                    .and_then(|value| value.strip_prefix("Bearer "))
            })
            .unwrap_or_default();

        // Every candidate is compared, even after a match, so the time taken does not say
        // which integration the presented key belongs to.
        let mut matched: Option<&'static str> = None;
        for (name, secret) in &keys.entries {
            if constant_time_eq(provided.as_bytes(), secret.as_bytes()) {
                matched = Some(name);
            }
        }

        let Some(name) = matched else {
            return Outcome::Error((
                Status::Unauthorized,
                AppError::Unauthorized(
                    "Missing or invalid API key (X-API-Key or Authorization: Bearer)".to_string(),
                ),
            ));
        };

        if !within_quota(name) {
            return Outcome::Error((
                Status::TooManyRequests,
                AppError::TooManyRequests(format!(
                    "Quota exceeded for key \"{}\": {} requests per minute",
                    name,
                    quota_per_minute()
                )),
            ));
        }

        req.local_cache(|| KeyName(name));
        Outcome::Success(ApiKey(name))
    }
}

// ------------ The public tier ------------

/// Name a request is attributed to when it came in without a key on an open tier
const PUBLIC: &str = "public";

/// Is the free tier switched on?
///
/// Off by default, which is the whole point: a deployment that set `API_KEY` stays exactly
/// as closed as it was this morning. Turning it on is what makes the public tool pages
/// usable by a visitor who has no token and never will.
pub fn public_tier() -> bool {
    static PUBLIC_TIER: OnceLock<bool> = OnceLock::new();
    *PUBLIC_TIER.get_or_init(|| {
        matches!(
            env::var("PUBLIC_TOOLS")
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Requests per minute an anonymous visitor may make, counted per client address
fn public_quota_per_minute() -> u32 {
    static QUOTA: OnceLock<u32> = OnceLock::new();
    *QUOTA.get_or_init(|| {
        env::var("PUBLIC_QUOTA_PER_MINUTE")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(20)
    })
}

/// A fixed window per client address. Bounded so a botnet cannot turn the counter itself
/// into the memory leak that takes the service down.
const MAX_TRACKED_ADDRESSES: usize = 4096;

fn public_counters() -> &'static Mutex<HashMap<String, Window>> {
    static COUNTERS: OnceLock<Mutex<HashMap<String, Window>>> = OnceLock::new();
    COUNTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn within_public_quota(address: &str) -> bool {
    let quota = public_quota_per_minute();
    if quota == 0 {
        return true;
    }

    let mut guard = match public_counters().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    // Windows older than the period are dead weight; clearing them here costs one pass over
    // a map that never grows past the ceiling, and saves a background task.
    if guard.len() >= MAX_TRACKED_ADDRESSES {
        guard.retain(|_, window| window.started.elapsed() < Duration::from_secs(60));
        if guard.len() >= MAX_TRACKED_ADDRESSES {
            // Still full: refuse rather than grow. An anonymous request is the one we can
            // afford to lose.
            return false;
        }
    }

    let window = guard.entry(address.to_string()).or_insert_with(|| Window {
        started: Instant::now(),
        count: 0,
    });

    if window.started.elapsed() >= Duration::from_secs(60) {
        window.started = Instant::now();
        window.count = 0;
    }

    window.count += 1;
    window.count <= quota
}

/// Guard for the endpoints the public tool pages drive.
///
/// It is `ApiKey` with one extra branch: when `PUBLIC_TOOLS` is on and no key was
/// presented, the request proceeds as `public`, rate-limited by client address. A valid key
/// always wins — an integration keeps its own name, its own quota and its own metrics line.
///
/// Deliberately *not* applied to `/api/render`: a Tera template from an anonymous caller is
/// a template engine facing the open internet, which is a different conversation.
pub struct PublicOrKey(pub &'static str);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for PublicOrKey {
    type Error = AppError;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match ApiKey::from_request(req).await {
            Outcome::Success(key) => Outcome::Success(PublicOrKey(key.0)),
            Outcome::Error((status, error)) => {
                // A quota refusal is the key's own, and must not be laundered into an
                // anonymous request that would get a second allowance.
                if status == Status::TooManyRequests || !public_tier() {
                    return Outcome::Error((status, error));
                }

                // A key that was presented and is wrong stays wrong: falling back to the
                // public tier would hide a typo in someone's deployment for months.
                let presented = req.headers().get_one("X-API-Key").is_some()
                    || req.headers().get_one("Authorization").is_some();
                if presented {
                    return Outcome::Error((status, error));
                }

                let address = req
                    .client_ip()
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                if !within_public_quota(&address) {
                    return Outcome::Error((
                        Status::TooManyRequests,
                        AppError::TooManyRequests(format!(
                            "Free tier: {} requests per minute. Use an API key for more.",
                            public_quota_per_minute()
                        )),
                    ));
                }

                req.local_cache(|| KeyName(PUBLIC));
                Outcome::Success(PublicOrKey(PUBLIC))
            }
            Outcome::Forward(status) => Outcome::Forward(status),
        }
    }
}

/// Compare two secrets without leaking their length or content through timing
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_key_leaves_the_api_open() {
        let keys = parse("");
        assert!(keys.open);
        assert!(keys.entries.is_empty());

        let keys = parse("   ");
        assert!(keys.open);
    }

    /// The behaviour every current deployment depends on: one bare secret, no names.
    #[test]
    fn a_bare_secret_stays_one_anonymous_key() {
        let keys = parse("s3cr3t");
        assert!(!keys.open);
        assert_eq!(keys.entries.len(), 1);
        assert_eq!(keys.entries[0].0, ANONYMOUS);
        assert_eq!(keys.entries[0].1, "s3cr3t");
    }

    /// A secret containing a comma must not be sliced into two useless halves
    #[test]
    fn a_bare_secret_with_a_comma_is_not_split() {
        let keys = parse("abc,def");
        assert_eq!(keys.entries.len(), 1);
        assert_eq!(keys.entries[0].1, "abc,def");
    }

    #[test]
    fn a_list_mints_one_key_per_integration() {
        let keys = parse("core=aaa, zapier=bbb ,portal=ccc");
        assert_eq!(keys.entries.len(), 3);
        assert_eq!(keys.entries[0], ("core", "aaa".to_string()));
        assert_eq!(keys.entries[1], ("zapier", "bbb".to_string()));
        assert_eq!(keys.entries[2], ("portal", "ccc".to_string()));
    }

    /// Mixed forms are what a migration actually looks like
    #[test]
    fn an_unnamed_entry_inside_a_list_keeps_working() {
        let keys = parse("legacysecret,core=aaa");
        assert_eq!(keys.entries.len(), 2);
        assert_eq!(keys.entries[0], (ANONYMOUS, "legacysecret".to_string()));
        assert_eq!(keys.entries[1], ("core", "aaa".to_string()));
    }

    /// The name reaches a metric label and a log field: it cannot carry quotes or newlines
    #[test]
    fn names_are_reduced_to_a_safe_alphabet() {
        let keys = parse("core\"}\n=aaa,ok-name_2=bbb");
        assert_eq!(keys.entries[0].0, "core");
        assert_eq!(keys.entries[1].0, "ok-name_2");
    }

    #[test]
    fn entries_missing_a_name_or_a_secret_are_dropped() {
        let keys = parse("=aaa,core=,good=bbb");
        assert_eq!(keys.entries.len(), 1);
        assert_eq!(keys.entries[0], ("good", "bbb".to_string()));
    }

    #[test]
    fn compares_secrets_without_leaking_the_prefix() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
