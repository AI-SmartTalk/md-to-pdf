use core::fmt;
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::io;

// ------------ PDF Engine enum ------------

#[derive(Debug, Clone, Default, Deserialize, FromFormField)]
#[serde(rename_all = "lowercase")]
pub enum PdfEngine {
    #[default]
    Weasyprint,
    Wkhtmltopdf,
    Pdflatex,
}

impl Display for PdfEngine {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            PdfEngine::Weasyprint => write!(f, "weasyprint"),
            PdfEngine::Wkhtmltopdf => write!(f, "wkhtmltopdf"),
            PdfEngine::Pdflatex => write!(f, "pdflatex"),
        }
    }
}

// ------------ Paper Size ------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaperSize {
    #[default]
    A4,
    A3,
    Letter,
}

impl Display for PaperSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PaperSize::A4 => write!(f, "A4"),
            PaperSize::A3 => write!(f, "A3"),
            PaperSize::Letter => write!(f, "letter"),
        }
    }
}

// ------------ Orientation ------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

// ------------ Margins ------------

#[derive(Debug, Clone, Deserialize)]
pub struct Margins {
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
}

// ------------ PDF Options ------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PdfOptions {
    pub paper_size: Option<PaperSize>,
    pub orientation: Option<Orientation>,
    pub margins: Option<Margins>,
    pub page_numbers: Option<bool>,
    pub page_number_format: Option<String>,
    pub toc: Option<bool>,
    pub toc_depth: Option<u8>,
    pub watermark: Option<String>,
}

// ------------ Legacy Form Data (backward compat) ------------

#[derive(FromForm)]
pub struct ConvertForm {
    pub markdown: String,
    pub css: Option<String>,
    pub engine: Option<PdfEngine>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

// ------------ JSON Request Types ------------

#[derive(Deserialize)]
pub struct ConvertRequest {
    pub markdown: String,
    pub css: Option<String>,
    pub engine: Option<PdfEngine>,
    pub options: Option<PdfOptions>,
    pub header_html: Option<String>,
    pub footer_html: Option<String>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Deserialize)]
pub struct RenderRequest {
    pub template: String,
    pub data: serde_json::Value,
    pub css: Option<String>,
    pub options: Option<PdfOptions>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Deserialize)]
pub struct HtmlToPdfRequest {
    pub html: String,
    pub css: Option<String>,
    pub options: Option<PdfOptions>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Deserialize)]
pub struct PreviewRequest {
    pub markdown: Option<String>,
    pub html: Option<String>,
    pub template: Option<String>,
    pub data: Option<serde_json::Value>,
    pub css: Option<String>,
    pub engine: Option<PdfEngine>,
    pub options: Option<PdfOptions>,
    pub header_html: Option<String>,
    pub footer_html: Option<String>,
    pub header_template: Option<String>,
    pub footer_template: Option<String>,
}

#[derive(Deserialize)]
pub struct MergeRequest {
    pub pdfs: Vec<String>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Deserialize)]
pub struct WatermarkRequest {
    pub pdf: String,
    pub text: String,
    pub opacity: Option<f32>,
    pub angle: Option<f32>,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ProtectRequest {
    pub pdf: String,
    pub password: String,
    pub client_id: Option<String>,
    pub pdf_name: Option<String>,
}

// ------------ JSON Response Types ------------

#[derive(Serialize)]
pub struct ConvertResponse {
    pub download_url: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub engines: Vec<String>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: String,
}

// ------------ App Error ------------

#[derive(Debug)]
pub enum AppError {
    ProcessFailed { message: String, stderr: String },
    Io(io::Error),
    BadRequest(String),
    NotFound(String),
    TemplateError(String),
    Timeout(String),
    Unauthorized(String),
}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> AppError {
        AppError::Io(err)
    }
}

impl From<tera::Error> for AppError {
    fn from(err: tera::Error) -> AppError {
        AppError::TemplateError(err.to_string())
    }
}

impl<'r> Responder<'r, 'static> for AppError {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        let (status, error, details) = match self {
            AppError::ProcessFailed { message, stderr } => {
                (Status::InternalServerError, message, stderr)
            }
            AppError::Io(err) => (
                Status::InternalServerError,
                "IO error".to_string(),
                err.to_string(),
            ),
            AppError::BadRequest(msg) => (Status::BadRequest, "Bad request".to_string(), msg),
            AppError::NotFound(msg) => (Status::NotFound, "Not found".to_string(), msg),
            AppError::TemplateError(msg) => (Status::BadRequest, "Template error".to_string(), msg),
            AppError::Timeout(msg) => (Status::GatewayTimeout, "Timeout".to_string(), msg),
            AppError::Unauthorized(msg) => (Status::Unauthorized, "Unauthorized".to_string(), msg),
        };

        let body_str = serde_json::to_string(&ErrorResponse { error, details })
            .unwrap_or_else(|_| r#"{"error":"Internal error","details":""}"#.to_string());

        Response::build()
            .header(ContentType::JSON)
            .status(status)
            .sized_body(body_str.len(), io::Cursor::new(body_str))
            .ok()
    }
}

// ------------ PdfResponse wrapper (for download with headers) ------------

pub struct PdfResponse(pub Response<'static>);

impl<'r> Responder<'r, 'static> for PdfResponse {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        Ok(self.0)
    }
}

// ------------ Legacy ConvertError (backward compat for POST /) ------------

#[derive(Debug)]
pub enum ConvertError {
    Message(Status, String),
    IO(#[allow(dead_code)] io::Error),
}

impl From<io::Error> for ConvertError {
    fn from(err: io::Error) -> ConvertError {
        ConvertError::IO(err)
    }
}

/// The legacy endpoint answers in plain text, so AppError is flattened into a message
/// while keeping the status the JSON API would have returned.
impl From<AppError> for ConvertError {
    fn from(err: AppError) -> ConvertError {
        match err {
            AppError::Io(e) => ConvertError::IO(e),
            AppError::ProcessFailed { message, stderr } => ConvertError::Message(
                Status::BadRequest,
                if stderr.is_empty() { message } else { stderr },
            ),
            AppError::BadRequest(msg) => ConvertError::Message(Status::BadRequest, msg),
            AppError::NotFound(msg) => ConvertError::Message(Status::NotFound, msg),
            AppError::TemplateError(msg) => ConvertError::Message(Status::BadRequest, msg),
            AppError::Timeout(msg) => ConvertError::Message(Status::GatewayTimeout, msg),
            AppError::Unauthorized(msg) => ConvertError::Message(Status::Unauthorized, msg),
        }
    }
}

impl<'r> Responder<'r, 'static> for ConvertError {
    fn respond_to(self, _: &Request) -> response::Result<'static> {
        let mut builder = Response::build();
        match self {
            ConvertError::Message(status, message) => builder
                .header(ContentType::Plain)
                .sized_body(message.len(), io::Cursor::new(message))
                .status(status),
            ConvertError::IO(_) => builder.status(Status::InternalServerError),
        };
        builder.ok()
    }
}
