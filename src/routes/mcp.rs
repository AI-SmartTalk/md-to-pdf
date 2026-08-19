//! The Model Context Protocol surface.
//!
//! Every other endpoint of this service answers a program. This one answers a model: an
//! agent connects, reads the catalogue, and drives the document engine on its own. It is
//! the one place where the whole toolbelt is described in prose rather than in a reference
//! manual, because the caller reads the descriptions and decides from them.
//!
//! Transport is streamable HTTP with a plain JSON reply — no SSE. The protocol allows it,
//! every client supports it, and a server that never pushes has nothing to stream.
//!
//! Two rules shape everything below:
//!
//! - **A tool that fails answers `isError: true` inside its result, not a JSON-RPC error.**
//!   A protocol error is invisible to the model; a result it can read is a mistake it can
//!   correct on the next call. JSON-RPC errors are reserved for what the model did not
//!   write: an unreadable body, an unknown method, malformed `params`.
//! - **Everything a tool produces comes back as an `asset://` handle**, never as bytes.
//!   There is no socket to stream a file down here, and a handle is what lets the model
//!   chain five operations without ever moving a document through its own context.
//!
//! `document_preview` is the exception that justifies the module: it returns PNG image
//! blocks, which is the only way an agent ever *sees* the page it just produced.

use crate::auth::ApiKey;
use crate::config::config;
use crate::exec;
use crate::helpers;
use crate::pdfops;
use crate::pipeline::{self, RenderSpec, Source, UrlPolicy};
use crate::themes;
use crate::types::{AppError, BlockWarning, LayoutReport, PdfEngine, PdfOptions, ToolOutput};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::Either;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Revision of the protocol this server implements. Older clients are answered in the
/// version they asked for when we still speak it, because an agent that negotiates down is
/// a working agent and an agent that gets an unexpected version is a disconnected one.
const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

// JSON-RPC 2.0 error codes, as specified. Nothing here is ours to choose.
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

/// Pages one preview may return. A model pays for every image in its context window, and
/// four pages is already the point where it stops looking at them individually.
const MAX_PREVIEW_PAGES: usize = 4;
/// Enough to read body text on an A4 page, cheap enough to send several of them
const DEFAULT_PREVIEW_DPI: u32 = 110;
const MIN_PREVIEW_DPI: u32 = 36;
const MAX_PREVIEW_DPI: u32 = 150;
/// Ceiling on the raw PNG bytes one preview may carry, before base64 inflates it by 4/3.
/// Past this the answer stops being a preview and becomes a context-window incident.
const MAX_PREVIEW_BYTES: usize = 3 * 1024 * 1024;

/// A tool error is read by a model, and a Ghostscript stderr is a thousand lines of stack
const MAX_ERROR_CHARS: usize = 2000;

/// The catalogue, in the order it is advertised. Kept as a constant so the dispatcher can
/// name what it knows when a model invents a tool.
const TOOL_NAMES: [&str; 12] = [
    "document_render",
    "document_preview",
    "document_audit",
    "document_compose",
    "document_compress",
    "document_ocr",
    "document_extract",
    "document_pages",
    "document_convert_office",
    "document_attest",
    "document_verify",
    "themes_list",
];

// ------------ Transport ------------

/// The single JSON-RPC entry point.
///
/// Answers `202 Accepted` with no body when the payload held nothing but notifications —
/// that is what the specification asks for, and a client that gets a body back for a
/// notification treats the exchange as desynchronised.
#[post("/", format = "json", data = "<req>")]
pub async fn mcp_post(key: ApiKey, req: Json<Value>) -> Either<Json<Value>, Status> {
    // Declared once, here: the protocol chain below is deliberately free of Rocket, and
    // threading a key through six layers of JSON-RPC dispatch to reach the asset store
    // would put it back into the protocol. Every file these tools produce belongs to the
    // agent's key.
    match crate::exec::as_owner(key.0, answer(req.into_inner())).await {
        Some(response) => Either::Left(Json(response)),
        None => Either::Right(Status::Accepted),
    }
}

/// The whole protocol, free of Rocket. `None` means "202 Accepted, no body": the payload
/// held nothing but notifications, and a client that gets a body back for one treats the
/// exchange as desynchronised.
async fn answer(payload: Value) -> Option<Value> {
    match payload {
        // A batch: same handling, one response per request that had an id. A handful of
        // lines, and clients that always batch simply do not work without them.
        Value::Array(items) => {
            if items.is_empty() {
                return Some(error_response(
                    Value::Null,
                    INVALID_REQUEST,
                    "An empty batch carries no request".to_string(),
                ));
            }

            let mut answers = Vec::with_capacity(items.len());
            for item in items {
                if let Some(answer) = handle(item).await {
                    answers.push(answer);
                }
            }

            if answers.is_empty() {
                return None;
            }
            Some(Value::Array(answers))
        }
        object @ Value::Object(_) => handle(object).await,
        // A body that is neither an object nor an array cannot carry a call at all. A body
        // that is not JSON never reaches this handler: Rocket's guard rejects it with a 400
        // before the route runs, which is the same information at the HTTP layer.
        _ => Some(error_response(
            Value::Null,
            PARSE_ERROR,
            "The body is not a JSON-RPC request: expected an object, or an array of objects"
                .to_string(),
        )),
    }
}

/// What this server is, in a form a human reading the URL can act on.
///
/// Unauthenticated on purpose: it holds no data, and an integrator who cannot see the
/// endpoint exists cannot configure the key that opens it.
#[get("/")]
pub fn mcp_get() -> Json<Value> {
    let tools: Vec<Value> = catalogue()
        .into_iter()
        .map(|tool| json!({ "name": tool["name"], "description": tool["description"] }))
        .collect();

    Json(json!({
        "name": config().service_name,
        "product": "AI SmartTalk Documents",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "Model Context Protocol",
        "protocolVersion": PROTOCOL_VERSION,
        "transport": "Streamable HTTP, JSON-RPC 2.0 over POST to this same URL. Replies are \
                      plain JSON; this server never opens an SSE stream.",
        "authentication": "X-API-Key: <key>, or Authorization: Bearer <key>, on the POST. \
                           This description is open.",
        "methods": ["initialize", "notifications/initialized", "ping", "tools/list", "tools/call"],
        "tools": tools,
    }))
}

// ------------ JSON-RPC ------------

/// What a single element of the payload turned out to be
enum Incoming {
    /// A request with an id: it gets an answer
    Call {
        id: Value,
        method: String,
        params: Value,
    },
    /// A notification: it gets nothing back, whatever it asked for
    Notification,
    /// Malformed beyond the point where a method could be read
    Invalid(Value),
}

/// Read one element of the payload. Sync and free of any tool, so the protocol layer can
/// be tested without a PDF anywhere near it.
fn classify(item: &Value) -> Incoming {
    let Some(object) = item.as_object() else {
        return Incoming::Invalid(error_response(
            Value::Null,
            INVALID_REQUEST,
            "A JSON-RPC request must be an object".to_string(),
        ));
    };

    // `id` is read before anything is validated: an error answer that drops the id leaves
    // the client with a response it cannot match to a call.
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let is_notification = !object.contains_key("id");

    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        if is_notification {
            return Incoming::Notification;
        }
        return Incoming::Invalid(error_response(
            id,
            INVALID_REQUEST,
            "\"jsonrpc\" must be exactly \"2.0\"".to_string(),
        ));
    }

    let Some(method) = object.get("method").and_then(Value::as_str) else {
        if is_notification {
            return Incoming::Notification;
        }
        return Incoming::Invalid(error_response(
            id,
            INVALID_REQUEST,
            "\"method\" is missing or is not a string".to_string(),
        ));
    };

    if is_notification {
        return Incoming::Notification;
    }

    Incoming::Call {
        id,
        method: method.to_string(),
        params: object.get("params").cloned().unwrap_or(Value::Null),
    }
}

/// Route one request to its method. `None` means "answer nothing", which is the only
/// correct reply to a notification.
async fn handle(item: Value) -> Option<Value> {
    match classify(&item) {
        Incoming::Notification => None,
        Incoming::Invalid(response) => Some(response),
        Incoming::Call { id, method, params } => match dispatch(&method, params).await {
            Ok(result) => Some(success_response(id, result)),
            Err(err) => Some(error_response(id, err.code, err.message)),
        },
    }
}

/// A failure of the protocol itself, as opposed to a tool that ran and did not like its
/// arguments — those never come through here.
#[derive(Debug)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcError {
    fn new(code: i32, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
        }
    }
}

async fn dispatch(method: &str, params: Value) -> Result<Value, RpcError> {
    match method {
        "initialize" => Ok(initialize(&params)),
        // Answering an empty result rather than "method not found" costs nothing and
        // covers the clients that send it as a request instead of a notification.
        "notifications/initialized" | "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": catalogue() })),
        "tools/call" => call(params).await,
        other => Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!(
                "Unknown method \"{}\". This server implements initialize, \
                 notifications/initialized, ping, tools/list and tools/call.",
                clip(other, 64)
            ),
        )),
    }
}

fn initialize(params: &Value) -> Value {
    // Echo the client's version when we speak it. Answering our own newest revision to a
    // client that asked for an older one is how a working integration turns into a silent
    // capability mismatch.
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = match requested {
        Some(version) if SUPPORTED_PROTOCOLS.contains(&version) => version,
        _ => PROTOCOL_VERSION,
    };

    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": config().service_name,
            "title": "AI SmartTalk Documents",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions":
            "This server produces, inspects, transforms and proves PDF documents.\n\n\
             Files are referenced, never inlined. A reference is either \"asset://as_<32 \
             hex>\" — what an upload to POST /api/files or a previous tool call returned — \
             or \"/download/<client_id>/<name>.pdf\" for a document this service saved. \
             Every tool that produces a file answers with a new asset reference, so a chain \
             of operations never moves bytes through this conversation.\n\n\
             Assets expire; treat a reference as valid for the current task, not forever.\n\n\
             Two habits pay off here. Call document_preview to look at a page before \
             reporting a document as finished — it returns the rendered image, and it is \
             the only way to catch a layout that reads badly. Call document_compose rather \
             than document_render when the result has to satisfy a constraint (a page \
             limit, no table split across pages): it renders, audits, corrects and tells \
             you what it could not meet.",
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ------------ tools/call ------------

async fn call(params: Value) -> Result<Value, RpcError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(RpcError::new(
            INVALID_PARAMS,
            "\"params.name\" is missing: name the tool to call",
        ));
    };
    let name = name.to_string();

    // Absent arguments are an empty object, not an error: `themes_list` takes none.
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(value @ Value::Object(_)) => value.clone(),
        Some(_) => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "\"params.arguments\" must be an object",
            ))
        }
    };

    if !TOOL_NAMES.contains(&name.as_str()) {
        return Err(RpcError::new(
            METHOD_NOT_FOUND,
            format!(
                "Unknown tool \"{}\". Available: {}",
                clip(&name, 64),
                TOOL_NAMES.join(", ")
            ),
        ));
    }

    // From here the tool exists and the model wrote the arguments: whatever goes wrong is
    // something it can fix, so it comes back as a readable result rather than a protocol
    // error it would never see.
    Ok(match run(&name, arguments).await {
        Ok(content) => json!({ "content": content, "isError": false }),
        Err(err) => failure(err),
    })
}

async fn run(name: &str, args: Value) -> Result<Vec<Value>, AppError> {
    match name {
        "document_render" => render(args).await,
        "document_preview" => preview(args).await,
        "document_audit" => audit(args).await,
        "document_compose" => compose(args).await,
        "document_compress" => compress(args).await,
        "document_ocr" => ocr(args).await,
        "document_extract" => extract(args).await,
        "document_pages" => pages(args).await,
        "document_convert_office" => convert_office(args).await,
        "document_attest" => attest(args).await,
        "document_verify" => verify(args).await,
        "themes_list" => Ok(vec![json_block(&themes::list())?]),
        // `call` filtered on TOOL_NAMES already; this arm exists so the two lists cannot
        // drift into a panic.
        other => Err(AppError::BadRequest(format!("Unknown tool \"{}\"", other))),
    }
}

// ------------ The tools ------------

async fn render(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        markdown: Option<String>,
        html: Option<String>,
        template: Option<String>,
        data: Option<Value>,
        css: Option<String>,
        engine: Option<PdfEngine>,
        options: Option<PdfOptions>,
        header_html: Option<String>,
        footer_html: Option<String>,
        client_id: Option<String>,
        pdf_name: Option<String>,
    }

    let args: Args = decode(args, "document_render")?;
    let source = source_of(args.markdown, args.html, args.template, args.data)?;

    let spec = RenderSpec {
        source,
        css: args.css,
        engine: args.engine.unwrap_or_default(),
        options: args.options.unwrap_or_default(),
        header_html: args.header_html,
        footer_html: args.footer_html,
        header_template: None,
        footer_template: None,
        url_policy: UrlPolicy::default(),
    };

    struct Produced {
        response: crate::types::ToolResponse,
        layout: Option<LayoutReport>,
        warnings: Vec<BlockWarning>,
    }

    let produced = exec::offload(move || {
        let outcome = pipeline::render_blocking(spec)?;
        let pages = pdfops::page_count(&outcome.pdf)?;

        // Always an asset: there is no byte stream to hand a model, and a handle is what
        // lets it pass the document to the next tool.
        let mut response = helpers::finish_tool(
            &outcome.pdf,
            args.client_id,
            args.pdf_name,
            ToolOutput::Asset,
            "document.pdf",
        )?;
        response.pages = Some(pages);

        Ok(Produced {
            response,
            layout: outcome.layout,
            warnings: outcome.warnings,
        })
    })
    .await?;

    // The agent path keeps the same quality record as the human one: work done through MCP
    // belongs to the account whose key drove it, and shows up in the same list.
    crate::history::record(&produced.response, "markdown-to-pdf");

    let mut result = to_value(&produced.response)?;
    if let Some(report) = produced.layout {
        result["layout"] = to_value(&report)?;
    }
    if !produced.warnings.is_empty() {
        // Named apart from `warnings`, which the tool response uses for its own strings
        result["block_warnings"] = to_value(&produced.warnings)?;
    }

    Ok(vec![json_block(&result)?])
}

async fn preview(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        pdf: Option<String>,
        markdown: Option<String>,
        html: Option<String>,
        template: Option<String>,
        data: Option<Value>,
        css: Option<String>,
        options: Option<PdfOptions>,
        pages: Option<String>,
        dpi: Option<u32>,
    }

    let args: Args = decode(args, "document_preview")?;
    let (first, last) = page_range(args.pages.as_deref())?;
    let dpi = match args.dpi {
        Some(dpi) if !(MIN_PREVIEW_DPI..=MAX_PREVIEW_DPI).contains(&dpi) => {
            return Err(AppError::BadRequest(format!(
                "\"dpi\" must be between {} and {}",
                MIN_PREVIEW_DPI, MAX_PREVIEW_DPI
            )))
        }
        Some(dpi) => dpi,
        None => DEFAULT_PREVIEW_DPI,
    };

    // Rendering and rasterising share one slot: splitting them would make a preview queue
    // twice for a single answer.
    let existing = args.pdf.clone();
    let rendered = exec::offload(move || {
        let (path, keepalive) = match existing {
            Some(reference) => (helpers::resolve_pdf_source(&reference)?, None),
            None => {
                let source = source_of(args.markdown, args.html, args.template, args.data)?;
                let mut spec = RenderSpec::new(source);
                spec.css = args.css;
                spec.options = args.options.unwrap_or_default();
                let outcome = pipeline::render_blocking(spec)?;
                (outcome.pdf.to_path_buf(), Some(outcome.pdf))
            }
        };

        let total = pdfops::page_count(&path)?;
        if first > total {
            return Err(AppError::BadRequest(format!(
                "This document has {} page(s): page {} does not exist",
                total, first
            )));
        }

        let mut rasters = pdfops::rasterize(&path, first, last.min(total), dpi)?;
        rasters.sort_by_key(|raster| raster.page);
        // The temp file is only released once pdftoppm is done with it
        drop(keepalive);
        Ok((rasters, total))
    })
    .await?;

    let (rasters, total) = rendered;
    let mut content = Vec::new();
    let mut shown = Vec::new();
    let mut budget = MAX_PREVIEW_BYTES;

    for raster in &rasters {
        if raster.png.len() > budget {
            break;
        }
        budget -= raster.png.len();
        shown.push(raster.page);
        content.push(image_block(&raster.png));
    }

    let mut summary = format!(
        "{} of {} page(s) rendered at {} dpi, in document order: {}",
        shown.len(),
        total,
        dpi,
        shown
            .iter()
            .map(|page| page.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if shown.len() < rasters.len() {
        summary.push_str(
            ". The remaining pages were dropped to keep the answer small: ask for them by \
             range, or lower \"dpi\".",
        );
    }

    // The caption goes last so the images are the first thing the model sees
    content.push(text_block(summary));
    Ok(content)
}

async fn audit(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        pdf: String,
    }

    let args: Args = decode(args, "document_audit")?;
    let report = exec::offload(move || {
        let path = helpers::resolve_pdf_source(&args.pdf)?;
        crate::layout::analyze(&path)
    })
    .await?;

    Ok(vec![json_block(&report)?])
}

async fn compose(args: Value) -> Result<Vec<Value>, AppError> {
    let mut request: super::compose::ComposeRequest = decode(args, "document_compose")?;
    request.output = Some(ToolOutput::Asset);

    let (_pdf, response) = exec::offload(move || super::compose::run(request)).await?;
    Ok(vec![json_block(&response)?])
}

async fn compress(args: Value) -> Result<Vec<Value>, AppError> {
    let mut request: super::compress::CompressRequest = decode(args, "document_compress")?;
    request.output = Some(ToolOutput::Asset);

    let (_pdf, response) = exec::offload(move || super::compress::run(request)).await?;
    Ok(vec![json_block(&response)?])
}

async fn ocr(args: Value) -> Result<Vec<Value>, AppError> {
    let mut request: super::ocr::OcrRequest = decode(args, "document_ocr")?;
    request.output = Some(ToolOutput::Asset);

    let (_pdf, response) = exec::offload(move || super::ocr::run(request)).await?;
    Ok(vec![json_block(&response)?])
}

async fn extract(args: Value) -> Result<Vec<Value>, AppError> {
    let request: super::extract::ExtractRequest = decode(args, "document_extract")?;
    let response = exec::offload(move || super::extract::run(request)).await?;
    Ok(vec![json_block(&response)?])
}

async fn pages(args: Value) -> Result<Vec<Value>, AppError> {
    let mut request: super::pages::PagesRequest = decode(args, "document_pages")?;
    request.output = Some(ToolOutput::Asset);

    let (_pdf, response) = exec::offload(move || super::pages::run(request)).await?;
    Ok(vec![json_block(&response)?])
}

/// One tool for both directions, because a model asked to "convert this file" should not
/// have to know which of two endpoints its bytes belong to. The direction is read from
/// `to`: absent or `pdf` means "into a PDF", anything else means "out of a PDF".
async fn convert_office(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        file: String,
        to: Option<String>,
        client_id: Option<String>,
        pdf_name: Option<String>,
    }

    let args: Args = decode(args, "document_convert_office")?;
    let (client_id, pdf_name) = (args.client_id, args.pdf_name);
    let to = args.to.unwrap_or_else(|| "pdf".to_string());

    let response = if to == "pdf" {
        let request = super::office::OfficeToPdfRequest {
            file: args.file,
            client_id,
            pdf_name,
            output: Some(ToolOutput::Asset),
        };
        exec::offload(move || super::office::run(request)).await?.1
    } else {
        let request = super::office::PdfToOfficeRequest {
            pdf: args.file,
            to,
            client_id,
            pdf_name,
            output: Some(ToolOutput::Asset),
        };
        exec::offload(move || super::office::run_pdf_to_office(request))
            .await?
            .1
    };

    Ok(vec![json_block(&response)?])
}

async fn attest(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        pdf: String,
        engine: Option<String>,
        theme: Option<String>,
        pdf_variant: Option<String>,
        source_sha256: Option<String>,
        operations: Option<Vec<String>>,
    }

    let args: Args = decode(args, "document_attest")?;

    if let Some(ref hash) = args.source_sha256 {
        // It is copied verbatim into a signed record: a malformed value would be sealed
        // and then puzzle whoever reads it back.
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::BadRequest(
                "\"source_sha256\" must be 64 hexadecimal characters".to_string(),
            ));
        }
    }

    #[derive(Serialize)]
    struct Sealed {
        attestation: String,
        claims: crate::attest::Attestation,
    }

    let sealed = exec::offload(move || {
        let path = helpers::resolve_pdf_source(&args.pdf)?;
        let pages = pdfops::page_count(&path)?;

        let mut claims = crate::attest::Attestation::of(&path, pages)?;
        claims.engine = args.engine;
        claims.theme = args.theme;
        claims.pdf_variant = args.pdf_variant;
        claims.source_sha256 = args.source_sha256;
        if let Some(operations) = args.operations {
            // Bounded and cleaned: these end up in a signed record and in the logs
            claims.operations = operations
                .into_iter()
                .take(16)
                .map(|op| {
                    op.chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                        .take(32)
                        .collect()
                })
                .filter(|op: &String| !op.is_empty())
                .collect();
        }

        Ok(Sealed {
            attestation: claims.seal()?,
            claims,
        })
    })
    .await?;

    Ok(vec![json_block(&sealed)?])
}

async fn verify(args: Value) -> Result<Vec<Value>, AppError> {
    #[derive(Deserialize)]
    struct Args {
        pdf: String,
        attestation: String,
    }

    let args: Args = decode(args, "document_verify")?;
    let verification = exec::offload(move || {
        let path = helpers::resolve_pdf_source(&args.pdf)?;
        crate::attest::verify(&args.attestation, &path)
    })
    .await?;

    Ok(vec![json_block(&verification)?])
}

// ------------ The catalogue ------------

/// The tool descriptions are read by a model, not by a developer: each says what the tool
/// guarantees and, where it matters more, what it does not.
fn catalogue() -> Vec<Value> {
    vec![
        tool(
            "document_render",
            "Render Markdown (or raw HTML, or a Tera template plus its data) into a PDF with \
             the AI SmartTalk document engine. Supports branded themes, a table of contents, \
             page numbers, cover pages, and ```chart``` / ```mermaid``` fenced blocks which \
             become vector figures. Returns an asset:// handle for the PDF and, when \
             \"options.autolayout\" is true, the layout report. Guarantees byte-identical \
             output for identical input. Does NOT guarantee the document fits any page \
             budget or that no table is cut across pages — use document_compose when that \
             matters, and document_preview to look at the result.",
            json!({
                "type": "object",
                "properties": {
                    "markdown": { "type": "string", "description": "The document, in Markdown. Provide exactly one of markdown, html, or template+data." },
                    "html": { "type": "string", "description": "A complete HTML document, rendered as-is." },
                    "template": { "type": "string", "description": "A Tera template; requires \"data\"." },
                    "data": { "type": "object", "description": "Values the template is rendered with." },
                    "css": { "type": "string", "description": "Extra CSS, applied after the theme so it wins." },
                    "engine": engine_schema(),
                    "options": options_schema(),
                    "header_html": { "type": "string", "description": "HTML repeated at the top of every page." },
                    "footer_html": { "type": "string", "description": "HTML repeated at the bottom of every page." },
                    "client_id": { "type": "string", "description": "With \"pdf_name\", also saves the PDF under a stable /download URL. Both are required together." },
                    "pdf_name": { "type": "string", "description": "File name for the saved copy, e.g. \"rapport.pdf\"." }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "document_preview",
            "Render pages as PNG images and return them, so you can actually see the \
             document instead of inferring it. Give either \"pdf\" to look at a file that \
             already exists, or a source (markdown / html / template+data) to render and \
             look at in one call. Returns up to 4 page images plus a caption naming which \
             pages came back. This is the only way to catch a layout that is technically \
             valid and reads badly; use it before telling a user a document is finished. It \
             produces no file and modifies nothing.",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("Preview an existing document instead of rendering one."),
                    "markdown": { "type": "string", "description": "Render this Markdown, then preview it." },
                    "html": { "type": "string" },
                    "template": { "type": "string", "description": "A Tera template; requires \"data\"." },
                    "data": { "type": "object" },
                    "css": { "type": "string" },
                    "options": options_schema(),
                    "pages": {
                        "type": "string",
                        "description": "Which pages to show: \"1\" (default), \"2-5\", or \"all\". At most 4 pages come back in one call.",
                        "pattern": "^(all|[0-9]+(-[0-9]+)?)$"
                    },
                    "dpi": {
                        "type": "integer",
                        "description": "Rasterisation resolution. 110 is enough to read body text; raise it only to inspect fine detail, and expect fewer pages to fit in the answer.",
                        "minimum": MIN_PREVIEW_DPI,
                        "maximum": MAX_PREVIEW_DPI,
                        "default": DEFAULT_PREVIEW_DPI
                    }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "document_audit",
            "Inspect a PDF that already exists and report what is wrong with its layout: a \
             score out of 100, and one entry per issue — text overflowing its column, a \
             blank page, a heading stranded at the foot of a page, a table cut across a \
             break — each with its page number and, where it can be located, its bounding \
             box in PDF points. Reads only what the text layer says; a scanned page carries \
             no text and will look empty, so run document_ocr first if the document is an \
             image. Changes nothing.",
            json!({
                "type": "object",
                "properties": { "pdf": pdf_reference("The document to inspect.") },
                "required": ["pdf"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_compose",
            "Render a document against a contract instead of hoping for the best. You state \
             constraints — at most N pages, no table split across a break, a minimum layout \
             score — and the engine renders, audits, applies corrective CSS, and re-renders \
             until they hold or its passes run out. Always returns the best document it \
             produced, plus \"verdict\" (\"met\" or \"unmet\"), the log of every pass, and \
             \"unmet\": the constraints it could not satisfy. Entirely deterministic. Prefer \
             this over document_render whenever the result has to obey a rule; note that an \
             unmet contract is a normal, successful answer, not an error.",
            json!({
                "type": "object",
                "properties": {
                    "markdown": { "type": "string", "description": "Provide exactly one of markdown, html, or template+data." },
                    "html": { "type": "string" },
                    "template": { "type": "string", "description": "A Tera template; requires \"data\"." },
                    "data": { "type": "object" },
                    "css": { "type": "string" },
                    "engine": engine_schema(),
                    "options": options_schema(),
                    "header_html": { "type": "string" },
                    "footer_html": { "type": "string" },
                    "constraints": {
                        "type": "object",
                        "description": "What the finished document must satisfy. Every field is optional; with none, this is a render with a report.",
                        "properties": {
                            "max_pages": { "type": "integer", "minimum": 1, "description": "Refuse to settle for a document longer than this." },
                            "min_pages": { "type": "integer", "minimum": 1 },
                            "no_split_tables": { "type": "boolean", "description": "No table may be cut across a page break." },
                            "no_orphan_headings": { "type": "boolean", "description": "No heading may sit alone at the foot of a page." },
                            "no_blank_pages": { "type": "boolean" },
                            "no_overflow": { "type": "boolean", "description": "Nothing may spill outside its column or the page box." },
                            "min_layout_score": { "type": "integer", "minimum": 0, "maximum": 100, "description": "Floor on the layout score, 0..100." }
                        },
                        "additionalProperties": false
                    },
                    "max_passes": {
                        "type": "integer",
                        "description": "Corrective renders to attempt. Each one costs a full render.",
                        "minimum": 1,
                        "maximum": 5,
                        "default": 3
                    },
                    "attest": { "type": "boolean", "description": "Seal the result with a signed attestation, returned alongside it.", "default": false },
                    "client_id": { "type": "string", "description": "With \"pdf_name\", also saves the PDF under a stable /download URL." },
                    "pdf_name": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "document_compress",
            "Shrink a PDF with Ghostscript, and say what it cost. Returns the new asset plus \
             a verdict measuring the two things compression destroys silently: the page count \
             and the extractable text. A file that came out bigger than it went in is \
             discarded and the original returned instead, which the verdict states. On a \
             scanned document, \"screen\" will usually make the text unreadable — start at \
             \"ebook\" and check the verdict before going lower.",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The document to compress."),
                    "level": {
                        "type": "string",
                        "description": "Ghostscript preset, from most aggressive to most faithful.",
                        "enum": ["screen", "ebook", "printer", "prepress"],
                        "default": "ebook"
                    },
                    "dpi": {
                        "type": "integer",
                        "description": "Override the preset's image resolution. Below about 100 dpi, scanned text stops being legible.",
                        "minimum": 36,
                        "maximum": 1200
                    },
                    "client_id": { "type": "string", "description": "With \"pdf_name\", also saves the result under a stable /download URL." },
                    "pdf_name": { "type": "string" }
                },
                "required": ["pdf"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_ocr",
            "Add a searchable text layer to a scanned PDF with ocrmypdf and Tesseract. \
             Returns the new asset plus a verdict naming the pages that came back with too \
             little text to be trusted — which is the answer to \"which of these scans went \
             wrong\". Only the languages installed in this image can be used. OCR is a \
             guess, not a transcription: the verdict tells you how confident it is, it does \
             not promise correctness. Capped at 300 pages per call; split a longer document \
             with document_pages first.",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The scanned document."),
                    "languages": {
                        "type": "array",
                        "description": "Languages to recognise, most likely first. Defaults to [\"fra\", \"eng\"].",
                        "items": { "type": "string", "enum": ["fra", "eng", "deu", "spa", "ita"] },
                        "minItems": 1
                    },
                    "mode": {
                        "type": "string",
                        "description": "auto: only read pages that carry no text (safe default). force: rasterise and read everything again, losing the original text layer. redo: replace a text layer a previous OCR produced, keeping a native one.",
                        "enum": ["auto", "force", "redo"],
                        "default": "auto"
                    },
                    "pdfa": { "type": "boolean", "description": "Also produce a PDF/A archival file in the same pass.", "default": false },
                    "client_id": { "type": "string" },
                    "pdf_name": { "type": "string" }
                },
                "required": ["pdf"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_extract",
            "Read a PDF back out as Markdown, plain text, or structured JSON blocks, with \
             recognised tables returned separately as rows. Reads the existing text layer \
             only — it does not run OCR, so a scanned document comes back nearly empty and \
             you should call document_ocr first. Table detection is a column heuristic over \
             character positions: it is right on ruled and well-aligned tables and \
             approximate on the rest. Long documents are truncated, and say so; ask for the \
             next pages with \"pages\".",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The document to read."),
                    "format": {
                        "type": "string",
                        "description": "markdown keeps headings and tables; text is the raw reading; json returns one entry per page with its blocks.",
                        "enum": ["markdown", "text", "json"],
                        "default": "markdown"
                    },
                    "pages": {
                        "type": "string",
                        "description": "\"1\", \"2-5\" or \"all\" (default).",
                        "pattern": "^(all|[0-9]+(-[0-9]+)?)$"
                    },
                    "layout": {
                        "type": "boolean",
                        "description": "Keep the column layout of the page, which is what makes tables recoverable. Turn it off for reading order instead, at the cost of every table.",
                        "default": true
                    }
                },
                "required": ["pdf"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_pages",
            "Reorganise the pages of a PDF: extract a selection, delete pages, reorder them, \
             rotate by a quarter turn, or split the document into several files. Lossless — \
             page content is copied, never re-encoded. Returns one asset, or a list of assets \
             for \"split\". Page numbers are 1-based and a range specification looks like \
             \"1,3,5-9\", \"2-\", \"all\", \"odd\" or \"even\".",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The document to reorganise."),
                    "op": {
                        "type": "string",
                        "description": "extract and delete need \"pages\"; reorder needs \"order\"; rotate needs \"angle\"; split optionally takes \"every\".",
                        "enum": ["extract", "delete", "reorder", "rotate", "split"]
                    },
                    "pages": {
                        "type": "string",
                        "description": "Which pages the operation applies to: \"1,3,5-9\", \"2-\", \"all\", \"odd\", \"even\"."
                    },
                    "order": {
                        "type": "string",
                        "description": "For \"reorder\": the complete new order, listing every page of the document exactly once, e.g. \"3,1,2\"."
                    },
                    "angle": {
                        "type": "integer",
                        "description": "For \"rotate\": a quarter turn, clockwise when positive.",
                        "enum": [90, 180, 270, -90, -180, -270]
                    },
                    "every": {
                        "type": "integer",
                        "description": "For \"split\": pages per output file. Defaults to one file per page.",
                        "minimum": 1
                    },
                    "client_id": { "type": "string" },
                    "pdf_name": { "type": "string" }
                },
                "required": ["pdf", "op"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_convert_office",
            "Convert between office formats and PDF with LibreOffice. Leave \"to\" out (or \
             set it to \"pdf\") to turn a docx / xlsx / pptx / odt / ods / odp into a PDF; \
             set it to docx, xlsx or pptx to go the other way. Converting a PDF back into an \
             editable document is a reconstruction, not a recovery: a PDF holds character \
             positions, not paragraphs, tables or styles, so the structure is guessed and \
             the result must be reread before it is sent to anyone. The verdict reports how \
             much text survived. The forward direction (into PDF) is faithful.",
            json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "The source file, as \"asset://as_<32 hex>\". A PDF may also be given as a \"/download/<client_id>/<name>.pdf\" path when converting out of PDF."
                    },
                    "to": {
                        "type": "string",
                        "description": "Target format. \"pdf\" (the default) converts an office document into a PDF; the others convert a PDF into that format.",
                        "enum": ["pdf", "docx", "xlsx", "pptx"],
                        "default": "pdf"
                    },
                    "client_id": { "type": "string" },
                    "pdf_name": { "type": "string" }
                },
                "required": ["file"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_attest",
            "Issue a signed attestation for a PDF: a single line recording its SHA-256, its \
             size, its page count, when it was issued, and whatever provenance you pass in. \
             It proves that this exact file is the one this service attested and that nobody \
             has changed a byte since — check it later with document_verify. It says nothing \
             about who wrote the document or whether its contents are true, and it is not a \
             PAdES signature embedded in the file.",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The document to attest."),
                    "engine": { "type": "string", "description": "Renderer that produced it, recorded as-is." },
                    "theme": { "type": "string", "description": "Theme and version, e.g. \"aismarttalk@1\"." },
                    "pdf_variant": { "type": "string", "description": "Conformance variant, e.g. \"pdf/a-2b\"." },
                    "source_sha256": { "type": "string", "description": "SHA-256 of the source document, if you kept it. Exactly 64 hexadecimal characters.", "pattern": "^[0-9a-fA-F]{64}$" },
                    "operations": {
                        "type": "array",
                        "description": "What was applied, in order, e.g. [\"convert\", \"compress\"]. Kept to 16 short identifiers.",
                        "items": { "type": "string" }
                    }
                },
                "required": ["pdf"],
                "additionalProperties": false
            }),
        ),
        tool(
            "document_verify",
            "Check a sealed attestation against the file it claims to describe. Answers one \
             of four verdicts: \"valid\" (genuine record, untouched file), \"altered\" \
             (genuine record, the document changed since), \"forged\" (the record itself does \
             not check out, or came from another deployment), or \"unreadable\". Needs \
             nothing but the file and the attestation line — there is no registry to consult. \
             A mismatch is a normal answer, not a tool failure.",
            json!({
                "type": "object",
                "properties": {
                    "pdf": pdf_reference("The document the attestation is supposed to describe."),
                    "attestation": { "type": "string", "description": "The sealed line, of the form \"v1.<payload>.<signature>\"." }
                },
                "required": ["pdf", "attestation"],
                "additionalProperties": false
            }),
        ),
        tool(
            "themes_list",
            "List the branded themes installed on this deployment: name, version, label, \
             fonts, colour palette, and whether the theme provides a cover, a header and a \
             footer. Pass \"name@version\" as \"options.theme\" to document_render or \
             document_compose to pin one; a bare name follows the latest version. Call this \
             before guessing a theme name — an unknown theme is refused, not ignored.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

/// The reference every file-consuming tool takes, described once so the two accepted forms
/// are worded identically everywhere.
fn pdf_reference(purpose: &str) -> Value {
    json!({
        "type": "string",
        "description": format!(
            "{} Either \"asset://as_<32 hex>\", as returned by an upload or by a previous \
             tool call, or a \"/download/<client_id>/<name>.pdf\" path this service issued.",
            purpose
        )
    })
}

fn engine_schema() -> Value {
    json!({
        "type": "string",
        "description": "Rendering engine. weasyprint is the default and the only one with full CSS support; change it only for a document that needs it.",
        "enum": ["weasyprint", "wkhtmltopdf", "pdflatex"],
        "default": "weasyprint"
    })
}

/// Mirrors `PdfOptions` field for field. A schema that drifts from the struct is worse than
/// no schema at all: the model calls with confidence and the field is silently dropped.
fn options_schema() -> Value {
    json!({
        "type": "object",
        "description": "Page setup and document furniture. Every field is optional and omitting the object renders the defaults.",
        "properties": {
            "paper_size": { "type": "string", "enum": ["a4", "a3", "letter"], "default": "a4" },
            "orientation": { "type": "string", "enum": ["portrait", "landscape"], "default": "portrait" },
            "margins": {
                "type": "object",
                "description": "CSS lengths, e.g. \"2cm\" or \"18mm\".",
                "properties": {
                    "top": { "type": "string" },
                    "bottom": { "type": "string" },
                    "left": { "type": "string" },
                    "right": { "type": "string" }
                },
                "additionalProperties": false
            },
            "page_numbers": { "type": "boolean", "default": false },
            "page_number_format": { "type": "string", "description": "Template for the footer number, e.g. \"Page {page} / {pages}\"." },
            "toc": { "type": "boolean", "description": "Insert a table of contents built from the headings.", "default": false },
            "toc_depth": { "type": "integer", "description": "Deepest heading level listed in the table of contents.", "minimum": 1, "maximum": 6 },
            "watermark": { "type": "string", "description": "Text drawn diagonally across every page." },
            "theme": { "type": "string", "description": "\"name\" for the latest version, or \"name@version\" to pin one. Call themes_list for what is installed." },
            "autolayout": { "type": "boolean", "description": "Audit the result and apply corrective CSS, and return the layout report with it.", "default": false },
            "censor_label": { "type": "string", "description": "Caption shown over blurred CENSOR regions." },
            "charts": { "type": "boolean", "description": "Expand ```chart``` and ```mermaid``` fenced blocks into vector figures.", "default": true },
            "cover": {
                "type": "object",
                "description": "Adds a cover page. Requires a theme that provides one.",
                "properties": {
                    "title": { "type": "string" },
                    "subtitle": { "type": "string" },
                    "logo": { "type": "string", "description": "URL or data: URI of the logo." },
                    "date": { "type": "string" }
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    })
}

// ------------ Plumbing ------------

fn text_block(text: impl Into<String>) -> Value {
    json!({ "type": "text", "text": text.into() })
}

fn image_block(png: &[u8]) -> Value {
    json!({ "type": "image", "data": helpers::base64(png), "mimeType": "image/png" })
}

/// A result the model reads: pretty-printed, because a model parses indented JSON more
/// reliably than one long line.
fn json_block<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_string_pretty(value)
        .map(text_block)
        .map_err(|e| AppError::ProcessFailed {
            message: "Could not serialise the tool result".to_string(),
            stderr: e.to_string(),
        })
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(|e| AppError::ProcessFailed {
        message: "Could not serialise the tool result".to_string(),
        stderr: e.to_string(),
    })
}

/// Turn a tool failure into something the model can act on. The error class is prefixed so
/// it can tell "I wrote nonsense" from "the service is saturated" without parsing prose.
fn failure(err: AppError) -> Value {
    let kind = err.kind();
    let detail = match err {
        AppError::BadRequest(message)
        | AppError::NotFound(message)
        | AppError::TemplateError(message)
        | AppError::Timeout(message)
        | AppError::Unauthorized(message)
        | AppError::TooManyRequests(message) => message,
        AppError::Conflict(message) => message,
        AppError::ProcessFailed { message, stderr } => {
            if stderr.is_empty() {
                message
            } else {
                format!("{}: {}", message, stderr)
            }
        }
        AppError::Io(e) => e.to_string(),
        AppError::Upstream { service, details } => format!("{} unavailable: {}", service, details),
    };

    json!({
        "content": [text_block(format!("{}: {}", kind, clip(&detail, MAX_ERROR_CHARS)))],
        "isError": true
    })
}

fn decode<T: DeserializeOwned>(args: Value, tool: &str) -> Result<T, AppError> {
    serde_json::from_value(args)
        .map_err(|e| AppError::BadRequest(format!("Invalid arguments for {}: {}", tool, e)))
}

/// The three source shapes every rendering tool accepts, resolved in one place so they
/// cannot disagree between tools.
fn source_of(
    markdown: Option<String>,
    html: Option<String>,
    template: Option<String>,
    data: Option<Value>,
) -> Result<Source, AppError> {
    if let Some(markdown) = markdown {
        return Ok(Source::Markdown(markdown));
    }
    if let Some(template) = template {
        let data = data.ok_or_else(|| {
            AppError::BadRequest("\"template\" requires a \"data\" object".to_string())
        })?;
        return Ok(Source::Template { template, data });
    }
    if let Some(html) = html {
        return Ok(Source::Html(html));
    }

    Err(AppError::BadRequest(
        "Provide one of: markdown, html, or template+data".to_string(),
    ))
}

/// `"3"`, `"2-5"` or `"all"`. Bounded to `MAX_PREVIEW_PAGES` here rather than after
/// rasterising: a hundred-page range would otherwise spawn a hundred PNGs to throw away.
fn page_range(spec: Option<&str>) -> Result<(usize, usize), AppError> {
    let spec = match spec {
        Some(spec) => spec.trim(),
        None => return Ok((1, 1)),
    };

    if spec.is_empty() || spec.eq_ignore_ascii_case("all") {
        return Ok((1, MAX_PREVIEW_PAGES));
    }

    let malformed = || {
        AppError::BadRequest(format!(
            "\"pages\" must be \"1\", \"2-5\" or \"all\", not \"{}\"",
            clip(spec, 32)
        ))
    };

    let (first, last) = match spec.split_once('-') {
        Some((first, last)) => (
            first.trim().parse::<usize>().map_err(|_| malformed())?,
            last.trim().parse::<usize>().map_err(|_| malformed())?,
        ),
        None => {
            let page = spec.parse::<usize>().map_err(|_| malformed())?;
            (page, page)
        }
    };

    if first == 0 || last < first {
        return Err(malformed());
    }

    Ok((first, last.min(first + MAX_PREVIEW_PAGES - 1)))
}

/// Truncate on a character boundary: these strings carry French error messages and cutting
/// mid-codepoint would panic.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: Value, method: &str) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method })
    }

    // ------------ JSON-RPC decoding ------------

    #[test]
    fn a_request_with_an_id_is_a_call() {
        match classify(&request(json!(1), "tools/list")) {
            Incoming::Call { id, method, params } => {
                assert_eq!(id, json!(1));
                assert_eq!(method, "tools/list");
                assert_eq!(params, Value::Null);
            }
            _ => panic!("expected a call"),
        }
    }

    /// A string id is as legal as a number, and dropping it would leave the client unable
    /// to match the answer to its call.
    #[test]
    fn a_string_id_survives_the_round_trip() {
        let response = success_response(json!("abc"), json!({}));
        assert_eq!(response["id"], json!("abc"));
        assert_eq!(response["jsonrpc"], json!("2.0"));
    }

    #[test]
    fn a_request_without_an_id_is_a_notification() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(matches!(classify(&notification), Incoming::Notification));
    }

    /// An explicit null id is still an id: the answer carries it back
    #[test]
    fn an_explicit_null_id_still_gets_an_answer() {
        let call = json!({ "jsonrpc": "2.0", "id": null, "method": "ping" });
        assert!(matches!(classify(&call), Incoming::Call { .. }));
    }

    #[test]
    fn a_wrong_protocol_version_is_an_invalid_request() {
        let call = json!({ "jsonrpc": "1.0", "id": 7, "method": "ping" });
        match classify(&call) {
            Incoming::Invalid(response) => {
                assert_eq!(response["error"]["code"], json!(INVALID_REQUEST));
                assert_eq!(response["id"], json!(7));
            }
            _ => panic!("expected an invalid request"),
        }
    }

    #[test]
    fn a_missing_method_is_an_invalid_request() {
        let call = json!({ "jsonrpc": "2.0", "id": 7 });
        match classify(&call) {
            Incoming::Invalid(response) => {
                assert_eq!(response["error"]["code"], json!(INVALID_REQUEST));
            }
            _ => panic!("expected an invalid request"),
        }
    }

    /// A malformed notification stays silent: answering it would desynchronise the client
    #[test]
    fn a_malformed_notification_gets_no_answer() {
        let notification = json!({ "jsonrpc": "1.0", "method": "whatever" });
        assert!(matches!(classify(&notification), Incoming::Notification));
    }

    #[test]
    fn a_non_object_element_is_an_invalid_request() {
        match classify(&json!("hello")) {
            Incoming::Invalid(response) => {
                assert_eq!(response["error"]["code"], json!(INVALID_REQUEST));
                assert_eq!(response["id"], Value::Null);
            }
            _ => panic!("expected an invalid request"),
        }
    }

    // ------------ Encoding ------------

    #[test]
    fn an_error_response_carries_the_code_and_the_id() {
        let response = error_response(json!(3), METHOD_NOT_FOUND, "nope".to_string());
        assert_eq!(response["jsonrpc"], json!("2.0"));
        assert_eq!(response["id"], json!(3));
        assert_eq!(response["error"]["code"], json!(METHOD_NOT_FOUND));
        assert_eq!(response["error"]["message"], json!("nope"));
        assert!(response.get("result").is_none());
    }

    #[test]
    fn a_success_response_carries_no_error_field() {
        let response = success_response(json!(1), json!({ "tools": [] }));
        assert!(response.get("error").is_none());
        assert_eq!(response["result"]["tools"], json!([]));
    }

    // ------------ Dispatch ------------

    #[rocket::async_test]
    async fn an_unknown_method_is_method_not_found() {
        let err = dispatch("tools/invent", Value::Null).await.err().unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[rocket::async_test]
    async fn a_notification_produces_no_response() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(notification).await.is_none());
    }

    #[rocket::async_test]
    async fn ping_answers_an_empty_result() {
        let response = handle(request(json!(9), "ping")).await.unwrap();
        assert_eq!(response["result"], json!({}));
    }

    #[rocket::async_test]
    async fn tools_list_answers_the_whole_catalogue() {
        let response = handle(request(json!(1), "tools/list")).await.unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), TOOL_NAMES.len());
    }

    // ------------ Batches ------------

    #[rocket::async_test]
    async fn a_batch_answers_one_response_per_request_that_had_an_id() {
        let batch = json!([
            request(json!(1), "ping"),
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            request(json!(2), "tools/list"),
        ]);

        let answers = answer(batch).await.unwrap();
        let answers = answers.as_array().unwrap();
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0]["id"], json!(1));
        assert_eq!(answers[1]["id"], json!(2));
    }

    /// A batch of nothing but notifications gets 202 Accepted with no body
    #[rocket::async_test]
    async fn a_batch_of_notifications_answers_nothing() {
        let batch = json!([
            { "jsonrpc": "2.0", "method": "notifications/initialized" },
            { "jsonrpc": "2.0", "method": "notifications/cancelled" },
        ]);
        assert!(answer(batch).await.is_none());
    }

    #[rocket::async_test]
    async fn an_empty_batch_is_an_invalid_request() {
        let response = answer(json!([])).await.unwrap();
        assert_eq!(response["error"]["code"], json!(INVALID_REQUEST));
    }

    /// A body that carries no request at all: neither an object nor an array
    #[rocket::async_test]
    async fn an_unusable_body_is_a_parse_error() {
        let response = answer(json!("initialize")).await.unwrap();
        assert_eq!(response["error"]["code"], json!(PARSE_ERROR));
        assert_eq!(response["id"], Value::Null);
    }

    #[rocket::async_test]
    async fn a_lone_notification_answers_nothing() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(answer(notification).await.is_none());
    }

    // ------------ tools/call ------------

    #[rocket::async_test]
    async fn a_call_without_a_name_is_invalid_params() {
        let err = call(json!({ "arguments": {} })).await.err().unwrap();
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[rocket::async_test]
    async fn non_object_arguments_are_invalid_params() {
        let err = call(json!({ "name": "themes_list", "arguments": "nope" }))
            .await
            .err()
            .unwrap();
        assert_eq!(err.code, INVALID_PARAMS);
    }

    /// An unknown tool is a protocol error, not a tool error: nothing ran
    #[rocket::async_test]
    async fn an_unknown_tool_is_method_not_found() {
        let err = call(json!({ "name": "document_invent" }))
            .await
            .err()
            .unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    /// The rule the whole module rests on: a tool that refuses its arguments answers a
    /// result the model can read, not an error it cannot see.
    #[rocket::async_test]
    async fn bad_arguments_come_back_as_a_readable_tool_error() {
        let result = call(json!({ "name": "document_audit", "arguments": { "pdf": 42 } }))
            .await
            .unwrap();
        assert_eq!(result["isError"], json!(true));
        assert_eq!(result["content"][0]["type"], json!("text"));
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("document_audit"));
    }

    #[rocket::async_test]
    async fn themes_list_needs_no_arguments() {
        let result = call(json!({ "name": "themes_list" })).await.unwrap();
        assert_eq!(result["isError"], json!(false));
        assert_eq!(result["content"][0]["type"], json!("text"));
    }

    // ------------ initialize ------------

    #[test]
    fn initialize_echoes_a_protocol_version_we_speak() {
        let result = initialize(&json!({ "protocolVersion": "2024-11-05" }));
        assert_eq!(result["protocolVersion"], json!("2024-11-05"));
        assert_eq!(result["capabilities"]["tools"]["listChanged"], json!(false));
    }

    #[test]
    fn initialize_falls_back_to_our_own_version() {
        for params in [json!({ "protocolVersion": "1999-01-01" }), json!({})] {
            assert_eq!(
                initialize(&params)["protocolVersion"],
                json!(PROTOCOL_VERSION)
            );
        }
    }

    // ------------ Schemas ------------

    /// A wrong schema is worse than a missing tool: the model calls it with confidence.
    #[test]
    fn every_tool_declares_a_usable_object_schema() {
        for tool in catalogue() {
            let name = tool["name"].as_str().expect("a name");
            let schema = &tool["inputSchema"];

            assert_eq!(schema["type"], json!("object"), "{}", name);
            let properties = schema["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{} has no properties object", name));

            // Anything the schema requires has to be something it also describes
            if let Some(required) = schema.get("required") {
                for field in required.as_array().expect("required is an array") {
                    let field = field.as_str().expect("a field name");
                    assert!(
                        properties.contains_key(field),
                        "{} requires \"{}\" but does not describe it",
                        name,
                        field
                    );
                }
            }

            for (field, definition) in properties {
                assert!(
                    definition.get("type").is_some(),
                    "{}.{} has no type",
                    name,
                    field
                );
            }
        }
    }

    #[test]
    fn the_catalogue_matches_the_dispatcher() {
        let names: Vec<String> = catalogue()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        let expected: Vec<String> = TOOL_NAMES.iter().map(|name| name.to_string()).collect();
        assert_eq!(names, expected);
    }

    /// Tool names reach a model as identifiers: no dots, no spaces, no surprises
    #[test]
    fn tool_names_are_plain_identifiers() {
        for name in TOOL_NAMES {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{}",
                name
            );
        }
    }

    /// The descriptions are the entire interface: a one-liner leaves the model guessing
    #[test]
    fn every_description_says_enough_to_choose_the_tool() {
        for tool in catalogue() {
            let name = tool["name"].as_str().unwrap();
            let description = tool["description"].as_str().unwrap_or_default();
            assert!(description.len() > 120, "{} is described too thinly", name);
        }
    }

    /// The rendering options are the one schema written by hand against a struct; the
    /// fields listed here are `PdfOptions` in full.
    #[test]
    fn the_options_schema_covers_every_rendering_option() {
        let schema = options_schema();
        let properties = schema["properties"].as_object().unwrap();

        for field in [
            "paper_size",
            "orientation",
            "margins",
            "page_numbers",
            "page_number_format",
            "toc",
            "toc_depth",
            "watermark",
            "theme",
            "autolayout",
            "censor_label",
            "charts",
            "cover",
        ] {
            assert!(properties.contains_key(field), "missing {}", field);
        }
        assert_eq!(properties.len(), 13);
    }

    /// The enums have to be the values the routes actually parse, or the model is told to
    /// send something that will be refused.
    #[test]
    fn the_enums_match_what_the_routes_accept() {
        let tools = catalogue();
        let find = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["name"] == json!(name))
                .unwrap()
                .clone()
        };

        assert_eq!(
            find("document_compress")["inputSchema"]["properties"]["level"]["enum"],
            json!(["screen", "ebook", "printer", "prepress"])
        );
        assert_eq!(
            find("document_ocr")["inputSchema"]["properties"]["mode"]["enum"],
            json!(["auto", "force", "redo"])
        );
        assert_eq!(
            find("document_pages")["inputSchema"]["properties"]["op"]["enum"],
            json!(["extract", "delete", "reorder", "rotate", "split"])
        );
        assert_eq!(
            find("document_extract")["inputSchema"]["properties"]["format"]["enum"],
            json!(["markdown", "text", "json"])
        );
        assert_eq!(
            find("document_convert_office")["inputSchema"]["properties"]["to"]["enum"],
            json!(["pdf", "docx", "xlsx", "pptx"])
        );
    }

    // ------------ Content blocks ------------

    /// Without an image block the agent stays blind, and the tool loses its whole point
    #[test]
    fn a_preview_image_is_a_base64_png_block() {
        let block = image_block(&[0x89, 0x50, 0x4e, 0x47]);
        assert_eq!(block["type"], json!("image"));
        assert_eq!(block["mimeType"], json!("image/png"));
        assert_eq!(block["data"], json!("iVBORw=="));
    }

    #[test]
    fn a_tool_failure_names_its_class_and_stays_bounded() {
        let long = "x".repeat(MAX_ERROR_CHARS * 2);
        let result = failure(AppError::BadRequest(long));
        assert_eq!(result["isError"], json!(true));

        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("bad_request: "));
        assert!(text.chars().count() < MAX_ERROR_CHARS + 32);
    }

    // ------------ Helpers ------------

    #[test]
    fn a_page_range_defaults_to_the_first_page() {
        assert_eq!(page_range(None).unwrap(), (1, 1));
        assert_eq!(page_range(Some("3")).unwrap(), (3, 3));
        assert_eq!(page_range(Some(" 2-4 ")).unwrap(), (2, 4));
    }

    #[test]
    fn a_page_range_is_capped_so_a_preview_stays_a_preview() {
        assert_eq!(page_range(Some("1-200")).unwrap(), (1, MAX_PREVIEW_PAGES));
        assert_eq!(page_range(Some("all")).unwrap(), (1, MAX_PREVIEW_PAGES));
    }

    #[test]
    fn a_page_range_refuses_what_it_cannot_read() {
        for spec in ["0", "5-2", "abc", "1-", "-3", "1,2"] {
            assert!(page_range(Some(spec)).is_err(), "{}", spec);
        }
    }

    #[test]
    fn a_source_needs_exactly_one_shape() {
        assert!(source_of(Some("# hi".into()), None, None, None).is_ok());
        assert!(source_of(None, Some("<p>hi</p>".into()), None, None).is_ok());
        assert!(source_of(None, None, Some("{{ a }}".into()), Some(json!({}))).is_ok());
        // A template without data would render the literal placeholders
        assert!(source_of(None, None, Some("{{ a }}".into()), None).is_err());
        assert!(source_of(None, None, None, None).is_err());
    }

    #[test]
    fn clipping_never_cuts_a_character_in_half() {
        assert_eq!(clip("été", 10), "été");
        assert_eq!(clip("étérnité", 3), "été…");
    }
}
