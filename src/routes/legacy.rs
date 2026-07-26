use crate::config::config;
use crate::pipeline::{self, RenderSpec, Source, UrlPolicy};
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

    debug!(
        "Legacy convert: {} bytes of markdown",
        form_data.markdown.len()
    );

    let engine = allowed_engine(form_data.engine.unwrap_or_default())?;

    let mut spec = RenderSpec::new(Source::Markdown(form_data.markdown));
    spec.css = form_data.css;
    spec.engine = engine;
    spec.header_template = form_data.header_template;
    spec.footer_template = form_data.footer_template;
    // This endpoint carries no options, so a caller cannot ask for block expansion — and
    // must therefore not get it: a ```mermaid fence has always come out as a code listing,
    // and an anonymous body of them is what turns one request into thousands of calls.
    spec.options.charts = Some(false);
    // Nor did it ever refuse a whole document over one unreachable link.
    spec.url_policy = UrlPolicy::Warn;

    let outcome = pipeline::render(spec).await?;
    Ok(pipeline::respond(outcome, form_data.client_id, form_data.pdf_name).await?)
}

/// This endpoint takes no API key, so it only offers the engine that renders behind the
/// fetcher guard of `deploy/weasyprint-safe.py`. wkhtmltopdf follows HTTP redirects on its
/// own and pdflatex opens local files on its own; the URL scan sees neither, since it only
/// reads the document as it was posted. `PDF_UNAUTHENTICATED_ENGINES` reopens them.
fn allowed_engine(engine: PdfEngine) -> Result<PdfEngine, AppError> {
    let name = engine.to_string();
    if config().unauthenticated_engines.contains(&name) {
        return Ok(engine);
    }

    Err(AppError::BadRequest(format!(
        "engine \"{}\" requires an API key, use /api/convert",
        name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_guarded_engine_is_open() {
        assert!(allowed_engine(PdfEngine::Weasyprint).is_ok());
        assert!(allowed_engine(PdfEngine::Wkhtmltopdf).is_err());
        assert!(allowed_engine(PdfEngine::Pdflatex).is_err());
    }
}
