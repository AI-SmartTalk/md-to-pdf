//! Content-addressed PDF cache.
//!
//! Entries live under `public/cache`, never under `public/pdf`: production purges the
//! latter on its own schedule and would otherwise delete entries the cache still counts on.

use crate::config::config;
use crate::obs;
use crate::types::{AppError, LayoutReport};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Bump this whenever the pipeline can produce a different PDF from the same input,
/// otherwise a deployment keeps serving output rendered by the previous version.
const CACHE_VERSION: &str = "2";

/// Sweeping walks the whole directory, so it runs at most this often
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Evicting down to the exact ceiling would trigger a new sweep on the next insertion
const EVICT_TARGET_PERCENT: u64 = 80;

/// A part separator cannot appear inside a length prefix, so two different sets of parts
/// can never hash to the same value.
const PART_SEPARATOR: u8 = 0x1f;

/// Identity of a rendered document: the hex SHA-256 of everything that shaped it.
#[derive(Debug, Clone)]
pub struct CacheKey(String);

impl CacheKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Hash the parts that fully determine a PDF. Each part is length-prefixed: `["ab", "c"]`
/// and `["a", "bc"]` must not collide.
pub fn key_for(parts: &[&str]) -> CacheKey {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_VERSION.as_bytes());
    hasher.update([PART_SEPARATOR]);

    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([PART_SEPARATOR]);
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{:02x}", byte);
    }

    CacheKey(hex)
}

/// Path of a cached PDF, fanned out by the first two characters of the hash so a single
/// directory never holds tens of thousands of entries.
fn pdf_path(key: &CacheKey) -> PathBuf {
    let hash = key.as_str();
    let shard = hash.get(..2).unwrap_or("00");
    root().join(shard).join(format!("{}.pdf", hash))
}

/// Sidecar holding the layout report of the entry, so a cache hit can answer with the
/// report the first render produced instead of dropping it.
fn meta_path(key: &CacheKey) -> PathBuf {
    pdf_path(key).with_extension("json")
}

fn root() -> PathBuf {
    Path::new("public").join("cache")
}

/// Cached PDF for this key, or `None` when absent, expired or the cache is off.
/// Counts the hit or the miss, so callers never have to.
pub fn get(key: &CacheKey) -> Option<PathBuf> {
    if !config().cache_enabled {
        return None;
    }

    let path = pdf_path(key);
    let fresh = match fs::metadata(&path) {
        Ok(meta) => meta.len() > 0 && age(&meta) <= config().cache_ttl,
        Err(_) => false,
    };

    if !fresh {
        // An expired entry is dropped now rather than waiting for the next sweep
        if path.exists() {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(meta_path(key));
        }
        obs::record_cache_miss();
        return None;
    }

    touch(&path);
    obs::record_cache_hit();
    Some(path)
}

/// Store a rendered PDF. Returns the path it can be served from; a disabled cache is a
/// no-op that hands the original path back.
pub fn put(key: &CacheKey, pdf: &Path) -> Result<PathBuf, AppError> {
    if !config().cache_enabled {
        return Ok(pdf.to_path_buf());
    }

    let destination = pdf_path(key);
    let directory = destination
        .parent()
        .ok_or_else(|| AppError::Io(std::io::Error::other("cache path has no parent")))?;
    fs::create_dir_all(directory)?;

    // Same directory then rename: a concurrent reader either sees no entry or a complete
    // one, never a half-written PDF.
    let staged = tempfile::Builder::new()
        .prefix(".staging-")
        .suffix(".pdf")
        .tempfile_in(directory)?;
    fs::copy(pdf, staged.path())?;
    staged
        .persist(&destination)
        .map_err(|e| AppError::Io(e.error))?;

    maybe_sweep();

    Ok(destination)
}

/// Remember the layout report attached to an entry. Best effort: losing it only costs the
/// report on a later cache hit, never the PDF.
pub fn put_layout(key: &CacheKey, report: &LayoutReport) {
    if !config().cache_enabled {
        return;
    }

    let path = meta_path(key);
    match serde_json::to_vec(report) {
        Ok(bytes) => {
            if let Err(e) = fs::write(&path, bytes) {
                debug!("Could not write the cache sidecar {:?}: {}", path, e);
            }
        }
        Err(e) => debug!("Could not serialize the layout report: {}", e),
    }
}

/// Layout report memorized for this entry, when there was one
pub fn layout_for(key: &CacheKey) -> Option<LayoutReport> {
    let bytes = fs::read(meta_path(key)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn age(meta: &fs::Metadata) -> Duration {
    meta.modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .unwrap_or_default()
}

/// Refresh the entry's mtime so eviction can order entries by last use, and so a hot entry
/// does not expire under the TTL. Rewriting the last byte with its own value is the only
/// way to move an mtime without pulling `libc` or `filetime` in; the content is unchanged,
/// so a reader racing with it sees the same bytes either way.
fn touch(path: &Path) {
    let mut file = match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(_) => return,
    };

    let length = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(_) => return,
    };
    if length == 0 {
        return;
    }

    let mut last = [0u8; 1];
    if file.seek(SeekFrom::Start(length - 1)).is_err()
        || file.read_exact(&mut last).is_err()
        || file.seek(SeekFrom::Start(length - 1)).is_err()
    {
        return;
    }

    let _ = file.write_all(&last);
}

/// Walking the cache on every insertion would dominate the cost of a cache hit
fn maybe_sweep() {
    static LAST_SWEEP: AtomicU64 = AtomicU64::new(0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);

    let last = LAST_SWEEP.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < SWEEP_INTERVAL.as_secs() {
        return;
    }

    // Claim the sweep before doing it: two concurrent insertions must not both walk
    if LAST_SWEEP
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }

    if let Err(e) = sweep() {
        warn!("Cache eviction failed: {e:?}");
    }
}

/// One cache entry, as eviction sees it
struct Entry {
    pdf: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

/// Drop the least recently used entries until the cache is back under the ceiling
fn sweep() -> Result<(), AppError> {
    let ceiling = config().cache_max_mb.saturating_mul(1024 * 1024);
    if ceiling == 0 {
        return Ok(());
    }

    let mut entries = Vec::new();
    let mut total: u64 = 0;

    let shards = match fs::read_dir(root()) {
        Ok(shards) => shards,
        // Nothing has been cached yet
        Err(_) => return Ok(()),
    };

    for shard in shards.flatten() {
        let files = match fs::read_dir(shard.path()) {
            Ok(files) => files,
            Err(_) => continue,
        };

        for file in files.flatten() {
            let path = file.path();
            let meta = match file.metadata() {
                Ok(meta) if meta.is_file() => meta,
                _ => continue,
            };

            total += meta.len();

            // Sidecars are evicted with their PDF, never on their own
            if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
                continue;
            }

            entries.push(Entry {
                pdf: path,
                bytes: meta.len(),
                modified: meta.modified().unwrap_or(UNIX_EPOCH),
            });
        }
    }

    if total <= ceiling {
        return Ok(());
    }

    let target = ceiling / 100 * EVICT_TARGET_PERCENT;
    entries.sort_by_key(|entry| entry.modified);

    for entry in entries {
        if total <= target {
            break;
        }

        let sidecar = entry.pdf.with_extension("json");
        let sidecar_bytes = fs::metadata(&sidecar).map(|m| m.len()).unwrap_or(0);

        if fs::remove_file(&entry.pdf).is_ok() {
            total = total.saturating_sub(entry.bytes);
            if fs::remove_file(&sidecar).is_ok() {
                total = total.saturating_sub(sidecar_bytes);
            }
        }
    }

    Ok(())
}
