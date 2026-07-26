//! Anti-SSRF guard.
//!
//! The renderer itself is guarded (see `deploy/weasyprint-safe.py`, which owns the
//! `url_fetcher` policy). This module is the layer in front of it: it turns a document
//! that would obviously be refused into an immediate 400 naming the offending URL,
//! instead of a PDF with a silently missing image.
//!
//! The analysis is purely textual — no HTML parser, no regex — because the same
//! function has to run on Markdown, on raw HTML and on a Tera template, and because it
//! sits on the request path of every render.

use crate::config::config;
use crate::types::AppError;
use std::net::Ipv6Addr;

/// Attributes WeasyPrint may dereference. `data-*` attributes are deliberately not in
/// the list: the backward read below returns the whole identifier, so `data-id="a:b"`
/// never matches `data`.
const TRACKED_ATTRS: &[&str] = &[
    "src",
    "href",
    "xlink:href",
    "srcset",
    "poster",
    "background",
    "data",
    "cite",
    "longdesc",
    "manifest",
    "codebase",
];

/// Schemes that fetch nothing: allowed whatever they point at.
const INERT_SCHEMES: &[&str] = &["data", "mailto", "tel", "sms", "callto"];

/// Names that can only designate an internal resource, whatever DNS says today.
const DENIED_HOSTS: &[&str] = &[
    "localhost",
    "instance-data",
    "metadata",
    "metadata.google.internal",
    "metadata.goog",
];
const DENIED_HOST_SUFFIXES: &[&str] = &[
    ".localhost",
    ".local",
    ".internal",
    ".intranet",
    ".home.arpa",
];

/// Longest URL kept in an error message
const MAX_REPORTED_URL: usize = 120;

/// Reject a document that would make the renderer fetch something it must not reach
/// (link-local metadata endpoints, private ranges, `file://`, hosts outside
/// `config().allowed_url_hosts`, ...).
///
/// Called before any rendering happens, so a rejected document costs no process spawn.
pub fn check_document(input: &str) -> Result<(), AppError> {
    check_with(input, &policy(true))
}

/// Same check for a document that is not Markdown (raw HTML, a Tera template).
///
/// The code-region heuristic below only makes sense on Markdown: applied to HTML it lets a
/// stray backtick run hide the rest of the document from the scan, while the renderer
/// happily dereferences what it hid.
pub fn check_markup(input: &str) -> Result<(), AppError> {
    check_with(input, &policy(false))
}

fn policy(markdown: bool) -> Policy<'static> {
    Policy {
        allowed_hosts: &config().allowed_url_hosts,
        strict_hosts: config().url_strict_hosts,
        markdown,
    }
}

struct Policy<'a> {
    allowed_hosts: &'a [String],
    strict_hosts: bool,
    /// Skip Markdown code spans and fenced blocks
    markdown: bool,
}

fn check_with(input: &str, policy: &Policy) -> Result<(), AppError> {
    let skipped = if policy.markdown {
        code_regions(input)
    } else {
        Vec::new()
    };
    let mut skip_index = 0usize;

    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // The scan runs left to right, so the skipped ranges are consumed in order.
        while skip_index < skipped.len() && skipped[skip_index].1 <= i {
            skip_index += 1;
        }
        if skip_index < skipped.len() && skipped[skip_index].0 <= i {
            i = skipped[skip_index].1;
            continue;
        }

        let (candidates, next) = match bytes[i] {
            b'=' => attribute_at(input, i),
            b'(' => paren_at(input, i),
            b'@' => import_at(input, i),
            b']' => reference_at(input, i),
            b'f' | b'F' => literal_file_at(input, i),
            _ => (Vec::new(), i + 1),
        };

        for candidate in candidates {
            if let Err(reason) = check_url(candidate, policy) {
                return Err(AppError::BadRequest(format!(
                    "document references a blocked URL: {} ({})",
                    shorten(candidate),
                    reason
                )));
            }
        }

        i = next.max(i + 1);
    }

    Ok(())
}

// ------------ candidate extraction ------------

/// `src=`, `href=`, ... The identifier is read backwards from the `=` so that
/// `data-src=` or `xlink:href=` are recognised for what they are.
fn attribute_at(input: &str, equal: usize) -> (Vec<&str>, usize) {
    let bytes = input.as_bytes();
    let mut start = equal;
    while start > 0 && bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    let end = start;
    while start > 0 && is_name_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start == end {
        return (Vec::new(), equal + 1);
    }
    let name = input[start..end].to_ascii_lowercase();
    if !TRACKED_ATTRS.contains(&name.as_str()) {
        return (Vec::new(), equal + 1);
    }

    let (value, next) = read_value(input, equal + 1);
    if name == "srcset" {
        // "a.png 1x, b.png 2x": every candidate is the first token of a part
        let parts = value
            .split(',')
            .filter_map(|part| part.split_whitespace().next())
            .collect();
        return (parts, next);
    }
    (vec![value], next)
}

/// CSS `url(...)` and the Markdown `](...)` of a link or an image
fn paren_at(input: &str, open: usize) -> (Vec<&str>, usize) {
    let bytes = input.as_bytes();
    let mut before = open;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    let is_url = before >= 3
        && input
            .get(before - 3..before)
            .is_some_and(|word| word.eq_ignore_ascii_case("url"));
    let is_markdown = before >= 1 && bytes[before - 1] == b']';
    if !is_url && !is_markdown {
        return (Vec::new(), open + 1);
    }

    let (value, next) = read_parenthesised(input, open + 1);
    let value = if is_markdown {
        // "(url \"title\")" — only the first token is fetched
        value.split_whitespace().next().unwrap_or("")
    } else {
        value
    };
    (vec![value], next)
}

/// `@import "..."`. The `@import url(...)` form is already covered by `paren_at`.
fn import_at(input: &str, at: usize) -> (Vec<&str>, usize) {
    if !starts_with_ignore_case(&input[at..], "@import") {
        return (Vec::new(), at + 1);
    }
    // Only the quoted form is read here. `@import url(...)` keeps the cursor where it
    // is so `paren_at` sees it: consuming it would step over the URL without checking.
    let after = input[at + 7..].trim_start();
    if !after.starts_with('"') && !after.starts_with('\'') {
        return (Vec::new(), at + 1);
    }
    let (value, next) = read_value(input, at + 7);
    (vec![value], next)
}

/// Markdown reference definition: `[label]: https://host/path`
fn reference_at(input: &str, bracket: usize) -> (Vec<&str>, usize) {
    let bytes = input.as_bytes();
    if bracket + 1 >= bytes.len() || bytes[bracket + 1] != b':' {
        return (Vec::new(), bracket + 1);
    }
    let mut start = bracket + 2;
    while start < bytes.len() && (bytes[start] == b' ' || bytes[start] == b'\t') {
        start += 1;
    }
    let mut end = start;
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    (vec![&input[start..end]], end)
}

/// A bare `file:/...` anywhere in the document. Restricted to the one scheme whose
/// mere presence in prose is implausible: scanning for every exotic scheme would
/// reject a sentence containing "javascript:".
fn literal_file_at(input: &str, at: usize) -> (Vec<&str>, usize) {
    if !starts_with_ignore_case(&input[at..], "file:") {
        return (Vec::new(), at + 1);
    }
    let rest = &input[at + 5..];
    if !rest.starts_with('/') {
        return (Vec::new(), at + 1);
    }
    let end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '"' || c == '\'' || c == '>' || c == ')')
        .map(|offset| at + 5 + offset)
        .unwrap_or(input.len());
    (vec![&input[at..end]], end)
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b':'
}

fn starts_with_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

/// Read an attribute or `@import` value: quoted, or up to the next delimiter
fn read_value(input: &str, from: usize) -> (&str, usize) {
    let bytes = input.as_bytes();
    let mut start = from;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() {
        return ("", bytes.len());
    }
    let quote = bytes[start];
    if quote == b'"' || quote == b'\'' {
        let value_start = start + 1;
        let end = input[value_start..]
            .find(quote as char)
            .map(|offset| value_start + offset)
            .unwrap_or(input.len());
        return (&input[value_start..end], (end + 1).min(input.len()));
    }
    let end = input[start..]
        .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == ';')
        .map(|offset| start + offset)
        .unwrap_or(input.len());
    (&input[start..end], end)
}

/// Read up to the closing parenthesis, counting nested ones as Markdown does
fn read_parenthesised(input: &str, from: usize) -> (&str, usize) {
    let bytes = input.as_bytes();
    let mut depth = 1usize;
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let end = index.min(input.len());
    let value = input[from..end].trim().trim_matches(['"', '\''].as_slice());
    (value, (end + 1).min(input.len()))
}

/// Byte ranges holding Markdown code: what is inside is escaped by pandoc and never
/// fetched, so scanning it would only reject documentation that talks about URLs.
/// Raw HTML is never skipped, and the renderer-side fetcher covers what this misses.
fn code_regions(input: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    let mut fence: Option<(usize, u8, usize)> = None; // start, marker, length
    let mut offset = 0usize;

    for line in input.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied().unwrap_or(0);
        let run = if marker == b'`' || marker == b'~' {
            trimmed.bytes().take_while(|b| *b == marker).count()
        } else {
            0
        };

        match fence {
            Some((start, open_marker, open_len)) => {
                if run >= open_len && marker == open_marker && indent <= 3 {
                    regions.push((start, offset + line.len()));
                    fence = None;
                }
            }
            None => {
                if run >= 3 && indent <= 3 {
                    fence = Some((offset, marker, run));
                } else {
                    inline_code(line, offset, &mut regions);
                }
            }
        }
        offset += line.len();
    }

    // A fence that never closes is not protection, it is the shape an attacker controls:
    // pandoc only swallows the rest of the document when the fence is a real one, and
    // inside a raw HTML block it emits the markup that follows. Scan it.
    regions
}

/// Single-line `` `code` `` spans
fn inline_code(line: &str, base: usize, regions: &mut Vec<(usize, usize)>) {
    let bytes = line.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }
        let run = bytes[index..].iter().take_while(|b| **b == b'`').count();
        let content = index + run;
        let mut cursor = content;
        while cursor < bytes.len() {
            if bytes[cursor] == b'`' {
                let closing = bytes[cursor..].iter().take_while(|b| **b == b'`').count();
                if closing == run {
                    regions.push((base + index, base + cursor + closing));
                    break;
                }
                cursor += closing;
                continue;
            }
            cursor += 1;
        }
        index = cursor.max(content);
    }
}

// ------------ policy ------------

fn check_url(raw: &str, policy: &Policy) -> Result<(), String> {
    let decoded = decode_entities(raw);
    let compact = compact(&decoded);
    if compact.is_empty() {
        return Ok(());
    }

    // A percent-encoded scheme (`%66ile:`) is not dereferenced by any renderer, but a
    // URL that decodes into one is never legitimate either.
    let percent = percent_decode(&compact);
    check_form(&compact, policy)?;
    if percent != compact {
        check_form(&percent, policy)?;
    }
    Ok(())
}

fn check_form(url: &str, policy: &Policy) -> Result<(), String> {
    let Some(first) = url.as_bytes().first().copied() else {
        return Ok(());
    };
    if first == b'#' || first == b'?' {
        return Ok(());
    }
    // Protocol-relative: the renderer inherits the scheme, the host still applies.
    if let Some(rest) = url.strip_prefix("//") {
        return check_authority(rest, policy);
    }
    match scheme_of(url) {
        None => Ok(()), // relative path: resolved against the base URL, never remote
        Some(scheme) => {
            let lower = scheme.to_ascii_lowercase();
            if INERT_SCHEMES.contains(&lower.as_str()) {
                return Ok(());
            }
            if lower != "http" && lower != "https" {
                return Err(format!("scheme `{}` is not allowed", lower));
            }
            // `http:/host` and `http:host` are normalised by every renderer, so the
            // authority is whatever follows the slashes.
            check_authority(url[scheme.len() + 1..].trim_start_matches('/'), policy)
        }
    }
}

fn scheme_of(url: &str) -> Option<&str> {
    let bytes = url.as_bytes();
    if !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let end = bytes
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'+' || *b == b'-' || *b == b'.'))?;
    if bytes[end] != b':' {
        return None;
    }
    Some(&url[..end])
}

fn check_authority(authority: &str, policy: &Policy) -> Result<(), String> {
    let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    let mut host = &authority[..end];
    // `http://allowed.example@10.0.0.1/` points at 10.0.0.1
    if let Some(at) = host.rfind('@') {
        host = &host[at + 1..];
    }
    let host = strip_port(host).to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    if host.is_empty() {
        return Err("empty host".to_string());
    }

    if let Some(reason) = ip_verdict(host) {
        return Err(reason);
    }

    if DENIED_HOSTS.contains(&host) || DENIED_HOST_SUFFIXES.iter().any(|s| host.ends_with(s)) {
        return Err(format!("`{}` is an internal host", host));
    }

    if !policy.allowed_hosts.is_empty() {
        let allowed = policy
            .allowed_hosts
            .iter()
            .any(|entry| host == entry || host.ends_with(&format!(".{}", entry)));
        return if allowed {
            Ok(())
        } else {
            Err(format!("`{}` is not in PDF_ALLOWED_URL_HOSTS", host))
        };
    }

    if policy.strict_hosts {
        return Err("PDF_ALLOWED_URL_HOSTS is empty, no host is allowed".to_string());
    }

    // A public name always carries a dot; a single label is a container or a service
    // on the internal network.
    if !host.contains('.') {
        return Err(format!("`{}` is not a public host name", host));
    }

    Ok(())
}

fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return match rest.find(']') {
            Some(end) => &rest[..end],
            None => rest,
        };
    }
    match host.rfind(':') {
        // More than one colon and no bracket: an IPv6 literal written without them
        Some(index) if host.matches(':').count() == 1 => &host[..index],
        _ => host,
    }
}

/// `Some(reason)` when the host is an IP literal pointing somewhere it must not.
/// Notations `inet_aton` accepts (decimal, octal, hexadecimal) are covered because
/// `http://2130706433/` reaches 127.0.0.1 just as well as `http://127.0.0.1/`.
fn ip_verdict(host: &str) -> Option<String> {
    if let Some(address) = parse_ipv4(host) {
        if !ipv4_is_public(address) {
            return Some(format!("{} is not a public address", format_ipv4(address)));
        }
        return None;
    }
    if let Ok(address) = host.parse::<Ipv6Addr>() {
        if !ipv6_is_public(&address) {
            return Some(format!("{} is not a public address", address));
        }
    }
    None
}

fn parse_ipv4(host: &str) -> Option<u32> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut values = Vec::with_capacity(parts.len());
    for part in &parts {
        values.push(parse_ipv4_part(part)?);
    }
    // inet_aton: the last part fills every byte the earlier ones left out.
    let last = *values.last()?;
    let leading = &values[..values.len() - 1];
    let filled = 4 - leading.len();
    if last >= 1u64 << (8 * filled) {
        return None;
    }
    let mut address = 0u32;
    for (index, value) in leading.iter().enumerate() {
        if *value > 255 {
            return None;
        }
        address |= (*value as u32) << (8 * (3 - index));
    }
    Some(address | last as u32)
}

fn parse_ipv4_part(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (radix, digits) =
        if let Some(rest) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
            (16, rest)
        } else if part.len() > 1 && part.starts_with('0') {
            (8, &part[1..])
        } else {
            (10, part)
        };
    if digits.is_empty() {
        return None;
    }
    u64::from_str_radix(digits, radix)
        .ok()
        .filter(|v| *v <= u32::MAX as u64)
}

fn format_ipv4(address: u32) -> String {
    let octets = address.to_be_bytes();
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

fn ipv4_is_public(address: u32) -> bool {
    let [a, b, _, _] = address.to_be_bytes();
    let in_range = |network: u32, bits: u32| address >> (32 - bits) == network >> (32 - bits);
    !(a == 0                                        // 0.0.0.0/8
        || a == 10                                  // private
        || a == 127                                 // loopback
        || (a == 100 && (64..128).contains(&b))     // 100.64.0.0/10, CGNAT
        || (a == 169 && b == 254)                   // link-local, cloud metadata
        || (a == 172 && (16..32).contains(&b))      // private
        || (a == 192 && b == 168)                   // private
        || in_range(0xc000_0000, 24)                // 192.0.0.0/24
        || in_range(0xc000_0200, 24)                // 192.0.2.0/24, documentation
        || in_range(0xc612_0000, 15)                // 198.18.0.0/15, benchmarking
        || in_range(0xc633_6400, 24)                // 198.51.100.0/24
        || in_range(0xcb00_7100, 24)                // 203.0.113.0/24
        || a >= 224) // multicast and reserved
}

fn ipv6_is_public(address: &Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return ipv4_is_public(u32::from(mapped));
    }
    let segments = address.segments();
    if segments[0] == 0x2002 {
        // 6to4 carries the IPv4 address it tunnels to
        let embedded = ((segments[1] as u32) << 16) | segments[2] as u32;
        return ipv4_is_public(embedded);
    }
    if segments[0] == 0x0064 && segments[1] == 0xff9b {
        let embedded = ((segments[6] as u32) << 16) | segments[7] as u32;
        return ipv4_is_public(embedded);
    }
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        || segments[0] & 0xfe00 == 0xfc00      // unique local
        || segments[0] & 0xffc0 == 0xfe80) // link-local
}

// ------------ normalisation ------------

/// Drop what a renderer ignores inside a URL: ASCII whitespace and C0 controls, which
/// are how `fi\nle:///etc/passwd` gets past a naive prefix test.
fn compact(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
        .collect()
}

fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'&' {
            let end = index + 1;
            out.push_str(&input[index..end.min(input.len())]);
            index = end;
            continue;
        }
        let tail = &input[index + 1..];
        let end = match tail.find(';') {
            Some(offset) if offset <= 10 => offset,
            _ => {
                out.push('&');
                index += 1;
                continue;
            }
        };
        let entity = &tail[..end];
        match decode_entity(entity) {
            Some(decoded) => {
                out.push(decoded);
                index += end + 2;
            }
            None => {
                out.push('&');
                index += 1;
            }
        }
    }
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    if let Some(digits) = entity.strip_prefix('#') {
        let code = if let Some(hex) = digits
            .strip_prefix('x')
            .or_else(|| digits.strip_prefix('X'))
        {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            digits.parse::<u32>().ok()?
        };
        return char::from_u32(code);
    }
    match entity.to_ascii_lowercase().as_str() {
        "colon" => Some(':'),
        "sol" => Some('/'),
        "period" => Some('.'),
        "num" => Some('#'),
        "quest" => Some('?'),
        "commat" => Some('@'),
        "lpar" => Some('('),
        "rpar" => Some(')'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "tab" => Some('\t'),
        "newline" => Some('\n'),
        "nbsp" => Some(' '),
        _ => None,
    }
}

fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &input[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Keep the error message readable, and free of anything that could confuse a log line
fn shorten(url: &str) -> String {
    let cleaned: String = url
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cleaned.chars().count() <= MAX_REPORTED_URL {
        return cleaned;
    }
    let truncated: String = cleaned.chars().take(MAX_REPORTED_URL).collect();
    format!("{}…", truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_policy() -> Policy<'static> {
        Policy {
            allowed_hosts: &[],
            strict_hosts: false,
            markdown: true,
        }
    }

    fn allowlist(hosts: &[String]) -> Policy<'_> {
        Policy {
            allowed_hosts: hosts,
            strict_hosts: false,
            markdown: true,
        }
    }

    fn accepts(document: &str) {
        assert!(
            check_with(document, &open_policy()).is_ok(),
            "should have been accepted: {}",
            document
        );
    }

    fn rejects(document: &str) {
        assert!(
            check_with(document, &open_policy()).is_err(),
            "should have been rejected: {}",
            document
        );
    }

    #[test]
    fn accepts_what_the_service_renders_today() {
        accepts("# Titre\n\nUn paragraphe, une [ancre](#section) et du texte.");
        accepts(r#"<img src="static/blured.png" alt="CONTENU PREMIUM">"#);
        accepts("![logo](images/logo.png)");
        accepts(r##"<a href="#chapitre-2">voir plus bas</a>"##);
        accepts(r#"<img src="data:image/png;base64,iVBORw0KGgo=">"#);
        accepts("<style>body { background: url('static/fond.png') }</style>");
        accepts("<style>@import \"themes/print.css\";</style>");
        accepts(r#"<a href="mailto:contact@aismarttalk.tech">écrire</a>"#);
        accepts("![photo](https://cdn.aismarttalk.tech/a.png)");
        accepts("[ref]: https://example.com/page\n\nvoir [ref]");
        accepts(r#"<div data-src="static/x.png" data-ratio="16:9"></div>"#);
    }

    #[test]
    fn accepts_the_censor_block() {
        let censored = crate::censor::process(
            "avant\n\n{{CENSOR}}\n\naprès",
            &crate::censor::CensorConfig::default(),
        );
        accepts(&censored);
    }

    #[test]
    fn rejects_local_files() {
        rejects(r#"<img src="file:///etc/passwd">"#);
        rejects("![](file:///etc/passwd)");
        rejects("<style>@import url(file:///etc/shadow);</style>");
        rejects("<style>@import 'file:///etc/shadow';</style>");
        rejects("un file:///etc/passwd perdu dans le texte");
        rejects(r#"<img src="FILE:///etc/passwd">"#);
    }

    #[test]
    fn rejects_internal_targets() {
        rejects(r#"<img src="http://169.254.169.254/latest/meta-data/">"#);
        rejects(r#"<img src="http://metadata.google.internal/computeMetadata/v1/">"#);
        rejects(r#"<img src="http://127.0.0.1:8000/api/health">"#);
        rejects(r#"<img src="http://10.0.0.5/interne.png">"#);
        rejects(r#"<img src="http://192.168.1.1/">"#);
        rejects(r#"<img src="http://172.16.0.1/">"#);
        rejects(r#"<img src="http://localhost/x.png">"#);
        rejects(r#"<img src="http://redis/x.png">"#);
        rejects(r#"<img src="http://imprimante.local/x.png">"#);
    }

    #[test]
    fn rejects_exotic_schemes() {
        rejects(r#"<img src="gopher://example.com/1">"#);
        rejects(r#"<a href="javascript:alert(1)">x</a>"#);
        rejects(r#"<img src="jar:http://example.com/a.jar!/b.png">"#);
        rejects(r#"<img src="ftp://example.com/a.png">"#);
    }

    #[test]
    fn handles_evasions() {
        // casse mixte
        rejects(r#"<IMG SRC="HtTp://169.254.169.254/">"#);
        // espaces et retours ligne dans le schéma
        rejects("<img src=\"fi le:///etc/passwd\">");
        rejects("<img src=\"fi\nle:///etc/passwd\">");
        // encodage pourcent
        rejects(r#"<img src="%66ile:///etc/passwd">"#);
        // entités HTML, nommées et numériques
        rejects(r#"<img src="file&colon;///etc/passwd">"#);
        rejects(r#"<img src="file&#58;///etc/passwd">"#);
        rejects(r#"<img src="file&#x3a;///etc/passwd">"#);
        // URL protocole-relative
        rejects(r#"<img src="//169.254.169.254/latest/">"#);
        rejects(r#"<img src="//localhost/x.png">"#);
        // IPv6, avec et sans crochets
        rejects(r#"<img src="http://[::1]/x.png">"#);
        rejects(r#"<img src="http://[::ffff:127.0.0.1]/x.png">"#);
        rejects(r#"<img src="http://[fd00::1]/x.png">"#);
        rejects(r#"<img src="http://[fe80::1]/x.png">"#);
        // notations entières de l'IPv4
        rejects(r#"<img src="http://2130706433/x.png">"#);
        rejects(r#"<img src="http://0177.0.0.1/x.png">"#);
        rejects(r#"<img src="http://0x7f000001/x.png">"#);
        rejects(r#"<img src="http://127.1/x.png">"#);
        // identifiant devant l'hôte
        rejects(r#"<img src="http://cdn.example.com@169.254.169.254/x.png">"#);
        // srcset
        rejects(r#"<img srcset="a.png 1x, http://10.0.0.1/b.png 2x">"#);
    }

    #[test]
    fn enforces_the_host_allowlist() {
        let hosts = vec!["cdn.aismarttalk.tech".to_string()];
        let policy = allowlist(&hosts);
        assert!(check_with(
            r#"<img src="https://cdn.aismarttalk.tech/logo.png">"#,
            &policy
        )
        .is_ok());
        assert!(check_with(
            r#"<img src="https://images.cdn.aismarttalk.tech/logo.png">"#,
            &policy
        )
        .is_ok());
        assert!(check_with(r#"<img src="https://evil.example/logo.png">"#, &policy).is_err());
        // data: reste autorisé quelle que soit la liste
        assert!(check_with(r#"<img src="data:image/png;base64,AAAA">"#, &policy).is_ok());
    }

    #[test]
    fn strict_mode_refuses_every_host() {
        let policy = Policy {
            allowed_hosts: &[],
            strict_hosts: true,
            markdown: true,
        };
        assert!(check_with(r#"<img src="https://example.com/a.png">"#, &policy).is_err());
        assert!(check_with(r#"<img src="static/blured.png">"#, &policy).is_ok());
    }

    #[test]
    fn ignores_code_blocks() {
        // Le contenu d'un bloc de code est échappé par pandoc, jamais récupéré :
        // le refuser transformerait une doc technique en 400.
        accepts("Exemple :\n\n```\n<img src=\"file:///etc/passwd\">\n```\n");
        accepts("Un chemin `file:///etc/passwd` cité en ligne.");
        // ... mais ce qui suit le bloc reste analysé
        rejects("```\ncode\n```\n\n<img src=\"file:///etc/passwd\">");
    }

    /// Une clôture manquante ne doit pas mettre la fin du document hors de portée : c'est
    /// exactement la forme qu'un attaquant contrôle, et pandoc, lui, émet la balise.
    #[test]
    fn an_unterminated_fence_protects_nothing() {
        rejects("<div>\n```\n<img src=\"http://10.1.2.3/internal.png\">\n</div>\n");
        rejects("```\n<img src=\"file:///etc/passwd\">\n");
    }

    /// Le heuristique de fence est propre au Markdown : appliqué à du HTML, un backtick
    /// égaré masquerait la moitié du document au scan.
    #[test]
    fn markup_is_never_read_as_markdown() {
        let html = "<div>\n```\n<img src=\"http://169.254.169.254/latest/\">\n```\n</div>\n";
        let markup = Policy {
            markdown: false,
            ..open_policy()
        };
        assert!(check_with(html, &open_policy()).is_ok());
        assert!(check_with(html, &markup).is_err());
    }

    #[test]
    fn error_names_the_offending_url() {
        let error = check_with(
            r#"<img src="http://169.254.169.254/latest/">"#,
            &open_policy(),
        )
        .unwrap_err();
        let message = format!("{:?}", error);
        assert!(message.contains("169.254.169.254"), "{}", message);
    }

    #[test]
    fn stays_fast_on_a_ten_megabyte_document() {
        let paragraph = "Lorem ipsum dolor sit amet, [lien](https://example.com/a) et \
                         <img src=\"static/x.png\"> puis du texte.\n\n";
        let mut document = String::with_capacity(10 * 1024 * 1024 + paragraph.len());
        while document.len() < 10 * 1024 * 1024 {
            document.push_str(paragraph);
        }
        let started = std::time::Instant::now();
        assert!(check_with(&document, &open_policy()).is_ok());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "scan took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn ipv4_ranges() {
        assert!(ipv4_is_public(u32::from_be_bytes([8, 8, 8, 8])));
        assert!(ipv4_is_public(u32::from_be_bytes([1, 1, 1, 1])));
        assert!(!ipv4_is_public(u32::from_be_bytes([0, 0, 0, 0])));
        assert!(!ipv4_is_public(u32::from_be_bytes([100, 64, 0, 1])));
        assert!(!ipv4_is_public(u32::from_be_bytes([198, 18, 0, 1])));
        assert!(!ipv4_is_public(u32::from_be_bytes([255, 255, 255, 255])));
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7f00_0001));
        assert_eq!(parse_ipv4("2130706433"), Some(0x7f00_0001));
        assert_eq!(parse_ipv4("0x7f000001"), Some(0x7f00_0001));
        assert_eq!(parse_ipv4("0177.0.0.1"), Some(0x7f00_0001));
        assert_eq!(parse_ipv4("example.com"), None);
        assert_eq!(parse_ipv4("999.1.1.1"), None);
    }
}
