//! Uploaded files, addressed by an opaque id and forgotten on a timer.
//!
//! Until this module existed the service could only work on documents it had produced
//! itself: every PDF-consuming route resolved a `/download/<client_id>/<name>` path, so a
//! caller arriving with a file of their own had no way in. An asset is that way in — and
//! deliberately nothing more. It is not a library, not a workspace and not a document
//! store: it is a file we agreed to hold for a couple of hours so a tool can read it.
//!
//! The expiry is the whole design. A file someone uploaded is personal data, and the only
//! retention policy that never surprises anyone is the one the service enforces itself.

use crate::config::config;
use crate::helpers;
use crate::types::AppError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Length of the random part of an asset id, in bytes (32 hex characters)
const ID_BYTES: usize = 16;

/// What the magic bytes said the file is. The extension is never trusted: a `.pdf` that
/// starts with `PK` is a zip, and handing it to Ghostscript is how a parser gets to run on
/// something nobody checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Pdf,
    Png,
    Jpeg,
    Gif,
    Webp,
    Tiff,
    /// OOXML or ODF — both are zip containers, told apart by the file name
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    /// Legacy OLE compound documents (.doc, .xls, .ppt)
    Ole,
    Html,
    Markdown,
    Text,
    Csv,
    /// Recognised as nothing we can process. Stored, but no tool will accept it.
    Unknown,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Pdf => "pdf",
            AssetKind::Png => "png",
            AssetKind::Jpeg => "jpeg",
            AssetKind::Gif => "gif",
            AssetKind::Webp => "webp",
            AssetKind::Tiff => "tiff",
            AssetKind::Docx => "docx",
            AssetKind::Xlsx => "xlsx",
            AssetKind::Pptx => "pptx",
            AssetKind::Odt => "odt",
            AssetKind::Ods => "ods",
            AssetKind::Odp => "odp",
            AssetKind::Ole => "ole",
            AssetKind::Html => "html",
            AssetKind::Markdown => "markdown",
            AssetKind::Text => "text",
            AssetKind::Csv => "csv",
            AssetKind::Unknown => "unknown",
        }
    }

    /// Extension the stored copy gets, so the tools that sniff by name behave
    pub fn extension(self) -> &'static str {
        match self {
            AssetKind::Jpeg => "jpg",
            AssetKind::Ole => "doc",
            AssetKind::Markdown => "md",
            AssetKind::Text => "txt",
            AssetKind::Unknown => "bin",
            other => other.as_str(),
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            AssetKind::Pdf => "application/pdf",
            AssetKind::Png => "image/png",
            AssetKind::Jpeg => "image/jpeg",
            AssetKind::Gif => "image/gif",
            AssetKind::Webp => "image/webp",
            AssetKind::Tiff => "image/tiff",
            AssetKind::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            AssetKind::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            AssetKind::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            AssetKind::Odt => "application/vnd.oasis.opendocument.text",
            AssetKind::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            AssetKind::Odp => "application/vnd.oasis.opendocument.presentation",
            AssetKind::Ole => "application/msword",
            AssetKind::Html => "text/html",
            AssetKind::Markdown => "text/markdown",
            AssetKind::Text => "text/plain",
            AssetKind::Csv => "text/csv",
            AssetKind::Unknown => "application/octet-stream",
        }
    }

    /// Can LibreOffice turn this into a PDF?
    pub fn is_office(self) -> bool {
        matches!(
            self,
            AssetKind::Docx
                | AssetKind::Xlsx
                | AssetKind::Pptx
                | AssetKind::Odt
                | AssetKind::Ods
                | AssetKind::Odp
                | AssetKind::Ole
                | AssetKind::Csv
        )
    }

    pub fn is_image(self) -> bool {
        matches!(
            self,
            AssetKind::Png | AssetKind::Jpeg | AssetKind::Gif | AssetKind::Webp | AssetKind::Tiff
        )
    }
}

/// What we know about a stored file. Travels in API responses, hence Serialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: String,
    /// Original file name, sanitised for display only — never used to build a path
    pub name: String,
    pub kind: AssetKind,
    pub bytes: u64,
    /// Page count, when the kind has pages and counting one was cheap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    /// Whether a PDF carries a selectable text layer. `None` when the question does not
    /// apply or could not be answered — never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_text: Option<bool>,
    pub created_at: String,
    pub expires_at: String,
    /// Unix seconds, kept for the purge; the two strings above are for humans
    pub expires_unix: u64,
    /// Name of the API key that uploaded the file — `public` and `open` are owners like any
    /// other, so no deployment loses isolation by not naming its keys.
    ///
    /// Optional on read, and that is deliberate: an asset written before this field existed
    /// carries no owner, and shipping ownership must not revoke the files already in flight.
    /// Absent means "anyone who knows the id", which is exactly what those files were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// May a request authenticated as `owner` see this asset?
///
/// An asset with no recorded owner is visible to everyone — see the field's note. Anything
/// written since is visible to one key name and no other; the id alone is not a capability,
/// because ids leak into logs, support tickets and `Referer` headers.
pub fn belongs_to(meta: &AssetMeta, owner: &str) -> bool {
    match meta.owner.as_deref() {
        None => true,
        Some(recorded) => recorded == owner,
    }
}

/// Root directory holding every uploaded asset
pub fn assets_root() -> PathBuf {
    Path::new("public").join("assets")
}

fn asset_dir(id: &str) -> PathBuf {
    assets_root().join(id)
}

// ------------ Identifiers ------------

/// Accept an id only in the exact shape we mint. It becomes a directory name, so this is
/// the boundary that keeps `../` and friends out of the asset root.
pub fn validate_id(id: &str) -> Result<&str, AppError> {
    let valid = id.len() == 3 + ID_BYTES * 2
        && id.starts_with("as_")
        && id[3..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());

    if !valid {
        return Err(AppError::BadRequest(format!(
            "Invalid asset id: {} (expected as_ followed by 32 hex characters)",
            id
        )));
    }

    Ok(id)
}

// ------------ Type detection ------------

/// Identify a file by its first bytes, falling back to the name only to tell apart the
/// formats that genuinely share a container (OOXML and ODF are both zip archives).
pub fn detect_kind(bytes: &[u8], name: &str) -> AssetKind {
    if bytes.starts_with(b"%PDF-") {
        return AssetKind::Pdf;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return AssetKind::Png;
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return AssetKind::Jpeg;
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return AssetKind::Gif;
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return AssetKind::Webp;
    }
    if bytes.starts_with(&[0x49, 0x49, 0x2a, 0x00]) || bytes.starts_with(&[0x4d, 0x4d, 0x00, 0x2a])
    {
        return AssetKind::Tiff;
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return AssetKind::Ole;
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return zip_kind(bytes, name);
    }

    // A PDF whose header is not at offset 0 is the most ordinary damage there is — a stray
    // BOM, a few bytes of an HTTP response, a botched concatenation — and it is precisely the
    // file `/api/repair` exists for. qpdf and poppler both scan the first kilobyte for the
    // header and open such a file without complaint, so refusing it here would mean the
    // repair tool cannot be handed the breakage it advertises. The scan runs after every
    // other magic number, so a container that merely quotes the string keeps its own type.
    if has_offset_pdf_header(bytes) {
        return AssetKind::Pdf;
    }

    text_kind(bytes, name)
}

/// How far in we look for a PDF header that is not at offset 0. The same kilobyte qpdf and
/// poppler scan: past it, a file no reader will open is not one we should call a PDF.
const PDF_HEADER_SCAN: usize = 1024;

/// Is there a displaced `%PDF-` header in the first kilobyte?
///
/// The version digit is required, and it is what keeps a document *about* PDFs from becoming
/// one: prose quotes `%PDF-` on its own, a real header always carries its version (`%PDF-1.7`).
fn has_offset_pdf_header(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(PDF_HEADER_SCAN)];
    let mut from = 0;

    while let Some(at) = find(&head[from..], b"%PDF-") {
        let version = from + at + b"%PDF-".len();
        if matches!(head.get(version), Some(byte) if byte.is_ascii_digit()) {
            return true;
        }
        from += at + 1;
    }

    false
}

/// A zip could be a .docx, a .odt or a holiday photo album. The OOXML and ODF containers
/// both name their type in the first few hundred bytes, which is cheaper and far more
/// honest than trusting the extension.
fn zip_kind(bytes: &[u8], name: &str) -> AssetKind {
    let head = &bytes[..bytes.len().min(4096)];

    if find(head, b"opendocument.text").is_some() {
        return AssetKind::Odt;
    }
    if find(head, b"opendocument.spreadsheet").is_some() {
        return AssetKind::Ods;
    }
    if find(head, b"opendocument.presentation").is_some() {
        return AssetKind::Odp;
    }

    match extension_of(name).as_str() {
        "docx" | "docm" => AssetKind::Docx,
        "xlsx" | "xlsm" => AssetKind::Xlsx,
        "pptx" | "pptm" => AssetKind::Pptx,
        "odt" => AssetKind::Odt,
        "ods" => AssetKind::Ods,
        "odp" => AssetKind::Odp,
        _ => AssetKind::Unknown,
    }
}

fn text_kind(bytes: &[u8], name: &str) -> AssetKind {
    // A NUL byte in the first kilobyte means binary; no text format we accept contains one
    let head = &bytes[..bytes.len().min(1024)];
    if head.contains(&0) || (!reads_as_text(head, bytes.len()) && bytes.len() > 4) {
        return AssetKind::Unknown;
    }

    let lowered = String::from_utf8_lossy(head).to_ascii_lowercase();
    if lowered.contains("<!doctype html") || lowered.contains("<html") {
        return AssetKind::Html;
    }

    match extension_of(name).as_str() {
        "md" | "markdown" => AssetKind::Markdown,
        "html" | "htm" => AssetKind::Html,
        "csv" => AssetKind::Csv,
        "txt" | "text" | "" => AssetKind::Text,
        _ => AssetKind::Text,
    }
}

/// Is this sample of the file valid text?
///
/// The sample stops at a fixed offset, which lands mid-character often enough to matter: a
/// French `.md` of more than a kilobyte has roughly one chance in fifty of putting an `é`
/// across the boundary, and calling that file binary would deny it to every tool. An error
/// whose `error_len()` is `None` says exactly "the buffer ran out mid-character" — tolerated
/// only when the buffer really was cut short, so a genuinely truncated encoding at the end of
/// a small file is still reported as what it is.
fn reads_as_text(head: &[u8], total_len: usize) -> bool {
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(err) => err.error_len().is_none() && head.len() < total_len,
    }
}

fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Keep a file name presentable without letting it near the filesystem: the stored copy is
/// always `file.<ext>`, this is only what we echo back and what tells a zip from a zip.
pub fn display_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(160).collect();

    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "upload".to_string()
    } else {
        trimmed.to_string()
    }
}

// ------------ Storage ------------

// ------------ Who the work in flight belongs to ------------

thread_local! {
    /// Key name the blocking job running on this thread was submitted under
    static CURRENT_OWNER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Attribute everything stored on this thread to `owner`, until the guard is dropped.
///
/// A tool's output belongs to the caller as much as the file they uploaded does. But the
/// function that stores it — `helpers::finish_tool` — sits three layers below the request
/// and has no key to hand it, and threading an owner through thirteen `run()` signatures
/// would put it in the request body, where a caller could forge it.
///
/// A thread-local set at the top of the blocking job can be neither forged nor forgotten: a
/// job owns its thread from end to end, which is the same property the wall-clock budget
/// relies on (see `exec::offload` and `helpers::Budget`).
pub struct OwnerScope;

pub fn owned_by(owner: &str) -> OwnerScope {
    CURRENT_OWNER.with(|slot| *slot.borrow_mut() = Some(owner.to_string()));
    OwnerScope
}

impl Drop for OwnerScope {
    fn drop(&mut self) {
        CURRENT_OWNER.with(|slot| *slot.borrow_mut() = None);
    }
}

fn current_owner() -> Option<String> {
    CURRENT_OWNER.with(|slot| slot.borrow().clone())
}

/// Move an already-written temporary file into the asset store.
///
/// Blocking work — page counting spawns pdfinfo — so it belongs on a render slot, never on
/// a tokio worker.
///
/// The owner is whatever `owned_by` declared for this thread. Outside such a scope the file
/// is stored unowned and readable by anyone holding the id, which is what every asset was
/// before ownership existed; `store_as` states an owner explicitly.
pub fn store(source: &Path, original_name: &str) -> Result<AssetMeta, AppError> {
    let owner = current_owner();
    store_as(source, original_name, owner.as_deref())
}

/// Move an already-written temporary file into the asset store on behalf of a key.
pub fn store_as(
    source: &Path,
    original_name: &str,
    owner: Option<&str>,
) -> Result<AssetMeta, AppError> {
    let bytes_len = fs::metadata(source)?.len();
    let max_bytes = config().asset_max_mb.saturating_mul(1024 * 1024);
    if bytes_len > max_bytes {
        return Err(AppError::BadRequest(format!(
            "File is {} MB, the limit is {} MB",
            bytes_len / (1024 * 1024),
            config().asset_max_mb
        )));
    }

    let head = read_head(source, 8192)?;
    let name = display_name(original_name);
    let kind = detect_kind(&head, &name);

    let id = helpers::random_id("as")?;
    let dir = asset_dir(&id);
    fs::create_dir_all(&dir)?;

    let stored = dir.join(format!("file.{}", kind.extension()));
    // Rename first: on the same filesystem it is atomic and free. A copy is the fallback
    // for the case where the temp directory lives on another mount.
    if fs::rename(source, &stored).is_err() {
        fs::copy(source, &stored)?;
    }

    let pages = match kind {
        AssetKind::Pdf => match crate::pdfops::page_count(&stored) {
            Ok(count) => {
                let max = config().asset_max_pages;
                if count > max {
                    let _ = fs::remove_dir_all(&dir);
                    return Err(AppError::BadRequest(format!(
                        "Document has {} pages, the limit is {}",
                        count, max
                    )));
                }
                Some(count)
            }
            // A PDF whose page count cannot be read is still a PDF; the tool that needs the
            // count will fail with its own message rather than being denied entry here.
            Err(err) => {
                warn!("Could not count pages of {}: {:?}", id, err);
                None
            }
        },
        _ => None,
    };

    let has_text = match kind {
        AssetKind::Pdf => has_text_layer(&stored),
        _ => None,
    };

    let now = now_unix();
    let expires_unix = now + config().asset_ttl.as_secs();
    let meta = AssetMeta {
        id: id.clone(),
        name,
        kind,
        bytes: bytes_len,
        pages,
        has_text,
        created_at: rfc3339(now),
        expires_at: rfc3339(expires_unix),
        expires_unix,
        owner: owner.map(str::to_string),
    };

    write_meta(&dir, &meta)?;
    Ok(meta)
}

/// Does this PDF carry a text layer a reader could select?
///
/// Answered on the first two pages only: the question is "is this a scan or a real
/// document", and a scan is a scan from its first page. Two `pdftotext` pages cost a few
/// milliseconds, and they are what lets the home page propose OCR to someone who dropped a
/// photocopy — instead of making them guess which of twenty-one tools they need.
///
/// `None` means "could not tell", never "no": a wrong `false` here would push somebody
/// towards an OCR pass their document did not need.
fn has_text_layer(pdf: &Path) -> Option<bool> {
    let output = helpers::run_capture(
        std::process::Command::new("pdftotext")
            .arg("-q")
            .arg("-l")
            .arg("2")
            .arg(helpers::path_to_str(pdf).ok()?)
            .arg("-"),
        "pdftotext",
        "text detection failed",
    )
    .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    // A handful of stray glyphs is what a scanner's header leaves behind; it is not a text
    // layer. Sixteen characters is low enough to catch a sparse cover page and high enough
    // to reject that noise.
    Some(text.chars().filter(|c| !c.is_whitespace()).count() >= 16)
}

fn read_head(path: &Path, limit: usize) -> Result<Vec<u8>, AppError> {
    let mut file = fs::File::open(path)?;
    let mut buf = vec![0u8; limit];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

fn write_meta(dir: &Path, meta: &AssetMeta) -> Result<(), AppError> {
    let json = serde_json::to_string(meta).map_err(|e| AppError::ProcessFailed {
        message: "Could not serialise asset metadata".to_string(),
        stderr: e.to_string(),
    })?;
    fs::write(dir.join("meta.json"), json)?;
    Ok(())
}

/// Metadata of a stored asset, or `NotFound` if it never existed or has expired.
pub fn meta(id: &str) -> Result<AssetMeta, AppError> {
    let id = validate_id(id)?;
    let dir = asset_dir(id);
    let raw = fs::read_to_string(dir.join("meta.json"))
        .map_err(|_| AppError::NotFound(format!("Asset not found: {}", id)))?;

    let meta: AssetMeta = serde_json::from_str(&raw).map_err(|e| AppError::ProcessFailed {
        message: "Could not read asset metadata".to_string(),
        stderr: e.to_string(),
    })?;

    // Expiry is enforced on read, not only by the sweeper: a file the sweeper has not got
    // to yet must already be invisible, otherwise the retention promise is only a promise.
    if meta.expires_unix <= now_unix() {
        let _ = fs::remove_dir_all(&dir);
        return Err(AppError::NotFound(format!("Asset expired: {}", id)));
    }

    Ok(meta)
}

/// Metadata of a stored asset, refused to anyone but its owner.
///
/// The refusal is a `NotFound` and not a `Forbidden`, on purpose: a 403 confirms that the id
/// exists, and confirmation is the only thing a leaked id should not buy.
pub fn meta_as(id: &str, owner: &str) -> Result<AssetMeta, AppError> {
    let meta = meta(id)?;
    if !belongs_to(&meta, owner) {
        return Err(AppError::NotFound(format!("Asset not found: {}", id)));
    }

    Ok(meta)
}

/// Filesystem path of a stored asset, validated and confined to the asset root
pub fn path(id: &str) -> Result<PathBuf, AppError> {
    path_of(&meta(id)?)
}

/// Filesystem path of a stored asset, refused to anyone but its owner
pub fn path_as(id: &str, owner: &str) -> Result<PathBuf, AppError> {
    path_of(&meta_as(id, owner)?)
}

fn path_of(meta: &AssetMeta) -> Result<PathBuf, AppError> {
    let id = &meta.id;
    let candidate = asset_dir(&meta.id).join(format!("file.{}", meta.kind.extension()));

    let root = assets_root();
    fs::create_dir_all(&root)?;
    let base = root
        .canonicalize()
        .map_err(|_| AppError::NotFound("Asset directory not found".to_string()))?;

    let canonical = candidate
        .canonicalize()
        .map_err(|_| AppError::NotFound(format!("Asset not found: {}", id)))?;

    // Defence in depth, exactly as `resolve_pdf_path` does for saved PDFs
    if !canonical.starts_with(&base) {
        return Err(AppError::BadRequest("Invalid asset path".to_string()));
    }

    Ok(canonical)
}

/// Forget an asset now, whatever its expiry said
pub fn delete(id: &str) -> Result<(), AppError> {
    let id = validate_id(id)?;
    let dir = asset_dir(id);
    if !dir.exists() {
        return Err(AppError::NotFound(format!("Asset not found: {}", id)));
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Forget an asset, refusing the request to anyone but its owner.
///
/// The owner is read from disk rather than through `meta`, so that an asset whose expiry has
/// passed but whose directory is still there stays deletable: forgetting a file is never the
/// operation that should fail, and this keeps `DELETE` answering exactly as it did before.
pub fn delete_as(id: &str, owner: &str) -> Result<(), AppError> {
    let id = validate_id(id)?;

    if let Some(recorded) = recorded_owner(id) {
        if recorded != owner {
            return Err(AppError::NotFound(format!("Asset not found: {}", id)));
        }
    }

    delete(id)
}

/// Owner written in `meta.json`, if there is one to read. Metadata we cannot parse yields
/// `None` — the same "belongs to no one" the purge already assumes.
fn recorded_owner(id: &str) -> Option<String> {
    let raw = fs::read_to_string(asset_dir(id).join("meta.json")).ok()?;
    serde_json::from_str::<AssetMeta>(&raw).ok()?.owner
}

/// Drop every expired asset. Cheap enough to run on a timer and idempotent, so a failed
/// sweep is simply retried at the next one.
pub fn purge_expired() -> usize {
    let root = assets_root();
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };

    let now = now_unix();
    let mut removed = 0;

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }

        let expired = match fs::read_to_string(dir.join("meta.json")) {
            Ok(raw) => serde_json::from_str::<AssetMeta>(&raw)
                .map(|meta| meta.expires_unix <= now)
                // Metadata we cannot parse belongs to no one: an asset without a readable
                // expiry would otherwise live for ever.
                .unwrap_or(true),
            Err(_) => true,
        };

        if expired && fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
        }
    }

    removed
}

/// Register the asset root and clear whatever a previous run left behind
pub fn init() {
    if let Err(e) = fs::create_dir_all(assets_root()) {
        error!("Could not create {:?}: {}", assets_root(), e);
        return;
    }

    let removed = purge_expired();
    if removed > 0 {
        info!(
            "Asset store: {} expired uploads removed at startup",
            removed
        );
    }
}

/// Never sweep more often than this, however short the configured TTL is. A sweep walks the
/// whole asset root; a one-minute TTL set for a test must not turn that walk into a busy loop.
const MIN_SWEEP_PERIOD: Duration = Duration::from_secs(60);

/// How often the sweeper runs for a given retention.
///
/// A quarter of the TTL means a file outlives its expiry by at most 25% of the window it was
/// promised — close enough that the promise stays true, far enough from a walk per second.
fn sweep_period(ttl: Duration) -> Duration {
    (ttl / 4).max(MIN_SWEEP_PERIOD)
}

/// Start the background sweep that makes the retention promise true.
///
/// Expiry on read only hides the asset that someone happens to ask for again; nothing else
/// touches the file. Without this loop a container that runs for a month keeps a month of its
/// visitors' documents on disk while the API answers "expired" — a retention claim the
/// service does not honour, and a volume that only ever grows (one `split` of a 500-page PDF
/// creates 500 asset directories in a single call).
///
/// Wiring is the caller's: this is spawned from the startup path, not from this module.
pub fn start_sweeper() {
    let period = sweep_period(config().asset_ttl);

    rocket::tokio::spawn(async move {
        let mut ticker = rocket::tokio::time::interval(period);
        // The first tick fires immediately and `init` has just swept; skip it rather than
        // walk the tree twice in the same second.
        ticker.tick().await;

        loop {
            ticker.tick().await;

            // Reading and unlinking a directory tree is blocking work: on a tokio worker it
            // would stall the request that /api/health is answering.
            match rocket::tokio::task::spawn_blocking(purge_expired).await {
                // Silence when there was nothing to do — a line per half-hour saying "0" is
                // how a log stops being read.
                Ok(0) => {}
                Ok(removed) => info!("Asset store: {} expired uploads swept", removed),
                Err(e) => error!("Asset sweep did not run: {}", e),
            }
        }
    });

    info!(
        "Asset sweeper: every {}s, dropping uploads past their {}s retention",
        period.as_secs(),
        config().asset_ttl.as_secs()
    );
}

// ------------ Time ------------

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds to `YYYY-MM-DDTHH:MM:SSZ`.
///
/// A date crate for one format string would be one more dependency in an image that is
/// audited by hand; this is the civil-from-days algorithm, which is exact.
pub fn rfc3339(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs_of_day = unix % 86_400;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Strip the `asset://` scheme, if the reference carries one
pub fn strip_scheme(reference: &str) -> Option<&str> {
    reference.strip_prefix("asset://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_formats_by_their_first_bytes() {
        assert_eq!(detect_kind(b"%PDF-1.7\n", "x.doc"), AssetKind::Pdf);
        assert_eq!(
            detect_kind(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a], "x.pdf"),
            AssetKind::Png
        );
        assert_eq!(detect_kind(&[0xff, 0xd8, 0xff, 0xe0], "x"), AssetKind::Jpeg);
        assert_eq!(detect_kind(b"GIF89a....", "x"), AssetKind::Gif);
        assert_eq!(detect_kind(b"RIFF\0\0\0\0WEBPVP8 ", "x"), AssetKind::Webp);
        assert_eq!(
            detect_kind(&[0xd0, 0xcf, 0x11, 0xe0], "x.doc"),
            AssetKind::Ole
        );
    }

    /// The extension is the last resort, never the first: this is what stops a zip from
    /// reaching a PDF parser just because someone renamed it.
    #[test]
    fn a_renamed_zip_is_not_a_pdf() {
        assert_ne!(
            detect_kind(b"PK\x03\x04....", "invoice.pdf"),
            AssetKind::Pdf
        );

        // Even one that quotes the header inside a member name: the container wins
        let mut zip = b"PK\x03\x04".to_vec();
        zip.extend_from_slice(b"\0\0\0\0sample-%PDF-1.7-bytes.txt");
        assert_ne!(detect_kind(&zip, "invoice.pdf"), AssetKind::Pdf);
    }

    /// The damage `/api/repair` exists for: a few stray bytes before the header. qpdf and
    /// poppler open this file; refusing it would mean the repair tool cannot be handed the
    /// breakage it advertises.
    #[test]
    fn a_pdf_behind_a_junk_prefix_is_still_a_pdf() {
        let mut damaged = b"\xef\xbb\xbfHTTP/1.1 200 OK\r\n\r\n".to_vec();
        damaged.extend_from_slice(b"%PDF-1.4\n1 0 obj\n<< >>\nendobj\n");
        assert_eq!(detect_kind(&damaged, "contract.pdf"), AssetKind::Pdf);

        // Far enough in and no reader would open it either, so neither do we
        let mut buried = vec![b'.'; 2000];
        buried.extend_from_slice(b"%PDF-1.4\n");
        assert_ne!(detect_kind(&buried, "contract.pdf"), AssetKind::Pdf);
    }

    /// Prose about PDFs is not a PDF: the version digit is what tells them apart
    #[test]
    fn a_document_quoting_the_header_stays_text() {
        let text = b"# Notes\n\nA PDF file starts with the bytes `%PDF-`, then its version.\n";
        assert_eq!(detect_kind(text, "notes.md"), AssetKind::Markdown);
    }

    #[test]
    fn tells_ooxml_from_odf_inside_the_same_container() {
        let mut odt = b"PK\x03\x04".to_vec();
        odt.extend_from_slice(b"mimetypeapplication/vnd.oasis.opendocument.text");
        assert_eq!(detect_kind(&odt, "report.bin"), AssetKind::Odt);

        assert_eq!(
            detect_kind(b"PK\x03\x04\0\0", "report.docx"),
            AssetKind::Docx
        );
        assert_eq!(
            detect_kind(b"PK\x03\x04\0\0", "photos.zip"),
            AssetKind::Unknown
        );
    }

    #[test]
    fn reads_text_formats() {
        assert_eq!(
            detect_kind(b"# Title\n\ntext", "notes.md"),
            AssetKind::Markdown
        );
        assert_eq!(
            detect_kind(b"<!DOCTYPE html><html>", "page.txt"),
            AssetKind::Html
        );
        assert_eq!(detect_kind(b"a,b,c\n1,2,3", "data.csv"), AssetKind::Csv);
    }

    /// One accented character across the 1 KiB sampling boundary used to make a perfectly
    /// ordinary French `.md` unreadable to every tool in the service.
    #[test]
    fn a_multibyte_character_on_the_sample_boundary_is_still_text() {
        let mut markdown = b"# Compte rendu\n\n".to_vec();
        markdown.resize(1023, b'a');
        // 'é' is two bytes: the first lands at 1023, the second outside the sampled kilobyte
        markdown.extend_from_slice("é et la suite du document".as_bytes());
        assert!(markdown.len() > 1024);

        assert_eq!(
            detect_kind(&markdown, "compte-rendu.md"),
            AssetKind::Markdown
        );
    }

    /// Tolerating a cut-off character must not tolerate binary that merely ends badly
    #[test]
    fn broken_encoding_inside_the_sample_is_still_unknown() {
        let mut bytes = vec![0xc3, 0x28];
        bytes.extend_from_slice(b"the rest reads fine");
        assert_eq!(detect_kind(&bytes, "notes.md"), AssetKind::Unknown);
    }

    /// An id is not a capability: it leaks into logs and tickets, so it names a file for its
    /// owner only. An asset stored before ownership existed has none, and stays readable.
    #[test]
    fn an_asset_belongs_to_the_key_that_uploaded_it() {
        let mut meta = AssetMeta {
            id: format!("as_{}", "c".repeat(32)),
            name: "contract.pdf".to_string(),
            kind: AssetKind::Pdf,
            bytes: 1024,
            pages: Some(3),
            has_text: Some(true),
            created_at: rfc3339(0),
            expires_at: rfc3339(7200),
            expires_unix: 7200,
            owner: Some("client-a".to_string()),
        };

        assert!(belongs_to(&meta, "client-a"));
        assert!(!belongs_to(&meta, "client-b"));

        // The free tier and a keyless deployment are owners like any other
        meta.owner = Some("public".to_string());
        assert!(belongs_to(&meta, "public"));
        assert!(!belongs_to(&meta, "client-a"));

        meta.owner = Some("open".to_string());
        assert!(belongs_to(&meta, "open"));
        assert!(!belongs_to(&meta, "public"));

        // Written before the field existed: readable, or upgrading revokes files in flight
        meta.owner = None;
        assert!(belongs_to(&meta, "client-a"));
        assert!(belongs_to(&meta, "public"));
    }

    /// Metadata written by the previous version has no `owner` key at all
    #[test]
    fn metadata_without_an_owner_still_parses() {
        let raw = r#"{"id":"as_1","name":"a.pdf","kind":"pdf","bytes":10,
                      "created_at":"1970-01-01T00:00:00Z","expires_at":"1970-01-01T02:00:00Z",
                      "expires_unix":7200}"#;
        let meta: AssetMeta = serde_json::from_str(raw).expect("legacy metadata must parse");
        assert!(meta.owner.is_none());
        assert!(belongs_to(&meta, "anything"));
    }

    /// The refusal end to end, on a real stored file: another key must not read it, must not
    /// delete it, and must not be told it exists.
    #[test]
    fn another_key_can_neither_read_nor_delete_someone_elses_asset() {
        let source = tempfile::Builder::new()
            .suffix(".txt")
            .tempfile()
            .expect("temp file");
        fs::write(source.path(), b"a contract, in plain text\n").expect("write");

        let stored = store_as(source.path(), "contract.txt", Some("client-a")).expect("store");
        let id = stored.id.clone();
        assert_eq!(stored.owner.as_deref(), Some("client-a"));

        assert!(meta_as(&id, "client-a").is_ok());
        assert!(path_as(&id, "client-a").is_ok());

        // 404, and worded exactly like an id that names nothing at all
        match meta_as(&id, "client-b") {
            Err(AppError::NotFound(message)) => {
                assert_eq!(message, format!("Asset not found: {}", id))
            }
            other => panic!("client-b should not see the asset: {:?}", other),
        }
        assert!(path_as(&id, "client-b").is_err());
        assert!(delete_as(&id, "client-b").is_err());

        // Refusing to delete means refusing to delete, not pretending to
        assert!(meta_as(&id, "client-a").is_ok());
        assert!(delete_as(&id, "client-a").is_ok());
        assert!(meta(&id).is_err());
    }

    /// A quarter of the retention window, but never a walk of the tree every second
    #[test]
    fn the_sweep_period_follows_the_retention_and_has_a_floor() {
        assert_eq!(
            sweep_period(Duration::from_secs(7200)),
            Duration::from_secs(1800)
        );
        assert_eq!(
            sweep_period(Duration::from_secs(240)),
            Duration::from_secs(60)
        );
        assert_eq!(sweep_period(Duration::from_secs(30)), MIN_SWEEP_PERIOD);
    }

    #[test]
    fn accepts_only_ids_of_the_exact_shape() {
        let good = format!("as_{}", "a".repeat(32));
        assert!(validate_id(&good).is_ok());

        for bad in [
            "as_short",
            "as_../../etc/passwd",
            &format!("as_{}", "A".repeat(32)),
            &format!("xx_{}", "a".repeat(32)),
            &format!("as_{}", "a".repeat(33)),
        ] {
            assert!(validate_id(bad).is_err(), "{} should be rejected", bad);
        }
    }

    #[test]
    fn a_file_name_never_becomes_a_path() {
        assert_eq!(display_name("../../etc/passwd"), "passwd");
        assert_eq!(display_name("C:\\Windows\\evil.docx"), "evil.docx");
        assert_eq!(display_name("   "), "upload");
        assert_eq!(display_name(""), "upload");
    }

    #[test]
    fn formats_timestamps_without_a_date_crate() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_000_000_000), "2001-09-09T01:46:40Z");
        // A leap day, which is where a hand-rolled calendar usually breaks
        assert_eq!(rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn reads_the_scheme_of_a_reference() {
        let id = format!("as_{}", "b".repeat(32));
        assert_eq!(strip_scheme(&format!("asset://{}", id)), Some(id.as_str()));
        assert_eq!(strip_scheme("/download/client/file.pdf"), None);
    }
}
