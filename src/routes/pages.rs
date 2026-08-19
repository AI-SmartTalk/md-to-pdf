//! Page organisation: keep, drop, reorder, turn and cut a document.
//!
//! Every operation is a page selection, which is why the range parser below is the whole
//! endpoint: `qpdf` does the copying, we do the arithmetic. The parser is deliberately
//! strict — a range that names a page the document does not have is a caller mistake worth
//! a 400, not a silently shorter PDF.

use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use serde::Deserialize;
use std::fmt::Write as _;
use std::process::Command;
use tempfile::{Builder, TempPath};

/// A split that produced more files than this filled a disk, not a need: page-by-page on a
/// 2000 page document is a mistake the caller should hear about before we write anything.
const MAX_PARTS: usize = 500;

/// A range specification is a handful of numbers. Anything longer is someone probing the
/// parser, and the parse is quadratic in the number of items.
const MAX_SPEC_LEN: usize = 4096;

#[derive(Deserialize)]
pub struct PagesRequest {
    pub pdf: String,
    /// `extract`, `delete`, `reorder`, `rotate` or `split`
    pub op: String,
    /// Range specification: `1,3,5-9`, `2-`, `all`, `odd`, `even`
    pub pages: Option<String>,
    /// Complete new page order, for `reorder`
    pub order: Option<String>,
    /// Rotation in degrees, for `rotate`
    pub angle: Option<i32>,
    /// Pages per file, for `split`
    pub every: Option<usize>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
    pub output: Option<ToolOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Extract,
    Delete,
    Reorder,
    Rotate,
    Split,
}

#[post("/pages", format = "json", data = "<req>")]
pub async fn pages(
    key: PublicOrKey,
    req: Json<PagesRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    // Everything that can be judged without opening the document is judged here: a request
    // that names an unknown verb must not wait for a render slot to be told so.
    validate(&req)?;

    let (produced, response) = exec::as_owner(key.0, exec::offload(move || run(req))).await?;
    helpers::deliver_tool(produced, response).await
}

/// All the blocking work. Public and free of Rocket: the asynchronous job dispatcher calls
/// it as it is.
pub fn run(req: PagesRequest) -> Result<(TempPath, ToolResponse), AppError> {
    let op = validate(&req)?;

    let source = helpers::resolve_pdf_source(&req.pdf)?;
    let source_arg = helpers::path_to_str(&source)?.to_string();
    let total = crate::pdfops::page_count(&source)?;
    if total == 0 {
        return Err(AppError::BadRequest(
            "The document has no page to organise".to_string(),
        ));
    }

    if op == Op::Split {
        return split(req, &source_arg, total);
    }

    let out_temp = Builder::new().suffix(".pdf").tempfile()?;
    let out_arg = helpers::path_to_str(out_temp.path())?.to_string();

    match op {
        Op::Extract => {
            let kept = sorted(parse_pages(spec(&req.pages), total)?);
            select(&source_arg, &to_range_spec(&kept), &out_arg)?;
        }
        Op::Delete => {
            let dropped = parse_pages(spec(&req.pages), total)?;
            let kept = complement(&dropped, total);
            if kept.is_empty() {
                return Err(AppError::BadRequest(
                    "Deleting these pages would leave an empty document".to_string(),
                ));
            }
            select(&source_arg, &to_range_spec(&kept), &out_arg)?;
        }
        Op::Reorder => {
            let order = parse_pages(spec(&req.order), total)?;
            if order.len() != total {
                return Err(AppError::BadRequest(format!(
                    "\"order\" must list every page of the {} page document, it lists {}",
                    total,
                    order.len()
                )));
            }
            select(&source_arg, &to_range_spec(&order), &out_arg)?;
        }
        Op::Rotate => {
            let turned = sorted(parse_pages(req.pages.as_deref().unwrap_or("all"), total)?);
            let angle = req.angle.unwrap_or(0);
            rotate(&source_arg, &to_range_spec(&turned), angle, &out_arg)?;
        }
        Op::Split => unreachable!("split returns above"),
    }

    let produced = out_temp.into_temp_path();

    // Measured before `finish_tool`, which moves the file away when an asset was asked for
    let pages = crate::pdfops::page_count(&produced)?;

    let mut response = helpers::finish_tool(
        &produced,
        req.client_id,
        req.pdf_name,
        req.output.unwrap_or_default(),
        "pages.pdf",
    )?;
    response.pages = Some(pages);

    Ok((produced, response))
}

/// Cut a document into several files.
///
/// The signature has to hand back one `TempPath`, so the first part is copied aside before
/// the parts are stored — `assets::store` moves the file it is given.
fn split(
    req: PagesRequest,
    source: &str,
    total: usize,
) -> Result<(TempPath, ToolResponse), AppError> {
    let mut warnings: Vec<String> = Vec::new();

    let ranges = match req.pages.as_deref() {
        Some(cuts) => {
            if req.every.is_some() {
                warnings.push("\"every\" was ignored: \"pages\" gives the cut points".to_string());
            }
            cut_at(&parse_pages(cuts, total)?, total)?
        }
        None => chunks(req.every.unwrap_or(1), total)?,
    };

    let mut parts: Vec<TempPath> = Vec::new();
    for (first, last) in &ranges {
        let part = Builder::new().suffix(".pdf").tempfile()?;
        let part_arg = helpers::path_to_str(part.path())?.to_string();
        select(source, &format!("{}-{}", first, last), &part_arg)?;
        parts.push(part.into_temp_path());
    }

    let first_copy = Builder::new().suffix(".pdf").tempfile()?;
    std::fs::copy(&parts[0], first_copy.path())?;
    let produced = first_copy.into_temp_path();

    let stem = req
        .pdf_name
        .as_deref()
        .map(name_stem)
        .unwrap_or_else(|| "part".to_string());

    let mut download_url = None;
    let mut assets = Vec::with_capacity(parts.len());

    for (index, part) in parts.iter().enumerate() {
        let name = format!("{}-{}.pdf", stem, index + 1);

        // Saving before storing, for the same reason `finish_tool` does it in that order
        if let (Some(client_id), Some(_)) = (req.client_id.as_deref(), req.pdf_name.as_deref()) {
            let url = helpers::save_pdf(part, client_id, &name)?;
            if download_url.is_none() {
                download_url = Some(url);
            }
        }

        assets.push(crate::assets::store(part, &name)?);
    }

    let response = ToolResponse {
        download_url,
        assets: Some(assets),
        // The document that was cut: each part carries its own page count in `assets`
        pages: Some(total),
        warnings: (!warnings.is_empty()).then_some(warnings),
        ..Default::default()
    };

    Ok((produced, response))
}

// ------------ Validation ------------

/// Everything that can be checked without reading the document. Called twice on purpose:
/// once by the route so a bad request never queues, once by `run` so the job dispatcher
/// gets the same 400.
fn validate(req: &PagesRequest) -> Result<Op, AppError> {
    let op = parse_op(&req.op)?;

    for (field, value) in [("pages", &req.pages), ("order", &req.order)] {
        if let Some(value) = value {
            if value.len() > MAX_SPEC_LEN {
                return Err(AppError::BadRequest(format!(
                    "\"{}\" is longer than {} characters",
                    field, MAX_SPEC_LEN
                )));
            }
        }
    }

    match op {
        Op::Extract | Op::Delete => require(&req.pages, "pages", &req.op)?,
        Op::Reorder => require(&req.order, "order", &req.op)?,
        Op::Rotate => {
            let angle = req.angle.ok_or_else(|| {
                AppError::BadRequest(
                    "\"angle\" is required for \"op\":\"rotate\" (90, 180, 270 or their negatives)"
                        .to_string(),
                )
            })?;
            if !matches!(angle, 90 | 180 | 270 | -90 | -180 | -270) {
                return Err(AppError::BadRequest(format!(
                    "\"angle\": {} is not a quarter turn (expected 90, 180, 270, -90, -180 or -270)",
                    angle
                )));
            }
        }
        Op::Split => {
            if req.every == Some(0) {
                return Err(AppError::BadRequest(
                    "\"every\" must be at least 1 page per file".to_string(),
                ));
            }
        }
    }

    Ok(op)
}

fn parse_op(op: &str) -> Result<Op, AppError> {
    match op.trim().to_ascii_lowercase().as_str() {
        "extract" => Ok(Op::Extract),
        "delete" => Ok(Op::Delete),
        "reorder" => Ok(Op::Reorder),
        "rotate" => Ok(Op::Rotate),
        "split" => Ok(Op::Split),
        other => Err(AppError::BadRequest(format!(
            "Unknown \"op\": {:?} (expected extract, delete, reorder, rotate or split)",
            other
        ))),
    }
}

fn require(value: &Option<String>, field: &str, op: &str) -> Result<(), AppError> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err(AppError::BadRequest(format!(
            "\"{}\" is required for \"op\":\"{}\"",
            field, op
        ))),
    }
}

/// The field was proven present by `validate`; this keeps the call sites free of unwraps
fn spec(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

// ------------ Range parsing ------------

/// Turn a range specification into the 1-based pages it names.
///
/// Order is preserved and duplicates are dropped, because `reorder` reads the result as the
/// new page order while every other operation sorts it. Out of range, inverted and
/// unparsable are all errors that name what is wrong: a range is written by a human, and a
/// document quietly missing a page is the worst possible way to find out about a typo.
fn parse_pages(spec: &str, total: usize) -> Result<Vec<usize>, AppError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "Empty page range (expected something like \"1,3,5-9\", \"2-\", \"all\", \"odd\" or \"even\")"
                .to_string(),
        ));
    }

    let lowered = trimmed.to_ascii_lowercase();
    let selected: Vec<usize> = match lowered.as_str() {
        "all" => (1..=total).collect(),
        "odd" => (1..=total).step_by(2).collect(),
        "even" => (2..=total).step_by(2).collect(),
        _ => parse_items(&lowered, trimmed, total)?,
    };

    if selected.is_empty() {
        return Err(AppError::BadRequest(format!(
            "{:?} selects no page in a {} page document",
            trimmed, total
        )));
    }

    Ok(selected)
}

fn parse_items(lowered: &str, original: &str, total: usize) -> Result<Vec<usize>, AppError> {
    let mut selected: Vec<usize> = Vec::new();

    for item in lowered.split(',') {
        let item = item.trim();
        if item.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Empty item in page range {:?}",
                original
            )));
        }

        let (first, last) = match item.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (from.trim(), to.trim());
                if from.is_empty() && to.is_empty() {
                    return Err(AppError::BadRequest(format!(
                        "Range {:?} has no bound (write \"2-\" for page 2 to the end)",
                        item
                    )));
                }
                let first = if from.is_empty() {
                    1
                } else {
                    page_number(from, original)?
                };
                let last = if to.is_empty() {
                    total
                } else {
                    page_number(to, original)?
                };
                (first, last)
            }
            None => {
                let page = page_number(item, original)?;
                (page, page)
            }
        };

        if first > last {
            return Err(AppError::BadRequest(format!(
                "Range {:?} is inverted: {} comes after {}",
                item, first, last
            )));
        }
        if last > total {
            return Err(AppError::BadRequest(format!(
                "Page {} is out of range: the document has {} pages",
                last, total
            )));
        }

        for page in first..=last {
            if !selected.contains(&page) {
                selected.push(page);
            }
        }
    }

    Ok(selected)
}

fn page_number(value: &str, original: &str) -> Result<usize, AppError> {
    match value.parse::<usize>() {
        Ok(0) => Err(AppError::BadRequest(format!(
            "Page 0 does not exist in {:?}: pages are numbered from 1",
            original
        ))),
        Ok(page) => Ok(page),
        Err(_) => Err(AppError::BadRequest(format!(
            "{:?} is not a page number in {:?}",
            value, original
        ))),
    }
}

/// Collapse consecutive ascending runs, so a 2000 page selection is a short argument.
/// Order is preserved, which is what lets `reorder` reuse this.
fn to_range_spec(pages: &[usize]) -> String {
    let mut spec = String::new();
    let mut index = 0;

    while index < pages.len() {
        let start = pages[index];
        let mut end = start;
        while index + 1 < pages.len() && pages[index + 1] == end + 1 {
            index += 1;
            end = pages[index];
        }

        if !spec.is_empty() {
            spec.push(',');
        }
        if end > start {
            let _ = write!(spec, "{}-{}", start, end);
        } else {
            let _ = write!(spec, "{}", start);
        }
        index += 1;
    }

    spec
}

fn sorted(mut pages: Vec<usize>) -> Vec<usize> {
    pages.sort_unstable();
    pages
}

fn complement(dropped: &[usize], total: usize) -> Vec<usize> {
    (1..=total).filter(|page| !dropped.contains(page)).collect()
}

// ------------ Split plans ------------

/// Fixed size parts: 3 pages every file on 7 pages gives 1-3, 4-6, 7-7
fn chunks(every: usize, total: usize) -> Result<Vec<(usize, usize)>, AppError> {
    if every == 0 {
        return Err(AppError::BadRequest(
            "\"every\" must be at least 1 page per file".to_string(),
        ));
    }

    let count = total.div_ceil(every);
    guard_parts(count, total)?;

    Ok((0..count)
        .map(|index| {
            let first = index * every + 1;
            (first, (first + every - 1).min(total))
        })
        .collect())
}

/// Each named page opens a new file: cutting a 10 page document at 3 and 7 gives 1-2, 3-6,
/// 7-10. Naming page 1 is redundant rather than wrong, so it is absorbed.
fn cut_at(cuts: &[usize], total: usize) -> Result<Vec<(usize, usize)>, AppError> {
    let mut starts = sorted(cuts.to_vec());
    starts.retain(|page| *page > 1);
    starts.insert(0, 1);

    guard_parts(starts.len(), total)?;

    Ok(starts
        .iter()
        .enumerate()
        .map(|(index, first)| {
            let last = starts.get(index + 1).map(|next| next - 1).unwrap_or(total);
            (*first, last)
        })
        .collect())
}

fn guard_parts(count: usize, total: usize) -> Result<(), AppError> {
    if count > MAX_PARTS {
        return Err(AppError::BadRequest(format!(
            "This split would produce {} files from {} pages, the limit is {}",
            count, total, MAX_PARTS
        )));
    }
    Ok(())
}

/// `report.pdf` and `report` both name the parts `report-1.pdf`, `report-2.pdf`, ...
fn name_stem(pdf_name: &str) -> String {
    let trimmed = pdf_name.trim();
    let stem = trimmed.strip_suffix(".pdf").unwrap_or(trimmed);
    if stem.is_empty() {
        "part".to_string()
    } else {
        stem.to_string()
    }
}

// ------------ qpdf ------------

/// `--warning-exit-0` because qpdf exits 3 on a document it recovered from, and a PDF a
/// caller uploaded is exactly the kind that carries a recoverable defect: refusing to
/// organise it would be a 500 for a file that opens fine everywhere else.
fn select(source: &str, spec: &str, output: &str) -> Result<(), AppError> {
    helpers::run_tool(
        Command::new("qpdf")
            .arg("--warning-exit-0")
            .arg(source)
            .arg("--pages")
            .arg(source)
            .arg(spec)
            .arg("--")
            .arg(output),
        "qpdf",
        "Page selection failed",
    )
}

fn rotate(source: &str, spec: &str, angle: i32, output: &str) -> Result<(), AppError> {
    // The sign is what makes the turn relative to the rotation the page already carries
    let sign = if angle < 0 { '-' } else { '+' };

    helpers::run_tool(
        Command::new("qpdf")
            .arg("--warning-exit-0")
            .arg(format!("--rotate={}{}:{}", sign, angle.abs(), spec))
            .arg(source)
            .arg(output),
        "qpdf",
        "Page rotation failed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(op: &str) -> PagesRequest {
        PagesRequest {
            pdf: "asset://as_0".to_string(),
            op: op.to_string(),
            pages: None,
            order: None,
            angle: None,
            every: None,
            client_id: None,
            pdf_name: None,
            output: None,
        }
    }

    #[test]
    fn reads_lists_ranges_and_open_ends() {
        assert_eq!(
            parse_pages("1,3,5-9", 10).unwrap(),
            vec![1, 3, 5, 6, 7, 8, 9]
        );
        assert_eq!(parse_pages("2-", 4).unwrap(), vec![2, 3, 4]);
        assert_eq!(parse_pages("-3", 9).unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_pages("7", 7).unwrap(), vec![7]);
    }

    #[test]
    fn tolerates_spaces_and_case() {
        assert_eq!(parse_pages("  1 , 3 - 5 ", 5).unwrap(), vec![1, 3, 4, 5]);
        assert_eq!(parse_pages(" ALL ", 3).unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_pages("Odd", 5).unwrap(), vec![1, 3, 5]);
        assert_eq!(parse_pages("EVEN", 5).unwrap(), vec![2, 4]);
    }

    #[test]
    fn keeps_the_order_written_and_drops_duplicates() {
        // What `reorder` depends on: 3 stays first, and the repeat is absorbed
        assert_eq!(parse_pages("3,1,2,1", 3).unwrap(), vec![3, 1, 2]);
        assert_eq!(parse_pages("2-4,3", 5).unwrap(), vec![2, 3, 4]);
    }

    #[test]
    fn a_page_the_document_does_not_have_is_a_named_error() {
        let err = parse_pages("1,12", 10).unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(m) if m.contains("Page 12") && m.contains("10 pages"))
        );

        assert!(parse_pages("9-20", 10).is_err());
        assert!(parse_pages("0", 10).is_err());
        assert!(parse_pages("0-3", 10).is_err());
    }

    #[test]
    fn an_inverted_range_is_refused_rather_than_reversed() {
        let err = parse_pages("9-3", 10).unwrap_err();
        assert!(matches!(&err, AppError::BadRequest(m) if m.contains("inverted")));
    }

    #[test]
    fn invalid_syntax_says_what_it_could_not_read() {
        for spec in [
            "", "   ", ",", "1,,2", "1-2-3", "abc", "1,x", "-", "1..3", "1 2",
        ] {
            assert!(
                parse_pages(spec, 10).is_err(),
                "{:?} should be rejected",
                spec
            );
        }
    }

    #[test]
    fn a_selection_that_matches_nothing_is_an_error_not_an_empty_pdf() {
        assert!(parse_pages("even", 1).is_err());
        assert!(parse_pages("all", 0).is_err());
    }

    #[test]
    fn collapses_consecutive_pages_into_qpdf_ranges() {
        assert_eq!(to_range_spec(&[1, 2, 3, 7, 9, 10]), "1-3,7,9-10");
        assert_eq!(to_range_spec(&[4]), "4");
        assert_eq!(to_range_spec(&[]), "");
        // A reordering is not sorted, and must stay in the order it was given
        assert_eq!(to_range_spec(&[3, 1, 2]), "3,1-2");
        assert_eq!(to_range_spec(&[3, 2, 1]), "3,2,1");
    }

    #[test]
    fn deleting_keeps_everything_else_in_order() {
        assert_eq!(complement(&[2, 4], 5), vec![1, 3, 5]);
        assert_eq!(complement(&[1, 2, 3], 3), Vec::<usize>::new());
    }

    #[test]
    fn cuts_documents_into_fixed_size_parts() {
        assert_eq!(chunks(3, 7).unwrap(), vec![(1, 3), (4, 6), (7, 7)]);
        assert_eq!(chunks(1, 3).unwrap(), vec![(1, 1), (2, 2), (3, 3)]);
        assert_eq!(chunks(10, 4).unwrap(), vec![(1, 4)]);
        assert!(chunks(0, 4).is_err());
    }

    #[test]
    fn cut_points_open_the_file_they_name() {
        assert_eq!(cut_at(&[3, 7], 10).unwrap(), vec![(1, 2), (3, 6), (7, 10)]);
        // Naming the first page adds nothing rather than producing an empty part
        assert_eq!(cut_at(&[1, 5], 8).unwrap(), vec![(1, 4), (5, 8)]);
        assert_eq!(cut_at(&[4], 4).unwrap(), vec![(1, 3), (4, 4)]);
    }

    #[test]
    fn a_split_that_would_flood_the_disk_is_refused() {
        assert!(chunks(1, MAX_PARTS).is_ok());
        let err = chunks(1, MAX_PARTS + 1).unwrap_err();
        assert!(matches!(&err, AppError::BadRequest(m) if m.contains("the limit is 500")));

        let cuts: Vec<usize> = (2..=MAX_PARTS + 2).collect();
        assert!(cut_at(&cuts, 2000).is_err());
    }

    #[test]
    fn names_the_verb_it_did_not_understand() {
        assert_eq!(parse_op(" Extract ").unwrap(), Op::Extract);
        assert_eq!(parse_op("split").unwrap(), Op::Split);

        let err = parse_op("shuffle").unwrap_err();
        assert!(
            matches!(&err, AppError::BadRequest(m) if m.contains("shuffle") && m.contains("reorder"))
        );
    }

    #[test]
    fn refuses_an_operation_without_what_it_needs() {
        assert!(validate(&request("extract")).is_err());
        assert!(validate(&request("delete")).is_err());
        assert!(validate(&request("reorder")).is_err());
        assert!(validate(&request("rotate")).is_err());

        let mut extract = request("extract");
        extract.pages = Some("  ".to_string());
        assert!(validate(&extract).is_err());

        extract.pages = Some("1-3".to_string());
        assert_eq!(validate(&extract).unwrap(), Op::Extract);
    }

    #[test]
    fn only_quarter_turns_are_rotations() {
        let mut req = request("rotate");
        for angle in [90, 180, 270, -90, -180, -270] {
            req.angle = Some(angle);
            assert!(validate(&req).is_ok(), "{} should be accepted", angle);
        }
        for angle in [0, 45, 360, 91, -45] {
            req.angle = Some(angle);
            assert!(validate(&req).is_err(), "{} should be refused", angle);
        }
    }

    #[test]
    fn split_defaults_to_one_file_per_page_and_refuses_zero() {
        let mut req = request("split");
        assert_eq!(validate(&req).unwrap(), Op::Split);

        req.every = Some(0);
        assert!(validate(&req).is_err());
    }

    #[test]
    fn an_overlong_range_never_reaches_the_parser() {
        let mut req = request("extract");
        req.pages = Some("1,".repeat(MAX_SPEC_LEN));
        assert!(
            matches!(validate(&req), Err(AppError::BadRequest(m)) if m.contains("longer than"))
        );
    }

    /// `assets::store` renames the file it is handed, so a measurement taken after
    /// `finish_tool` reads a path that no longer exists the day `/tmp` and the asset root
    /// share a filesystem. Only the order of the two statements prevents it, and `run` cannot
    /// be exercised in a unit test — it needs qpdf — so the order itself is what is asserted.
    #[test]
    fn the_page_count_is_taken_before_the_file_is_published() {
        let source = include_str!("pages.rs");
        let measured = source
            .find("page_count(&produced)")
            .expect("run measures the file it produced");
        let published = source
            .find("helpers::finish_tool")
            .expect("run publishes the file it produced");

        assert!(
            measured < published,
            "page_count must run before finish_tool moves the file away"
        );
    }

    #[test]
    fn parts_are_named_after_the_document() {
        assert_eq!(name_stem("report.pdf"), "report");
        assert_eq!(name_stem(" report "), "report");
        assert_eq!(name_stem(".pdf"), "part");
    }
}
