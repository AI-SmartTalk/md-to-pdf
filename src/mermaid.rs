//! Mermaid diagram rendering through the Mermaid Studio API.

use crate::config::config;
use crate::types::AppError;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::sync::{Mutex, OnceLock};

const SERVICE: &str = "mermaid-studio";

/// Themes the Studio accepts. Checking here turns a typo into a message that names the
/// mistake instead of an opaque 400 from the other side.
const THEMES: [&str; 4] = ["default", "dark", "forest", "neutral"];

/// The Studio lays node labels out in `<foreignObject>` by default and WeasyPrint drops
/// those: without this directive every diagram reaches the PDF with its labels missing.
const TEXT_LABELS: &str =
    "%%{init: {\"htmlLabels\": false, \"flowchart\": {\"htmlLabels\": false}} }%%";

/// Elements that have no business in a document we print: they execute, they fetch, or
/// WeasyPrint cannot draw them anyway.
const DROPPED_ELEMENTS: [&str; 8] = [
    "script",
    "foreignobject",
    "iframe",
    "object",
    "embed",
    "handler",
    "audio",
    "video",
];

/// A diagram that big is a bug upstream, not a diagram
const MAX_SVG_BYTES: u64 = 4 * 1024 * 1024;

const CACHE_MAX_ENTRIES: usize = 128;
const CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Render Mermaid source to an inline `<svg>` element.
///
/// The HTTP client is blocking on purpose: this only runs inside a closure handed to
/// `exec::offload`, never on the async executor.
pub fn render_mermaid(code: &str, theme: Option<&str>) -> Result<String, AppError> {
    let code = code.trim();
    if code.is_empty() {
        return Err(AppError::BadRequest(
            "mermaid: the diagram is empty".to_string(),
        ));
    }

    let theme = checked_theme(theme)?;
    let key = fingerprint(code, theme);
    if let Some(svg) = cached(&key) {
        return Ok(svg);
    }

    let svg = normalize_svg(&fetch(code, theme)?, &namespace(&key))?;
    remember(key, &svg);
    Ok(svg)
}

fn checked_theme(theme: Option<&str>) -> Result<Option<&str>, AppError> {
    match theme.map(str::trim).filter(|theme| !theme.is_empty()) {
        None => Ok(None),
        Some(theme) if THEMES.iter().any(|known| known.eq_ignore_ascii_case(theme)) => {
            Ok(Some(theme))
        }
        Some(theme) => Err(AppError::BadRequest(format!(
            "mermaid: unknown theme {:?}, expected one of {}",
            theme,
            THEMES.join(", ")
        ))),
    }
}

// ------------ Rendered diagram cache ------------

/// Diagrams are cached in memory rather than through `cache.rs`: that cache stores PDFs on
/// disk and counts its hits in the render metrics, while a diagram is a few kilobytes of
/// markup reused within a single document. The PDF built out of it is cached on disk anyway.
#[derive(Default)]
struct MemoryCache {
    entries: HashMap<String, String>,
    order: VecDeque<String>,
    bytes: usize,
}

fn diagrams() -> &'static Mutex<MemoryCache> {
    static CACHE: OnceLock<Mutex<MemoryCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(MemoryCache::default()))
}

/// A poisoned lock must not take the render path down: the cache is an optimisation
fn locked() -> std::sync::MutexGuard<'static, MemoryCache> {
    diagrams()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn cached(key: &str) -> Option<String> {
    locked().entries.get(key).cloned()
}

/// Insertion order eviction, not recency: the same diagram comes back inside one document
/// or not at all, so tracking recency would only buy bookkeeping.
fn remember(key: String, svg: &str) {
    let mut cache = locked();
    if cache.entries.contains_key(&key) {
        return;
    }

    cache.bytes += svg.len();
    cache.order.push_back(key.clone());
    cache.entries.insert(key, svg.to_string());

    while cache.order.len() > CACHE_MAX_ENTRIES || cache.bytes > CACHE_MAX_BYTES {
        match cache.order.pop_front() {
            Some(oldest) => {
                if let Some(evicted) = cache.entries.remove(&oldest) {
                    cache.bytes = cache.bytes.saturating_sub(evicted.len());
                }
            }
            None => break,
        }
    }
}

fn fingerprint(code: &str, theme: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    hasher.update([0u8]);
    hasher.update(theme.unwrap_or_default().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Prefix every id of one diagram with a fingerprint of its source, so two diagrams in the
/// same document cannot capture each other's markers and gradients. Derived from the
/// content and not from a counter: the same document must expand to the same bytes, or the
/// PDF cache never hits again.
fn namespace(key: &str) -> String {
    format!("mmd{}-", &key[..10])
}

// ------------ Mermaid Studio call ------------

fn fetch(code: &str, theme: Option<&str>) -> Result<String, AppError> {
    let base = config()
        .mermaid_api_url
        .as_deref()
        .ok_or_else(|| upstream("MERMAID_API_URL is not configured"))?;

    let mut payload = serde_json::json!({ "code": with_text_labels(code), "format": "svg" });
    if let Some(theme) = theme {
        payload["theme"] = serde_json::Value::String(theme.to_string());
    }

    let mut request = client()?
        .post(format!("{}/render/image", base))
        .json(&payload);
    if let Some(api_key) = config().mermaid_api_key.as_deref() {
        request = request.header("X-Api-Key", api_key);
    }

    let response = request.send().map_err(|e| upstream(&e.to_string()))?;
    let status = response.status();
    let body = read_capped(response)?;

    if status.is_success() {
        return Ok(body);
    }

    // 4xx means the diagram or its options are wrong, which is something the caller can fix
    if status.is_client_error() {
        return Err(AppError::BadRequest(format!(
            "mermaid: {}",
            failure_detail(&body)
        )));
    }

    Err(upstream(&format!(
        "HTTP {}: {}",
        status.as_u16(),
        failure_detail(&body)
    )))
}

fn client() -> Result<&'static reqwest::blocking::Client, AppError> {
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();

    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(config().mermaid_timeout)
                .user_agent(concat!("md-to-pdf/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| error!("Could not build the Mermaid HTTP client: {}", e))
                .ok()
        })
        .as_ref()
        .ok_or_else(|| upstream("the HTTP client could not be built"))
}

fn read_capped(response: reqwest::blocking::Response) -> Result<String, AppError> {
    let mut buffer = Vec::new();
    let mut limited = response.take(MAX_SVG_BYTES + 1);
    limited
        .read_to_end(&mut buffer)
        .map_err(|e| upstream(&e.to_string()))?;

    if buffer.len() as u64 > MAX_SVG_BYTES {
        return Err(upstream("the diagram exceeds 4 MiB"));
    }

    String::from_utf8(buffer).map_err(|_| upstream("the response is not valid UTF-8"))
}

/// The caller's own directive comes after ours and wins: asking for HTML labels back stays
/// possible, it just has to be explicit.
fn with_text_labels(code: &str) -> String {
    if code.contains("htmlLabels") {
        return code.to_string();
    }
    format!("{}\n{}", TEXT_LABELS, code)
}

/// The Studio reports failures as `{"error": {"message": "..."}}`; anything else is quoted
/// back trimmed, since it ends up in a warning the caller reads.
fn failure_detail(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
        {
            return message.to_string();
        }
    }

    let flat: String = body
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect();
    flat.trim().to_string()
}

fn upstream(details: &str) -> AppError {
    AppError::Upstream {
        service: SERVICE.to_string(),
        details: details.to_string(),
    }
}

// ------------ SVG normalization and sanitization ------------

/// Take the markup as it comes off the wire and make it safe to paste into a document:
/// unknown code from another service, sized so that a page can hold it.
fn normalize_svg(raw: &str, prefix: &str) -> Result<String, AppError> {
    let root = extract_root(raw)?;
    let (sanitized, ids) = sanitize(root, prefix)?;
    let namespaced = rewrite_references(&sanitized, &ids, prefix);
    Ok(single_line(&namespaced))
}

/// Anything before `<svg` (XML prolog, doctype, entity declarations) and after `</svg>` is
/// dropped rather than parsed.
fn extract_root(raw: &str) -> Result<&str, AppError> {
    let lower = raw.to_ascii_lowercase();
    let start = lower
        .find("<svg")
        .ok_or_else(|| upstream("the response is not an SVG document"))?;
    let end = lower
        .rfind("</svg>")
        .ok_or_else(|| upstream("the SVG document is truncated"))?;

    if end <= start {
        return Err(upstream("the SVG document is malformed"));
    }

    Ok(&raw[start..end + "</svg>".len()])
}

struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, Option<&'a str>)>,
    self_closing: bool,
    /// Byte index just past the closing `>`
    end: usize,
}

fn sanitize(body: &str, prefix: &str) -> Result<(String, HashSet<String>), AppError> {
    let mut out = String::with_capacity(body.len());
    let mut ids = HashSet::new();
    let mut cursor = 0usize;
    // Depth inside an element being dropped along with everything it contains
    let mut skipping = 0usize;
    let mut in_style = false;
    let mut root_seen = false;

    while cursor < body.len() {
        let next = match body[cursor..].find('<') {
            Some(offset) => cursor + offset,
            None => body.len(),
        };

        if next > cursor && skipping == 0 {
            let text = &body[cursor..next];
            if in_style {
                out.push_str(&sanitize_css(text));
            } else {
                out.push_str(text);
            }
        }

        if next == body.len() {
            break;
        }
        cursor = next;
        let rest = &body[cursor..];

        if rest.starts_with("<!--") {
            cursor = skip_past(body, cursor, "-->");
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            cursor = skip_past(body, cursor, "]]>");
            continue;
        }
        // Doctypes, entity declarations and processing instructions: an external entity is
        // the classic way to make an XML parser read a file for the attacker.
        if rest.starts_with("<!") || rest.starts_with("<?") {
            cursor = skip_past(body, cursor, ">");
            continue;
        }

        if rest.starts_with("</") {
            let end = match body[cursor..].find('>') {
                Some(offset) => cursor + offset + 1,
                None => break,
            };
            if skipping > 0 {
                skipping -= 1;
            } else {
                if body[cursor + 2..end - 1]
                    .trim()
                    .eq_ignore_ascii_case("style")
                {
                    in_style = false;
                }
                out.push_str(&body[cursor..end]);
            }
            cursor = end;
            continue;
        }

        // Markup we cannot parse to its end is markup we stop copying
        let Some(tag) = parse_start_tag(body, cursor) else {
            break;
        };

        if skipping > 0 {
            if !tag.self_closing {
                skipping += 1;
            }
        } else if DROPPED_ELEMENTS
            .iter()
            .any(|dropped| tag.name.eq_ignore_ascii_case(dropped))
        {
            if !tag.self_closing {
                skipping = 1;
            }
        } else {
            let root = !root_seen && tag.name.eq_ignore_ascii_case("svg");
            emit_tag(&mut out, &tag, root, prefix, &mut ids);
            root_seen |= root;
            if !tag.self_closing && tag.name.eq_ignore_ascii_case("style") {
                in_style = true;
            }
        }

        cursor = tag.end;
    }

    if !root_seen {
        return Err(upstream("the response is not an SVG document"));
    }

    Ok((out, ids))
}

fn skip_past(body: &str, from: usize, terminator: &str) -> usize {
    match body[from..].find(terminator) {
        Some(offset) => from + offset + terminator.len(),
        None => body.len(),
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
}

fn parse_start_tag(body: &str, start: usize) -> Option<Tag<'_>> {
    let bytes = body.as_bytes();
    let mut i = start + 1;

    let name_start = i;
    while i < bytes.len() && is_name_byte(bytes[i]) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = &body[name_start..i];

    let mut attrs = Vec::new();
    let mut self_closing = false;

    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }

        match bytes[i] {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                self_closing = true;
                i += 1;
            }
            _ => {
                let key_start = i;
                while i < bytes.len() && is_name_byte(bytes[i]) {
                    i += 1;
                }
                if i == key_start {
                    i += 1; // stray byte between attributes
                    continue;
                }
                let key = &body[key_start..i];

                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }

                let mut value = None;
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return None;
                    }

                    let quote = bytes[i];
                    if quote == b'"' || quote == b'\'' {
                        i += 1;
                        let value_start = i;
                        while i < bytes.len() && bytes[i] != quote {
                            i += 1;
                        }
                        if i >= bytes.len() {
                            return None;
                        }
                        value = Some(&body[value_start..i]);
                        i += 1;
                    } else {
                        let value_start = i;
                        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>'
                        {
                            i += 1;
                        }
                        value = Some(&body[value_start..i]);
                    }
                }

                attrs.push((key, value));
            }
        }
    }

    Some(Tag {
        name,
        attrs,
        self_closing,
        end: i,
    })
}

fn emit_tag(out: &mut String, tag: &Tag, root: bool, prefix: &str, ids: &mut HashSet<String>) {
    out.push('<');
    out.push_str(tag.name);

    let mut view_box = None;
    let mut fallback = (None, None);
    // A label the Studio splits over sibling tspans carries its separating space at the
    // start of the next one: `<tspan>Requete</tspan><tspan> HTTP</tspan>`. The markup then
    // travels through pandoc, which re-wraps long raw-HTML lines at spaces, and the newline
    // it inserts is *dropped* rather than turned back into a space — "RequeteHTTP".
    // WeasyPrint does not inherit `xml:space` into a tspan, so the attribute has to sit on
    // the elements that hold the text, not only on the root.
    let text_bearing = matches!(
        tag.name.to_ascii_lowercase().as_str(),
        "text" | "tspan" | "textpath"
    );

    for &(key, value) in &tag.attrs {
        let lower = key.to_ascii_lowercase();

        // Re-emitted below with a value of our own, so upstream cannot set it to `default`
        // and undo the protection
        if (root || text_bearing) && lower == "xml:space" {
            continue;
        }

        if root {
            // The Studio ships `width="100%"` with an absolute `max-width` in its style
            // attribute, which makes the printed size depend on whatever CSS is around.
            match lower.as_str() {
                "width" => {
                    fallback.0 = value.and_then(length);
                    continue;
                }
                "height" => {
                    fallback.1 = value.and_then(length);
                    continue;
                }
                "style" => continue,
                "viewbox" => view_box = value,
                _ => {}
            }
        }

        if lower == "id" {
            if let Some(id) = value.filter(|id| namespaceable(id)) {
                ids.insert(id.to_string());
                push_attribute(out, key, &format!("{}{}", prefix, id));
            } else if let Some(id) = value {
                push_attribute(out, key, id);
            }
            continue;
        }

        if let Some(value) = keep_attribute(&lower, value) {
            push_attribute(out, key, &value);
        }
    }

    if root {
        push_attribute(out, "style", &sizing_style(view_box, fallback));
    }

    if root || text_bearing {
        push_attribute(out, "xml:space", "preserve");
    }

    if tag.self_closing {
        out.push_str("/>");
    } else {
        out.push('>');
    }
}

/// Returns the value to emit, or `None` when the attribute must go
fn keep_attribute(lower: &str, value: Option<&str>) -> Option<String> {
    // Event handlers are the whole reason a printed SVG must not be trusted
    if lower.starts_with("on") {
        return None;
    }
    if matches!(lower, "xml:base" | "xlink:base") {
        return None;
    }

    let Some(value) = value else {
        return Some(String::new());
    };

    if matches!(lower, "href" | "xlink:href" | "src" | "data") && !local_reference(value) {
        return None;
    }

    let lowered = value.to_ascii_lowercase();
    if lowered.contains("javascript:") {
        return None;
    }

    if lower == "style" || lowered.contains("url(") || lowered.contains("@import") {
        return Some(sanitize_css(value));
    }

    Some(value.to_string())
}

/// Only what the document already carries: a fragment of this very SVG, or bytes inline
fn local_reference(value: &str) -> bool {
    let value = value.trim();
    if value.starts_with('#') {
        return true;
    }

    let lowered = value.to_ascii_lowercase();
    ["data:image/png;base64,", "data:image/jpeg;base64,"]
        .iter()
        .any(|allowed| lowered.starts_with(allowed))
}

fn push_attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out.push('"');
}

/// Keep the aspect ratio of the `viewBox`, never grow past the intrinsic size, never past
/// the width of the page. Measured against WeasyPrint: an explicit `width` in pixels makes
/// it reserve the unscaled height, and no width at all makes it drop the diagram entirely.
fn sizing_style(view_box: Option<&str>, fallback: (Option<f32>, Option<f32>)) -> String {
    let dimensions = view_box_dimensions(view_box).or(match fallback {
        (Some(width), Some(height)) => Some((width, height)),
        _ => None,
    });

    match dimensions {
        Some((width, _)) if width > 0.0 => {
            format!("width:100%;max-width:{}px;height:auto", number(width))
        }
        _ => "width:100%;height:auto".to_string(),
    }
}

fn view_box_dimensions(view_box: Option<&str>) -> Option<(f32, f32)> {
    let values: Vec<f32> = view_box?
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<f32>().ok())
        .collect();

    match values[..] {
        [_, _, width, height] if width > 0.0 && height > 0.0 => Some((width, height)),
        _ => None,
    }
}

/// `120`, `120px` or `120pt` are sizes; `100%` is not one we can reason about
fn length(value: &str) -> Option<f32> {
    let value = value.trim().trim_end_matches("px").trim_end_matches("pt");
    value.parse::<f32>().ok().filter(|value| *value > 0.0)
}

fn number(value: f32) -> String {
    let rendered = format!("{:.2}", value);
    rendered
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// A CSS colour such as `#333` must not be mistaken for a reference to an element, so ids
/// that read as hex literals are left alone on both sides (declaration and references).
fn namespaceable(id: &str) -> bool {
    if id.is_empty() || !id.bytes().all(is_name_byte) {
        return false;
    }

    let hex_literal =
        matches!(id.len(), 3 | 4 | 6 | 8) && id.bytes().all(|b| b.is_ascii_hexdigit());
    !hex_literal
}

/// Rewrite `url(#id)`, `href="#id"` and the `#id` selectors of the embedded stylesheet to
/// the namespaced ids emitted above. Longest match wins, and only on a name boundary.
fn rewrite_references(svg: &str, ids: &HashSet<String>, prefix: &str) -> String {
    if ids.is_empty() {
        return svg.to_string();
    }

    let longest = ids.iter().map(String::len).max().unwrap_or(0);
    let mut out = String::with_capacity(svg.len());
    let bytes = svg.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'#' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'#' {
                i += 1;
            }
            out.push_str(&svg[start..i]);
            continue;
        }

        let run_start = i + 1;
        let mut run_end = run_start;
        while run_end < bytes.len() && is_name_byte(bytes[run_end]) && run_end - run_start < longest
        {
            run_end += 1;
        }

        let mut matched = None;
        let mut end = run_end;
        while end > run_start {
            let candidate = &svg[run_start..end];
            let boundary = svg.as_bytes().get(end).is_none_or(|b| !is_name_byte(*b));
            if boundary && ids.contains(candidate) {
                matched = Some(candidate);
                break;
            }
            end -= 1;
        }

        out.push('#');
        match matched {
            Some(id) => {
                out.push_str(prefix);
                out.push_str(id);
                i = run_start + id.len();
            }
            None => i = run_start,
        }
    }

    out
}

/// Neutralize what a stylesheet can reach out to. Applies to `<style>` bodies as well as to
/// `style` and presentation attributes. One forward pass: a hostile sheet with thousands of
/// rules must not cost quadratic time.
fn sanitize_css(css: &str) -> String {
    const TOKENS: [&str; 4] = ["url(", "@import", "expression(", "javascript:"];

    let lowered = css.to_ascii_lowercase();
    let mut out = String::with_capacity(css.len());
    let mut i = 0usize;

    while i < css.len() {
        let found = TOKENS
            .iter()
            .filter_map(|token| lowered[i..].find(token).map(|at| (i + at, *token)))
            .min_by_key(|(at, _)| *at);

        let Some((at, token)) = found else {
            out.push_str(&css[i..]);
            break;
        };

        out.push_str(&css[i..at]);

        match token {
            "url(" => match css[at..].find(')') {
                None => {
                    // Unterminated: everything after it is unparseable, so it goes
                    i = css.len();
                }
                Some(offset) => {
                    let end = at + offset + 1;
                    let target = css[at + token.len()..end - 1]
                        .trim()
                        .trim_matches(['"', '\'']);
                    if local_reference(target) {
                        out.push_str(&css[at..end]);
                    } else {
                        out.push_str("none");
                    }
                    i = end;
                }
            },
            "@import" => {
                i = css[at..]
                    .find(';')
                    .map(|offset| at + offset + 1)
                    .unwrap_or(css.len());
            }
            "expression(" => {
                out.push_str("none(");
                i = at + token.len();
            }
            _ => i = at + token.len(),
        }
    }

    out
}

/// pandoc reads a raw HTML block up to the first blank line, so the diagram must hold on
/// exactly one line.
fn single_line(svg: &str) -> String {
    svg.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "mmd0123456789-";

    fn normalized(raw: &str) -> String {
        normalize_svg(raw, PREFIX).expect("the SVG should be accepted")
    }

    /// Pandoc re-wraps long raw-HTML lines at spaces, and the newline it inserts is dropped
    /// instead of becoming a space: `<tspan>Requete</tspan><tspan> HTTP</tspan>` prints
    /// "RequeteHTTP". WeasyPrint does not inherit `xml:space` into a tspan, so the root
    /// carrying it is not enough.
    #[test]
    fn every_element_holding_text_preserves_whitespace() {
        let svg = normalized(
            r#"<svg viewBox="0 0 200 174" xml:space="default"><text y="-10"><tspan xml:space="default">Requete</tspan><tspan> HTTP</tspan></text><rect/></svg>"#,
        );

        assert_eq!(svg.matches(r#"xml:space="preserve""#).count(), 4);
        assert!(!svg.contains(r#"xml:space="default""#));
        // The attribute belongs on the elements that hold text, not on everything
        assert!(svg.contains("<rect/>"));
    }

    #[test]
    fn keeps_the_ratio_and_caps_the_width() {
        let svg = normalized(
            r#"<svg id="rendered-diagram" width="100%" viewBox="0.5 0 119.8 327.8" style="max-width: 119.8px;"><rect/></svg>"#,
        );

        assert!(svg.contains(r#"viewBox="0.5 0 119.8 327.8""#));
        assert!(svg.contains("width:100%;max-width:119.8px;height:auto"));
        assert!(!svg.contains(r#"width="100%""#));
        assert!(!svg.contains("max-width: 119.8px"));
    }

    #[test]
    fn falls_back_to_the_declared_size_without_a_view_box() {
        let svg = normalized(r#"<svg width="300px" height="150"><rect/></svg>"#);
        assert!(svg.contains("width:100%;max-width:300px;height:auto"));
    }

    #[test]
    fn keeps_a_usable_width_when_nothing_says_how_big_it_is() {
        let svg = normalized(r#"<svg width="100%"><rect/></svg>"#);
        assert!(svg.contains(r#"style="width:100%;height:auto""#));
    }

    #[test]
    fn drops_scripts_with_their_content() {
        let svg =
            normalized(r#"<svg viewBox="0 0 10 10"><script>alert(1)</script><rect x="1"/></svg>"#);
        assert!(!svg.contains("alert"));
        assert!(!svg.contains("script"));
        assert!(svg.contains(r#"<rect x="1"/>"#));
    }

    #[test]
    fn drops_foreign_objects_and_everything_nested_in_them() {
        let svg = normalized(
            r#"<svg viewBox="0 0 10 10"><g><foreignObject width="10"><div><span>label</span></div></foreignObject></g><rect/></svg>"#,
        );
        assert!(!svg.contains("label"));
        assert!(!svg.contains("foreignObject"));
        assert!(svg.contains("<g>"));
        assert!(svg.contains("</g>"));
        assert!(svg.contains("<rect/>"));
    }

    #[test]
    fn drops_event_handlers() {
        let svg = normalized(
            r#"<svg viewBox="0 0 10 10"><rect onload="fetch('http://x')" onclick="x()" fill="red"/></svg>"#,
        );
        assert!(!svg.contains("onload"));
        assert!(!svg.contains("onclick"));
        assert!(svg.contains(r#"fill="red""#));
    }

    #[test]
    fn drops_remote_references_but_keeps_local_ones() {
        let svg = normalized(
            r##"<svg viewBox="0 0 10 10"><use href="https://evil.test/x.svg#a"/><image href="http://evil.test/x.png"/><use xlink:href="#local"/><a href="https://evil.test">t</a></svg>"##,
        );
        assert!(!svg.contains("evil.test"));
        assert!(svg.contains(r##"xlink:href="#local""##));
    }

    #[test]
    fn drops_doctypes_entities_and_processing_instructions() {
        let svg = normalized(
            "<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><svg viewBox=\"0 0 10 10\"><rect/></svg>",
        );
        assert!(!svg.contains("ENTITY"));
        assert!(!svg.contains("passwd"));
        assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn neutralizes_remote_urls_in_the_embedded_stylesheet() {
        let svg = normalized(
            r#"<svg viewBox="0 0 10 10"><style>@import url("https://evil.test/x.css");.node{fill:#333;background:url(http://evil.test/y.png);mask:url(#local)}</style><rect/></svg>"#,
        );
        assert!(!svg.contains("evil.test"));
        assert!(!svg.contains("@import"));
        assert!(svg.contains("fill:#333"));
        assert!(svg.contains("mask:url(#mmd0123456789-local)") || svg.contains("mask:url(#local)"));
    }

    #[test]
    fn namespaces_ids_and_the_selectors_that_point_at_them() {
        let svg = normalized(
            r#"<svg id="rendered-diagram" viewBox="0 0 10 10"><style>#rendered-diagram .node rect{fill:#333;}</style><marker id="rendered-diagram_pointEnd"/><path marker-end="url(#rendered-diagram_pointEnd)"/></svg>"#,
        );

        assert!(svg.contains(r#"id="mmd0123456789-rendered-diagram""#));
        assert!(svg.contains("#mmd0123456789-rendered-diagram .node rect"));
        assert!(svg.contains(r#"id="mmd0123456789-rendered-diagram_pointEnd""#));
        assert!(svg.contains("url(#mmd0123456789-rendered-diagram_pointEnd)"));
        // A colour is not a reference
        assert!(svg.contains("fill:#333"));
    }

    #[test]
    fn does_not_rewrite_a_reference_to_an_id_that_was_never_declared() {
        let svg = normalized(
            r#"<svg viewBox="0 0 10 10"><rect id="a"/><path fill="url(#ab)" stroke="url(#a)"/></svg>"#,
        );
        assert!(svg.contains("url(#ab)"));
        assert!(svg.contains("url(#mmd0123456789-a)"));
    }

    #[test]
    fn refuses_a_response_that_is_not_an_svg() {
        let error = normalize_svg("{\"error\":\"nope\"}", PREFIX).expect_err("should be refused");
        assert!(matches!(error, AppError::Upstream { .. }));
    }

    #[test]
    fn holds_on_a_single_line() {
        let svg = normalized("<svg viewBox=\"0 0 10 10\">\n  <rect/>\n</svg>");
        assert!(!svg.contains('\n'));
    }

    #[test]
    fn adds_the_text_label_directive_unless_the_diagram_sets_it() {
        assert!(with_text_labels("flowchart TD\n A-->B").starts_with("%%{init:"));
        let explicit = "%%{init: {\"flowchart\": {\"htmlLabels\": true}} }%%\nflowchart TD";
        assert_eq!(with_text_labels(explicit), explicit);
    }

    #[test]
    fn refuses_an_unknown_theme_without_a_round_trip() {
        assert!(matches!(
            checked_theme(Some("bogus")),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(checked_theme(Some("dark")), Ok(Some("dark"))));
        assert!(matches!(checked_theme(Some("  ")), Ok(None)));
    }

    #[test]
    fn the_same_diagram_always_gets_the_same_namespace() {
        let first = namespace(&fingerprint("flowchart TD\n A-->B", Some("dark")));
        let second = namespace(&fingerprint("flowchart TD\n A-->B", Some("dark")));
        let other = namespace(&fingerprint("flowchart TD\n A-->B", None));

        assert_eq!(first, second);
        assert_ne!(first, other);
    }
}
