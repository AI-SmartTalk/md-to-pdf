use crate::auth::ApiKey;
use crate::obs::RequestId;
use crate::pipeline::{self, RenderSpec, Source, UrlPolicy};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;

#[post("/convert", format = "json", data = "<req>")]
pub async fn convert(
    _key: ApiKey,
    trace: RequestId,
    req: Json<ConvertRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    let spec = RenderSpec {
        source: Source::Markdown(req.markdown),
        css: req.css,
        engine: req.engine.unwrap_or_default(),
        options: req.options.unwrap_or_default(),
        header_html: req.header_html,
        footer_html: req.footer_html,
        header_template: req.header_template,
        footer_template: req.footer_template,
        url_policy: UrlPolicy::default(),
    };

    let outcome = pipeline::render_traced(spec, trace.0).await?;
    pipeline::respond(outcome, req.client_id, req.pdf_name).await
}
