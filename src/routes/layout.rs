//! `POST /api/layout`: audit a PDF that already exists, without touching it.
//!
//! The same analysis the renderer runs on itself when `autolayout` is asked for, exposed on
//! its own so a document produced elsewhere can be checked before it is sent to a client.

use crate::auth::ApiKey;
use crate::exec;
use crate::helpers;
use crate::obs::{self, RequestId};
use crate::types::{AppError, LayoutReport};
use rocket::serde::json::Json;
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;

#[derive(Deserialize)]
pub struct LayoutRequest {
    /// A download URL of this service, as returned by any rendering endpoint
    pub pdf: String,
}

#[post("/layout", format = "json", data = "<req>")]
pub async fn analyze_layout(
    _key: ApiKey,
    trace: RequestId,
    req: Json<LayoutRequest>,
) -> Result<Json<LayoutReport>, AppError> {
    let source = helpers::resolve_pdf_path(&req.into_inner().pdf)?;

    let started = Instant::now();
    let report = match exec::offload(move || crate::layout::analyze(&source)).await {
        Ok(report) => report,
        Err(err) => {
            obs::event(
                obs::Level::Error,
                "layout failed",
                vec![
                    ("trace_id", json!(trace.0)),
                    ("route", json!("/api/layout")),
                    obs::err_field(err.kind(), &format!("{:?}", err)),
                ],
            );
            return Err(err);
        }
    };

    obs::event(
        obs::Level::Info,
        "layout",
        vec![
            ("trace_id", json!(trace.0)),
            ("route", json!("/api/layout")),
            ("pages", json!(report.pages)),
            ("layout_score", json!(report.score)),
            ("issues", json!(report.issues.len())),
            ("duration_ms", json!(started.elapsed().as_millis())),
        ],
    );

    Ok(Json(report))
}
