//! The document that refuses to come out non-conforming.
//!
//! Every other endpoint answers "here is your PDF, hope it is fine". This one takes
//! constraints instead of a request — at most four pages, no table cut in half, no heading
//! stranded at the foot of a page — renders, audits, corrects, re-renders, and answers with
//! the file **and** the log of what it had to do. If it cannot meet the contract it says
//! so, in the response, naming what is still unmet.
//!
//! Entirely deterministic: no LLM, no outbound call. It is the Layout Doctor raised to the
//! rank of a contract, which is what turns "I hope it looks right" into something a caller
//! can assert in a test.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::layout;
use crate::pipeline::{self, RenderSpec, Source};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempPath;

/// Ceiling on the corrective loop. Each pass is a full render; a contract that needs more
/// than this is a document problem, not a layout problem, and saying so quickly is more
/// useful than burning the request's whole time budget to fail anyway.
const MAX_PASSES_CEILING: u8 = 5;
const DEFAULT_MAX_PASSES: u8 = 3;

#[derive(Deserialize)]
pub struct ComposeRequest {
    pub markdown: Option<String>,
    pub html: Option<String>,
    pub template: Option<String>,
    pub data: Option<Value>,
    pub css: Option<String>,
    pub engine: Option<PdfEngine>,
    pub options: Option<PdfOptions>,
    pub header_html: Option<String>,
    pub footer_html: Option<String>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub constraints: Option<Constraints>,
    pub max_passes: Option<u8>,
    /// Seal the result with a signed attestation of what was rendered
    pub attest: Option<bool>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

/// What the caller requires of the finished document. Every field is optional, and a
/// request with no constraint at all is simply a render with a report.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Constraints {
    pub max_pages: Option<usize>,
    pub min_pages: Option<usize>,
    pub no_split_tables: Option<bool>,
    pub no_orphan_headings: Option<bool>,
    pub no_blank_pages: Option<bool>,
    pub no_overflow: Option<bool>,
    /// 0..=100, checked against the Layout Doctor score
    pub min_layout_score: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pass {
    pub n: u8,
    pub score: u8,
    pub pages: usize,
    /// Corrective rules this pass introduced, in plain language
    pub applied: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<crate::assets::AssetMeta>,
    /// `met` when every constraint holds, `unmet` otherwise
    pub verdict: String,
    pub score: u8,
    pub pages: usize,
    pub passes: Vec<Pass>,
    /// Constraints still broken, in the caller's own vocabulary
    pub unmet: Vec<String>,
    pub layout: LayoutReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<BlockWarning>>,
}

#[post("/compose", format = "json", data = "<req>")]
pub async fn compose(
    key: PublicOrKey,
    req: Json<ComposeRequest>,
) -> Result<Either<NamedFile, Json<ComposeResponse>>, AppError> {
    let req = req.into_inner();
    let wants_json = req.client_id.is_some() && req.pdf_name.is_some()
        || req.output.unwrap_or_default() == ToolOutput::Asset;

    let (pdf, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;

    // A contract that was not met still returns the best document produced: refusing to
    // hand it over would leave the caller with a report and nothing to look at.
    if wants_json {
        return Ok(Either::Right(Json(response)));
    }

    Ok(Either::Left(
        NamedFile::open(&pdf).await.map_err(AppError::Io)?,
    ))
}

/// All the blocking work: render, audit, correct, repeat.
pub fn run(req: ComposeRequest) -> Result<(TempPath, ComposeResponse), AppError> {
    let constraints = req.constraints.clone().unwrap_or_default();
    let max_passes = req
        .max_passes
        .unwrap_or(DEFAULT_MAX_PASSES)
        .clamp(1, MAX_PASSES_CEILING);

    let source = source_of(&req)?;
    let base_css = req.css.clone().unwrap_or_default();

    // The caller's own autolayout preference is deliberately overridden: asking for a
    // contract *is* asking for the audit, and a report is needed to judge the first pass.
    let mut options = req.options.clone().unwrap_or_default();
    options.autolayout = Some(true);

    let mut corrective = String::new();
    let mut passes: Vec<Pass> = Vec::new();
    let mut best: Option<(TempPath, LayoutReport, Vec<BlockWarning>)> = None;

    for n in 1..=max_passes {
        // A pass the deadline cannot finish costs a render and returns nothing useful
        if n > 1
            && helpers::Budget::remaining()
                .is_some_and(|left| left < crate::config::config().process_timeout)
        {
            warn!("Compose: not enough budget left for pass {}", n);
            break;
        }

        let mut spec = RenderSpec::new(source_clone(source, &req)?);
        spec.css = Some(merge_css(&base_css, &corrective));
        spec.engine = req.engine.clone().unwrap_or_default();
        spec.options = options.clone();
        spec.header_html = req.header_html.clone();
        spec.footer_html = req.footer_html.clone();
        spec.header_template = req.header_template.clone();
        spec.footer_template = req.footer_template.clone();

        let outcome = pipeline::render_blocking(spec)?;
        let report = match outcome.layout {
            Some(report) => report,
            // autolayout was forced on above, so this cannot happen; treating it as a bug
            // rather than defaulting to a perfect score keeps the contract honest.
            None => layout::analyze(&outcome.pdf)?,
        };

        let unmet = evaluate(&constraints, &report);
        let improved = best
            .as_ref()
            .map(|(_, previous, _)| better(&report, previous))
            .unwrap_or(true);

        if improved {
            best = Some((outcome.pdf, report.clone(), outcome.warnings));
        }

        passes.push(Pass {
            n,
            score: report.score,
            pages: report.pages,
            applied: describe(&corrective),
        });

        if unmet.is_empty() {
            break;
        }

        // Nothing left to try: another identical render would only spend the budget
        let next = next_corrective(&corrective, &report, &constraints);
        if next == corrective {
            break;
        }
        corrective = next;
    }

    let Some((pdf, report, warnings)) = best else {
        return Err(AppError::ProcessFailed {
            message: "Compose produced no document".to_string(),
            stderr: String::new(),
        });
    };

    let unmet = evaluate(&constraints, &report);

    let mut response = ComposeResponse {
        download_url: helpers::save_if_requested(&pdf, req.client_id, req.pdf_name)?,
        asset: None,
        verdict: if unmet.is_empty() { "met" } else { "unmet" }.to_string(),
        score: report.score,
        pages: report.pages,
        passes,
        unmet,
        layout: report.clone(),
        attestation: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
    };

    if req.attest.unwrap_or(false) {
        let mut attestation = crate::attest::Attestation::of(&pdf, report.pages)?;
        attestation.engine = Some(req.engine.unwrap_or_default().to_string());
        attestation.theme = options.theme.clone();
        attestation.layout_score = Some(report.score);
        response.attestation = Some(attestation.with_operation("compose").seal()?);
    }

    if req.output.unwrap_or_default() == ToolOutput::Asset {
        response.asset = Some(crate::assets::store(&pdf, "composed.pdf")?);
    }

    Ok((pdf, response))
}

// ------------ Constraints ------------

/// Which constraints the finished document breaks, phrased the way the caller wrote them.
fn evaluate(constraints: &Constraints, report: &LayoutReport) -> Vec<String> {
    let mut unmet = Vec::new();
    let count = |kind: &str| report.issues.iter().filter(|i| i.kind == kind).count();
    let pages_of = |kind: &str| {
        report
            .issues
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| i.page.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    if let Some(max) = constraints.max_pages {
        if report.pages > max {
            unmet.push(format!(
                "max_pages: the document is {} pages, the contract allows {}",
                report.pages, max
            ));
        }
    }

    if let Some(min) = constraints.min_pages {
        if report.pages < min {
            unmet.push(format!(
                "min_pages: the document is {} pages, the contract requires {}",
                report.pages, min
            ));
        }
    }

    for (asked, kind, label) in [
        (
            constraints.no_split_tables.unwrap_or(false),
            "split_table",
            "no_split_tables",
        ),
        (
            constraints.no_orphan_headings.unwrap_or(false),
            "orphan_heading",
            "no_orphan_headings",
        ),
        (
            constraints.no_blank_pages.unwrap_or(false),
            "blank_page",
            "no_blank_pages",
        ),
        (
            constraints.no_overflow.unwrap_or(false),
            "overflow",
            "no_overflow",
        ),
    ] {
        if asked && count(kind) > 0 {
            unmet.push(format!(
                "{}: still {} on page {}",
                label,
                count(kind),
                pages_of(kind)
            ));
        }
    }

    if let Some(min) = constraints.min_layout_score {
        if report.score < min {
            unmet.push(format!(
                "min_layout_score: scored {}, the contract requires {}",
                report.score, min
            ));
        }
    }

    unmet
}

/// Is this render better than the one we are holding?
///
/// Page count breaks the tie before the score does: a caller who asked for four pages would
/// rather have four pages scoring 88 than six scoring 92.
fn better(candidate: &LayoutReport, current: &LayoutReport) -> bool {
    if candidate.pages != current.pages {
        return candidate.pages < current.pages && candidate.score + 10 >= current.score;
    }
    candidate.score > current.score
}

/// The stylesheet for the next attempt.
///
/// The Layout Doctor already knows how to answer overflow, orphan headings and split
/// tables. What it has no opinion on is a page budget, because shrinking a document is a
/// decision, not a repair — so that rule is added here, and only when the caller asked for
/// a page ceiling.
fn next_corrective(current: &str, report: &LayoutReport, constraints: &Constraints) -> String {
    let mut next = String::from(current);

    let from_doctor = layout::corrective_css(report);
    if !from_doctor.is_empty() && !next.contains(&from_doctor) {
        next.push('\n');
        next.push_str(&from_doctor);
    }

    if let Some(max) = constraints.max_pages {
        if report.pages > max {
            // Step down in fixed increments rather than computing a ratio: the relation
            // between font size and page count is not linear, and a computed jump
            // overshoots into unreadable type on the first pass.
            let step = next.matches("--compose-shrink").count();
            let scale = match step {
                0 => 96,
                1 => 92,
                2 => 88,
                _ => 85,
            };
            next.push_str(&format!(
                "\n/* --compose-shrink: {} pages for a {}-page contract */\n\
                 html {{ font-size: {}%; }}\n\
                 @page {{ margin: 1.6cm; }}\n",
                report.pages, max, scale
            ));
        }
    }

    next
}

/// Turn the accumulated stylesheet into the short lines a caller reads in `passes`
fn describe(corrective: &str) -> Vec<String> {
    corrective
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let inner = line.strip_prefix("/*")?.strip_suffix("*/")?;
            Some(inner.trim().to_string())
        })
        .collect()
}

// ------------ Source ------------

fn source_of(req: &ComposeRequest) -> Result<&'static str, AppError> {
    match (&req.markdown, &req.html, &req.template) {
        (Some(_), None, None) => Ok("markdown"),
        (None, Some(_), None) => Ok("html"),
        (None, None, Some(_)) => Ok("template"),
        (None, None, None) => Err(AppError::BadRequest(
            "One of \"markdown\", \"html\" or \"template\" is required".to_string(),
        )),
        _ => Err(AppError::BadRequest(
            "Send exactly one of \"markdown\", \"html\" or \"template\"".to_string(),
        )),
    }
}

/// Each pass needs its own `Source`, and `Source` owns its text: rebuilding it per pass is
/// a clone of the document, which is cheaper than the render that follows it.
fn source_clone(kind: &str, req: &ComposeRequest) -> Result<Source, AppError> {
    match kind {
        "markdown" => Ok(Source::Markdown(req.markdown.clone().unwrap_or_default())),
        "html" => Ok(Source::Html(req.html.clone().unwrap_or_default())),
        _ => Ok(Source::Template {
            template: req.template.clone().unwrap_or_default(),
            data: req
                .data
                .clone()
                .unwrap_or(Value::Object(Default::default())),
        }),
    }
}

fn merge_css(base: &str, corrective: &str) -> String {
    match (base.is_empty(), corrective.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_string(),
        (true, false) => corrective.to_string(),
        (false, false) => format!("{}\n{}", base, corrective),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(pages: usize, score: u8, issues: &[(&str, usize)]) -> LayoutReport {
        LayoutReport {
            pages,
            score,
            issues: issues
                .iter()
                .map(|(kind, page)| LayoutIssue::new(kind, *page, "warn", String::new()))
                .collect(),
            passes: None,
        }
    }

    #[test]
    fn a_document_with_no_constraint_is_always_conforming() {
        let unmet = evaluate(&Constraints::default(), &report(12, 40, &[("overflow", 3)]));
        assert!(unmet.is_empty());
    }

    #[test]
    fn names_the_constraint_and_the_page_that_breaks_it() {
        let constraints = Constraints {
            max_pages: Some(4),
            no_split_tables: Some(true),
            min_layout_score: Some(90),
            ..Default::default()
        };

        let unmet = evaluate(&constraints, &report(6, 71, &[("split_table", 3)]));

        assert_eq!(unmet.len(), 3);
        assert!(unmet[0].contains("max_pages") && unmet[0].contains("6 pages"));
        assert!(unmet[1].contains("no_split_tables") && unmet[1].contains("page 3"));
        assert!(unmet[2].contains("min_layout_score") && unmet[2].contains("71"));
    }

    /// An issue the caller did not forbid must not fail their contract
    #[test]
    fn only_the_constraints_that_were_asked_for_are_checked() {
        let constraints = Constraints {
            no_blank_pages: Some(true),
            ..Default::default()
        };
        assert!(evaluate(&constraints, &report(3, 80, &[("overflow", 1)])).is_empty());
        assert_eq!(
            evaluate(&constraints, &report(3, 80, &[("blank_page", 2)])).len(),
            1
        );
    }

    #[test]
    fn fewer_pages_wins_over_a_slightly_better_score() {
        assert!(better(&report(4, 88, &[]), &report(6, 92, &[])));
        // …but not at any price: a collapse in quality is not an improvement
        assert!(!better(&report(4, 60, &[]), &report(6, 92, &[])));
        assert!(better(&report(6, 95, &[]), &report(6, 92, &[])));
        assert!(!better(&report(6, 92, &[]), &report(6, 92, &[])));
    }

    #[test]
    fn shrinks_one_step_at_a_time_when_the_page_budget_is_broken() {
        let constraints = Constraints {
            max_pages: Some(4),
            ..Default::default()
        };
        let over = report(6, 90, &[]);

        let first = next_corrective("", &over, &constraints);
        assert!(first.contains("font-size: 96%"));

        let second = next_corrective(&first, &over, &constraints);
        assert!(second.contains("font-size: 92%"));

        // The floor holds instead of shrinking into unreadable type
        let mut current = second;
        for _ in 0..6 {
            current = next_corrective(&current, &over, &constraints);
        }
        assert!(current.contains("font-size: 85%"));
        assert!(!current.contains("font-size: 40%"));
    }

    #[test]
    fn adds_no_shrink_rule_when_the_page_count_is_within_budget() {
        let constraints = Constraints {
            max_pages: Some(8),
            ..Default::default()
        };
        assert!(!next_corrective("", &report(6, 90, &[]), &constraints).contains("compose-shrink"));
    }

    #[test]
    fn reads_the_applied_rules_out_of_the_stylesheet_comments() {
        let css = "/* layout: content past the content box */\ntable { width: 100%; }\n\
                   /* --compose-shrink: 6 pages for a 4-page contract */\nhtml { font-size: 96%; }";
        let applied = describe(css);
        assert_eq!(applied.len(), 2);
        assert!(applied[0].starts_with("layout:"));
        assert!(applied[1].starts_with("--compose-shrink"));
    }

    #[test]
    fn requires_exactly_one_source() {
        let empty = ComposeRequest {
            markdown: None,
            html: None,
            template: None,
            data: None,
            css: None,
            engine: None,
            options: None,
            header_html: None,
            footer_html: None,
            header_template: None,
            footer_template: None,
            constraints: None,
            max_passes: None,
            attest: None,
            client_id: None,
            pdf_name: None,
            output: None,
        };
        assert!(source_of(&empty).is_err());

        let markdown = ComposeRequest {
            markdown: Some("# Title".to_string()),
            ..empty_like(&empty)
        };
        assert_eq!(source_of(&markdown).unwrap(), "markdown");

        let both = ComposeRequest {
            markdown: Some("# Title".to_string()),
            html: Some("<p>x</p>".to_string()),
            ..empty_like(&empty)
        };
        assert!(source_of(&both).is_err());
    }

    fn empty_like(_: &ComposeRequest) -> ComposeRequest {
        ComposeRequest {
            markdown: None,
            html: None,
            template: None,
            data: None,
            css: None,
            engine: None,
            options: None,
            header_html: None,
            footer_html: None,
            header_template: None,
            footer_template: None,
            constraints: None,
            max_passes: None,
            attest: None,
            client_id: None,
            pdf_name: None,
            output: None,
        }
    }

    #[test]
    fn merges_the_client_stylesheet_before_the_corrections() {
        assert_eq!(merge_css("", ""), "");
        assert_eq!(merge_css("a{}", ""), "a{}");
        assert_eq!(merge_css("", "b{}"), "b{}");
        assert_eq!(merge_css("a{}", "b{}"), "a{}\nb{}");
    }
}
