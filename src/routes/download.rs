use crate::helpers;
use crate::types::PdfResponse;
use rocket::fs::NamedFile;
use rocket::http::{ContentType, Header};
use rocket::response::Response;

#[get("/<client_id>/<pdf_name>?<signature>")]
pub async fn download_pdf(
    client_id: &str,
    pdf_name: &str,
    signature: Option<&str>,
) -> Option<PdfResponse> {
    let signature = signature?;
    // Both segments are used to build a filesystem path: reject anything but a plain name
    let client_id = helpers::sanitize_path_component(client_id, "client_id").ok()?;
    let pdf_name = helpers::sanitize_path_component(pdf_name, "pdf_name").ok()?;
    if !crate::sign::verify_download(&client_id, &pdf_name, signature) {
        return None;
    }

    let path = helpers::pdf_root().join(&client_id).join(&pdf_name);

    NamedFile::open(path).await.ok().map(|file| {
        let download_name = if !pdf_name.ends_with(".pdf") {
            format!("{}.pdf", pdf_name)
        } else {
            pdf_name
        };

        PdfResponse(
            Response::build()
                .header(ContentType::PDF)
                .header(Header::new(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", download_name),
                ))
                .header(Header::new("Cache-Control", "private, no-store"))
                .header(Header::new("Referrer-Policy", "no-referrer"))
                .sized_body(None, file.take_file())
                .finalize(),
        )
    })
}
