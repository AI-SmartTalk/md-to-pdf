//! PDF primitives shared by the Layout Doctor, redaction, diff and preview.
//!
//! Everything here spawns a poppler tool through `helpers`, so the global process timeout
//! applies and no call can hang a render thread forever.

use crate::helpers::{self, base64};
use crate::types::AppError;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::{Builder, TempPath};

/// Page geometry in PDF points (1/72 inch), as the renderer laid it out
#[derive(Debug, Clone)]
pub struct PageBox {
    /// 1-based page number
    pub page: usize,
    pub width: f32,
    pub height: f32,
}

/// One word and its bounding box, in points, origin at the top-left of the page
#[derive(Debug, Clone)]
pub struct Word {
    pub page: usize,
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// One rasterized page: the PNG bytes plus what is needed to map pixels back to points
#[derive(Debug, Clone)]
pub struct RasterPage {
    pub page: usize,
    pub png: Vec<u8>,
    /// Pixel size, for callers that map a point box onto the raster
    pub width_px: u32,
    pub height_px: u32,
    pub page_box: PageBox,
}

/// Number of pages, via `pdfinfo`
pub fn page_count(pdf: &Path) -> Result<usize, AppError> {
    let output = helpers::run_capture(
        Command::new("pdfinfo").arg(helpers::path_to_str(pdf)?),
        "pdfinfo",
        "pdfinfo failed",
    )?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            if let Ok(count) = rest.trim().parse::<usize>() {
                return Ok(count);
            }
        }
    }

    Err(AppError::ProcessFailed {
        message: "pdfinfo did not report a page count".to_string(),
        stderr: text.chars().take(500).collect(),
    })
}

/// Every word of the document with its box, via `pdftotext -bbox-layout`.
///
/// The output is XHTML, but pulling an XML crate in for four attributes is not worth it:
/// the scanner below extracts exactly the tags it knows and ignores everything else, so an
/// unexpected shape degrades into fewer words instead of a panic.
pub fn words(pdf: &Path) -> Result<(Vec<PageBox>, Vec<Word>), AppError> {
    let output = helpers::run_capture(
        Command::new("pdftotext")
            .arg("-bbox-layout")
            .arg(helpers::path_to_str(pdf)?)
            .arg("-"),
        "pdftotext",
        "pdftotext failed",
    )?;

    let xml = String::from_utf8_lossy(&output.stdout);
    let (boxes, words) = parse_bbox(&xml);

    if boxes.is_empty() {
        return Err(AppError::ProcessFailed {
            message: "pdftotext -bbox-layout reported no page".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(500)
                .collect(),
        });
    }

    Ok((boxes, words))
}

/// Rasterize a page range with `pdftoppm`.
///
/// The PNGs are produced inside a temporary directory that is removed when this function
/// returns, whatever the outcome: no stray image can survive an error.
pub fn rasterize(
    pdf: &Path,
    first: usize,
    last: usize,
    dpi: u32,
) -> Result<Vec<RasterPage>, AppError> {
    if first == 0 || last < first || dpi == 0 {
        return Err(AppError::BadRequest(format!(
            "Invalid raster range: pages {}..{} at {} dpi",
            first, last, dpi
        )));
    }

    let workdir = Builder::new().prefix("raster-").tempdir()?;
    let prefix = workdir.path().join("page");

    helpers::run_capture(
        Command::new("pdftoppm")
            .arg("-png")
            .arg("-f")
            .arg(first.to_string())
            .arg("-l")
            .arg(last.to_string())
            .arg("-r")
            .arg(dpi.to_string())
            .arg(helpers::path_to_str(pdf)?)
            .arg(helpers::path_to_str(&prefix)?),
        "pdftoppm",
        "pdftoppm failed",
    )?;

    // Page sizes are best effort: a missing pdfinfo only costs sub-point precision
    let boxes = page_boxes(pdf, first, last).unwrap_or_default();

    let mut pages: Vec<RasterPage> = Vec::new();
    for entry in fs::read_dir(workdir.path())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }

        // pdftoppm names its output <prefix>-<n>.png, zero-padded on some versions
        let number = trailing_number(&path).unwrap_or(first);
        let png = fs::read(&path)?;
        let (width_px, height_px) =
            png_dimensions(&png).ok_or_else(|| AppError::ProcessFailed {
                message: "pdftoppm produced a file that is not a PNG".to_string(),
                stderr: String::new(),
            })?;

        let page_box = boxes
            .iter()
            .find(|candidate| candidate.page == number)
            .cloned()
            .unwrap_or_else(|| PageBox {
                page: number,
                width: width_px as f32 * 72.0 / dpi as f32,
                height: height_px as f32 * 72.0 / dpi as f32,
            });

        pages.push(RasterPage {
            page: number,
            png,
            width_px,
            height_px,
            page_box,
        });
    }

    if pages.is_empty() {
        return Err(AppError::ProcessFailed {
            message: "PNG file not found after pdftoppm".to_string(),
            stderr: String::new(),
        });
    }

    pages.sort_by_key(|page| page.page);
    Ok(pages)
}

/// Rebuild a PDF whose pages are the given images, each at its exact size in points.
///
/// Used by flattened redaction: the text layer is gone because there is no text left, only
/// pixels. Images travel as `data:` URIs so the document does not depend on any path.
pub fn compose_pdf_from_pngs(pages: &[RasterPage]) -> Result<TempPath, AppError> {
    if pages.is_empty() {
        return Err(AppError::BadRequest(
            "No page to compose into a PDF".to_string(),
        ));
    }

    helpers::run_weasyprint_plain(&compose_html(pages))
}

/// One named `@page` per image: the name change is what forces the page break, and the
/// explicit `size` is what keeps every page at the exact geometry of the original.
fn compose_html(pages: &[RasterPage]) -> String {
    let mut styles = String::new();
    let mut body = String::new();

    for (index, page) in pages.iter().enumerate() {
        let name = format!("p{}", index + 1);
        let _ = write!(
            styles,
            "@page {name} {{ size: {width:.3}pt {height:.3}pt; margin: 0; }}\n.{name} {{ page: {name}; }}\n",
            name = name,
            width = page.page_box.width.max(1.0),
            height = page.page_box.height.max(1.0),
        );
        let _ = write!(
            body,
            "<div class=\"{}\"><img src=\"data:image/png;base64,{}\"></div>",
            name,
            base64(&page.png)
        );
    }

    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>\n\
         html, body {{ margin: 0; padding: 0; }}\n\
         div {{ margin: 0; padding: 0; }}\n\
         img {{ display: block; width: 100%; height: 100%; }}\n\
         {}</style></head><body>{}</body></html>",
        styles, body
    )
}

/// Scan the `-bbox-layout` XHTML for pages and words, ignoring every other tag
fn parse_bbox(xml: &str) -> (Vec<PageBox>, Vec<Word>) {
    let mut boxes: Vec<PageBox> = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut page = 0usize;
    let mut cursor = xml;

    while let Some(open) = cursor.find('<') {
        let after = &cursor[open + 1..];

        if let Some((attrs, rest)) = open_tag(after, "page") {
            page += 1;
            boxes.push(PageBox {
                page,
                width: attr_f32(attrs, "width").unwrap_or(0.0),
                height: attr_f32(attrs, "height").unwrap_or(0.0),
            });
            cursor = rest;
        } else if let Some((attrs, rest)) = open_tag(after, "word") {
            let (text, rest) = match rest.find("</word>") {
                Some(end) => (&rest[..end], &rest[end + "</word>".len()..]),
                None => (rest, ""),
            };
            words.push(Word {
                page: page.max(1),
                text: decode_entities(text),
                x0: attr_f32(attrs, "xMin").unwrap_or(0.0),
                y0: attr_f32(attrs, "yMin").unwrap_or(0.0),
                x1: attr_f32(attrs, "xMax").unwrap_or(0.0),
                y1: attr_f32(attrs, "yMax").unwrap_or(0.0),
            });
            cursor = rest;
        } else {
            cursor = after;
        }
    }

    (boxes, words)
}

/// Page sizes in points, via `pdfinfo -f -l`
fn page_boxes(pdf: &Path, first: usize, last: usize) -> Result<Vec<PageBox>, AppError> {
    let output = helpers::run_capture(
        Command::new("pdfinfo")
            .arg("-f")
            .arg(first.to_string())
            .arg("-l")
            .arg(last.to_string())
            .arg(helpers::path_to_str(pdf)?),
        "pdfinfo",
        "pdfinfo failed",
    )?;

    Ok(parse_page_boxes(
        &String::from_utf8_lossy(&output.stdout),
        first,
    ))
}

/// `pdfinfo` reports "Page    3 size: 595.276 x 841.89 pts (A4)", or "Page size: ..." when
/// it was not asked for a range.
fn parse_page_boxes(text: &str, first: usize) -> Vec<PageBox> {
    let mut boxes = Vec::new();

    for line in text.lines() {
        // "Page    3 size: 595.276 x 841.89 pts (A4)", or "Page size: ..." for a lone page
        let rest = match line.strip_prefix("Page") {
            Some(rest) => rest.trim(),
            None => continue,
        };

        let (number, rest) = match rest.strip_prefix("size:") {
            Some(rest) => (first, rest),
            None => {
                let mut parts = rest.splitn(2, "size:");
                let head = parts.next().unwrap_or_default().trim();
                let tail = match parts.next() {
                    Some(tail) => tail,
                    None => continue,
                };
                match head.parse::<usize>() {
                    Ok(number) => (number, tail),
                    Err(_) => continue,
                }
            }
        };

        let mut dimensions = rest.split('x');
        let width = dimensions
            .next()
            .and_then(|value| value.trim().parse::<f32>().ok());
        // The height is followed by the unit and the paper name: "841.89 pts (A4)"
        let height = dimensions.next().and_then(|value| {
            value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f32>().ok())
        });

        if let (Some(width), Some(height)) = (width, height) {
            boxes.push(PageBox {
                page: number,
                width,
                height,
            });
        }
    }

    boxes
}

/// Split `<name attrs>` once the leading `<` has been consumed
fn open_tag<'a>(after_lt: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let rest = after_lt.strip_prefix(name)?;
    if !rest.starts_with([' ', '\t', '\r', '\n', '>', '/']) {
        return None;
    }
    let end = rest.find('>')?;
    Some((&rest[..end], &rest[end + 1..]))
}

fn attr_f32(attrs: &str, name: &str) -> Option<f32> {
    let needle = format!("{}=\"", name);
    let start = attrs.find(&needle)? + needle.len();
    let rest = attrs.get(start..)?;
    let end = rest.find('"')?;
    rest.get(..end)?.trim().parse().ok()
}

/// pdftotext only emits the five predefined entities
fn decode_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Trailing index of a `<prefix>-<n>.png` file name
fn trailing_number(path: &Path) -> Option<usize> {
    path.file_stem()?.to_str()?.rsplit('-').next()?.parse().ok()
}

/// Width and height from the PNG IHDR chunk, without an image decoder
fn png_dimensions(png: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    if png.len() < 24 || png[..8] != SIGNATURE {
        return None;
    }

    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim sample of `pdftotext -bbox-layout`, the shape the scanner has to survive
    const BBOX: &str = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><html xmlns="http://www.w3.org/1999/xhtml">
<head>
<title>-</title>
<meta name="Creator" content="pandoc"/>
</head>
<body>
<doc>
  <page width="595.275591" height="841.889764">
    <flow>
      <block xMin="80.692913" yMin="128.213151" xMax="290.885050" yMax="154.612565">
        <line xMin="80.692913" yMin="128.213151" xMax="290.885050" yMax="154.612565">
          <word xMin="80.692913" yMin="128.213151" xMax="124.463142" yMax="154.612565">API</word>
          <word xMin="131.802179" yMin="128.213151" xMax="231.090376" yMax="154.612565">A &amp; B</word>
        </line>
      </block>
    </flow>
  </page>
  <page width="200.0" height="300.0">
    <flow><block><line><word xMin="1.0" yMin="2.0" xMax="3.0" yMax="4.0">second</word></line></block></flow>
  </page>
</doc>
</body>
</html>
"#;

    #[test]
    fn parses_pages_and_words() {
        let (boxes, words) = parse_bbox(BBOX);

        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].page, 1);
        assert!((boxes[0].width - 595.275_6).abs() < 0.01);
        assert!((boxes[1].height - 300.0).abs() < 0.01);

        assert_eq!(words.len(), 3);
        assert_eq!(words[0].text, "API");
        assert!((words[0].x0 - 80.692_91).abs() < 0.001);
        assert!((words[0].y1 - 154.612_56).abs() < 0.001);
        // Entities are decoded, and a word belongs to the page it was found in
        assert_eq!(words[1].text, "A & B");
        assert_eq!(words[2].page, 2);
    }

    #[test]
    fn unexpected_output_yields_no_page_instead_of_panicking() {
        assert_eq!(parse_bbox("").0.len(), 0);
        // An unterminated tag is not a tag: no page, which is what makes `words` fail loudly
        assert_eq!(parse_bbox("<page width=\"oops\"").0.len(), 0);
        assert_eq!(parse_bbox("<page width=\"1\" height=\"2\">").0.len(), 1);

        // A word without its closing tag still yields what could be read
        let (boxes, words) = parse_bbox("<word xMin=\"1.0\">dangling");
        assert!(boxes.is_empty());
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "dangling");
        assert_eq!(words[0].y1, 0.0);
    }

    #[test]
    fn parses_pdfinfo_page_sizes() {
        let text = "Pages:           2\nPage    1 size:  595.276 x 841.89 pts (A4)\nPage    1 rot:   0\nPage    2 size:  841.89 x 595.276 pts (A4)\n";
        let boxes = parse_page_boxes(text, 1);

        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[1].page, 2);
        assert!((boxes[1].width - 841.89).abs() < 0.01);
        assert!((boxes[1].height - 595.276).abs() < 0.01);
    }

    #[test]
    fn parses_pdfinfo_without_a_page_range() {
        let boxes = parse_page_boxes("Page size:      612 x 792 pts (letter)\n", 3);

        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].page, 3);
        assert!((boxes[0].width - 612.0).abs() < 0.01);
    }

    #[test]
    fn reads_png_geometry_from_the_header() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        png.extend_from_slice(&1240u32.to_be_bytes());
        png.extend_from_slice(&1754u32.to_be_bytes());

        assert_eq!(png_dimensions(&png), Some((1240, 1754)));
        assert_eq!(png_dimensions(b"not a png at all really"), None);
    }

    #[test]
    fn reads_the_page_index_pdftoppm_padded() {
        assert_eq!(trailing_number(Path::new("/tmp/x/page-7.png")), Some(7));
        assert_eq!(trailing_number(Path::new("/tmp/x/page-07.png")), Some(7));
        assert_eq!(trailing_number(Path::new("/tmp/x/page.png")), None);
    }

    #[test]
    fn composes_one_named_page_per_image() {
        let pages = vec![RasterPage {
            page: 1,
            png: vec![1, 2, 3],
            width_px: 100,
            height_px: 200,
            page_box: PageBox {
                page: 1,
                width: 595.276,
                height: 841.89,
            },
        }];

        let html = compose_html(&pages);
        assert!(html.contains("@page p1 { size: 595.276pt 841.890pt; margin: 0; }"));
        assert!(html.contains("class=\"p1\""));
        assert!(html.contains("data:image/png;base64,AQID"));
    }
}
