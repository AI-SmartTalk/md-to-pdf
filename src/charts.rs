//! Server-side chart rendering: a `chart` code block's JSON specification becomes an inline
//! `<svg>`.
//!
//! The SVG is emitted by hand rather than by a charting library because the only renderer that
//! matters here is WeasyPrint, whose SVG support is partial. Everything below is written against
//! what was verified to render in the production image: presentation attributes only (no CSS
//! sheet, no `<foreignObject>`), explicit `x`/`y`/`text-anchor` on every string (no
//! `dominant-baseline`, which WeasyPrint ignores), `<pattern>` tiles drawn as explicit paths (a
//! `patternTransform="rotate(45)"` renders as a crosshatch), and generic font families. The root
//! carries `width`, `height`, `viewBox` and `max-width:100%` — and deliberately not `height:auto`,
//! which makes WeasyPrint keep the declared height and letterbox the drawing inside it; with
//! `max-width` alone an over-wide chart scales down whole and never overflows the page. It also
//! carries `xml:space="preserve"`, because the markup travels through pandoc, which re-wraps long
//! raw-HTML lines at spaces: under the default whitespace rules a newline inside `<text>` is
//! *removed* rather than turned back into a space, and "Chiffre d'affaires" comes out as
//! "Chiffred'affaires". Escaping the spaces does not help — pandoc decodes character references.
//!
//! Two consequences of "this is a PDF that gets printed" drive the design. Text is measured, not
//! guessed: WeasyPrint's generic sans is DejaVu Sans, whose advance widths are tabulated below, so
//! a label that would not fit is truncated or dropped instead of overlapping its neighbour. And no
//! information is ever carried by colour alone — multi-series fills combine hue with a 45°/135°
//! hatch, lines combine hue with a marker shape, and values are labelled wherever they fit.
//!
//! Palette, mark specs and the series cap come from the shared data-visualisation standard
//! (categorical slots validated for colour-vision deficiency on a white surface).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;

use crate::types::AppError;

// ------------ Palette (categorical slots, validated against a white surface) ------------

const SURFACE: &str = "#ffffff";
const INK: &str = "#0b0b0b";
const INK_SECONDARY: &str = "#52514e";
const INK_MUTED: &str = "#898781";
const GRID: &str = "#e1e0d9";
const AXIS: &str = "#c3c2b7";

/// Fixed order: the ordering is the colour-vision-deficiency safety mechanism, not decoration.
/// Slots are assigned in sequence and never cycled.
const SERIES_COLORS: [&str; 8] = [
    "#2a78d6", // blue
    "#eb6834", // orange
    "#1baf7a", // aqua
    "#eda100", // yellow
    "#e87ba4", // magenta
    "#008300", // green
    "#4a3aa7", // violet
    "#e34948", // red
];

/// Past eight the ninth hue would be indistinguishable from an existing one under CVD, so the
/// caller is told to fold the tail instead of the palette silently cycling.
const MAX_SERIES: usize = SERIES_COLORS.len();
/// A pie past this many slices stops being readable at a glance; the tail is folded into one.
const MAX_SLICES: usize = 8;
const MAX_POINTS: usize = 2000;

// ------------ Type sizes and spacing ------------

const PAD: f64 = 16.0;
const TITLE_SIZE: f64 = 16.0;
const SUBTITLE_SIZE: f64 = 12.0;
const AXIS_SIZE: f64 = 11.0;
const LEGEND_SIZE: f64 = 11.0;
const VALUE_SIZE: f64 = 10.0;
/// Bars never fill their band: the leftover is the air that keeps the chart quiet.
const MAX_BAR_THICKNESS: f64 = 24.0;
/// White doing the separating, between stacked segments and between grouped bars alike.
const GAP: f64 = 2.0;
const LINE_WIDTH: f64 = 2.0;
const MARKER_R: f64 = 4.0;
const ROT_ANGLE_SIN: f64 = 0.573_576_4; // sin(35°)
const ROT_ANGLE_COS: f64 = 0.819_152_0; // cos(35°)

// ------------ Font metrics (DejaVu Sans, the generic sans of the production image) ------------

/// Advance widths in em for U+0020..U+007E, read from the font shipped in the image.
#[rustfmt::skip]
const W_REGULAR: [f64; 95] = [
    0.3179, 0.4009, 0.4600, 0.8379, 0.6362, 0.9502, 0.7798, 0.2749, 0.3901, 0.3901,
    0.5000, 0.8379, 0.3179, 0.3608, 0.3179, 0.3369, 0.6362, 0.6362, 0.6362, 0.6362,
    0.6362, 0.6362, 0.6362, 0.6362, 0.6362, 0.6362, 0.3369, 0.3369, 0.8379, 0.8379,
    0.8379, 0.5308, 1.0000, 0.6841, 0.6860, 0.6982, 0.7700, 0.6318, 0.5752, 0.7749,
    0.7520, 0.2949, 0.2949, 0.6558, 0.5571, 0.8628, 0.7480, 0.7871, 0.6030, 0.7871,
    0.6948, 0.6348, 0.6108, 0.7319, 0.6841, 0.9888, 0.6851, 0.6108, 0.6851, 0.3901,
    0.3369, 0.3901, 0.8379, 0.5000, 0.5000, 0.6128, 0.6348, 0.5498, 0.6348, 0.6152,
    0.3521, 0.6348, 0.6338, 0.2778, 0.2778, 0.5791, 0.2778, 0.9741, 0.6338, 0.6118,
    0.6348, 0.6348, 0.4111, 0.5210, 0.3921, 0.6338, 0.5918, 0.8179, 0.5918, 0.5918,
    0.5249, 0.6362, 0.3369, 0.6362, 0.8379,
];

#[rustfmt::skip]
const W_BOLD: [f64; 95] = [
    0.3481, 0.4561, 0.5210, 0.8379, 0.6958, 1.0020, 0.8721, 0.3062, 0.4570, 0.4570,
    0.5229, 0.8379, 0.3799, 0.4150, 0.3799, 0.3652, 0.6958, 0.6958, 0.6958, 0.6958,
    0.6958, 0.6958, 0.6958, 0.6958, 0.6958, 0.6958, 0.3999, 0.3999, 0.8379, 0.8379,
    0.8379, 0.5801, 1.0000, 0.7739, 0.7622, 0.7339, 0.8301, 0.6831, 0.6831, 0.8208,
    0.8369, 0.3721, 0.3721, 0.7749, 0.6372, 0.9951, 0.8369, 0.8501, 0.7329, 0.8501,
    0.7700, 0.7202, 0.6821, 0.8120, 0.7739, 1.1030, 0.7710, 0.7241, 0.7251, 0.4570,
    0.3652, 0.4570, 0.8379, 0.5000, 0.5000, 0.6748, 0.7158, 0.5928, 0.7158, 0.6782,
    0.4351, 0.7158, 0.7119, 0.3428, 0.3428, 0.6650, 0.3428, 1.0420, 0.7119, 0.6870,
    0.7158, 0.7158, 0.4932, 0.5952, 0.4780, 0.7119, 0.6519, 0.9238, 0.6450, 0.6519,
    0.5820, 0.7119, 0.3652, 0.7119, 0.8379,
];

fn char_width(c: char, bold: bool) -> f64 {
    let u = c as u32;
    if (0x20..0x7F).contains(&u) {
        let idx = (u - 0x20) as usize;
        if bold {
            W_BOLD[idx]
        } else {
            W_REGULAR[idx]
        }
    } else if u < 0x20 {
        0.0
    } else if is_wide(u) {
        1.0
    } else if bold {
        0.70
    } else {
        0.62
    }
}

/// Ideographic and fullwidth ranges advance roughly one em; everything else in Latin-1 and its
/// neighbours sits close to the generic fallback above.
fn is_wide(u: u32) -> bool {
    (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
}

fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    s.chars().map(|c| char_width(c, bold)).sum::<f64>() * size
}

/// Truncate to an ellipsis rather than let a label run under its neighbour or off the viewBox.
fn truncate_to(s: &str, max_width: f64, size: f64, bold: bool) -> String {
    if text_width(s, size, bold) <= max_width {
        return s.to_string();
    }
    let ell = char_width('\u{2026}', bold) * size;
    let budget = max_width - ell;
    if budget <= 0.0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let cw = char_width(c, bold) * size;
        if w + cw > budget {
            break;
        }
        w += cw;
        out.push(c);
    }
    while out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        return String::new();
    }
    out.push('\u{2026}');
    out
}

// ------------ Escaping ------------

/// Everything user-supplied goes through this. Non-ASCII becomes a numeric character reference so
/// the SVG survives being embedded in a document of any declared encoding.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {}
            c if c.is_ascii() => out.push(c),
            c => {
                let _ = write!(out, "&#{};", c as u32);
            }
        }
    }
    out
}

// ------------ Number formatting ------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueFormat {
    Plain,
    Compact,
    Percent,
    Eur,
}

impl ValueFormat {
    fn parse(s: &str) -> Option<ValueFormat> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plain" | "number" => Some(ValueFormat::Plain),
            "compact" | "short" => Some(ValueFormat::Compact),
            "percent" | "percentage" | "%" => Some(ValueFormat::Percent),
            "eur" | "euro" | "euros" | "currency" => Some(ValueFormat::Eur),
            _ => None,
        }
    }

    fn format(self, v: f64) -> String {
        if !v.is_finite() {
            return String::new();
        }
        match self {
            ValueFormat::Plain => group(&trim_decimals(v, 2), ','),
            ValueFormat::Compact => compact(v),
            ValueFormat::Percent => format!("{}%", trim_decimals(v, 1)),
            // Euro amounts follow French typography: the thousands separator and the space before
            // the sign are both non-breaking so the amount never wraps mid-number.
            ValueFormat::Eur => format!("{}\u{a0}\u{20ac}", group(&trim_decimals(v, 2), '\u{a0}')),
        }
    }
}

fn trim_decimals(v: f64, decimals: usize) -> String {
    let mut s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

fn group(s: &str, sep: char) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let (int, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    let digits: Vec<char> = int.chars().collect();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(sep);
        }
        grouped.push(*c);
    }
    let mut out = format!("{sign}{grouped}");
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

fn compact(v: f64) -> String {
    let a = v.abs();
    let (div, suffix) = if a >= 1e12 {
        (1e12, "T")
    } else if a >= 1e9 {
        (1e9, "B")
    } else if a >= 1e6 {
        (1e6, "M")
    } else if a >= 1e3 {
        (1e3, "K")
    } else {
        return trim_decimals(v, 2);
    };
    let scaled = v / div;
    let decimals = if scaled.abs() < 10.0 { 1 } else { 0 };
    format!("{}{}", trim_decimals(scaled, decimals), suffix)
}

// ------------ Chart model ------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Bar,
    HBar,
    StackedBar,
    GroupedBar,
    Line,
    Area,
    Pie,
    Donut,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        let norm: String = s
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c == '_' || c == ' ' { '-' } else { c })
            .collect();
        match norm.as_str() {
            "bar" | "column" | "columns" | "vbar" => Some(Kind::Bar),
            "hbar" | "horizontal-bar" | "barh" | "row" => Some(Kind::HBar),
            "stacked-bar" | "stacked" | "stack" => Some(Kind::StackedBar),
            "grouped-bar" | "grouped" | "group" | "clustered-bar" => Some(Kind::GroupedBar),
            "line" | "lines" => Some(Kind::Line),
            "area" => Some(Kind::Area),
            "pie" => Some(Kind::Pie),
            "donut" | "doughnut" => Some(Kind::Donut),
            _ => None,
        }
    }

    fn is_round(self) -> bool {
        matches!(self, Kind::Pie | Kind::Donut)
    }

    fn is_line_like(self) -> bool {
        matches!(self, Kind::Line | Kind::Area)
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Bar => "Column chart",
            Kind::HBar => "Bar chart",
            Kind::StackedBar => "Stacked bar chart",
            Kind::GroupedBar => "Grouped bar chart",
            Kind::Line => "Line chart",
            Kind::Area => "Area chart",
            Kind::Pie => "Pie chart",
            Kind::Donut => "Donut chart",
        }
    }
}

struct Series {
    name: String,
    data: Vec<Option<f64>>,
    color: String,
    slot: usize,
}

struct Chart {
    kind: Kind,
    title: Option<String>,
    subtitle: Option<String>,
    labels: Vec<String>,
    series: Vec<Series>,
    x_label: Option<String>,
    y_label: Option<String>,
    fmt: ValueFormat,
    width: f64,
    height: f64,
    legend: Option<bool>,
    grid: bool,
    texture: bool,
    /// Suffix that makes `<defs>` ids unique when several charts land in one document.
    uid: String,
}

impl Chart {
    fn points(&self) -> usize {
        self.series.iter().map(|s| s.data.len()).max().unwrap_or(0)
    }

    fn show_legend(&self) -> bool {
        self.legend.unwrap_or(self.series.len() > 1)
    }

    /// Hue alone must not carry identity on paper: a second channel comes in as soon as there is
    /// more than one series to tell apart.
    fn patterned(&self) -> bool {
        self.texture && self.series.len() > 1
    }
}

// ------------ Public entry point ------------

/// Turn a chart specification (JSON: type, labels, series, options) into an inline `<svg>`
/// element. No JS runtime and no headless browser: the SVG is emitted by hand.
pub fn render_chart(spec_json: &str) -> Result<String, AppError> {
    let chart = parse_spec(spec_json)?;
    Ok(draw(&chart))
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::BadRequest(format!("chart: {}", msg.into()))
}

// ------------ Specification parsing ------------

fn parse_spec(spec_json: &str) -> Result<Chart, AppError> {
    let root: Value = serde_json::from_str(spec_json.trim()).map_err(|e| {
        bad(format!(
            "invalid JSON ({e}); expected an object with \"type\" and \"series\""
        ))
    })?;
    let obj = root
        .as_object()
        .ok_or_else(|| bad("expected a JSON object with \"type\" and \"series\""))?;

    let kind_raw = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("field \"type\": required (bar, hbar, stacked-bar, grouped-bar, line, area, pie, donut)"))?;
    let mut kind = Kind::parse(kind_raw).ok_or_else(|| {
        bad(format!(
            "field \"type\": unknown chart type {kind_raw:?} (expected bar, hbar, stacked-bar, grouped-bar, line, area, pie, donut)"
        ))
    })?;

    let title = opt_text(obj, "title")?;
    let subtitle = opt_text(obj, "subtitle")?;
    let x_label = opt_text(obj, "x_label")?;
    let y_label = opt_text(obj, "y_label")?;

    let fmt = match obj.get("value_format") {
        None | Some(Value::Null) => ValueFormat::Compact,
        Some(Value::String(s)) => ValueFormat::parse(s).ok_or_else(|| {
            bad(format!(
                "field \"value_format\": unknown format {s:?} (expected compact, percent, eur, plain)"
            ))
        })?,
        Some(_) => return Err(bad("field \"value_format\": expected a string")),
    };

    let width = opt_dimension(obj, "width", 640.0, 240.0, 2000.0)?;
    let height = opt_dimension(obj, "height", 360.0, 160.0, 2000.0)?;
    let legend = opt_bool(obj, "legend")?;
    let grid = opt_bool(obj, "grid")?.unwrap_or(true);
    let texture = opt_bool(obj, "texture")?.unwrap_or(true);

    let mut series = parse_series(obj)?;
    let labels = parse_labels(obj)?;

    // A `bar` with several series is unambiguous: it is a grouped bar chart.
    if kind == Kind::Bar && series.len() > 1 {
        kind = Kind::GroupedBar;
    }
    // Pie and donut show one distribution; extra series have no place to go.
    if kind.is_round() {
        series.truncate(1);
    }

    let longest = series.iter().map(|s| s.data.len()).max().unwrap_or(0);
    if longest == 0 {
        return Err(bad(
            "field \"series[0].data\": empty; at least one value is required",
        ));
    }
    if longest > MAX_POINTS {
        return Err(bad(format!(
            "field \"series[0].data\": {longest} points exceeds the {MAX_POINTS}-point limit; aggregate the data first"
        )));
    }

    let mut labels = normalize_labels(labels, longest);

    if kind.is_round() {
        fold_tail(&mut series, &mut labels);
    }

    let mut chart = Chart {
        kind,
        title,
        subtitle,
        labels,
        series,
        x_label,
        y_label,
        fmt,
        width,
        height,
        legend,
        grid,
        texture,
        uid: uid_for(spec_json),
    };
    // Slices are the categories on a pie: identity is per slice, not per series.
    if chart.kind.is_round() {
        chart.legend = chart.legend.or(None);
    }
    Ok(chart)
}

fn opt_text(obj: &serde_json::Map<String, Value>, field: &str) -> Result<Option<String>, AppError> {
    match obj.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let t = s.trim();
            Ok(if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            })
        }
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(bad(format!("field {field:?}: expected a string"))),
    }
}

fn opt_bool(obj: &serde_json::Map<String, Value>, field: &str) -> Result<Option<bool>, AppError> {
    match obj.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(format!("field {field:?}: expected true or false"))),
    }
}

fn opt_dimension(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    default: f64,
    min: f64,
    max: f64,
) -> Result<f64, AppError> {
    match obj.get(field) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| bad(format!("field {field:?}: expected a number of pixels")))?;
            if n < min || n > max {
                return Err(bad(format!(
                    "field {field:?}: {} is out of range (expected {} to {} pixels)",
                    trim_decimals(n, 2),
                    trim_decimals(min, 0),
                    trim_decimals(max, 0)
                )));
            }
            Ok(n)
        }
    }
}

fn parse_series(obj: &serde_json::Map<String, Value>) -> Result<Vec<Series>, AppError> {
    let raw = obj
        .get("series")
        .ok_or_else(|| bad("field \"series\": required (an array of {name, data} objects)"))?;
    let arr = raw
        .as_array()
        .ok_or_else(|| bad("field \"series\": expected an array of {name, data} objects"))?;
    if arr.is_empty() {
        return Err(bad(
            "field \"series\": empty; at least one series is required",
        ));
    }
    if arr.len() > MAX_SERIES {
        return Err(bad(format!(
            "field \"series\": {} series exceeds the {MAX_SERIES}-colour palette; fold the tail into an \"Other\" series or split the chart",
            arr.len()
        )));
    }

    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let so = item.as_object().ok_or_else(|| {
            bad(format!(
                "field \"series[{i}]\": expected an object with \"data\""
            ))
        })?;
        let data_raw = so
            .get("data")
            .ok_or_else(|| bad(format!("field \"series[{i}].data\": required")))?
            .as_array()
            .ok_or_else(|| {
                bad(format!(
                    "field \"series[{i}].data\": expected an array of numbers"
                ))
            })?;

        let mut data = Vec::with_capacity(data_raw.len());
        for (j, v) in data_raw.iter().enumerate() {
            data.push(parse_datum(v, i, j)?);
        }

        let name = opt_text(so, "name")?.unwrap_or_else(|| format!("Series {}", i + 1));
        let color = match so.get("color") {
            None | Some(Value::Null) => SERIES_COLORS[i].to_string(),
            Some(Value::String(s)) => parse_color(s).ok_or_else(|| {
                bad(format!(
                    "field \"series[{i}].color\": {s:?} is not a hex colour such as \"#3366cc\""
                ))
            })?,
            Some(_) => {
                return Err(bad(format!(
                    "field \"series[{i}].color\": expected a hex colour string such as \"#3366cc\""
                )))
            }
        };
        out.push(Series {
            name,
            data,
            color,
            slot: i,
        });
    }
    Ok(out)
}

/// Numbers, nulls and numeric strings are all accepted: generated specifications routinely quote
/// their figures. Anything else names its own index in the error.
fn parse_datum(v: &Value, series: usize, index: usize) -> Result<Option<f64>, AppError> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() {
                Ok(Some(f))
            } else {
                Err(bad(format!(
                    "field \"series[{series}].data[{index}]\": {n} is not a finite number"
                )))
            }
        }
        Value::String(s) => {
            let cleaned: String = s
                .chars()
                .filter(|c| {
                    !matches!(
                        c,
                        ' ' | '\u{a0}' | '\u{202f}' | ',' | '%' | '\u{20ac}' | '$'
                    )
                })
                .collect();
            if cleaned.is_empty() {
                return Ok(None);
            }
            cleaned
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite())
                .map(Some)
                .ok_or_else(|| {
                    bad(format!(
                        "field \"series[{series}].data[{index}]\": {s:?} is not a number"
                    ))
                })
        }
        _ => Err(bad(format!(
            "field \"series[{series}].data[{index}]\": expected a number or null"
        ))),
    }
}

/// Only hex is accepted: the value is interpolated straight into an SVG attribute, and a closed
/// syntax is what keeps that safe.
fn parse_color(s: &str) -> Option<String> {
    let t = s.trim();
    let hex = t.strip_prefix('#')?;
    if !matches!(hex.len(), 3 | 6) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if hex.len() == 3 {
        let mut out = String::from("#");
        for c in hex.chars() {
            out.push(c);
            out.push(c);
        }
        Some(out.to_ascii_lowercase())
    } else {
        Some(format!("#{}", hex.to_ascii_lowercase()))
    }
}

fn parse_labels(obj: &serde_json::Map<String, Value>) -> Result<Vec<String>, AppError> {
    match obj.get("labels") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .enumerate()
            .map(|(i, v)| match v {
                Value::String(s) => Ok(s.trim().to_string()),
                Value::Number(n) => Ok(n.to_string()),
                Value::Bool(b) => Ok(b.to_string()),
                Value::Null => Ok(String::new()),
                _ => Err(bad(format!(
                    "field \"labels[{i}]\": expected a string or a number"
                ))),
            })
            .collect(),
        Some(_) => Err(bad("field \"labels\": expected an array of strings")),
    }
}

/// Series of different lengths and a short label list are normal in generated content: the
/// category axis is as long as the longest series, and missing labels fall back to their rank.
fn normalize_labels(mut labels: Vec<String>, n: usize) -> Vec<String> {
    labels.truncate(n);
    for i in labels.len()..n {
        labels.push((i + 1).to_string());
    }
    for (i, l) in labels.iter_mut().enumerate() {
        if l.is_empty() {
            *l = (i + 1).to_string();
        }
    }
    labels
}

/// A pie past `MAX_SLICES` cannot be told apart slice by slice, so the smallest ones become one
/// segment rather than the palette inventing hues.
fn fold_tail(series: &mut [Series], labels: &mut Vec<String>) {
    let Some(s) = series.first_mut() else { return };
    if s.data.len() <= MAX_SLICES {
        return;
    }
    let mut order: Vec<usize> = (0..s.data.len()).collect();
    order.sort_by(|a, b| {
        let va = s.data[*a].unwrap_or(0.0).abs();
        let vb = s.data[*b].unwrap_or(0.0).abs();
        vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
    });
    let keep: BTreeSet<usize> = order.iter().take(MAX_SLICES - 1).copied().collect();
    let mut data = Vec::with_capacity(MAX_SLICES);
    let mut kept_labels = Vec::with_capacity(MAX_SLICES);
    let mut rest = 0.0;
    for (i, v) in s.data.iter().enumerate() {
        if keep.contains(&i) {
            data.push(*v);
            kept_labels.push(labels.get(i).cloned().unwrap_or_default());
        } else {
            rest += v.unwrap_or(0.0);
        }
    }
    data.push(Some(rest));
    kept_labels.push("Other".to_string());
    s.data = data;
    *labels = kept_labels;
}

/// FNV-1a over the specification: stable across runs (so the cache key of a rendered document
/// stays stable) and different for two charts in the same document.
fn uid_for(spec: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in spec.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:x}", h & 0xffff_ffff)
}

// ------------ Scales ------------

struct Scale {
    lo: f64,
    hi: f64,
    step: f64,
}

impl Scale {
    fn ticks(&self) -> Vec<f64> {
        let mut out = Vec::new();
        let count = ((self.hi - self.lo) / self.step).round() as i64;
        for i in 0..=count.clamp(0, 64) {
            out.push(self.lo + self.step * i as f64);
        }
        if out.is_empty() {
            out.push(self.lo);
        }
        out
    }

    /// Fraction of the axis, 0 at `lo` and 1 at `hi`.
    fn frac(&self, v: f64) -> f64 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON {
            0.0
        } else {
            ((v - self.lo) / span).clamp(-1.0, 2.0)
        }
    }
}

/// Round graduations only: 1, 2 or 5 times a power of ten. Handles a constant series, a single
/// value and a domain that straddles zero.
fn nice_scale(min: f64, max: f64, target: usize) -> Scale {
    let target = target.max(2) as f64;
    let (mut min, mut max) = (min, max);
    if !min.is_finite() || !max.is_finite() {
        min = 0.0;
        max = 1.0;
    }
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    if (max - min).abs() < 1e-12 {
        // A constant series still deserves a readable axis rather than a zero-height plot.
        if min > 0.0 {
            min = 0.0;
        } else if min < 0.0 {
            max = 0.0;
        } else {
            max = 1.0;
        }
    }
    if (max - min).abs() < 1e-12 {
        max = min + 1.0;
    }

    let raw = (max - min) / target;
    let mag = 10f64.powf(raw.abs().log10().floor());
    let norm = if mag > 0.0 && mag.is_finite() {
        raw / mag
    } else {
        1.0
    };
    let mut step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    } * mag;
    if !step.is_finite() || step <= 0.0 {
        step = (max - min).max(1e-9) / target;
    }

    let mut lo = (min / step).floor() * step;
    let mut hi = (max / step).ceil() * step;
    let mut guard = 0;
    while (hi - lo) / step > 32.0 && guard < 32 {
        step *= 2.0;
        lo = (min / step).floor() * step;
        hi = (max / step).ceil() * step;
        guard += 1;
    }
    if (hi - lo).abs() < 1e-12 {
        hi = lo + step;
    }
    Scale { lo, hi, step }
}

/// Value range of the chart, per type: anything with an area mark is measured from zero.
fn value_range(chart: &Chart) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    if chart.kind == Kind::StackedBar {
        for i in 0..chart.points() {
            let (mut pos, mut neg) = (0.0, 0.0);
            for s in &chart.series {
                match s.data.get(i).copied().flatten() {
                    Some(v) if v >= 0.0 => pos += v,
                    Some(v) => neg += v,
                    None => {}
                }
            }
            lo = lo.min(neg);
            hi = hi.max(pos);
        }
    } else {
        for s in &chart.series {
            for v in s.data.iter().flatten() {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    // Bars and areas encode magnitude by length, which is only honest against a zero baseline.
    // A line may sit on a truncated axis, which is what makes small variations readable.
    if chart.kind != Kind::Line {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    (lo, hi)
}

// ------------ SVG primitives ------------

/// Two decimals is below a printer's resolution and keeps the markup small and deterministic.
fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let r = (v * 100.0).round() / 100.0;
    if r == 0.0 {
        "0".to_string()
    } else {
        trim_decimals(r, 2)
    }
}

struct Canvas {
    out: String,
}

impl Canvas {
    fn new() -> Canvas {
        Canvas {
            out: String::with_capacity(4096),
        }
    }

    fn raw(&mut self, s: &str) {
        self.out.push_str(s);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f64, y: f64, anchor: &str, size: f64, bold: bool, fill: &str, s: &str) {
        if s.is_empty() {
            return;
        }
        let weight = if bold { " font-weight=\"600\"" } else { "" };
        let _ = write!(
            self.out,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" font-size=\"{}\"{} fill=\"{}\">{}</text>",
            n(x),
            n(y),
            anchor,
            n(size),
            weight,
            fill,
            esc(s)
        );
    }

    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str, w: f64) {
        let _ = write!(
            self.out,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
            n(x1),
            n(y1),
            n(x2),
            n(y2),
            stroke,
            n(w)
        );
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, fill: &str) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let _ = write!(
            self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            n(x),
            n(y),
            n(w),
            n(h),
            fill
        );
    }

    fn path(&mut self, d: &str, fill: &str) {
        if d.is_empty() {
            return;
        }
        let _ = write!(self.out, "<path d=\"{d}\" fill=\"{fill}\"/>");
    }
}

/// Which end of a bar carries the 4px radius. The baseline end stays square so the mark reads as
/// growing from the axis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RoundEnd {
    Top,
    Bottom,
    Right,
    Left,
    None,
}

fn bar_path(x: f64, y: f64, w: f64, h: f64, radius: f64, end: RoundEnd) -> String {
    if w <= 0.0 || h <= 0.0 {
        return String::new();
    }
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.4 || end == RoundEnd::None {
        return format!("M{} {}H{}V{}H{}Z", n(x), n(y), n(x + w), n(y + h), n(x));
    }
    let (x1, y1) = (x + w, y + h);
    match end {
        RoundEnd::Top => format!(
            "M{} {}V{}A{} {} 0 0 1 {} {}H{}A{} {} 0 0 1 {} {}V{}Z",
            n(x),
            n(y1),
            n(y + r),
            n(r),
            n(r),
            n(x + r),
            n(y),
            n(x1 - r),
            n(r),
            n(r),
            n(x1),
            n(y + r),
            n(y1)
        ),
        RoundEnd::Bottom => format!(
            "M{} {}V{}A{} {} 0 0 0 {} {}H{}A{} {} 0 0 0 {} {}V{}Z",
            n(x),
            n(y),
            n(y1 - r),
            n(r),
            n(r),
            n(x + r),
            n(y1),
            n(x1 - r),
            n(r),
            n(r),
            n(x1),
            n(y1 - r),
            n(y)
        ),
        RoundEnd::Right => format!(
            "M{} {}H{}A{} {} 0 0 1 {} {}V{}A{} {} 0 0 1 {} {}H{}Z",
            n(x),
            n(y),
            n(x1 - r),
            n(r),
            n(r),
            n(x1),
            n(y + r),
            n(y1 - r),
            n(r),
            n(r),
            n(x1 - r),
            n(y1),
            n(x)
        ),
        RoundEnd::Left => format!(
            "M{} {}H{}V{}H{}A{} {} 0 0 1 {} {}V{}A{} {} 0 0 1 {} {}Z",
            n(x + r),
            n(y),
            n(x1),
            n(y1),
            n(x + r),
            n(r),
            n(r),
            n(x),
            n(y1 - r),
            n(y + r),
            n(r),
            n(r),
            n(x + r),
            n(y)
        ),
        RoundEnd::None => String::new(),
    }
}

// ------------ Texture (the print / greyscale channel) ------------

fn darken(hex: &str, factor: f64) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return "#000000".to_string();
    }
    let mut out = String::from("#");
    for i in 0..3 {
        let v = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0) as f64;
        let _ = write!(out, "{:02x}", (v * factor).round().clamp(0.0, 255.0) as u8);
    }
    out
}

fn relative_luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return 0.0;
    }
    let ch = |i: usize| {
        let v = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap_or(0) as f64 / 255.0;
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(0) + 0.7152 * ch(1) + 0.0722 * ch(2)
}

/// Slot 0 stays solid, slots 1 and 2 take the 45° and 135° hatch, and the cycle repeats — so two
/// marks that touch always differ in texture as well as in hue.
fn pattern_id(chart: &Chart, slot: usize) -> Option<String> {
    if !chart.patterned() || slot.is_multiple_of(3) {
        return None;
    }
    Some(format!("mtp{}-t{}", chart.uid, slot))
}

fn fill_for(chart: &Chart, s: &Series) -> String {
    match pattern_id(chart, s.slot) {
        Some(id) => format!("url(#{id})"),
        None => s.color.clone(),
    }
}

fn defs(chart: &Chart) -> String {
    if !chart.patterned() {
        return String::new();
    }
    let mut out = String::from("<defs>");
    for s in &chart.series {
        let Some(id) = pattern_id(chart, s.slot) else {
            continue;
        };
        let ink = darken(&s.color, 0.55);
        // The tile draws its own diagonals: WeasyPrint renders `patternTransform="rotate(45)"` as
        // a crosshatch, so the angle lives in the path instead.
        let d = if s.slot % 3 == 1 {
            "M-2 2L2 -2M0 8L8 0M6 10L10 6"
        } else {
            "M-2 6L2 10M0 0L8 8M6 -2L10 2"
        };
        let _ = write!(
            out,
            "<pattern id=\"{id}\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\">\
             <rect width=\"8\" height=\"8\" fill=\"{}\"/>\
             <path d=\"{d}\" stroke=\"{ink}\" stroke-width=\"1.5\" fill=\"none\"/></pattern>",
            s.color
        );
    }
    out.push_str("</defs>");
    out
}

// ------------ Markers (the line-chart identity channel) ------------

fn marker(cx: f64, cy: f64, slot: usize, color: &str) -> String {
    let r = MARKER_R;
    let ring = format!(" stroke=\"{SURFACE}\" stroke-width=\"2\"");
    match slot % 8 {
        0 => format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{color}\"{ring}/>",
            n(cx),
            n(cy),
            n(r)
        ),
        1 => polygon(&regular_polygon(cx, cy, r * 1.15, 4, 45.0), color, &ring),
        2 => polygon(&regular_polygon(cx, cy, r * 1.2, 3, 0.0), color, &ring),
        3 => polygon(&regular_polygon(cx, cy, r * 1.25, 4, 0.0), color, &ring),
        4 => polygon(&regular_polygon(cx, cy, r * 1.2, 3, 180.0), color, &ring),
        5 => polygon(&regular_polygon(cx, cy, r * 1.15, 5, 0.0), color, &ring),
        6 => polygon(&regular_polygon(cx, cy, r * 1.15, 6, 30.0), color, &ring),
        _ => polygon(&star(cx, cy, r * 1.35), color, &ring),
    }
}

/// `rotation` is measured from straight up, clockwise, in degrees.
fn regular_polygon(cx: f64, cy: f64, r: f64, sides: usize, rotation: f64) -> Vec<(f64, f64)> {
    let start = -std::f64::consts::FRAC_PI_2 + rotation.to_radians();
    (0..sides)
        .map(|i| {
            let a = start + std::f64::consts::TAU * i as f64 / sides as f64;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn star(cx: f64, cy: f64, r: f64) -> Vec<(f64, f64)> {
    let start = -std::f64::consts::FRAC_PI_2;
    (0..10)
        .map(|i| {
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            let a = start + std::f64::consts::PI * i as f64 / 5.0;
            (cx + rr * a.cos(), cy + rr * a.sin())
        })
        .collect()
}

fn polygon(points: &[(f64, f64)], fill: &str, extra: &str) -> String {
    let pts: Vec<String> = points
        .iter()
        .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
        .collect();
    format!(
        "<polygon points=\"{}\" fill=\"{fill}\"{extra}/>",
        pts.join(" ")
    )
}

// ------------ Category axis planning ------------

struct CatAxis {
    labels: Vec<String>,
    every: usize,
    rotate: bool,
    band: f64,
}

/// Decides how the category labels fit: straight, thinned, or rotated 35° — and truncates them to
/// the room actually available rather than letting them run off the viewBox. `first_anchor` is the
/// x of the leftmost label's anchor: a rotated label trails down and to the left of it, so it is
/// what bounds the length of the first label.
fn plan_cat_axis(labels: &[String], band: f64, max_band_height: f64, first_anchor: f64) -> CatAxis {
    let size = AXIS_SIZE;
    let widest = labels
        .iter()
        .map(|l| text_width(l, size, false))
        .fold(0.0_f64, f64::max);

    if widest + 6.0 <= band {
        return CatAxis {
            labels: labels.to_vec(),
            every: 1,
            rotate: false,
            band: size + 9.0,
        };
    }

    // Rotated labels need roughly one line height of horizontal room measured along the axis.
    let needed = size * 1.05 / ROT_ANGLE_SIN;
    let every = if band <= 0.0 {
        labels.len().max(1)
    } else {
        (needed / band).ceil().max(1.0) as usize
    };
    let allowed_height = (max_band_height - 10.0).max(size + 4.0);
    let allowed_width =
        (allowed_height / ROT_ANGLE_SIN).min((first_anchor - 2.0).max(24.0) / ROT_ANGLE_COS);
    let shown: Vec<String> = labels
        .iter()
        .map(|l| truncate_to(l, allowed_width, size, false))
        .collect();
    let widest_shown = shown
        .iter()
        .enumerate()
        .filter(|(i, _)| i % every == 0)
        .map(|(_, l)| text_width(l, size, false))
        .fold(0.0_f64, f64::max);
    CatAxis {
        labels: shown,
        every,
        rotate: true,
        band: widest_shown * ROT_ANGLE_SIN + 12.0,
    }
}

// ------------ Layout & drawing ------------

struct Header {
    /// First y available to the plot.
    top: f64,
    markup: String,
}

fn draw(chart: &Chart) -> String {
    let mut out = String::with_capacity(8192);
    let (w, h) = (chart.width, chart.height);
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" \
         role=\"img\" style=\"max-width:100%\" font-family=\"sans-serif\" xml:space=\"preserve\">",
        n(w),
        n(h),
        n(w),
        n(h)
    );
    let _ = write!(
        out,
        "<title>{}</title><desc>{}</desc>",
        esc(chart.title.as_deref().unwrap_or(chart.kind.label())),
        esc(&describe(chart))
    );
    out.push_str(&defs(chart));
    let _ = write!(
        out,
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{SURFACE}\"/>",
        n(w),
        n(h)
    );

    let header = header_block(chart);
    out.push_str(&header.markup);

    let body = match chart.kind {
        Kind::Pie | Kind::Donut => draw_pie(chart, header.top),
        Kind::HBar => draw_hbar(chart, header.top),
        _ => draw_cartesian(chart, header.top),
    };
    out.push_str(&body);
    out.push_str("</svg>");
    out
}

fn describe(chart: &Chart) -> String {
    let mut d = String::from(chart.kind.label());
    if chart.kind.is_round() {
        let _ = write!(
            d,
            ", {} slices: {}",
            chart.labels.len(),
            chart.labels.join(", ")
        );
    } else {
        let names: Vec<&str> = chart.series.iter().map(|s| s.name.as_str()).collect();
        let _ = write!(
            d,
            ", {} categories, {} series: {}",
            chart.points(),
            chart.series.len(),
            names.join(", ")
        );
        let (lo, hi) = value_range(chart);
        let _ = write!(
            d,
            ". Values from {} to {}",
            chart.fmt.format(lo),
            chart.fmt.format(hi)
        );
    }
    d.push('.');
    d
}

fn header_block(chart: &Chart) -> Header {
    let mut c = Canvas::new();
    let mut y = PAD;
    if let Some(t) = &chart.title {
        let t = truncate_to(t, chart.width - 2.0 * PAD, TITLE_SIZE, true);
        c.text(
            PAD,
            y + TITLE_SIZE * 0.82,
            "start",
            TITLE_SIZE,
            true,
            INK,
            &t,
        );
        y += TITLE_SIZE * 0.82 + 7.0;
    }
    if let Some(s) = &chart.subtitle {
        let s = truncate_to(s, chart.width - 2.0 * PAD, SUBTITLE_SIZE, false);
        c.text(
            PAD,
            y + SUBTITLE_SIZE * 0.82,
            "start",
            SUBTITLE_SIZE,
            false,
            INK_SECONDARY,
            &s,
        );
        y += SUBTITLE_SIZE * 0.82 + 6.0;
    }
    if chart.show_legend() {
        let (markup, height) = legend_block(chart, y + 6.0);
        c.raw(&markup);
        y += 6.0 + height;
    }
    if chart.title.is_some() || chart.subtitle.is_some() || chart.show_legend() {
        y += 6.0;
    }
    Header {
        top: y,
        markup: c.out,
    }
}

/// The dependable identity channel: always present for two or more series, with the same swatch
/// the marks use (hatch included) so the match is exact in greyscale too.
fn legend_block(chart: &Chart, top: f64) -> (String, f64) {
    let mut c = Canvas::new();
    let avail = chart.width - 2.0 * PAD;
    let entries: Vec<(String, String, usize)> = if chart.kind.is_round() {
        chart
            .labels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.clone(), slice_fill(chart, i), i))
            .collect()
    } else {
        chart
            .series
            .iter()
            .map(|s| (s.name.clone(), fill_for(chart, s), s.slot))
            .collect()
    };
    if entries.is_empty() {
        return (String::new(), 0.0);
    }

    let key_w = 16.0;
    let per_item_extra = key_w + 6.0 + 18.0;
    let budget = (avail / entries.len().max(1) as f64 - per_item_extra).max(40.0);
    let items: Vec<(String, String, usize, f64)> = entries
        .into_iter()
        .map(|(name, fill, slot)| {
            let label = truncate_to(&name, budget.max(48.0), LEGEND_SIZE, false);
            let w = key_w + 6.0 + text_width(&label, LEGEND_SIZE, false) + 18.0;
            (label, fill, slot, w)
        })
        .collect();

    let row_h = 18.0;
    let mut x = PAD;
    let mut y = top;
    let mut rows = 1.0;
    for (label, fill, slot, w) in &items {
        if x > PAD && x + w - 18.0 > PAD + avail {
            x = PAD;
            y += row_h;
            rows += 1.0;
        }
        let cy = y + row_h / 2.0;
        if chart.kind.is_line_like() {
            c.line(x, cy, x + key_w, cy, series_color(chart, *slot), LINE_WIDTH);
            c.raw(&marker(
                x + key_w / 2.0,
                cy,
                *slot,
                series_color(chart, *slot),
            ));
        } else {
            let _ = write!(
                c.out,
                "<rect x=\"{}\" y=\"{}\" width=\"11\" height=\"11\" rx=\"2\" fill=\"{}\"/>",
                n(x),
                n(cy - 5.5),
                fill
            );
        }
        c.text(
            x + key_w + 6.0,
            cy + LEGEND_SIZE * 0.35,
            "start",
            LEGEND_SIZE,
            false,
            INK_SECONDARY,
            label,
        );
        x += w;
    }
    (c.out, rows * row_h)
}

fn series_color(chart: &Chart, slot: usize) -> &str {
    chart
        .series
        .iter()
        .find(|s| s.slot == slot)
        .map(|s| s.color.as_str())
        .unwrap_or(SERIES_COLORS[slot % MAX_SERIES])
}

fn slice_fill(chart: &Chart, index: usize) -> String {
    let base = chart
        .series
        .first()
        .filter(|s| s.slot == index)
        .map(|s| s.color.clone())
        .unwrap_or_else(|| SERIES_COLORS[index % MAX_SERIES].to_string());
    if chart.texture && chart.labels.len() > 1 && !index.is_multiple_of(3) {
        format!("url(#mtp{}-s{})", chart.uid, index)
    } else {
        base
    }
}

// ------------ Cartesian charts (bar, grouped, stacked, line, area) ------------

fn draw_cartesian(chart: &Chart, top: f64) -> String {
    let mut c = Canvas::new();
    let (min, max) = value_range(chart);
    let target_ticks = if chart.height >= 300.0 { 5 } else { 4 };
    let scale = nice_scale(min, max, target_ticks);
    let ticks = scale.ticks();
    let tick_labels: Vec<String> = ticks.iter().map(|t| chart.fmt.format(*t)).collect();
    let tick_w = tick_labels
        .iter()
        .map(|t| text_width(t, AXIS_SIZE, false))
        .fold(0.0_f64, f64::max);

    let left = PAD + if chart.y_label.is_some() { 16.0 } else { 0.0 } + tick_w + 8.0;
    let right = chart.width - PAD - 6.0;
    let bottom_reserved = PAD + if chart.x_label.is_some() { 17.0 } else { 0.0 };
    let plot_w = (right - left).max(20.0);

    let bands = chart.points().max(1);
    let band = plot_w / bands as f64;
    let max_cat_band = (chart.height * 0.34).min(96.0);
    let cat = plan_cat_axis(&chart.labels, band, max_cat_band, left + band / 2.0 + 3.0);

    let plot_top = top;
    let plot_bottom = (chart.height - bottom_reserved - cat.band).max(plot_top + 30.0);
    let plot_h = plot_bottom - plot_top;

    let y_of = |v: f64| plot_bottom - scale.frac(v) * plot_h;
    let zero_y = y_of(0.0);

    // Grid first: it must sit under every mark.
    if chart.grid {
        for t in &ticks {
            let y = y_of(*t);
            c.line(left, y, right, y, GRID, 1.0);
        }
    }
    for (t, label) in ticks.iter().zip(tick_labels.iter()) {
        let y = y_of(*t);
        c.text(
            left - 8.0,
            y + AXIS_SIZE * 0.35,
            "end",
            AXIS_SIZE,
            false,
            INK_MUTED,
            label,
        );
    }
    if scale.lo < 0.0 && scale.hi > 0.0 {
        c.line(left, zero_y, right, zero_y, AXIS, 1.0);
    } else {
        c.line(left, plot_bottom, right, plot_bottom, AXIS, 1.0);
    }

    match chart.kind {
        Kind::StackedBar => draw_stacked(&mut c, chart, left, band, plot_bottom, plot_h, &scale),
        Kind::Line | Kind::Area => draw_lines(
            &mut c,
            chart,
            left,
            band,
            plot_top,
            plot_bottom,
            plot_h,
            &scale,
        ),
        _ => draw_columns(&mut c, chart, left, band, plot_bottom, plot_h, &scale),
    }

    draw_cat_labels(&mut c, &cat, left, band, plot_bottom);

    if let Some(x) = &chart.x_label {
        let t = truncate_to(x, chart.width - 2.0 * PAD, AXIS_SIZE, false);
        c.text(
            (left + right) / 2.0,
            chart.height - PAD,
            "middle",
            AXIS_SIZE,
            false,
            INK_SECONDARY,
            &t,
        );
    }
    if let Some(y) = &chart.y_label {
        let t = truncate_to(y, plot_h, AXIS_SIZE, false);
        let cx = PAD + 4.0;
        let cy = (plot_top + plot_bottom) / 2.0;
        let _ = write!(
            c.out,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"{}\" \
             transform=\"rotate(-90 {} {})\">{}</text>",
            n(cx),
            n(cy),
            n(AXIS_SIZE),
            INK_SECONDARY,
            n(cx),
            n(cy),
            esc(&t)
        );
    }
    c.out
}

fn draw_cat_labels(c: &mut Canvas, cat: &CatAxis, left: f64, band: f64, plot_bottom: f64) {
    for (i, label) in cat.labels.iter().enumerate() {
        if i % cat.every != 0 || label.is_empty() {
            continue;
        }
        let cx = left + band * i as f64 + band / 2.0;
        if cat.rotate {
            let px = cx + 3.0;
            let py = plot_bottom + 12.0;
            let _ = write!(
                c.out,
                "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" font-size=\"{}\" fill=\"{}\" \
                 transform=\"rotate(-35 {} {})\">{}</text>",
                n(px),
                n(py),
                n(AXIS_SIZE),
                INK_MUTED,
                n(px),
                n(py),
                esc(label)
            );
        } else {
            c.text(
                cx,
                plot_bottom + AXIS_SIZE + 5.0,
                "middle",
                AXIS_SIZE,
                false,
                INK_MUTED,
                label,
            );
        }
    }
}

fn draw_columns(
    c: &mut Canvas,
    chart: &Chart,
    left: f64,
    band: f64,
    plot_bottom: f64,
    plot_h: f64,
    scale: &Scale,
) {
    let y_of = |v: f64| plot_bottom - scale.frac(v) * plot_h;
    let zero_y = y_of(0.0);
    let k = chart.series.len().max(1);
    // The band keeps 30% as air; inside it every series gets an equal slot minus the 2px gap.
    let group_w = (band * 0.7).min(MAX_BAR_THICKNESS * k as f64 + GAP * (k as f64 - 1.0));
    let slot_w = ((group_w - GAP * (k as f64 - 1.0)) / k as f64).max(0.5);

    let values: Vec<String> = chart
        .series
        .iter()
        .flat_map(|s| s.data.iter().flatten().map(|v| chart.fmt.format(*v)))
        .collect();
    let widest_value = values
        .iter()
        .map(|v| text_width(v, VALUE_SIZE, false))
        .fold(0.0_f64, f64::max);
    // All the value labels ship or none do: a chart with half its bars labelled reads as an error.
    let label_values = widest_value + 3.0 <= slot_w.max(band / k as f64) && plot_h > 60.0;

    for (i, _) in chart.labels.iter().enumerate() {
        let band_x = left + band * i as f64 + (band - group_w) / 2.0;
        for (j, s) in chart.series.iter().enumerate() {
            let Some(v) = s.data.get(i).copied().flatten() else {
                continue;
            };
            let x = band_x + (slot_w + GAP) * j as f64;
            let y_v = y_of(v);
            let (y, h, end) = if v >= 0.0 {
                (y_v, zero_y - y_v, RoundEnd::Top)
            } else {
                (zero_y, y_v - zero_y, RoundEnd::Bottom)
            };
            let h = h.max(0.0);
            if h < 0.5 && v != 0.0 {
                // Keep a hairline so a tiny value is still visible as a mark.
                c.rect(
                    x,
                    if v >= 0.0 { zero_y - 1.0 } else { zero_y },
                    slot_w,
                    1.0,
                    &fill_for(chart, s),
                );
            } else {
                c.path(&bar_path(x, y, slot_w, h, 4.0, end), &fill_for(chart, s));
            }
            if label_values {
                let ty = if v >= 0.0 {
                    y_v - 5.0
                } else {
                    y_v + VALUE_SIZE + 2.0
                };
                c.text(
                    x + slot_w / 2.0,
                    ty,
                    "middle",
                    VALUE_SIZE,
                    false,
                    INK_SECONDARY,
                    &chart.fmt.format(v),
                );
            }
        }
    }
}

fn draw_stacked(
    c: &mut Canvas,
    chart: &Chart,
    left: f64,
    band: f64,
    plot_bottom: f64,
    plot_h: f64,
    scale: &Scale,
) {
    let y_of = |v: f64| plot_bottom - scale.frac(v) * plot_h;
    // Wider than the thin-mark cap on purpose: on paper there is no tooltip and no table view, so
    // the in-segment label is the only place a segment's value can live, and it needs the room.
    let bar_w = (band * 0.7).min(MAX_BAR_THICKNESS * 1.6);

    for i in 0..chart.points() {
        let x = left + band * i as f64 + (band - bar_w) / 2.0;
        let (mut acc_pos, mut acc_neg) = (0.0_f64, 0.0_f64);
        let top_pos = chart
            .series
            .iter()
            .rposition(|s| s.data.get(i).copied().flatten().is_some_and(|v| v > 0.0));
        for (j, s) in chart.series.iter().enumerate() {
            let Some(v) = s.data.get(i).copied().flatten() else {
                continue;
            };
            if v == 0.0 {
                continue;
            }
            let (y0, y1) = if v > 0.0 {
                let a = acc_pos;
                acc_pos += v;
                (y_of(acc_pos), y_of(a))
            } else {
                let a = acc_neg;
                acc_neg += v;
                (y_of(a), y_of(acc_neg))
            };
            let is_top = Some(j) == top_pos;
            // The 2px separator is surface showing through, not a stroke around the mark.
            let seg_top = if is_top { y0 } else { y0 + GAP / 2.0 };
            let seg_h = (y1 - seg_top - if v > 0.0 { GAP / 2.0 } else { 0.0 }).max(0.0);
            let end = if is_top {
                RoundEnd::Top
            } else {
                RoundEnd::None
            };
            c.path(
                &bar_path(x, seg_top, bar_w, seg_h, 4.0, end),
                &fill_for(chart, s),
            );

            let text = chart.fmt.format(v);
            let tw = text_width(&text, VALUE_SIZE, false);
            // An interior segment has no free end to push the label to: it either fits or the
            // legend carries it.
            if tw + 10.0 <= bar_w && seg_h >= VALUE_SIZE + 6.0 {
                let ink = if relative_luminance(&s.color) > 0.45 {
                    INK
                } else {
                    SURFACE
                };
                // Glyphs sitting straight on a hatch are noisy: the label gets a plate of the
                // segment's own solid colour so the texture never runs through the digits.
                if pattern_id(chart, s.slot).is_some() {
                    let _ = write!(
                        c.out,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{}\"/>",
                        n(x + bar_w / 2.0 - tw / 2.0 - 4.0),
                        n(seg_top + seg_h / 2.0 - (VALUE_SIZE + 4.0) / 2.0),
                        n(tw + 8.0),
                        n(VALUE_SIZE + 4.0),
                        s.color
                    );
                }
                c.text(
                    x + bar_w / 2.0,
                    seg_top + seg_h / 2.0 + VALUE_SIZE * 0.35,
                    "middle",
                    VALUE_SIZE,
                    false,
                    ink,
                    &text,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_lines(
    c: &mut Canvas,
    chart: &Chart,
    left: f64,
    band: f64,
    plot_top: f64,
    plot_bottom: f64,
    plot_h: f64,
    scale: &Scale,
) {
    let y_of = |v: f64| plot_bottom - scale.frac(v) * plot_h;
    let points = chart.points();
    // A single point has no band to sit in the middle of; put it in the middle of the plot.
    let x_of = |i: usize| {
        if points <= 1 {
            left + band * 0.5
        } else {
            left + band * i as f64 + band / 2.0
        }
    };
    let show_markers = points * chart.series.len() <= 80;

    for s in &chart.series {
        // A null is a hole in the data, not a zero: the line breaks and picks up after it.
        let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut run: Vec<(f64, f64)> = Vec::new();
        for i in 0..points {
            match s.data.get(i).copied().flatten() {
                Some(v) => run.push((x_of(i), y_of(v))),
                None => {
                    if !run.is_empty() {
                        runs.push(std::mem::take(&mut run));
                    }
                }
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }

        if chart.kind == Kind::Area {
            let base = y_of(scale.lo.max(0.0).min(scale.hi));
            for r in &runs {
                if r.len() < 2 {
                    continue;
                }
                let mut d = format!("M{} {}", n(r[0].0), n(base));
                for (x, y) in r {
                    let _ = write!(d, "L{} {}", n(*x), n(*y));
                }
                let _ = write!(d, "L{} {}Z", n(r[r.len() - 1].0), n(base));
                let _ = write!(
                    c.out,
                    "<path d=\"{d}\" fill=\"{}\" fill-opacity=\"0.1\"/>",
                    s.color
                );
            }
        }

        for r in &runs {
            if r.len() < 2 {
                continue;
            }
            let pts: Vec<String> = r
                .iter()
                .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
                .collect();
            let _ = write!(
                c.out,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" \
                 stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                pts.join(" "),
                s.color,
                n(LINE_WIDTH)
            );
        }

        if show_markers {
            for r in &runs {
                for (x, y) in r {
                    c.raw(&marker(*x, *y, s.slot, &s.color));
                }
            }
        } else {
            // Too dense for a marker per point: mark the extremes and the endpoint, which are the
            // points a reader actually looks for.
            for r in &runs {
                if let (Some(first), Some(last)) = (r.first(), r.last()) {
                    let hi = r.iter().min_by(|a, b| a.1.total_cmp(&b.1));
                    let lo = r.iter().max_by(|a, b| a.1.total_cmp(&b.1));
                    for p in [Some(last), hi, lo, Some(first)].into_iter().flatten() {
                        c.raw(&marker(p.0, p.1, s.slot, &s.color));
                    }
                }
            }
        }
    }

    // A single series carries no legend, so its endpoint gets the value instead.
    if chart.series.len() == 1 {
        if let Some(s) = chart.series.first() {
            if let Some((i, v)) = (0..points)
                .rev()
                .find_map(|i| s.data.get(i).copied().flatten().map(|v| (i, v)))
            {
                let text = chart.fmt.format(v);
                let x = x_of(i);
                let y = y_of(v).clamp(plot_top + VALUE_SIZE, plot_bottom - 2.0);
                let tw = text_width(&text, VALUE_SIZE, false);
                let (tx, anchor) = if x + 8.0 + tw <= chart.width - PAD {
                    (x + 8.0, "start")
                } else {
                    (x - 8.0, "end")
                };
                c.text(tx, y - 8.0, anchor, VALUE_SIZE, false, INK_SECONDARY, &text);
            }
        }
    }
}

// ------------ Horizontal bars ------------

fn draw_hbar(chart: &Chart, top: f64) -> String {
    let mut c = Canvas::new();
    let (min, max) = value_range(chart);
    let scale = nice_scale(min, max, 5);

    let cat_w_cap = chart.width * 0.32;
    let labels: Vec<String> = chart
        .labels
        .iter()
        .map(|l| truncate_to(l, cat_w_cap, AXIS_SIZE, false))
        .collect();
    let cat_w = labels
        .iter()
        .map(|l| text_width(l, AXIS_SIZE, false))
        .fold(0.0_f64, f64::max);

    let values: Vec<String> = chart
        .series
        .iter()
        .flat_map(|s| s.data.iter().flatten().map(|v| chart.fmt.format(*v)))
        .collect();
    let value_w = values
        .iter()
        .map(|v| text_width(v, VALUE_SIZE, false))
        .fold(0.0_f64, f64::max);

    let left = PAD + if chart.y_label.is_some() { 16.0 } else { 0.0 } + cat_w + 8.0;
    let right = (chart.width - PAD - value_w - 8.0).max(left + 30.0);
    let plot_w = right - left;
    let bottom =
        chart.height - PAD - AXIS_SIZE - 6.0 - if chart.x_label.is_some() { 17.0 } else { 0.0 };
    let plot_h = (bottom - top).max(20.0);

    let bands = chart.points().max(1);
    let band = plot_h / bands as f64;
    let k = chart.series.len().max(1);
    let group_h = (band * 0.7).min(MAX_BAR_THICKNESS * k as f64 + GAP * (k as f64 - 1.0));
    let slot_h = ((group_h - GAP * (k as f64 - 1.0)) / k as f64).max(0.5);

    let x_of = |v: f64| left + scale.frac(v) * plot_w;
    let zero_x = x_of(0.0);

    if chart.grid {
        for t in scale.ticks() {
            let x = x_of(t);
            c.line(x, top, x, bottom, GRID, 1.0);
        }
    }
    for t in scale.ticks() {
        c.text(
            x_of(t),
            bottom + AXIS_SIZE + 5.0,
            "middle",
            AXIS_SIZE,
            false,
            INK_MUTED,
            &chart.fmt.format(t),
        );
    }
    c.line(zero_x, top, zero_x, bottom, AXIS, 1.0);

    let label_values = slot_h >= VALUE_SIZE;
    for (i, label) in labels.iter().enumerate() {
        let band_y = top + band * i as f64 + (band - group_h) / 2.0;
        if band >= AXIS_SIZE + 2.0 {
            c.text(
                left - 8.0,
                top + band * i as f64 + band / 2.0 + AXIS_SIZE * 0.35,
                "end",
                AXIS_SIZE,
                false,
                INK_MUTED,
                label,
            );
        }
        for (j, s) in chart.series.iter().enumerate() {
            let Some(v) = s.data.get(i).copied().flatten() else {
                continue;
            };
            let y = band_y + (slot_h + GAP) * j as f64;
            let x_v = x_of(v);
            let (x, w, end) = if v >= 0.0 {
                (zero_x, x_v - zero_x, RoundEnd::Right)
            } else {
                (x_v, zero_x - x_v, RoundEnd::Left)
            };
            let w = w.max(0.0);
            if w < 0.5 && v != 0.0 {
                c.rect(
                    if v >= 0.0 { zero_x } else { zero_x - 1.0 },
                    y,
                    1.0,
                    slot_h,
                    &fill_for(chart, s),
                );
            } else {
                c.path(&bar_path(x, y, w, slot_h, 4.0, end), &fill_for(chart, s));
            }
            if label_values {
                let text = chart.fmt.format(v);
                let (tx, anchor) = if v >= 0.0 {
                    (x + w + 5.0, "start")
                } else {
                    (x - 5.0, "end")
                };
                c.text(
                    tx,
                    y + slot_h / 2.0 + VALUE_SIZE * 0.35,
                    anchor,
                    VALUE_SIZE,
                    false,
                    INK_SECONDARY,
                    &text,
                );
            }
        }
    }

    if let Some(x) = &chart.x_label {
        let t = truncate_to(x, chart.width - 2.0 * PAD, AXIS_SIZE, false);
        c.text(
            (left + right) / 2.0,
            chart.height - PAD,
            "middle",
            AXIS_SIZE,
            false,
            INK_SECONDARY,
            &t,
        );
    }
    c.out
}

// ------------ Pie and donut ------------

fn draw_pie(chart: &Chart, top: f64) -> String {
    let mut c = Canvas::new();
    let Some(series) = chart.series.first() else {
        return c.out;
    };
    // Negative values have no meaning in a part-to-whole; their magnitude is what is shown.
    let values: Vec<f64> = series.data.iter().map(|v| v.unwrap_or(0.0).abs()).collect();
    let total: f64 = values.iter().sum();

    let box_top = top;
    let box_bottom = chart.height - PAD;
    let box_h = (box_bottom - box_top).max(40.0);

    let label_texts: Vec<String> = chart
        .labels
        .iter()
        .zip(values.iter())
        .map(|(l, v)| {
            let pct = if total > 0.0 { v / total * 100.0 } else { 0.0 };
            format!("{} {}%", l, trim_decimals(pct, 1))
        })
        .collect();
    let widest_label = label_texts
        .iter()
        .map(|t| text_width(t, VALUE_SIZE, false))
        .fold(0.0_f64, f64::max);

    let label_room = widest_label.min(chart.width * 0.26);
    let r_by_w = (chart.width - 2.0 * PAD - 2.0 * (label_room + 10.0)) / 2.0;
    let r_by_h = box_h / 2.0 - 10.0;
    let mut r = r_by_w.min(r_by_h);
    let mut labelled = true;
    if r < 46.0 {
        // Not enough room for outside labels: give the geometry back to the pie and let the
        // legend carry identity.
        r = (chart.width / 2.0 - PAD).min(r_by_h).max(12.0);
        labelled = false;
    }
    let cx = chart.width / 2.0;
    let cy = box_top + box_h / 2.0;
    let inner = if chart.kind == Kind::Donut {
        r * 0.58
    } else {
        0.0
    };

    if total <= 0.0 {
        c.text(
            cx,
            cy,
            "middle",
            AXIS_SIZE,
            false,
            INK_MUTED,
            "No positive value to plot",
        );
        return c.out;
    }

    // Slice textures need their own defs: the fills here are per slice, not per series.
    if chart.texture && values.len() > 1 {
        let mut d = String::from("<defs>");
        for i in 0..values.len() {
            if i % 3 == 0 {
                continue;
            }
            let base = SERIES_COLORS[i % MAX_SERIES];
            let ink = darken(base, 0.55);
            let path = if i % 3 == 1 {
                "M-2 2L2 -2M0 8L8 0M6 10L10 6"
            } else {
                "M-2 6L2 10M0 0L8 8M6 -2L10 2"
            };
            let _ = write!(
                d,
                "<pattern id=\"mtp{}-s{i}\" width=\"8\" height=\"8\" patternUnits=\"userSpaceOnUse\">\
                 <rect width=\"8\" height=\"8\" fill=\"{base}\"/>\
                 <path d=\"{path}\" stroke=\"{ink}\" stroke-width=\"1.5\" fill=\"none\"/></pattern>",
                chart.uid
            );
        }
        d.push_str("</defs>");
        c.raw(&d);
    }

    let gap_angle = if r > 0.0 { GAP / 2.0 / r } else { 0.0 };
    let mut angle = -std::f64::consts::FRAC_PI_2;
    let mut last_y: [f64; 2] = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut placed: Vec<(f64, f64, String, &'static str)> = Vec::new();

    for (i, v) in values.iter().enumerate() {
        let sweep = *v / total * std::f64::consts::TAU;
        let a0 = angle;
        let a1 = angle + sweep;
        angle = a1;
        if sweep <= 0.0 {
            continue;
        }
        let (s0, s1) = if sweep > gap_angle * 3.0 {
            (a0 + gap_angle, a1 - gap_angle)
        } else {
            (a0, a1)
        };
        c.path(&arc_path(cx, cy, r, inner, s0, s1), &slice_fill(chart, i));

        if !labelled {
            continue;
        }
        let mid = (a0 + a1) / 2.0;
        let (dx, dy) = (mid.cos(), mid.sin());
        let ly = cy + dy * (r + 12.0);
        let side = usize::from(dx < 0.0);
        // Two labels on the same side within a line height would collide; the legend still has
        // them, so the second one is dropped rather than nudged away from its slice.
        if (ly - last_y[side]).abs() < VALUE_SIZE + 3.0 || sweep < 0.045 {
            continue;
        }
        last_y[side] = ly;
        let lx = cx + dx * (r + 8.0);
        let anchor = if dx < 0.0 { "end" } else { "start" };
        let room = if dx < 0.0 {
            lx - PAD
        } else {
            chart.width - PAD - lx
        };
        let text = truncate_to(&label_texts[i], room.max(0.0), VALUE_SIZE, false);
        if !text.is_empty() {
            placed.push((lx, ly + VALUE_SIZE * 0.35, text, anchor));
        }
    }

    for (x, y, text, anchor) in placed {
        c.text(x, y, anchor, VALUE_SIZE, false, INK_SECONDARY, &text);
    }

    if chart.kind == Kind::Donut && inner > 26.0 {
        let total_text = chart.fmt.format(total);
        if text_width(&total_text, 15.0, true) <= inner * 1.7 {
            c.text(cx, cy + 2.0, "middle", 15.0, true, INK, &total_text);
            c.text(cx, cy + 16.0, "middle", 9.5, false, INK_MUTED, "Total");
        }
    }
    c.out
}

/// A slice, or a donut segment when `inner > 0`. A full turn is drawn as two half sweeps because
/// an arc whose ends coincide is degenerate.
fn arc_path(cx: f64, cy: f64, r: f64, inner: f64, a0: f64, a1: f64) -> String {
    let sweep = a1 - a0;
    if sweep >= std::f64::consts::TAU - 1e-6 {
        let mid = a0 + std::f64::consts::PI;
        return format!(
            "{} {}",
            arc_path(cx, cy, r, inner, a0, mid),
            arc_path(cx, cy, r, inner, mid, a0 + std::f64::consts::TAU)
        );
    }
    let large = u8::from(sweep > std::f64::consts::PI);
    let (x0, y0) = (cx + r * a0.cos(), cy + r * a0.sin());
    let (x1, y1) = (cx + r * a1.cos(), cy + r * a1.sin());
    if inner <= 0.0 {
        format!(
            "M{} {}L{} {}A{} {} 0 {large} 1 {} {}Z",
            n(cx),
            n(cy),
            n(x0),
            n(y0),
            n(r),
            n(r),
            n(x1),
            n(y1)
        )
    } else {
        let (ix1, iy1) = (cx + inner * a1.cos(), cy + inner * a1.sin());
        let (ix0, iy0) = (cx + inner * a0.cos(), cy + inner * a0.sin());
        format!(
            "M{} {}A{} {} 0 {large} 1 {} {}L{} {}A{} {} 0 {large} 0 {} {}Z",
            n(x0),
            n(y0),
            n(r),
            n(r),
            n(x1),
            n(y1),
            n(ix1),
            n(iy1),
            n(inner),
            n(inner),
            n(ix0),
            n(iy0)
        )
    }
}

// ------------ Tests ------------

#[cfg(test)]
mod tests {
    use super::*;

    fn render(spec: &str) -> String {
        render_chart(spec).expect("spec should render")
    }

    fn err(spec: &str) -> String {
        match render_chart(spec) {
            Err(AppError::BadRequest(m)) => m,
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    /// Not a parser, just enough structure checking to catch an unbalanced or unquoted emission.
    fn assert_well_formed(svg: &str) {
        assert!(
            svg.starts_with("<svg "),
            "missing svg root: {}",
            &svg[..40.min(svg.len())]
        );
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("role=\"img\""));
        assert!(svg.contains("<title>") && svg.contains("<desc>"));
        assert!(svg.contains("viewBox="));
        assert_eq!(
            svg.matches('<').count(),
            svg.matches('>').count(),
            "unbalanced angle brackets"
        );
        assert_eq!(svg.matches('"').count() % 2, 0, "unbalanced quotes");
        for tag in ["text", "svg", "pattern", "defs", "title", "desc"] {
            let open =
                svg.matches(&format!("<{tag} ")).count() + svg.matches(&format!("<{tag}>")).count();
            let close = svg.matches(&format!("</{tag}>")).count();
            assert_eq!(open, close, "unbalanced <{tag}>");
        }
        assert!(!svg.contains("NaN"), "NaN leaked into the markup");
        assert!(!svg.contains("inf"), "infinity leaked into the markup");
        assert!(!svg.contains("<foreignObject"));
    }

    const TWO_SERIES: &str = r#"{"labels":["Jan","Fev","Mar","Avr"],
        "series":[{"name":"CA","data":[120,150,90,170]},{"name":"Marge","data":[30,45,20,60]}]}"#;

    fn spec_for(kind: &str) -> String {
        format!("{{\"type\":\"{kind}\",{}", &TWO_SERIES[1..])
    }

    #[test]
    fn every_type_renders_well_formed_svg() {
        for kind in [
            "bar",
            "hbar",
            "stacked-bar",
            "grouped-bar",
            "line",
            "area",
            "pie",
            "donut",
        ] {
            let svg = render(&spec_for(kind));
            assert_well_formed(&svg);
            assert!(svg.contains("width=\"640\""), "{kind}: default width");
        }
    }

    #[test]
    fn bar_has_bars_and_axis_labels() {
        let svg = render(&spec_for("bar"));
        assert!(svg.contains("<path d=\"M"), "expected bar paths");
        assert!(svg.contains(">Jan<") && svg.contains(">Avr<"));
        assert!(
            svg.contains(">CA<") && svg.contains(">Marge<"),
            "legend entries"
        );
    }

    #[test]
    fn hbar_labels_values_at_the_tip() {
        let svg = render(&spec_for("hbar"));
        assert!(svg.contains(">120<"), "value label at the bar tip");
    }

    #[test]
    fn stacked_bar_stacks_and_rounds_only_the_top() {
        let svg = render(&spec_for("stacked-bar"));
        assert_well_formed(&svg);
        // Every band draws two segments, one per series.
        assert!(svg.matches("<path d=\"M").count() >= 8);
    }

    #[test]
    fn line_draws_polylines_and_markers() {
        let svg = render(&spec_for("line"));
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("<circle"), "slot 0 marker");
        assert!(svg.contains("<polygon"), "slot 1 marker shape");
    }

    #[test]
    fn area_uses_a_ten_percent_wash() {
        let svg = render(&spec_for("area"));
        assert!(svg.contains("fill-opacity=\"0.1\""));
    }

    #[test]
    fn pie_and_donut_draw_arcs_and_percentages() {
        let pie = render(&spec_for("pie"));
        assert!(pie.contains("A"), "arc command");
        assert!(pie.contains('%'), "percentage in the slice labels");
        let donut = render(&spec_for("donut"));
        assert!(donut.contains(">Total<"), "donut centre carries the total");
    }

    #[test]
    fn multi_series_adds_a_texture_channel() {
        let svg = render(&spec_for("bar"));
        assert!(svg.contains("<pattern id="), "second channel for print");
        assert!(svg.contains("url(#mtp"));
        // A single series needs no second channel and no legend box.
        let single =
            render(r#"{"type":"bar","labels":["a","b"],"series":[{"name":"x","data":[1,2]}]}"#);
        assert!(!single.contains("<pattern"));
    }

    #[test]
    fn texture_can_be_turned_off() {
        let svg = render(&format!(
            "{{\"type\":\"bar\",\"texture\":false,{}",
            &TWO_SERIES[1..]
        ));
        assert!(!svg.contains("<pattern"));
    }

    #[test]
    fn single_series_has_no_legend() {
        let svg = render(r#"{"type":"bar","labels":["a"],"series":[{"name":"only","data":[5]}]}"#);
        assert!(!svg.contains(">only<"), "no legend box for one series");
    }

    // ---- degenerate inputs ----

    #[test]
    fn series_of_different_lengths() {
        let svg = render(
            r#"{"type":"grouped-bar","labels":["a"],
                "series":[{"name":"s1","data":[1,2,3,4]},{"name":"s2","data":[5]}]}"#,
        );
        assert_well_formed(&svg);
        // The category axis is as long as the longest series, missing labels fall back to a rank.
        assert!(svg.contains(">4<"));
    }

    #[test]
    fn nulls_break_the_line_instead_of_reading_as_zero() {
        let svg = render(
            r#"{"type":"line","labels":["a","b","c","d"],
                "series":[{"name":"s","data":[1,null,3,4]}]}"#,
        );
        assert_well_formed(&svg);
        assert_eq!(svg.matches("<polyline").count(), 1, "one run of two points");
    }

    #[test]
    fn all_null_series_still_renders() {
        let svg = render(r#"{"type":"line","series":[{"name":"s","data":[null,null]}]}"#);
        assert_well_formed(&svg);
    }

    #[test]
    fn constant_series_gets_a_readable_axis() {
        let svg = render(r#"{"type":"bar","labels":["a","b","c"],"series":[{"data":[7,7,7]}]}"#);
        assert_well_formed(&svg);
        assert!(svg.contains(">0<"), "axis anchored at zero");
    }

    #[test]
    fn single_value_renders() {
        for kind in ["bar", "hbar", "line", "area", "pie", "donut", "stacked-bar"] {
            let svg = render(&format!(
                "{{\"type\":\"{kind}\",\"labels\":[\"only\"],\"series\":[{{\"data\":[42]}}]}}"
            ));
            assert_well_formed(&svg);
        }
    }

    #[test]
    fn zero_value_series_renders() {
        let svg = render(r#"{"type":"pie","labels":["a","b"],"series":[{"data":[0,0]}]}"#);
        assert_well_formed(&svg);
        assert!(svg.contains("No positive value"));
    }

    #[test]
    fn negative_values_grow_downwards_from_zero() {
        let svg =
            render(r#"{"type":"bar","labels":["a","b","c"],"series":[{"data":[-40,20,-10]}]}"#);
        assert_well_formed(&svg);
        assert!(svg.contains(">-40<"), "negative value labelled");
    }

    #[test]
    fn five_hundred_points_do_not_panic() {
        let data: Vec<String> = (0..500).map(|i| ((i * 7) % 97).to_string()).collect();
        for kind in ["bar", "line", "area", "hbar", "stacked-bar"] {
            let svg = render(&format!(
                "{{\"type\":\"{kind}\",\"series\":[{{\"data\":[{}]}}]}}",
                data.join(",")
            ));
            assert_well_formed(&svg);
        }
    }

    #[test]
    fn very_long_labels_are_truncated_not_clipped() {
        let long = "Chiffre d'affaires consolide du segment enterprise Europe du Sud".repeat(3);
        let svg = render(&format!(
            "{{\"type\":\"hbar\",\"labels\":[\"{long}\"],\"series\":[{{\"data\":[3]}}]}}"
        ));
        assert_well_formed(&svg);
        assert!(
            svg.contains("&#8230;"),
            "ellipsis instead of an overflowing label"
        );
    }

    #[test]
    fn rotated_labels_stay_inside_the_viewbox() {
        let long = "Documentation technique tres complete et interminable";
        let labels: Vec<String> = (0..5).map(|i| format!("\"{long} {i}\"")).collect();
        let svg = render(&format!(
            "{{\"type\":\"bar\",\"labels\":[{}],\"series\":[{{\"data\":[1,2,3,4,5]}}]}}",
            labels.join(",")
        ));
        assert_well_formed(&svg);
        // The leftmost rotated label trails down-left from its anchor; nothing may start left of 0.
        let plan = plan_cat_axis(
            &(0..5).map(|i| format!("{long} {i}")).collect::<Vec<_>>(),
            100.0,
            96.0,
            80.0,
        );
        assert!(plan.rotate);
        let widest = plan
            .labels
            .iter()
            .map(|l| text_width(l, AXIS_SIZE, false))
            .fold(0.0_f64, f64::max);
        assert!(
            80.0 - widest * ROT_ANGLE_COS >= 0.0,
            "first label would start off-canvas"
        );
    }

    #[test]
    fn eight_series_is_the_ceiling() {
        let mk = |k: usize| {
            let s: Vec<String> = (0..k)
                .map(|i| format!("{{\"name\":\"s{i}\",\"data\":[1,2]}}"))
                .collect();
            format!("{{\"type\":\"line\",\"series\":[{}]}}", s.join(","))
        };
        assert_well_formed(&render(&mk(8)));
        assert!(err(&mk(9)).contains("fold the tail"));
    }

    #[test]
    fn pie_folds_its_tail_rather_than_inventing_hues() {
        let labels: Vec<String> = (0..12).map(|i| format!("\"c{i}\"")).collect();
        let data: Vec<String> = (0..12).map(|i| (12 - i).to_string()).collect();
        let svg = render(&format!(
            "{{\"type\":\"pie\",\"labels\":[{}],\"series\":[{{\"data\":[{}]}}]}}",
            labels.join(","),
            data.join(",")
        ));
        assert_well_formed(&svg);
        assert!(svg.contains("Other"));
    }

    // ---- escaping & injection ----

    #[test]
    fn user_text_cannot_escape_its_context() {
        let svg = render(
            r#"{"type":"bar","title":"<script>alert(1)</script> & \"quotes\"",
                "labels":["a<b>","c&d"],"series":[{"name":"x\"y","data":[1,2]}]}"#,
        );
        assert_well_formed(&svg);
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
        assert!(svg.contains("&amp;"));
    }

    /// Pandoc re-wraps long raw-HTML lines at spaces; without `xml:space="preserve"` the newline
    /// it inserts inside `<text>` is deleted instead of collapsing back to a space.
    #[test]
    fn whitespace_survives_a_pandoc_rewrap() {
        let svg = render(
            r#"{"type":"bar","title":"Chiffre d affaires par mois","subtitle":"Exercice 2026",
                "labels":["Janvier 2026"],"series":[{"name":"Serie une","data":[1234]}]}"#,
        );
        assert!(svg.contains("xml:space=\"preserve\""));
        assert!(svg.contains("Chiffre d affaires par mois"));
    }

    #[test]
    fn non_ascii_becomes_numeric_references() {
        let svg = render(
            r#"{"type":"bar","labels":["Ete"],"title":"Chiffre d'affaires en euros","series":[{"data":[1]}]}"#,
        );
        assert!(svg.contains("&#39;"), "apostrophe escaped");
        let svg = render(r#"{"type":"bar","labels":["été"],"series":[{"data":[1]}]}"#);
        assert!(
            svg.contains("&#233;"),
            "accented label as a numeric reference"
        );
    }

    #[test]
    fn colors_must_be_hex() {
        assert!(
            err(r#"{"type":"bar","series":[{"data":[1],"color":"url(javascript:1)"}]}"#)
                .contains("series[0].color")
        );
        let svg = render(r##"{"type":"bar","series":[{"data":[1],"color":"#0A0"}]}"##);
        assert!(svg.contains("#00aa00"));
    }

    // ---- validation messages ----

    #[test]
    fn errors_point_at_the_offending_field() {
        assert!(err("not json").contains("invalid JSON"));
        assert!(err("[]").contains("expected a JSON object"));
        assert!(err(r#"{"series":[{"data":[1]}]}"#).contains("field \"type\""));
        assert!(err(r#"{"type":"pie3d","series":[{"data":[1]}]}"#).contains("unknown chart type"));
        assert!(err(r#"{"type":"bar"}"#).contains("field \"series\""));
        assert!(err(r#"{"type":"bar","series":[]}"#).contains("at least one series"));
        assert!(err(r#"{"type":"bar","series":[1]}"#).contains("series[0]"));
        assert!(err(r#"{"type":"bar","series":[{"name":"a"}]}"#).contains("series[0].data"));
        assert!(err(r#"{"type":"bar","series":[{"data":[]}]}"#).contains("empty"));
        assert!(err(r#"{"type":"bar","series":[{"data":[1,{}]}]}"#).contains("data[1]"));
        assert!(err(r#"{"type":"bar","series":[{"data":[1,"abc"]}]}"#).contains("data[1]"));
        assert!(
            err(r#"{"type":"bar","value_format":"dollars","series":[{"data":[1]}]}"#)
                .contains("value_format")
        );
        assert!(
            err(r#"{"type":"bar","width":40,"series":[{"data":[1]}]}"#).contains("out of range")
        );
        assert!(err(r#"{"type":"bar","legend":"yes","series":[{"data":[1]}]}"#).contains("legend"));
        assert!(
            err(r#"{"type":"bar","labels":[{}],"series":[{"data":[1]}]}"#).contains("labels[0]")
        );
    }

    #[test]
    fn numeric_strings_and_nulls_are_accepted() {
        let svg = render(r#"{"type":"bar","series":[{"data":["1 234",null,"56%"]}]}"#);
        assert_well_formed(&svg);
    }

    #[test]
    fn too_many_points_is_refused_explicitly() {
        let data: Vec<String> = (0..2001).map(|_| "1".to_string()).collect();
        assert!(err(&format!(
            "{{\"type\":\"line\",\"series\":[{{\"data\":[{}]}}]}}",
            data.join(",")
        ))
        .contains("point limit"));
    }

    // ---- units ----

    #[test]
    fn scale_picks_round_graduations() {
        for (min, max, expect_step) in [
            (0.0, 100.0, 20.0),
            (0.0, 9.0, 2.0),
            (0.0, 1.0, 0.2),
            (-30.0, 70.0, 20.0),
            (0.0, 4700.0, 1000.0),
        ] {
            let s = nice_scale(min, max, 5);
            assert!(
                (s.step - expect_step).abs() < 1e-9,
                "range {min}..{max}: step {} (expected {expect_step})",
                s.step
            );
            assert!(s.lo <= min && s.hi >= max);
            let mant = s.step / 10f64.powf(s.step.log10().floor());
            assert!(
                (mant - 1.0).abs() < 1e-9 || (mant - 2.0).abs() < 1e-9 || (mant - 5.0).abs() < 1e-9,
                "step {} is not 1/2/5 x 10^k",
                s.step
            );
        }
    }

    #[test]
    fn scale_survives_degenerate_domains() {
        for (a, b) in [(0.0, 0.0), (5.0, 5.0), (-5.0, -5.0), (1e-14, 1e-14)] {
            let s = nice_scale(a, b, 5);
            assert!(s.hi > s.lo, "{a}..{b} collapsed");
            assert!(s.step > 0.0 && s.step.is_finite());
            assert!(s.ticks().len() <= 65);
        }
    }

    #[test]
    fn formats_are_readable() {
        assert_eq!(ValueFormat::Compact.format(1234.0), "1.2K");
        assert_eq!(ValueFormat::Compact.format(12345.0), "12K");
        assert_eq!(ValueFormat::Compact.format(-2_500_000.0), "-2.5M");
        assert_eq!(ValueFormat::Compact.format(42.5), "42.5");
        assert_eq!(ValueFormat::Plain.format(1234567.0), "1,234,567");
        assert_eq!(ValueFormat::Percent.format(42.0), "42%");
        assert_eq!(ValueFormat::Eur.format(1234.0), "1\u{a0}234\u{a0}\u{20ac}");
    }

    #[test]
    fn text_measurement_matches_the_rendered_font() {
        // Twenty glyphs at 11px, measured from the PDF produced by the production image.
        assert!((text_width("MMMMMMMMMMMMMMMMMMMM", 11.0, false) - 189.83).abs() < 0.5);
        assert!((text_width("00000000000000000000", 11.0, false) - 139.96).abs() < 0.5);
        assert!((text_width("MMMMMMMMMMMMMMMMMMMM", 11.0, true) - 218.9).abs() < 0.5);
        assert!(text_width("", 11.0, false) == 0.0);
        assert!(
            text_width("\u{4e2d}\u{6587}", 10.0, false) > 15.0,
            "wide glyphs"
        );
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let s = "\u{e9}\u{e8}\u{ea}\u{e0}\u{e7} tr\u{e8}s long";
        for w in [0.0, 3.0, 12.0, 40.0, 400.0] {
            let t = truncate_to(s, w, 11.0, false);
            assert!(text_width(&t, 11.0, false) <= w.max(0.0) + 0.01);
        }
    }

    #[test]
    fn numbers_never_emit_nan_or_infinity() {
        assert_eq!(n(f64::NAN), "0");
        assert_eq!(n(f64::INFINITY), "0");
        assert_eq!(n(-0.0), "0");
        assert_eq!(n(12.3456), "12.35");
        assert_eq!(n(3.0), "3");
    }

    /// Renders one of each type into `target/chart-samples/` for visual inspection.
    #[test]
    fn write_visual_samples() {
        use std::fs;
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/chart-samples");
        if fs::create_dir_all(dir).is_err() {
            return; // a read-only target directory must not fail the suite
        }
        let samples: Vec<(&str, String)> = vec![
            ("bar", r#"{"type":"bar","title":"Chiffre d'affaires par mois","subtitle":"Exercice 2026, France","labels":["Janvier","Fevrier","Mars","Avril","Mai","Juin"],"series":[{"name":"CA","data":[42000,51000,48500,63000,58000,71000]}],"y_label":"Euros","value_format":"eur"}"#.to_string()),
            ("grouped-bar", r#"{"type":"grouped-bar","title":"Trafic par canal","labels":["T1","T2","T3","T4"],"series":[{"name":"Organique","data":[120,150,170,210]},{"name":"Payant","data":[80,95,70,120]},{"name":"Direct","data":[40,55,60,52]}],"value_format":"compact"}"#.to_string()),
            ("stacked-bar", r#"{"type":"stacked-bar","title":"Repartition des couts","subtitle":"En milliers d'euros","labels":["2023","2024","2025","2026"],"series":[{"name":"Salaires","data":[320,360,400,430]},{"name":"Infra","data":[110,130,145,160]},{"name":"Marketing","data":[60,90,120,140]}],"value_format":"plain"}"#.to_string()),
            ("hbar", r#"{"type":"hbar","title":"Top 7 des pages vues","labels":["Accueil","Tarifs","Documentation technique complete","Blog","Contact","A propos","Changelog"],"series":[{"name":"Vues","data":[128000,94000,61000,44000,23000,17000,9000]}],"value_format":"compact"}"#.to_string()),
            ("line", r#"{"type":"line","title":"Latence p95 par region","subtitle":"Millisecondes, 12 dernieres semaines","labels":["S1","S2","S3","S4","S5","S6","S7","S8","S9","S10","S11","S12"],"series":[{"name":"Europe","data":[210,205,198,220,215,190,185,180,175,170,168,161]},{"name":"Amerique","data":[280,275,290,265,260,255,250,null,245,240,238,230]},{"name":"Asie","data":[330,320,318,340,335,325,310,305,300,298,290,285]}],"y_label":"ms","value_format":"plain"}"#.to_string()),
            ("area", r#"{"type":"area","title":"Documents generes","subtitle":"Cumul mensuel","labels":["Jan","Fev","Mar","Avr","Mai","Juin","Juil","Aout"],"series":[{"name":"PDF","data":[1200,1800,2400,3100,4200,5600,6100,7400]}],"value_format":"compact"}"#.to_string()),
            ("pie", r#"{"type":"pie","title":"Sources de trafic","labels":["Recherche","Direct","Referral","Social","Email"],"series":[{"name":"Sessions","data":[4200,2100,900,620,380]}],"value_format":"compact"}"#.to_string()),
            ("donut", r#"{"type":"donut","title":"Repartition du budget","labels":["R&D","Ventes","Support","Admin"],"series":[{"name":"Budget","data":[480000,310000,150000,90000]}],"value_format":"eur"}"#.to_string()),
            ("negatives", r#"{"type":"bar","title":"Variation nette de tresorerie","labels":["T1","T2","T3","T4"],"series":[{"name":"Flux","data":[-42000,18000,-9000,63000]}],"value_format":"eur"}"#.to_string()),
            ("dense", format!(r#"{{"type":"line","title":"Serie dense (500 points)","series":[{{"name":"signal","data":[{}]}}]}}"#, (0..500).map(|i| format!("{:.2}", (i as f64 / 18.0).sin() * 40.0 + 60.0 + (i as f64) * 0.05)).collect::<Vec<_>>().join(","))),
        ];
        let mut index = String::from("<meta charset=\"utf-8\"><style>body{font-family:sans-serif;margin:24px;background:#f9f9f7}h2{font-size:13px;color:#52514e}div{background:#fff;display:inline-block;margin:8px;padding:8px;border:1px solid #e1e0d9}</style>");
        for (name, spec) in &samples {
            let svg = render(spec);
            assert_well_formed(&svg);
            let _ = fs::write(format!("{dir}/{name}.svg"), &svg);
            let _ = write!(index, "<div><h2>{name}</h2>{svg}</div>");
        }
        let _ = fs::write(format!("{dir}/index.html"), index);
    }
}
