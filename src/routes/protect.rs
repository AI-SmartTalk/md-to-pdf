use crate::auth::ApiKey;
use crate::exec;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use std::io::Write;
use std::process::Command;
use tempfile::{Builder, TempPath};

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

    let ProtectRequest {
        pdf,
        password,
        client_id,
        pdf_name,
    } = req;

    // qpdf on a large document is a long blocking run: it belongs on a render slot, not on
    // a tokio worker that `/api/health` needs.
    let (output, download_url) =
        exec::offload(move || encrypt(&pdf, &password, client_id, pdf_name)).await?;

    helpers::deliver(output, download_url).await
}

fn encrypt(
    pdf: &str,
    password: &str,
    client_id: Option<String>,
    pdf_name: Option<String>,
) -> Result<(TempPath, Option<String>), AppError> {
    let source_path = helpers::resolve_pdf_path(pdf)?;

    let output_temp = Builder::new().suffix(".pdf").tempfile()?;
    let output_path = helpers::path_to_str(output_temp.path())?.to_string();

    // qpdf reads its arguments from an @argfile, one per line: the password never shows
    // up in the process list.
    let mut arg_file = Builder::new().suffix(".qpdf-args").tempfile()?;
    writeln!(
        arg_file,
        "--encrypt\n{password}\n{password}\n256\n--\n{input}\n{output}",
        password = password,
        input = helpers::path_to_str(&source_path)?,
        output = output_path,
    )?;
    let arg_file_path = arg_file.into_temp_path();

    helpers::run_tool(
        Command::new("qpdf").arg(format!("@{}", helpers::path_to_str(&arg_file_path)?)),
        "qpdf",
        "PDF encryption failed",
    )?;

    let output = output_temp.into_temp_path();
    let download_url = helpers::save_if_requested(&output, client_id, pdf_name)?;
    Ok((output, download_url))
}
