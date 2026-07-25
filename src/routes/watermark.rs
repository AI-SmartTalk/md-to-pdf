use crate::auth::ApiKey;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::io::Write;
use std::process::Command;
use tempfile::Builder;

#[post("/watermark", format = "json", data = "<req>")]
pub async fn watermark(
    _key: ApiKey,
    req: Json<WatermarkRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    let source_path = helpers::resolve_pdf_path(&req.pdf)?;

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
        text = helpers::escape_html(&req.text)
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

    if let (Some(client_id), Some(pdf_name)) = (req.client_id, req.pdf_name) {
        let download_url = helpers::save_pdf(output_temp.path(), &client_id, &pdf_name)?;
        Ok(Either::Right(Json(ConvertResponse { download_url })))
    } else {
        Ok(Either::Left(
            NamedFile::open(output_temp.path())
                .await
                .map_err(AppError::Io)?,
        ))
    }
}
