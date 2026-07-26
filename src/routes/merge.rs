use crate::auth::ApiKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::process::Command;
use tempfile::{Builder, TempPath};

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

    let MergeRequest {
        pdfs,
        client_id,
        pdf_name,
    } = req;

    // pdfunite is as blocking as pandoc is: run it on a render slot, or a handful of merges
    // pins every tokio worker and `/api/health` stops answering.
    let (pdf, download_url) = exec::offload(move || unite(&pdfs, client_id, pdf_name)).await?;

    helpers::deliver(pdf, download_url).await
}

fn unite(
    pdfs: &[String],
    client_id: Option<String>,
    pdf_name: Option<String>,
) -> Result<(TempPath, Option<String>), AppError> {
    let mut resolved_paths = Vec::new();
    for pdf_url in pdfs {
        resolved_paths.push(helpers::resolve_pdf_path(pdf_url)?);
    }

    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let output_path = helpers::path_to_str(output_temp.path())?.to_string();

    let mut cmd = Command::new("pdfunite");
    for path in &resolved_paths {
        cmd.arg(helpers::path_to_str(path)?);
    }
    cmd.arg(&output_path);

    helpers::run_tool(&mut cmd, "pdfunite", "PDF merge failed")?;

    // No destination given: the merged file is streamed back instead of leaving an orphan
    // copy behind in public/pdf.
    let output = output_temp.into_temp_path();
    let download_url = helpers::save_if_requested(&output, client_id, pdf_name)?;

    Ok((output, download_url))
}
