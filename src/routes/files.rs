//! The way in.
//!
//! Every tool this service offers used to work only on documents it had produced itself,
//! because the only way to name a file was `/download/<client_id>/<name>`. This endpoint
//! is the other half: a caller hands over bytes, gets an opaque handle, and every tool
//! accepts that handle wherever it accepted a download path.
//!
//! What comes back is deliberately small — an id, a type we read from the bytes, a size, a
//! page count and an expiry. Not a library card.
//!
//! An asset belongs to the key that uploaded it, and to no other. The id is opaque, but it
//! is not a secret: it travels through logs, support tickets and `Referer` headers, and one
//! that gets out must not hand a third party the right to read — or to delete — someone
//! else's document. `public` is an owner like any other, so a deployment running the free
//! tier isolates its visitors from its integrations rather than pooling them.

use crate::assets::{self, AssetMeta};
use crate::auth::PublicOrKey;
use crate::exec;
use crate::types::AppError;
use rocket::form::Form;
use rocket::fs::{NamedFile, TempFile};
use rocket::http::{ContentType, Status};
use rocket::serde::json::Json;
use serde::Serialize;
use tempfile::Builder;

/// Files may be sent one at a time or all at once: `file=@a.pdf -F file=@b.pdf` is the
/// shape every HTTP client already knows how to produce.
#[derive(FromForm)]
pub struct UploadForm<'r> {
    pub file: Vec<TempFile<'r>>,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub files: Vec<AssetMeta>,
}

#[post("/files", data = "<form>")]
pub async fn upload(
    key: PublicOrKey,
    form: Form<UploadForm<'_>>,
) -> Result<(Status, Json<UploadResponse>), AppError> {
    let mut uploads = form.into_inner().file;

    if uploads.is_empty() {
        return Err(AppError::BadRequest(
            "No file received: send at least one multipart field named \"file\"".to_string(),
        ));
    }

    let mut files = Vec::with_capacity(uploads.len());

    for upload in uploads.iter_mut() {
        // The client's file name is only ever a label. `assets::display_name` strips it of
        // anything that looks like a path before it is stored or echoed back.
        let name = upload
            .raw_name()
            .map(|raw| raw.dangerous_unsafe_unsanitized_raw().to_string())
            .unwrap_or_else(|| "upload".to_string());

        // Rocket may have kept a small upload in memory; persisting it gives every branch
        // the same thing: one path on disk that the store can move into place.
        let staged = Builder::new()
            .suffix(".upload")
            .tempfile()?
            .into_temp_path();
        upload.persist_to(&staged).await.map_err(AppError::Io)?;

        let path = staged.to_path_buf();
        let owner = key.0;
        // Storing counts the pages of a PDF, which spawns pdfinfo: blocking work belongs on
        // a render slot, never on a tokio worker that /api/health needs.
        let meta = exec::offload(move || assets::store_as(&path, &name, Some(owner))).await?;
        files.push(meta);
    }

    Ok((Status::Created, Json(UploadResponse { files })))
}

/// Read a file back. An asset that belongs to another key answers 404, not 403: a 403 would
/// confirm that the id names something, which is the one thing a leaked id must not buy.
#[get("/files/<id>")]
pub async fn fetch(key: PublicOrKey, id: &str) -> Result<(ContentType, NamedFile), AppError> {
    let meta = assets::meta_as(id, key.0)?;
    let path = assets::path_as(id, key.0)?;

    let content_type =
        ContentType::parse_flexible(meta.kind.content_type()).unwrap_or(ContentType::Binary);

    let file = NamedFile::open(&path).await.map_err(AppError::Io)?;
    Ok((content_type, file))
}

#[get("/files/<id>/meta")]
pub fn describe(key: PublicOrKey, id: &str) -> Result<Json<AssetMeta>, AppError> {
    Ok(Json(assets::meta_as(id, key.0)?))
}

/// Forget a file before its expiry. Answering 204 rather than a body is deliberate: there
/// is nothing left to describe.
#[delete("/files/<id>")]
pub fn forget(key: PublicOrKey, id: &str) -> Result<Status, AppError> {
    assets::delete_as(id, key.0)?;
    Ok(Status::NoContent)
}
