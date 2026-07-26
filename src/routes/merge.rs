use crate::auth::ApiKey;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::process::Command;
use tempfile::Builder;

#[post("/merge", format = "json", data = "<req>")]
pub async fn merge(
    _key: ApiKey,
    req: Json<MergeRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    if req.pdfs.len() < 2 {
        return Err(AppError::BadRequest(
            "At least 2 PDFs are required for merging".to_string(),
        ));
    }

    // Resolve all PDF paths
    let mut resolved_paths = Vec::new();
    for pdf_url in &req.pdfs {
        resolved_paths.push(helpers::resolve_pdf_path(pdf_url)?);
    }

    // Use pdfunite to merge
    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let output_path = helpers::path_to_str(output_temp.path())?.to_string();

    let mut cmd = Command::new("pdfunite");
    for path in &resolved_paths {
        cmd.arg(helpers::path_to_str(path)?);
    }
    cmd.arg(&output_path);

    helpers::run_tool(&mut cmd, "pdfunite", "PDF merge failed")?;

    if let (Some(client_id), Some(pdf_name)) = (req.client_id, req.pdf_name) {
        let download_url = helpers::save_pdf(output_temp.path(), &client_id, &pdf_name)?;
        Ok(Either::Right(Json(ConvertResponse { download_url })))
    } else {
        // No destination given: stream the merged file back instead of leaving an
        // orphan copy behind in public/pdf.
        Ok(Either::Left(
            NamedFile::open(output_temp.path())
                .await
                .map_err(AppError::Io)?,
        ))
    }
}
