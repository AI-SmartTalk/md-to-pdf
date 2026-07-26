use crate::auth::ApiKey;
use crate::obs::RequestId;
use crate::pipeline::{self, RenderSpec, Source};
use crate::types::*;
use rocket::fs::NamedFile;
use rocket::serde::json::Json;
use rocket::Either;

#[post("/render", format = "json", data = "<req>")]
pub async fn render(
    _key: ApiKey,
    trace: RequestId,
    req: Json<RenderRequest>,
) -> Result<Either<NamedFile, Json<ConvertResponse>>, AppError> {
    let req = req.into_inner();

    let mut spec = RenderSpec::new(Source::Template {
        template: req.template,
        data: req.data,
    });
    spec.css = req.css;
    spec.options = req.options.unwrap_or_default();

    let outcome = pipeline::render_traced(spec, trace.0).await?;
    pipeline::respond(outcome, req.client_id, req.pdf_name).await
}
