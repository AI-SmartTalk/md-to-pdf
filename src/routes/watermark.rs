use crate::auth::PublicOrKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::io::Write;
use std::process::Command;
use tempfile::{Builder, TempPath};

#[post("/watermark", format = "json", data = "<req>")]
pub async fn watermark(
    key: PublicOrKey,
    req: Json<WatermarkRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
    let req = req.into_inner();

    validate_text(&req.text)?;

    let opacity = req.opacity.unwrap_or(0.06);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(AppError::BadRequest(
            "\"opacity\" must be between 0 and 1".to_string(),
        ));
    }

    let angle = req.angle.unwrap_or(-45.0);
    if !angle.is_finite() || !(-360.0..=360.0).contains(&angle) {
        return Err(AppError::BadRequest(
            "\"angle\" must be between -360 and 360".to_string(),
        ));
    }

    let WatermarkRequest {
        pdf,
        text,
        client_id,
        pdf_name,
        output,
        ..
    } = req;
    let output = output.unwrap_or_default();

    // weasyprint then qpdf, both long blocking runs on a large document: they belong on a
    // render slot like every other tool, or two watermarks pin every tokio worker and the
    // healthcheck restarts a service that was merely busy.
    let (produced, response) = exec::as_owner(
        key.0,
        exec::offload(move || overlay(&pdf, &text, opacity, angle, client_id, pdf_name, output)),
    )
    .await?;

    helpers::deliver_tool(produced, response, "watermark").await
}

/// A watermark that says nothing is not a watermark.
///
/// The endpoint used to accept `""`, draw an empty div, overlay it, and answer 200 with a
/// document byte-identical to the one it was given. The caller downloads it, sees a
/// "Résultat", and believes their document is marked — which is precisely the silent failure
/// every other tool here refuses to commit. `/api/protect` already turns an empty password
/// down for the same reason.
///
/// The limit is not a security boundary — the text is escaped before it reaches the HTML —
/// but a watermark longer than a line is a rendering accident, not an intent.
fn validate_text(text: &str) -> Result<(), AppError> {
    const MAX_CHARS: usize = 200;

    if text.trim().is_empty() {
        return Err(AppError::BadRequest(
            "\"text\" must not be empty: a watermark with no text would return the document unchanged".to_string(),
        ));
    }

    if text.chars().count() > MAX_CHARS {
        return Err(AppError::BadRequest(format!(
            "\"text\" is limited to {} characters",
            MAX_CHARS
        )));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn overlay(
    pdf: &str,
    text: &str,
    opacity: f32,
    angle: f32,
    client_id: Option<String>,
    pdf_name: Option<String>,
    output: ToolOutput,
) -> Result<(TempPath, ToolResponse), AppError> {
    let source_path = helpers::resolve_readable_pdf(pdf)?;

    // Create a watermark overlay PDF using weasyprint
    let watermark_html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
@page {{ size: A4; margin: 0; }}
body {{
  margin: 0;
  height: 100vh;
  width: 100vw;
}}
.watermark {{
  font-family: 'Helvetica', 'Arial', sans-serif;
  font-size: 80pt;
  color: rgba(0, 0, 0, {opacity});
  white-space: nowrap;
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%) rotate({angle}deg);
}}
</style>
</head>
<body>
<div class="watermark">{text}</div>
</body>
</html>"#,
        opacity = opacity,
        angle = angle,
        text = helpers::escape_html(text)
    );

    // Write watermark HTML to temp file
    let mut wm_html_file = Builder::new().suffix(".html").tempfile()?;
    wm_html_file.write_all(watermark_html.as_bytes())?;
    let wm_html_path = wm_html_file.into_temp_path();

    let wm_pdf_temp = Builder::new().suffix(".pdf").tempfile()?;
    let wm_pdf_path = helpers::path_to_str(wm_pdf_temp.path())?.to_string();

    // Generate watermark PDF with weasyprint
    helpers::run_tool(
        Command::new("weasyprint")
            .arg(helpers::path_to_str(&wm_html_path)?)
            .arg(&wm_pdf_path),
        "weasyprint",
        "Watermark PDF generation failed",
    )?;

    // Overlay watermark using qpdf
    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let output_path = helpers::path_to_str(output_temp.path())?.to_string();

    helpers::run_tool(
        Command::new("qpdf")
            .arg(helpers::path_to_str(&source_path)?)
            .arg("--overlay")
            .arg(&wm_pdf_path)
            .arg("--")
            .arg(&output_path),
        "qpdf",
        "Watermark overlay failed",
    )?;

    let produced = output_temp.into_temp_path();
    let response = helpers::finish_tool(&produced, client_id, pdf_name, output, "watermarked.pdf")?;

    Ok((produced, response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watermark_with_nothing_to_say_is_refused_rather_than_applied() {
        for empty in ["", "   ", "\n\t "] {
            match validate_text(empty) {
                Err(AppError::BadRequest(message)) => {
                    assert!(message.contains("unchanged"), "{}", message)
                }
                other => panic!("expected a bad request for {:?}, got {:?}", empty, other),
            }
        }
    }

    #[test]
    fn an_ordinary_mention_is_accepted_and_an_endless_one_is_not() {
        assert!(validate_text("CONFIDENTIEL").is_ok());
        assert!(validate_text("Confidentiel — ne pas diffuser · 2026").is_ok());
        // Counted in characters, not bytes: an accented mention is not shorter in French
        assert!(validate_text(&"é".repeat(200)).is_ok());
        assert!(validate_text(&"é".repeat(201)).is_err());
    }
}
