use crate::auth::ApiKey;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::io::Write;
use std::process::Command;
use tempfile::Builder;

#[post("/protect", format = "json", data = "<req>")]
pub async fn protect(
    _key: ApiKey,
    req: Json<ProtectRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    if req.password.is_empty() {
        return Err(AppError::BadRequest(
            "\"password\" must not be empty".to_string(),
        ));
    }

    if req.password.contains(['\n', '\r']) {
        return Err(AppError::BadRequest(
            "\"password\" must not contain line breaks".to_string(),
        ));
    }

    let source_path = helpers::resolve_pdf_path(&req.pdf)?;

    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let output_path = helpers::path_to_str(output_temp.path())?.to_string();

    // qpdf reads its arguments from an @argfile, one per line: the password never shows
    // up in the process list.
    let mut arg_file = Builder::new().suffix(".qpdf-args").tempfile()?;
    writeln!(
        arg_file,
        "--encrypt\n{password}\n{password}\n256\n--\n{input}\n{output}",
        password = req.password,
        input = helpers::path_to_str(&source_path)?,
        output = output_path,
    )?;
    let arg_file_path = arg_file.into_temp_path();

    helpers::run_tool(
        Command::new("qpdf").arg(format!("@{}", helpers::path_to_str(&arg_file_path)?)),
        "qpdf",
        "PDF encryption failed",
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
