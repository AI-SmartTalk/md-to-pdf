//! Expansion of ```chart and ```mermaid fenced blocks.

use crate::charts;
use crate::config::config;
use crate::helpers::escape_html;
use crate::mermaid;
use crate::types::{AppError, BlockWarning, PdfOptions};
use std::time::{Duration, Instant};

/// A fence indented past this is an indented code block, not a fence (CommonMark)
const MAX_FENCE_INDENT: usize = 3;

/// How many blocks a single document may have rendered.
///
/// Every mermaid block is one outbound call to the diagram service, and nothing else in the
/// pipeline bounds their number: a body full of distinct one-line diagrams defeats the
/// fingerprint cache and would hold a render slot for hours while hammering the Studio.
/// Past the cap the blocks stay code, with one warning saying so.
const MAX_EXPANDED_BLOCKS: usize = 64;

/// Share of the render deadline the expansion phase may spend. The rest belongs to pandoc
/// and weasyprint, which still have a PDF to produce once the diagrams are in.
const BLOCK_BUDGET_DIVISOR: u32 = 3;

/// Page geometry the pagination hint is decided against: A4 content box, in CSS pixels.
/// A diagram that fits is asked to stay whole; a taller one has to be allowed to break,
/// or the page it does not fit on is simply left blank.
const CONTENT_WIDTH_PX: f32 = 640.0;
const CONTENT_HEIGHT_PX: f32 = 900.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Chart,
    Mermaid,
}

impl Kind {
    fn from_language(language: &str) -> Option<Kind> {
        match language.trim().to_ascii_lowercase().as_str() {
            "chart" => Some(Kind::Chart),
            "mermaid" => Some(Kind::Mermaid),
            _ => None,
        }
    }

    fn tag(&self) -> &'static str {
        match self {
            Kind::Chart => "chart",
            Kind::Mermaid => "mermaid",
        }
    }
}

/// What a block carries besides its code: `theme=dark title="Flux de commande"`
#[derive(Debug, Default)]
struct Attributes {
    theme: Option<String>,
    title: Option<String>,
}

/// The renderers, injected so the expansion can be exercised without a network
struct Renderers {
    chart: fn(&str) -> Result<String, AppError>,
    mermaid: fn(&str, Option<&str>) -> Result<String, AppError>,
    /// `options.charts == Some(false)`: no block is expanded at all
    enabled: bool,
    /// No Mermaid Studio configured: diagrams are left as code, and said once
    mermaid_configured: bool,
    /// Ceiling on the number of blocks handed to a renderer
    max_blocks: usize,
    /// Wall-clock the whole expansion phase may spend
    budget: Duration,
}

impl Default for Renderers {
    fn default() -> Renderers {
        Renderers {
            chart: charts::render_chart,
            mermaid: mermaid::render_mermaid,
            enabled: true,
            mermaid_configured: config().mermaid_api_url.is_some(),
            max_blocks: MAX_EXPANDED_BLOCKS,
            budget: config().render_deadline / BLOCK_BUDGET_DIVISOR,
        }
    }
}

/// Replace every chart/diagram block by its inline SVG, unless `options.charts` is false.
///
/// Never fails the request: a block that cannot be rendered is left untouched in the
/// document and the reason travels back as a `BlockWarning`.
pub fn expand_with_options(input: &str, options: &PdfOptions) -> (String, Vec<BlockWarning>) {
    let renderers = Renderers {
        enabled: options.charts != Some(false),
        ..Renderers::default()
    };
    expand_inner(input, &renderers)
}

fn expand_inner(input: &str, renderers: &Renderers) -> (String, Vec<BlockWarning>) {
    let mut session = Session {
        renderers,
        warnings: Vec::new(),
        mermaid_notice_sent: false,
        started: Instant::now(),
        expanded: 0,
        limit_notice_sent: false,
    };

    let (fenced, protected) = expand_fences(input, &mut session);
    let output = expand_html_blocks(&fenced, &protected, &mut session);

    (output, session.warnings)
}

struct Session<'a> {
    renderers: &'a Renderers,
    warnings: Vec<BlockWarning>,
    mermaid_notice_sent: bool,
    started: Instant,
    expanded: usize,
    limit_notice_sent: bool,
}

impl Session<'_> {
    /// Has this document used up its share of blocks or of time? Reported once: a document
    /// that went over is one fact, not one fact per block.
    fn over_budget(&mut self, kind: Kind, line: usize) -> bool {
        let reason = if self.expanded >= self.renderers.max_blocks {
            format!(
                "at most {} blocks are rendered per document, the rest are left as code",
                self.renderers.max_blocks
            )
        } else if self.started.elapsed() >= self.renderers.budget {
            format!(
                "the {}s block rendering budget is spent, the rest are left as code",
                self.renderers.budget.as_secs()
            )
        } else {
            return false;
        };

        if !self.limit_notice_sent {
            self.limit_notice_sent = true;
            self.warnings
                .push(BlockWarning::new(kind.tag(), reason, Some(line)));
        }
        true
    }

    /// The rendered figure, or `None` when the original block has to stay where it is
    fn figure(
        &mut self,
        kind: Kind,
        code: &str,
        attrs: &Attributes,
        line: usize,
    ) -> Option<String> {
        if !self.renderers.enabled {
            return None;
        }

        if kind == Kind::Mermaid && !self.renderers.mermaid_configured {
            // One notice per document: a service that is off is a deployment fact, not a
            // property of each diagram.
            if !self.mermaid_notice_sent {
                self.mermaid_notice_sent = true;
                self.warnings.push(BlockWarning::new(
                    kind.tag(),
                    "Mermaid rendering is disabled: MERMAID_API_URL is not configured".to_string(),
                    Some(line),
                ));
            }
            return None;
        }

        if self.over_budget(kind, line) {
            return None;
        }
        self.expanded += 1;

        let rendered = match kind {
            Kind::Chart => (self.renderers.chart)(code),
            Kind::Mermaid => (self.renderers.mermaid)(code, attrs.theme.as_deref()),
        };

        match rendered {
            Ok(svg) => Some(figure(&svg, attrs.title.as_deref())),
            Err(e) => {
                self.warnings
                    .push(BlockWarning::new(kind.tag(), reason(&e), Some(line)));
                None
            }
        }
    }
}

/// The one place an `AppError` becomes the text of a warning, so every warning the API
/// returns reads the same way whatever produced it.
pub fn reason(error: &AppError) -> String {
    match error {
        AppError::BadRequest(message)
        | AppError::NotFound(message)
        | AppError::TemplateError(message)
        | AppError::Timeout(message)
        | AppError::Unauthorized(message)
        | AppError::TooManyRequests(message) => message.clone(),
        AppError::ProcessFailed { message, stderr } if stderr.is_empty() => message.clone(),
        AppError::ProcessFailed { message, stderr } => format!("{}: {}", message, stderr),
        AppError::Upstream { service, details } => format!("{} unavailable: {}", service, details),
        AppError::Io(e) => e.to_string(),
    }
}

// ------------ Output ------------

/// The container the SVG travels in. Styles are inline because the document stylesheet is
/// built elsewhere and a caller may replace it entirely.
fn figure(svg: &str, title: Option<&str>) -> String {
    let pagination = if fits_on_a_page(svg) {
        "break-inside:avoid;page-break-inside:avoid;"
    } else {
        ""
    };

    let caption = title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| {
            format!(
                "<figcaption style=\"font-size:0.9em;margin-top:0.4em\">{}</figcaption>",
                escape_html(title)
            )
        })
        .unwrap_or_default();

    let figure = format!(
        "<figure class=\"md2pdf-figure\" style=\"margin:1em 0;text-align:center;{}\">{}{}</figure>",
        pagination, svg, caption
    );

    // pandoc reads a raw HTML block up to the first blank line: the figure has to be one line
    figure.replace(['\n', '\r'], " ")
}

/// How tall the diagram will be once scaled to the page, from its own `viewBox`
fn fits_on_a_page(svg: &str) -> bool {
    let Some((width, height)) = view_box(svg) else {
        return false;
    };

    let displayed = width.min(CONTENT_WIDTH_PX);
    displayed * height / width <= CONTENT_HEIGHT_PX
}

fn view_box(svg: &str) -> Option<(f32, f32)> {
    let at = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let value = &svg[at..at + svg[at..].find('"')?];

    let numbers: Vec<f32> = value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter_map(|part| part.parse::<f32>().ok())
        .collect();

    match numbers[..] {
        [_, _, width, height] if width > 0.0 && height > 0.0 => Some((width, height)),
        _ => None,
    }
}

// ------------ Fenced blocks (markdown) ------------

struct Fence {
    indent: usize,
    marker: u8,
    length: usize,
}

/// A fence opener, with the info string that follows it
fn fence_opener(line: &str) -> Option<(Fence, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > MAX_FENCE_INDENT {
        return None;
    }

    let rest = &line[indent..];
    let marker = match rest.as_bytes().first()? {
        b'`' => b'`',
        b'~' => b'~',
        _ => return None,
    };

    let length = rest.bytes().take_while(|byte| *byte == marker).count();
    if length < 3 {
        return None;
    }

    let info = rest[length..].trim();
    // A backtick fence cannot carry a backtick in its info string (CommonMark), otherwise
    // an inline code span would open a block
    if marker == b'`' && info.contains('`') {
        return None;
    }

    Some((
        Fence {
            indent,
            marker,
            length,
        },
        info,
    ))
}

fn closes(line: &str, fence: &Fence) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > MAX_FENCE_INDENT + fence.indent {
        return false;
    }

    let rest = &line[indent..];
    let length = rest
        .bytes()
        .take_while(|byte| *byte == fence.marker)
        .count();

    length >= fence.length && rest[length..].trim().is_empty()
}

/// `chart theme=dark title="Ventes"` and the pandoc form `{.chart title="Ventes"}`
fn parse_info(info: &str) -> Option<(Kind, Attributes)> {
    let braced = info.starts_with('{') && info.ends_with('}');
    let inner = if braced {
        info[1..info.len() - 1].trim()
    } else {
        info
    };

    let mut kind = None;
    let mut attrs = Attributes::default();

    for (index, token) in tokens(inner).into_iter().enumerate() {
        if let Some((key, value)) = token.split_once('=') {
            let value = value.trim_matches(['"', '\'']).to_string();
            match key.trim().to_ascii_lowercase().as_str() {
                "theme" => attrs.theme = Some(value),
                "title" | "caption" => attrs.title = Some(value),
                _ => {}
            }
            continue;
        }

        // The language is the first bare token, or any `.class` in the pandoc form
        if let Some(class) = token.strip_prefix('.') {
            kind = kind.or(Kind::from_language(class));
        } else if index == 0 && !braced {
            // A fence in another language stops here: its attributes are none of our business
            kind = Some(Kind::from_language(&token)?);
        }
    }

    kind.map(|kind| (kind, attrs))
}

/// Split on whitespace, but not inside a quoted value
fn tokens(info: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for c in info.chars() {
        match quote {
            Some(open) => {
                current.push(c);
                if c == open {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.push(c);
            }
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Walks every fence, expands the ones that are ours, and reports the byte ranges of the
/// output that must stay untouched afterwards — a fenced block showing this very syntax
/// must survive the HTML pass below.
fn expand_fences(input: &str, session: &mut Session) -> (String, Vec<(usize, usize)>) {
    let lines: Vec<&str> = input.split_inclusive('\n').collect();
    let mut out = String::with_capacity(input.len());
    let mut protected = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let content = strip_eol(lines[i]);
        let Some((fence, info)) = fence_opener(content) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };

        // An unterminated fence runs to the end of the document (CommonMark)
        let mut close = lines.len();
        for (offset, line) in lines.iter().enumerate().skip(i + 1) {
            if closes(strip_eol(line), &fence) {
                close = offset;
                break;
            }
        }

        let expanded = parse_info(info).and_then(|(kind, attrs)| {
            let code = dedent(&lines[i + 1..close.min(lines.len())], fence.indent);
            session.figure(kind, &code, &attrs, i + 1)
        });

        let start = out.len();
        match expanded {
            Some(figure) => {
                // A raw HTML block needs a blank line on each side or pandoc folds it into
                // the surrounding paragraph
                separate(&mut out);
                out.push_str(&" ".repeat(fence.indent));
                out.push_str(&figure);
                out.push('\n');
                if lines.get(close + 1).is_some_and(|next| !is_blank(next)) {
                    out.push('\n');
                }
            }
            None => {
                for line in &lines[i..(close + 1).min(lines.len())] {
                    out.push_str(line);
                }
            }
        }
        protected.push((start, out.len()));

        i = close + 1;
    }

    (out, protected)
}

fn strip_eol(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

fn is_blank(line: &str) -> bool {
    strip_eol(line).trim().is_empty()
}

/// Give the block back its own left margin: a fence inside a list item is indented, and so
/// is everything it contains.
fn dedent(lines: &[&str], indent: usize) -> String {
    let mut out = String::new();
    for line in lines {
        let content = strip_eol(line);
        let strip = content
            .bytes()
            .take(indent)
            .take_while(|byte| *byte == b' ')
            .count();
        out.push_str(&content[strip..]);
        out.push('\n');
    }
    out
}

fn separate(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

// ------------ Highlighted blocks (HTML) ------------

/// `<pre><code class="language-mermaid">…</code></pre>`, the shape a markdown renderer
/// leaves behind when the caller sends HTML rather than markdown.
fn expand_html_blocks(input: &str, protected: &[(usize, usize)], session: &mut Session) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < input.len() {
        let Some(offset) = input[i..].find("<pre") else {
            out.push_str(&input[i..]);
            break;
        };
        let at = i + offset;

        if let Some(end) = protected_end(protected, at) {
            out.push_str(&input[i..end.min(input.len())]);
            i = end.min(input.len());
            continue;
        }

        out.push_str(&input[i..at]);

        match parse_pre_block(input, at) {
            None => {
                out.push_str("<pre");
                i = at + "<pre".len();
            }
            Some(block) => {
                let line = input[..at].matches('\n').count() + 1;
                let code = unescape(block.code);
                match session.figure(block.kind, &code, &block.attrs, line) {
                    Some(figure) => out.push_str(&figure),
                    None => out.push_str(&input[at..block.end]),
                }
                i = block.end;
            }
        }
    }

    out
}

fn protected_end(protected: &[(usize, usize)], at: usize) -> Option<usize> {
    protected
        .iter()
        .find(|(start, end)| at >= *start && at < *end)
        .map(|(_, end)| *end)
}

struct PreBlock<'a> {
    kind: Kind,
    attrs: Attributes,
    code: &'a str,
    end: usize,
}

fn parse_pre_block(input: &str, at: usize) -> Option<PreBlock<'_>> {
    let after_pre = at + input[at..].find('>')? + 1;
    if !input[after_pre..].trim_start().starts_with("<code") {
        return None;
    }

    let code_tag = at + input[at..].find("<code")?;
    let after_code = code_tag + input[code_tag..].find('>')? + 1;
    let (kind, attrs) = code_attributes(&input[code_tag..after_code])?;

    let code_end = after_code + input[after_code..].find("</code>")?;
    let after_close = code_end + "</code>".len();
    if !input[after_close..].trim_start().starts_with("</pre>") {
        return None;
    }
    let end = after_close + input[after_close..].find("</pre>")? + "</pre>".len();

    Some(PreBlock {
        kind,
        attrs,
        code: &input[after_code..code_end],
        end,
    })
}

/// `class="language-chart"`, `class="chart"`, plus the optional `data-theme` / `data-title`
fn code_attributes(tag: &str) -> Option<(Kind, Attributes)> {
    let mut kind = None;
    let mut attrs = Attributes::default();

    for (name, value) in html_attributes(tag) {
        match name.as_str() {
            "class" => {
                for class in value.split_whitespace() {
                    let language = class
                        .trim_start_matches("language-")
                        .trim_start_matches("lang-");
                    kind = kind.or(Kind::from_language(language));
                }
            }
            "data-theme" | "theme" => attrs.theme = Some(value),
            "data-title" | "title" => attrs.title = Some(value),
            _ => {}
        }
    }

    kind.map(|kind| (kind, attrs))
}

fn html_attributes(tag: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let bytes = tag.as_bytes();
    let mut i = tag.find(char::is_whitespace).unwrap_or(tag.len());

    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }

        let name_start = i;
        while i < bytes.len() && !(bytes[i] as char).is_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if i == name_start {
            break;
        }
        let name = tag[name_start..i].to_ascii_lowercase();

        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = unescape(&tag[value_start..i.min(bytes.len())]);
                i += 1;
            } else {
                let value_start = i;
                while i < bytes.len() && !(bytes[i] as char).is_whitespace() {
                    i += 1;
                }
                value = unescape(&tag[value_start..i]);
            }
        }

        attrs.push((name, value));
    }

    attrs
}

/// The code inside `<pre><code>` is escaped markup: the renderer needs the source back
fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = "<svg viewBox=\"0 0 100 50\"><rect/></svg>";

    fn ok_chart(_spec: &str) -> Result<String, AppError> {
        Ok(SVG.to_string())
    }

    fn ok_mermaid(_code: &str, theme: Option<&str>) -> Result<String, AppError> {
        Ok(format!(
            "<svg viewBox=\"0 0 100 50\" data-theme=\"{}\"><rect/></svg>",
            theme.unwrap_or("none")
        ))
    }

    fn failing_chart(_spec: &str) -> Result<String, AppError> {
        Err(AppError::BadRequest("charts: invalid JSON".to_string()))
    }

    fn failing_mermaid(_code: &str, _theme: Option<&str>) -> Result<String, AppError> {
        Err(AppError::Upstream {
            service: "mermaid-studio".to_string(),
            details: "connection refused".to_string(),
        })
    }

    fn working() -> Renderers {
        Renderers {
            chart: ok_chart,
            mermaid: ok_mermaid,
            enabled: true,
            mermaid_configured: true,
            max_blocks: MAX_EXPANDED_BLOCKS,
            budget: Duration::from_secs(60),
        }
    }

    fn broken() -> Renderers {
        Renderers {
            chart: failing_chart,
            mermaid: failing_mermaid,
            enabled: true,
            mermaid_configured: true,
            max_blocks: MAX_EXPANDED_BLOCKS,
            budget: Duration::from_secs(60),
        }
    }

    fn run(input: &str, renderers: Renderers) -> (String, Vec<BlockWarning>) {
        expand_inner(input, &renderers)
    }

    /// A document of distinct diagrams is what turns one request into thousands of
    /// outbound calls; the cap is what keeps a render slot from being held for hours.
    #[test]
    fn stops_expanding_past_the_block_cap() {
        let source: String = (0..MAX_EXPANDED_BLOCKS + 10)
            .map(|n| format!("```mermaid\ngraph TD;A{n}-->B\n```\n\n"))
            .collect();

        let (out, warnings) = run(&source, working());

        assert_eq!(out.matches("<figure").count(), MAX_EXPANDED_BLOCKS);
        assert_eq!(out.matches("```mermaid").count(), 10);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("at most"));
    }

    #[test]
    fn stops_expanding_once_the_time_budget_is_spent() {
        let slow = Renderers {
            budget: Duration::ZERO,
            ..working()
        };
        let source = "```mermaid\ngraph TD;A-->B\n```\n\n```chart\n{}\n```\n";

        let (out, warnings) = run(source, slow);

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("budget"));
    }

    #[test]
    fn expands_a_chart_and_a_diagram() {
        let (out, warnings) = run(
            "# Titre\n\n```chart\n{\"type\":\"bar\"}\n```\n\ntexte\n\n```mermaid\nflowchart TD\n A-->B\n```\n",
            working(),
        );

        assert_eq!(warnings.len(), 0);
        assert_eq!(out.matches("<figure").count(), 2);
        assert!(!out.contains("```"));
        assert!(out.contains("# Titre"));
        assert!(out.contains("texte"));
    }

    #[test]
    fn leaves_the_block_in_place_when_the_render_fails() {
        let source = "```chart\n{\"type\":\"bar\"}\n```\n";
        let (out, warnings) = run(source, broken());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, "chart");
        assert_eq!(warnings[0].line, Some(1));
        assert!(warnings[0].message.contains("invalid JSON"));
    }

    #[test]
    fn reports_an_unreachable_diagram_service_without_losing_the_block() {
        let source = "avant\n\n```mermaid\nflowchart TD\n A-->B\n```\n\napres\n";
        let (out, warnings) = run(source, broken());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].line, Some(3));
        assert!(warnings[0].message.contains("mermaid-studio unavailable"));
    }

    #[test]
    fn a_longer_fence_protects_the_block_it_contains() {
        let source = "````markdown\n```chart\n{\"type\":\"bar\"}\n```\n````\n";
        let (out, warnings) = run(source, working());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn handles_tilde_fences_and_longer_backtick_fences() {
        let (out, _) = run("~~~chart\n{}\n~~~\n", working());
        assert!(out.contains("<figure"));

        let (out, _) = run("`````chart\n{}\n`````\n", working());
        assert!(out.contains("<figure"));
    }

    #[test]
    fn reads_the_attributes_after_the_language() {
        let (out, _) = run(
            "```mermaid theme=dark title=\"Flux de commande\"\nflowchart TD\n A-->B\n```\n",
            working(),
        );

        assert!(out.contains("data-theme=\"dark\""));
        assert!(out.contains("<figcaption"));
        assert!(out.contains("Flux de commande"));
    }

    #[test]
    fn reads_the_pandoc_attribute_form() {
        let (out, _) = run(
            "```{.mermaid theme=forest}\nflowchart TD\n A-->B\n```\n",
            working(),
        );
        assert!(out.contains("data-theme=\"forest\""));
    }

    #[test]
    fn keeps_the_indentation_of_a_nested_block() {
        let source = "- item\n\n  ```chart\n  {\"type\":\"bar\"}\n  ```\n";
        let (out, warnings) = run(source, working());

        assert_eq!(warnings.len(), 0);
        assert!(out.contains("  <figure"));
        assert!(out.contains("- item"));
    }

    #[test]
    fn an_indented_code_block_is_not_a_fence() {
        let source = "texte\n\n    ```chart\n    {}\n    ```\n";
        let (out, warnings) = run(source, working());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn expands_a_highlighted_html_block() {
        let source = "<h1>T</h1>\n<pre><code class=\"language-mermaid\">flowchart TD\n  A--&gt;B</code></pre>\n<p>fin</p>";
        let (out, warnings) = run(source, working());

        assert_eq!(warnings.len(), 0);
        assert!(out.contains("<figure"));
        assert!(!out.contains("<pre>"));
        assert!(out.contains("<p>fin</p>"));
    }

    #[test]
    fn leaves_the_html_block_alone_when_the_render_fails() {
        let source = "<pre><code class=\"language-chart\">{}</code></pre>";
        let (out, warnings) = run(source, broken());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn does_not_expand_an_html_example_shown_inside_a_fence() {
        let source = "```html\n<pre><code class=\"language-chart\">{}</code></pre>\n```\n\ntexte\n";
        let (out, warnings) = run(source, working());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn ignores_a_pre_block_in_another_language() {
        let source = "<pre><code class=\"language-python\">print(1)</code></pre>";
        let (out, warnings) = run(source, working());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn charts_false_disables_every_expansion_without_a_warning() {
        let source = "```chart\n{}\n```\n\n```mermaid\nflowchart TD\n A-->B\n```\n";
        let renderers = Renderers {
            enabled: false,
            ..working()
        };
        let (out, warnings) = run(source, renderers);

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn warns_once_per_document_when_no_diagram_service_is_configured() {
        let source = "```mermaid\nA-->B\n```\n\n```mermaid\nC-->D\n```\n\n```chart\n{}\n```\n";
        let renderers = Renderers {
            mermaid_configured: false,
            ..working()
        };
        let (out, warnings) = run(source, renderers);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, "mermaid");
        assert!(out.contains("```mermaid"));
        // The chart in the same document is still expanded
        assert_eq!(out.matches("<figure").count(), 1);
    }

    #[test]
    fn asks_a_short_diagram_to_stay_whole_and_lets_a_tall_one_break() {
        let short = figure("<svg viewBox=\"0 0 100 50\"></svg>", None);
        assert!(short.contains("break-inside:avoid"));

        let tall = figure("<svg viewBox=\"0 0 100 2000\"></svg>", None);
        assert!(!tall.contains("break-inside:avoid"));
    }

    #[test]
    fn the_figure_holds_on_a_single_line() {
        let (out, _) = run("```chart\n{}\n```\n", working());
        let line = out
            .lines()
            .find(|line| line.contains("<figure"))
            .expect("the figure should be there");
        assert!(line.contains("</figure>"));
    }

    #[test]
    fn escapes_the_caption() {
        let (out, _) = run(
            "```chart title=\"<script>alert(1)</script>\"\n{}\n```\n",
            working(),
        );
        assert!(!out.contains("<script>"));
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn an_unterminated_fence_does_not_swallow_the_document() {
        let source = "```chart\n{}\n";
        let (out, warnings) = run(source, working());

        assert!(out.contains("<figure"));
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn a_document_without_any_block_is_untouched() {
        let source = "# Titre\n\nUn paragraphe avec `du code` en ligne.\n";
        let (out, warnings) = run(source, working());

        assert_eq!(out, source);
        assert_eq!(warnings.len(), 0);
    }
}
