//! CENSOR tag expansion.
//!
//! The promise of this module is that censored content is *removed* from the document, never
//! merely covered: whatever the syntax, the hidden text never reaches pandoc, so it cannot be
//! selected, searched or extracted from the PDF.
//!
//! Accepted syntaxes:
//!
//! | Tag                              | Effect                                                     |
//! |----------------------------------|------------------------------------------------------------|
//! | `{{CENSOR}}` / `<CENSOR>`        | v1 point replacement: the historical blurred PNG block     |
//! | `{{CENSOR:premium}}`             | same, with the label of that level                         |
//! | `{{CENSOR:start}}` … `:end`      | removes the region, block sized after the removed volume   |
//! | `{{CENSOR:teaser=25}}` … `:end`  | keeps the first 25 words, removes the rest of the region    |
//! | `{{CENSOR:inline}}` … `:end`     | removes a run of text without breaking the paragraph       |
//!
//! Modifiers combine with a comma (`{{CENSOR:start,confidentiel}}`); any unknown modifier is
//! read as a level name, so a typo yields a differently labelled block, never visible text.
//!
//! Malformed input always errs towards censoring, because a syntax mistake must not be a way
//! to reveal what was meant to be hidden:
//!
//! * an unclosed region is censored up to the end of the document;
//! * nested regions are depth-counted, so an inner `:end` does not reopen the outer region;
//! * a level tag that a `:end` follows opens a region — that is what a misspelled `start`
//!   looks like, and reading it as a point block would leave the region visible;
//! * a `:end` with no open region is dropped and the surrounding text stays visible — there
//!   is nothing to hide in that case.
//!
//! The v1 spellings are never subject to that lookahead: they render today exactly what they
//! rendered before this module existed, whatever surrounds them.
//!
//! Labels cascade: the level named on the tag wins, otherwise `options.censor_label`,
//! otherwise the historical wording. A label coming from the request is untrusted text.

use crate::helpers::escape_html;
use crate::types::PdfOptions;

/// The blurred premium block that replaces every CENSOR tag
const CENSOR_REPLACEMENT: &str = "<div style=\"width:100% !important; max-width:100% !important; margin-top:0 !important; margin-bottom:0 !important; margin-left:0 !important; margin-right:0 !important; padding:0 !important; overflow:hidden !important; position:relative !important; left:0 !important; right:auto !important; box-sizing:border-box !important; text-align:left !important;\"><img src=\"static/blured.png\" alt=\"{label}\" style=\"width:100% !important; height:auto !important; display:block !important; margin:0 !important; padding:0 !important; float:none !important;\"></div>";

/// Historical wording, kept as the default so an untouched request renders as before
const DEFAULT_LABEL: &str = "CONTENU PREMIUM - Achetez le rapport complet";

/// A label reaches an HTML attribute on a single line: anything longer or multi-line would
/// break the raw-HTML block pandoc has to pass through untouched.
const MAX_LABEL_CHARS: usize = 160;

/// Guard against matching a `{{ … }}` that merely happens to start like a tag
const MAX_ARGS_CHARS: usize = 200;

/// Vertical space one source line of removed content stands for
const LINE_MM: f32 = 5.6;
/// Even one removed word must read as a hole in the page
const MIN_BLOCK_MM: u32 = 22;
/// Beyond a full page the block stops growing: taller only wastes paper
const MAX_BLOCK_MM: u32 = 235;

const MIN_INLINE_EM: f32 = 2.0;
const MAX_INLINE_EM: f32 = 20.0;

/// Padlock, drawn inline so the block needs no asset and stays sharp at any zoom
const LOCK_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"12\" height=\"12\" fill=\"none\" stroke=\"#2c3e50\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><rect x=\"4.5\" y=\"10.5\" width=\"15\" height=\"10\" rx=\"2\"></rect><path d=\"M8.5 10.5V7.5a3.5 3.5 0 0 1 7 0v3\"></path></svg>";

/// Frosted stripes suggesting the lines of text that were taken out
const GLASS_BACKGROUND: &str = "background-color:#f4f7fa;background-image:linear-gradient(rgba(255,255,255,0.62),rgba(255,255,255,0.88)),repeating-linear-gradient(180deg,#dbe2ea 0,#dbe2ea 6px,#f4f7fa 6px,#f4f7fa 14px);";

/// Everything that can change how a CENSOR tag renders. Opt-in: the default reproduces
/// the block this service has always emitted.
#[derive(Debug, Clone, Default)]
pub struct CensorConfig {
    pub label: Option<String>,
}

impl CensorConfig {
    pub fn from_options(options: &PdfOptions) -> CensorConfig {
        CensorConfig {
            label: options.censor_label.clone(),
        }
    }
}

/// How a region hides its content
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Block level, height proportional to the removed volume
    Block,
    /// Runs inside a paragraph
    Inline,
    /// The first `n` words survive, the rest becomes a block
    Teaser(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// v1 point replacement
    Standalone,
    Open(Mode),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
struct Tag {
    start: usize,
    end: usize,
    kind: Kind,
    /// Level named on the tag, already turned into a label
    label: Option<String>,
}

/// Replace every accepted spelling of the CENSOR tag by the blurred block.
/// Applies to markdown as well as raw HTML input.
pub fn process(input: &str, cfg: &CensorConfig) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;

    while let Some(tag) = find_tag(input, pos) {
        out.push_str(&input[pos..tag.start]);
        match tag.kind {
            // A level tag closed further down is a region whose opener was misspelled: the
            // v1 spellings carry no level, so they never take this path.
            Kind::Standalone if tag.label.is_some() && closes_before_it_opens(input, tag.end) => {
                let (content, after) = collect_region(input, tag.end);
                let label = label_for(tag.label.as_deref(), cfg);
                out.push_str(&render_glass_block(&label, estimate_height_mm(content)));
                pos = after;
            }
            // The historical shape stays byte for byte what it always was
            Kind::Standalone => {
                out.push_str(&render_v1_block(&label_for(tag.label.as_deref(), cfg)));
                pos = tag.end;
            }
            // Nothing is open, so nothing is at risk: drop the stray tag only
            Kind::Close => pos = tag.end,
            Kind::Open(mode) => {
                let (content, after) = collect_region(input, tag.end);
                let label = label_for(tag.label.as_deref(), cfg);
                match mode {
                    Mode::Block => {
                        out.push_str(&render_glass_block(&label, estimate_height_mm(content)))
                    }
                    Mode::Inline => {
                        out.push_str(&render_glass_inline(&label, estimate_width_em(content)))
                    }
                    Mode::Teaser(words) => {
                        let (visible, hidden) = split_after_words(content, words);
                        out.push_str(visible);
                        // Nothing left to hide: emitting a block would announce a hole that
                        // does not exist
                        if !hidden.trim().is_empty() {
                            out.push_str(&render_glass_block(&label, estimate_height_mm(hidden)));
                        }
                    }
                }
                pos = after;
            }
        }
    }

    out.push_str(&input[pos..]);
    out
}

/// Content of the region opened at `from`, and the offset to resume at. Openers inside the
/// region raise the depth so that an inner `:end` closes the inner region, not this one; an
/// unterminated region swallows the rest of the document.
fn collect_region(input: &str, from: usize) -> (&str, usize) {
    let mut depth = 1usize;
    let mut cursor = from;

    while let Some(tag) = find_tag(input, cursor) {
        match tag.kind {
            Kind::Open(_) => depth += 1,
            Kind::Close => {
                depth -= 1;
                if depth == 0 {
                    return (&input[from..tag.start], tag.end);
                }
            }
            Kind::Standalone => {}
        }
        cursor = tag.end;
    }

    (&input[from..], input.len())
}

/// Whether the next region tag after `from` is a `:end`, i.e. whether an unmatched close is
/// waiting for an opener.
fn closes_before_it_opens(input: &str, from: usize) -> bool {
    let mut cursor = from;
    while let Some(tag) = find_tag(input, cursor) {
        match tag.kind {
            Kind::Close => return true,
            Kind::Open(_) => return false,
            Kind::Standalone => {}
        }
        cursor = tag.end;
    }
    false
}

fn find_tag(input: &str, from: usize) -> Option<Tag> {
    let bytes = input.as_bytes();
    let mut i = from;

    while i < bytes.len() {
        // `{` and `<` are single-byte in UTF-8, so slicing at their index is always a valid
        // char boundary; every other byte is only stepped over.
        match bytes[i] {
            b'{' if input[i..].starts_with("{{") => {
                if let Some(tag) = parse_braced(input, i) {
                    return Some(tag);
                }
                i += 2;
            }
            b'<' if input[i..].starts_with("<CENSOR>") => {
                return Some(Tag {
                    start: i,
                    end: i + "<CENSOR>".len(),
                    kind: Kind::Standalone,
                    label: None,
                });
            }
            _ => i += 1,
        }
    }

    None
}

/// Parse `{{ CENSOR[:args] }}` at `start`. Returns `None` for anything else, including a
/// template expression that only looks like one.
fn parse_braced(input: &str, start: usize) -> Option<Tag> {
    let rest = &input[start + 2..];
    let mut j = skip_spaces(rest, 0);

    if !rest[j..].starts_with("CENSOR") {
        return None;
    }
    j = skip_spaces(rest, j + "CENSOR".len());

    if rest[j..].starts_with("}}") {
        return Some(Tag {
            start,
            end: start + 2 + j + 2,
            kind: Kind::Standalone,
            label: None,
        });
    }

    if !rest[j..].starts_with(':') {
        return None;
    }
    let args_start = j + 1;
    let tail = &rest[args_start..];
    // A newline or a brace means the tag was never closed: refuse the match rather than
    // consuming half the document as arguments.
    let close = tail.find("}}")?;
    let args = &tail[..close];
    if args.len() > MAX_ARGS_CHARS || args.contains('\n') || args.contains('{') {
        return None;
    }

    let (kind, label) = parse_args(args);
    Some(Tag {
        start,
        end: start + 2 + args_start + close + 2,
        kind,
        label,
    })
}

fn skip_spaces(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

/// Comma-separated modifiers. An opener always outranks `end`, and an unknown word is a
/// level name: both rules keep a malformed tag on the censoring side.
fn parse_args(args: &str) -> (Kind, Option<String>) {
    let mut mode: Option<Mode> = None;
    let mut close = false;
    let mut label: Option<String> = None;

    for raw in args.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let lower = token.to_lowercase();
        match lower.as_str() {
            "start" | "region" => mode = Some(mode.unwrap_or(Mode::Block)),
            "inline" => mode = Some(Mode::Inline),
            "end" | "stop" => close = true,
            _ => {
                if let Some(count) = lower.strip_prefix("teaser") {
                    let count = count.trim_start_matches(['=', ':', ' ']);
                    // An unreadable count censors the whole region instead of guessing
                    mode = Some(Mode::Teaser(count.parse::<usize>().unwrap_or(0).min(2000)));
                } else if label.is_none() {
                    label = Some(level_label(&lower));
                }
            }
        }
    }

    match mode {
        Some(mode) => (Kind::Open(mode), label),
        None if close => (Kind::Close, label),
        None => (Kind::Standalone, label),
    }
}

/// Wording attached to a level name. The known levels read as French product wording; any
/// other name is echoed so a client can invent its own level without touching the service.
fn level_label(level: &str) -> String {
    match level {
        "premium" => DEFAULT_LABEL.to_string(),
        "confidentiel" | "confidential" => "CONTENU CONFIDENTIEL".to_string(),
        "interne" | "internal" => "CONTENU INTERNE".to_string(),
        "secret" => "CONTENU SECRET".to_string(),
        other => format!("CONTENU {}", other.to_uppercase()),
    }
}

fn label_for(tag_label: Option<&str>, cfg: &CensorConfig) -> String {
    match tag_label {
        Some(label) => label.to_string(),
        None => cfg.label.as_deref().unwrap_or(DEFAULT_LABEL).to_string(),
    }
}

/// Fold the label to one line and escape it: it can come straight from the request body.
fn safe_label(label: &str) -> String {
    let folded = label
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect::<String>();
    escape_html(&folded)
}

fn render_v1_block(label: &str) -> String {
    CENSOR_REPLACEMENT.replace("{label}", &safe_label(label))
}

/// One line of HTML on purpose: pandoc only passes a raw HTML block through untouched when
/// no blank line splits it, and the caller's own newlines are what makes it a block.
///
/// A `span` carrying `display:block` rather than a `div`, because pandoc drops a block-level
/// tag written inside a table cell — that is exactly where the v1 `div` disappears.
fn render_glass_block(label: &str, height_mm: u32) -> String {
    let label = safe_label(label);
    format!(
        "<span class=\"censor-block\" role=\"img\" aria-label=\"{label}\" style=\"box-sizing:border-box;display:block;width:100%;height:{height}mm;line-height:{height}mm;margin:1em 0;padding:0 8px;overflow:hidden;text-align:center;border:1px solid #dfe4ea;border-radius:8px;{glass}\"><span class=\"censor-label\" style=\"display:inline-block;vertical-align:middle;line-height:1.25;max-width:92%;padding:5px 13px;border:1px solid #dfe4ea;border-radius:999px;background:#ffffff;color:#2c3e50;font-size:10.5pt;font-weight:bold;letter-spacing:0.03em;\">{lock} {label}</span></span>",
        label = label,
        height = height_mm,
        glass = GLASS_BACKGROUND,
        lock = LOCK_SVG,
    )
}

fn render_glass_inline(label: &str, width_em: f32) -> String {
    format!(
        "<span class=\"censor-inline\" role=\"img\" aria-label=\"{label}\" style=\"display:inline-block;vertical-align:-0.15em;width:{width:.1}em;height:1.15em;line-height:1.15em;overflow:hidden;text-align:center;border:1px solid #dfe4ea;border-radius:4px;{glass}\">{lock}</span>",
        label = safe_label(label),
        width = width_em,
        glass = GLASS_BACKGROUND,
        lock = LOCK_SVG,
    )
}

/// Height of the hole left by the removed content. Deliberately an approximation, but a
/// monotone one: adding content never shortens the block.
fn estimate_height_mm(content: &str) -> u32 {
    let mut units = 0.0f32;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() {
            // Paragraph gap
            units += 0.35;
            continue;
        }
        // A long source line wraps into several printed ones
        let wrapped = (line.chars().count() as f32 / 95.0).ceil().max(1.0);
        let weight = if line.starts_with('#') {
            1.9
        } else if line.starts_with('|') || line.starts_with("<tr") || line.starts_with("<td") {
            1.15
        } else if line.starts_with("```") {
            0.9
        } else {
            1.0
        };
        units += weight * wrapped;
    }

    let mm = (units * LINE_MM).round().max(0.0) as u32;
    mm.clamp(MIN_BLOCK_MM, MAX_BLOCK_MM)
}

/// Width of an inline hole, from the number of characters taken out
fn estimate_width_em(content: &str) -> f32 {
    let chars = content.trim().chars().count() as f32;
    (chars * 0.5).clamp(MIN_INLINE_EM, MAX_INLINE_EM)
}

/// Split after the `n`-th word. The trailing whitespace of the visible part is kept so the
/// document's own line structure survives the cut.
fn split_after_words(content: &str, words: usize) -> (&str, &str) {
    if words == 0 {
        return ("", content);
    }

    let mut seen = 0usize;
    let mut in_word = false;

    for (index, c) in content.char_indices() {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            in_word = true;
            seen += 1;
            if seen > words {
                return content.split_at(index);
            }
        }
    }

    (content, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The v1 implementation, verbatim, as the reference for backward compatibility
    fn v1_process(input: &str, cfg: &CensorConfig) -> String {
        let label = cfg.label.as_deref().unwrap_or(DEFAULT_LABEL);
        let replacement = CENSOR_REPLACEMENT.replace("{label}", &escape_html(label));

        let mut result = input.to_string();
        result = result.replace("{{CENSOR}}", &replacement);
        result = result.replace("<CENSOR>", &replacement);
        result = result.replace("{{ CENSOR }}", &replacement);
        result = result.replace("{{CENSOR }}", &replacement);
        result = result.replace("{{ CENSOR}}", &replacement);
        result
    }

    const V1_FORMS: [&str; 5] = [
        "{{CENSOR}}",
        "<CENSOR>",
        "{{ CENSOR }}",
        "{{CENSOR }}",
        "{{ CENSOR}}",
    ];

    #[test]
    fn v1_forms_render_exactly_as_before() {
        let cfg = CensorConfig::default();
        for form in V1_FORMS {
            let doc = format!("# Rapport\n\nIntro\n\n{}\n\nConclusion\n", form);
            assert_eq!(process(&doc, &cfg), v1_process(&doc, &cfg), "form {}", form);
        }
        // All of them at once, plus repetitions
        let doc = format!("{}\n{}\n{}", V1_FORMS.join("\n"), V1_FORMS[0], V1_FORMS[1]);
        assert_eq!(process(&doc, &cfg), v1_process(&doc, &cfg));
    }

    #[test]
    fn v1_forms_render_exactly_as_before_with_custom_label() {
        let cfg = CensorConfig {
            label: Some("Réservé aux abonnés <Pro>".to_string()),
        };
        for form in V1_FORMS {
            let doc = format!("avant {} après", form);
            assert_eq!(process(&doc, &cfg), v1_process(&doc, &cfg), "form {}", form);
        }
    }

    #[test]
    fn v1_block_keeps_the_png_path_and_the_alt_selector() {
        let out = process("{{CENSOR}}", &CensorConfig::default());
        assert!(out.contains("static/blured.png"));
        assert!(out.contains("alt=\"CONTENU PREMIUM - Achetez le rapport complet\""));
    }

    #[test]
    fn text_that_only_looks_like_a_tag_is_untouched() {
        let cfg = CensorConfig::default();
        for doc in [
            "{{ other }}",
            "{{CENSORED}}",
            "{{censor}}",
            "<CENSORED>",
            "{{CENSOR",
            "{{CENSOR:start",
            "un { seul } accolade",
            "{{CENSOR:\nstart}}",
        ] {
            assert_eq!(process(doc, &cfg), doc, "doc {:?}", doc);
        }
    }

    #[test]
    fn region_removes_its_content() {
        let doc = "Visible avant.\n\n{{CENSOR:start}}\nSECRETSAUCE ligne une\n\nSECRETSAUCE ligne deux\n{{CENSOR:end}}\n\nVisible après.";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.contains("Visible avant."));
        assert!(out.contains("Visible après."));
        assert!(out.contains("class=\"censor-block\""));
        // The block sits on its own line, as the tags did
        assert!(out.contains("\n\n<span class=\"censor-block\""));
    }

    #[test]
    fn region_height_grows_with_the_removed_volume() {
        let small = estimate_height_mm("une ligne");
        let medium = estimate_height_mm(&"une ligne\n".repeat(6));
        let large = estimate_height_mm(&"une ligne\n".repeat(40));
        assert!(small < medium, "{} < {}", small, medium);
        assert!(medium < large, "{} < {}", medium, large);
        assert!(large <= MAX_BLOCK_MM);
        assert_eq!(estimate_height_mm(""), MIN_BLOCK_MM);
        // Monotone: appending never shortens the block
        let mut previous = 0;
        let mut doc = String::new();
        for i in 0..60 {
            doc.push_str(&format!("ligne {} du contenu retiré\n", i));
            let height = estimate_height_mm(&doc);
            assert!(height >= previous);
            previous = height;
        }
    }

    #[test]
    fn a_table_weighs_more_than_the_same_number_of_plain_lines() {
        let table = "| a | b |\n| - | - |\n| 1 | 2 |";
        let plain = "a b\nc d\ne f";
        assert!(estimate_height_mm(table) >= estimate_height_mm(plain));
    }

    #[test]
    fn levels_put_their_own_label_on_the_block() {
        let cfg = CensorConfig::default();
        assert!(process("{{CENSOR:confidentiel}}", &cfg).contains("CONTENU CONFIDENTIEL"));
        assert!(process("{{CENSOR:interne}}", &cfg).contains("CONTENU INTERNE"));
        assert!(process("{{CENSOR:tarifs}}", &cfg).contains("CONTENU TARIFS"));
        // A level alone is a point replacement, so it keeps the v1 rendering
        assert_eq!(
            process("{{CENSOR:premium}}", &cfg),
            process("{{CENSOR}}", &cfg)
        );
        // And it combines with a region
        let out = process(
            "{{CENSOR:start,confidentiel}}SECRETSAUCE{{CENSOR:end}}",
            &cfg,
        );
        assert!(out.contains("CONTENU CONFIDENTIEL"));
        assert!(!out.contains("SECRETSAUCE"));
    }

    #[test]
    fn the_level_of_a_tag_outranks_the_request_label() {
        let cfg = CensorConfig {
            label: Some("Label global".to_string()),
        };
        assert!(process("{{CENSOR:confidentiel}}", &cfg).contains("CONTENU CONFIDENTIEL"));
        assert!(process("{{CENSOR}}", &cfg).contains("Label global"));
        assert!(process("{{CENSOR:start}}SECRETSAUCE{{CENSOR:end}}", &cfg).contains("Label global"));
    }

    #[test]
    fn teaser_keeps_the_first_words_and_removes_the_rest() {
        let mut region = String::new();
        for i in 1..=40 {
            region.push_str(&format!("mot{} ", i));
        }
        let doc = format!(
            "{{{{CENSOR:teaser=25}}}}{}SECRETSAUCE{{{{CENSOR:end}}}}",
            region
        );
        let out = process(&doc, &CensorConfig::default());
        assert!(out.contains("mot25"));
        assert!(!out.contains("mot26"));
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.contains("class=\"censor-block\""));
    }

    #[test]
    fn teaser_wider_than_the_region_hides_nothing_and_adds_no_block() {
        let out = process(
            "{{CENSOR:teaser=100}}trois mots seulement{{CENSOR:end}}",
            &CensorConfig::default(),
        );
        assert_eq!(out, "trois mots seulement");
    }

    #[test]
    fn teaser_without_a_readable_count_censors_the_whole_region() {
        let out = process(
            "{{CENSOR:teaser=beaucoup}}SECRETSAUCE{{CENSOR:end}}",
            &CensorConfig::default(),
        );
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.contains("class=\"censor-block\""));
    }

    #[test]
    fn inline_region_does_not_break_the_paragraph() {
        let doc = "Le montant est de {{CENSOR:inline}}SECRETSAUCE{{CENSOR:end}} euros par mois.";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert!(!out.contains('\n'));
        assert!(out.starts_with("Le montant est de <span class=\"censor-inline\""));
        assert!(out.ends_with(" euros par mois."));
    }

    #[test]
    fn inline_width_grows_with_the_removed_text() {
        assert!(estimate_width_em("un nom un peu plus long") > estimate_width_em("x"));
        assert!(estimate_width_em(&"x".repeat(500)) <= MAX_INLINE_EM);
        assert!(estimate_width_em("") >= MIN_INLINE_EM);
    }

    #[test]
    fn an_unclosed_region_censors_up_to_the_end_of_the_document() {
        let doc = "Intro visible.\n\n{{CENSOR:start}}\nSECRETSAUCE\n\n## Titre secret\n\nencore SECRETSAUCE\n";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert!(!out.contains("Titre secret"));
        assert!(out.contains("Intro visible."));
        assert!(out.contains("class=\"censor-block\""));
    }

    #[test]
    fn nested_regions_produce_one_block_and_leak_nothing() {
        let doc = "avant {{CENSOR:start}}A SECRETSAUCE {{CENSOR:start}}B SECRETSAUCE{{CENSOR:end}} C SECRETSAUCE{{CENSOR:end}} après";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert_eq!(out.matches("class=\"censor-block\"").count(), 1);
        assert!(out.contains("avant "));
        assert!(out.contains(" après"));
    }

    #[test]
    fn an_inner_end_does_not_reopen_the_outer_region() {
        // The inner `:end` belongs to the inner region: everything up to the outer one goes
        let doc = "{{CENSOR:start}}SECRETSAUCE{{CENSOR:inline}}SECRETSAUCE{{CENSOR:end}}SECRETSAUCE{{CENSOR:end}}visible";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.ends_with("visible"));
    }

    #[test]
    fn a_close_without_an_open_region_is_dropped() {
        let out = process(
            "texte {{CENSOR:end}}toujours visible",
            &CensorConfig::default(),
        );
        assert_eq!(out, "texte toujours visible");
    }

    #[test]
    fn an_empty_or_unknown_argument_list_still_censors() {
        let cfg = CensorConfig::default();
        // `{{CENSOR:}}` and `{{CENSOR: , }}` fall back on the point replacement
        assert_eq!(process("{{CENSOR:}}", &cfg), process("{{CENSOR}}", &cfg));
        assert_eq!(process("{{CENSOR: , }}", &cfg), process("{{CENSOR}}", &cfg));
        // A misspelled opener followed by a close still hides the region
        let out = process("{{CENSOR:strat}}SECRETSAUCE{{CENSOR:end}}visible", &cfg);
        assert!(out.contains("CONTENU STRAT"));
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.ends_with("visible"));
    }

    #[test]
    fn a_v1_tag_is_never_turned_into_a_region_by_a_stray_close() {
        let cfg = CensorConfig::default();
        let doc = "{{CENSOR}}SECRETSAUCE mais public{{CENSOR:end}}suite";
        let out = process(doc, &cfg);
        assert!(out.contains("SECRETSAUCE mais public"));
        assert_eq!(out, v1_process(doc, &cfg).replace("{{CENSOR:end}}", ""));
    }

    #[test]
    fn nothing_censored_ever_reaches_the_output() {
        let doc = concat!(
            "# Rapport\n\n",
            "Résumé public.\n\n",
            "{{CENSOR:start}}\n## Analyse SECRETSAUCE\n\nDeux paragraphes SECRETSAUCE.\n\n",
            "| col | SECRETSAUCE |\n| --- | --- |\n| 1 | SECRETSAUCE |\n{{CENSOR:end}}\n\n",
            "Le prix est {{CENSOR:inline}}42 SECRETSAUCE €{{CENSOR:end}} hors taxe.\n\n",
            "{{CENSOR:teaser=3}}un deux trois SECRETSAUCE quatre{{CENSOR:end}}\n\n",
            "> {{CENSOR:start,confidentiel}}citation SECRETSAUCE{{CENSOR:end}}\n\n",
            "- item {{CENSOR:start}}SECRETSAUCE{{CENSOR:end}}\n\n",
            "{{CENSOR:start}}\nqueue de document SECRETSAUCE, région jamais fermée\n",
        );
        let out = process(doc, &CensorConfig::default());
        assert!(
            !out.contains("SECRETSAUCE"),
            "censored content leaked: {}",
            out
        );
        assert!(!out.contains("{{CENSOR"));
        assert!(!out.contains("<CENSOR>"));
        assert!(out.contains("Résumé public."));
    }

    #[test]
    fn an_untrusted_label_is_escaped() {
        let cfg = CensorConfig {
            label: Some("<script>alert(1)</script> \"x\" & 'y'".to_string()),
        };
        for doc in ["{{CENSOR}}", "{{CENSOR:start}}s{{CENSOR:end}}"] {
            let out = process(doc, &cfg);
            assert!(!out.contains("<script>"));
            assert!(out.contains("&lt;script&gt;"));
            assert!(out.contains("&quot;x&quot;"));
        }
    }

    #[test]
    fn a_multiline_or_oversized_label_stays_on_one_line() {
        let cfg = CensorConfig {
            label: Some(format!("ligne un\nligne deux {}", "x".repeat(400))),
        };
        let out = process("{{CENSOR:start}}s{{CENSOR:end}}", &cfg);
        assert_eq!(out.matches('\n').count(), 0);
        assert!(out.contains("ligne un ligne deux"));
    }

    #[test]
    fn multibyte_content_around_the_tags_is_handled() {
        let doc =
            "Émoji 🔒 et accents àéî {{CENSOR:start}}SECRETSAUCE 🙈 émoji{{CENSOR:end}} suite ✅";
        let out = process(doc, &CensorConfig::default());
        assert!(!out.contains("SECRETSAUCE"));
        assert!(out.contains("Émoji 🔒 et accents àéî "));
        assert!(out.ends_with(" suite ✅"));
    }

    #[test]
    fn a_document_without_any_tag_is_returned_unchanged() {
        let doc = "# Titre\n\nUn texte normal avec {{ variable }} et <span>html</span>.\n";
        assert_eq!(process(doc, &CensorConfig::default()), doc);
    }

    #[test]
    fn the_block_carries_the_lock_and_the_label_but_no_asset() {
        let out = process("{{CENSOR:start}}s{{CENSOR:end}}", &CensorConfig::default());
        assert!(out.contains("<svg"));
        assert!(!out.contains("blured.png"));
        assert!(out.contains("CONTENU PREMIUM - Achetez le rapport complet"));
    }
}
