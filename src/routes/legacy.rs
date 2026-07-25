use crate::helpers;
use crate::types::*;
use rocket::form::Form;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;

/// Original FormData endpoint, kept for backward compatibility.
/// Same contract as before: a PDF body, or a JSON download_url when client_id + pdf_name
/// are provided, and plain-text errors.
#[post("/", data = "<form>")]
pub async fn convert(
    form: Form<ConvertForm>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, ConvertError> {
    let form_data = form.into_inner();

    let processed_markdown = helpers::process_censor(&form_data.markdown);
    debug!(
        "Legacy convert: {} bytes of markdown",
        processed_markdown.len()
    );

    let engine = form_data.engine.unwrap_or_default();

    let css_path = helpers::build_css(form_data.css.as_deref(), None)?;
    let css_path_str = helpers::path_to_str(&css_path)?;

    let header_temp = helpers::resolve_header_footer(None, form_data.header_template.as_deref())?;
    let footer_temp = helpers::resolve_header_footer(None, form_data.footer_template.as_deref())?;

    let pdf_path = helpers::run_pandoc(
        &processed_markdown,
        css_path_str,
        &engine,
        None,
        header_temp.as_ref().and_then(|p| p.to_str()),
        footer_temp.as_ref().and_then(|p| p.to_str()),
    )?;

    if let (Some(client_id), Some(pdf_name)) = (form_data.client_id, form_data.pdf_name) {
        let download_url = helpers::save_pdf(&pdf_path, &client_id, &pdf_name)?;
        Ok(Either::Right(Json(ConvertResponse { download_url })))
    } else {
        Ok(Either::Left(NamedFile::open(&pdf_path).await?))
    }
}
