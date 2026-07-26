use crate::auth::ApiKey;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;

#[post("/html-to-pdf", format = "json", data = "<req>")]
pub async fn html_to_pdf(
    _key: ApiKey,
    req: Json<HtmlToPdfRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    let processed_html = helpers::process_censor(&req.html);

    let css_path = helpers::build_css(req.css.as_deref(), req.options.as_ref())?;
    let css_path_str = helpers::path_to_str(&css_path)?;

    let pdf_path = helpers::run_weasyprint(&processed_html, css_path_str)?;

    if let (Some(client_id), Some(pdf_name)) = (req.client_id, req.pdf_name) {
        let download_url = helpers::save_pdf(&pdf_path, &client_id, &pdf_name)?;
        Ok(Either::Right(Json(ConvertResponse { download_url })))
    } else {
        Ok(Either::Left(
            NamedFile::open(&pdf_path).await.map_err(AppError::Io)?,
        ))
    }
}
