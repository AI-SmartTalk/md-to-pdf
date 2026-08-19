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

#[post("/protect", format = "json", data = "<req>")]
pub async fn protect(
    key: PublicOrKey,
    req: Json<ProtectRequest>,
) -> Result<Either<NamedFile, Json<ToolResponse>>, AppError> {
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
        output,
    } = req;
    let output = output.unwrap_or_default();

    // qpdf on a large document is a long blocking run: it belongs on a render slot, not on
    // a tokio worker that `/api/health` needs.
    let (produced, response) = exec::as_owner(
        key.0,
        exec::offload(move || encrypt(&pdf, &password, client_id, pdf_name, output)),
    )
    .await?;

    helpers::deliver_tool(produced, response, "protect").await
}

fn encrypt(
    pdf: &str,
    password: &str,
    client_id: Option<String>,
    pdf_name: Option<String>,
    output: ToolOutput,
) -> Result<(TempPath, ToolResponse), AppError> {
    let source_path = helpers::resolve_pdf_source(pdf)?;

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

    let produced = output_temp.into_temp_path();
    let response = helpers::finish_tool(&produced, client_id, pdf_name, output, "protected.pdf")?;
    Ok((produced, response))
}
