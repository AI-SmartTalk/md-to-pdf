use crate::auth::ApiKey;
use crate::helpers;
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;
use tera::{Context, Tera};

#[post("/render", format = "json", data = "<req>")]
pub async fn render(
    _key: ApiKey,
    req: Json<RenderRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    // Validate that data is a JSON object
    if !req.data.is_object() {
        return Err(AppError::BadRequest(
            "\"data\" must be a JSON object".to_string(),
        ));
    }

    // CENSOR tags are expanded before Tera runs: `{{CENSOR}}` would otherwise be read as
    // an undefined template variable.
    let template = helpers::process_censor(&req.template);

    // Render the Tera template with the provided data
    let context = Context::from_value(req.data)?;
    let rendered_html = Tera::one_off(&template, &context, true)?;

    let css_path = helpers::build_css(req.css.as_deref(), req.options.as_ref())?;
    let css_path_str = helpers::path_to_str(&css_path)?;

    // Use weasyprint directly (no pandoc needed for pre-rendered HTML)
    let pdf_path = helpers::run_weasyprint(&rendered_html, css_path_str)?;

    if let (Some(client_id), Some(pdf_name)) = (req.client_id, req.pdf_name) {
        let download_url = helpers::save_pdf(&pdf_path, &client_id, &pdf_name)?;
        Ok(Either::Right(Json(ConvertResponse { download_url })))
    } else {
        Ok(Either::Left(
            NamedFile::open(&pdf_path).await.map_err(AppError::Io)?,
        ))
    }
}
