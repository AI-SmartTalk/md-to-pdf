use crate::auth::PublicOrKey;
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
    key: PublicOrKey,
    req: Json<MergeRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
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
        output,
    } = req;
    let output = output.unwrap_or_default();

    // pdfunite is as blocking as pandoc is: run it on a render slot, or a handful of merges
    // pins every tokio worker and `/api/health` stops answering.
    let (pdf, response) = exec::as_owner(
        key.0,
        exec::offload(move || unite(&pdfs, client_id, pdf_name, output)),
    )
    .await?;

    helpers::deliver_tool(pdf, response).await
}

fn unite(
    pdfs: &[String],
    client_id: Option<String>,
    pdf_name: Option<String>,
    output: ToolOutput,
) -> Result<(TempPath, ToolResponse), AppError> {
    let mut resolved_paths = Vec::new();
    for pdf_url in pdfs {
        resolved_paths.push(helpers::resolve_pdf_source(pdf_url)?);
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
    let produced = output_temp.into_temp_path();

    // Measured before `finish_tool`, which moves the file away when an asset was asked for.
    // The count is not optional any more either: pdfunite claimed success, so a merged file
    // whose pages cannot be counted is a broken result, and a franker error than a silent
    // `"pages": null` on a document the caller is about to archive.
    let pages = crate::pdfops::page_count(&produced)?;

    let mut response = helpers::finish_tool(&produced, client_id, pdf_name, output, "merged.pdf")?;
    response.pages = Some(pages);

    Ok((produced, response))
}

#[cfg(test)]
mod tests {
    /// `assets::store` renames the file it is handed, so a measurement taken after
    /// `finish_tool` reads a path that no longer exists the day `/tmp` and the asset root
    /// share a filesystem. Only the order of the two statements prevents it, and `unite`
    /// cannot be exercised in a unit test — it needs pdfunite — so the order itself is what
    /// is asserted.
    #[test]
    fn the_page_count_is_taken_before_the_file_is_published() {
        let source = include_str!("merge.rs");
        let measured = source
            .find("page_count(&produced)")
            .expect("unite measures the file it produced");
        let published = source
            .find("helpers::finish_tool")
            .expect("unite publishes the file it produced");

        assert!(
            measured < published,
            "page_count must run before finish_tool moves the file away"
        );
    }
}
