use crate::auth::ApiKey;
use crate::obs::RequestId;
use crate::pipeline::{self, RenderSpec, Source};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;

#[post("/html-to-pdf", format = "json", data = "<req>")]
pub async fn html_to_pdf(
    _key: ApiKey,
    trace: RequestId,
    req: Json<HtmlToPdfRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    let mut spec = RenderSpec::new(Source::Html(req.html));
    spec.css = req.css;
    spec.options = req.options.unwrap_or_default();

    let outcome = pipeline::render_traced(spec, trace.0).await?;
    pipeline::respond(outcome, req.client_id, req.pdf_name).await
}
