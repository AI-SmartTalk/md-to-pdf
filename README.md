# 🚀 md-to-pdf by AI SmartTalk

**A powerful fork of the original md-to-pdf with advanced features for professional document generation.**

This enhanced version, maintained by [AI SmartTalk](https://aismarttalk.tech), extends the original markdown-to-PDF converter with sophisticated features including customizable headers/footers and intelligent content censoring capabilities.

## ✨ Enhanced Features

- **🔒 Document Censoring:** Selectively blur confidential sections while maintaining document layout
- **📄 Customizable Headers & Footers:** Apply professional templates to your documents 
- **🎨 Improved Styling:** Enhanced CSS for better typography and visual presentation
- **🔄 Multiple PDF Engines:** Support for Weasyprint, Wkhtmltopdf, and Pdflatex

## 🔍 How to Use Document Censoring

Easily hide sensitive information in your PDFs with our censoring feature:

```markdown
# Document with Confidential Information

## Public Section
This content will remain visible to everyone.

## Restricted Information
{{CENSOR}}

## Conclusion
Back to publicly visible content.
```

The `{{CENSOR}}` tag can be used in any of these formats:
- `{{CENSOR}}`
- `<CENSOR>`
- `{{ CENSOR }}`
- `{{CENSOR }}`
- `{{ CENSOR}}`

When converted, censored sections appear as blurred areas, clearly indicating restricted content while preserving document flow.

## 🏷️ Header & Footer Templates

Create professional documents with custom headers and footers:

1. Place your custom template files in the `templates` folder with `.html` extension
2. Reference them in your API calls with `header_template` and `footer_template` parameters

Example templates are included to get you started!

## 🔌 API Usage

Point a browser at the service root (`http://localhost:8000`) for the landing page, the full
API reference and an interactive console that hits every endpoint for real. The contract is
also published as OpenAPI 3.0 in [`static/swagger.yaml`](static/swagger.yaml), served at
`/static/swagger.yaml`. Integration suite: `./test_api.sh [base_url]` (or `make test-api`)
against a running server.

### Legacy endpoint — `POST /` (FormData)

Unchanged, still supported:

```bash
curl --data-urlencode 'markdown=# Heading 1' \
     --data-urlencode 'header_template=corporate_header.html' \
     --data-urlencode 'footer_template=page_numbers.html' \
     --output document.pdf \
     https://your-deployment-url
```

| Parameter          | Required | Description                                                           |
|--------------------|----------|-----------------------------------------------------------------------|
| `markdown`         | ✅       | The Markdown content to convert                                       |
| `css`              | ❌       | Optional CSS styles to apply                                          |
| `engine`           | ❌       | Choose from `weasyprint`, `wkhtmltopdf`, or `pdflatex`               |
| `header_template`  | ❌       | Specify a custom header template from the `templates` folder          |
| `footer_template`  | ❌       | Specify a custom footer template from the `templates` folder          |
| `client_id`        | ❌       | Optional client identifier for document tracking                      |
| `pdf_name`         | ❌       | Custom name for the generated PDF                                     |

### JSON API — `/api/*`

| Endpoint              | Method | Description                                                        |
|-----------------------|--------|--------------------------------------------------------------------|
| `/api/health`         | GET    | Status, version and the PDF engines actually installed             |
| `/api/convert`        | POST   | Markdown → PDF (`markdown`, `css`, `engine`, `options`, header/footer) |
| `/api/render`         | POST   | Tera template + `data` → PDF                                       |
| `/api/html-to-pdf`    | POST   | Raw HTML → PDF                                                     |
| `/api/preview`        | POST   | First page as a PNG (`markdown`, `html`, or `template` + `data`)   |
| `/api/merge`          | POST   | Merge ≥ 2 previously saved PDFs                                    |
| `/api/watermark`      | POST   | Overlay a text watermark on a saved PDF                            |
| `/api/protect`        | POST   | AES-256 password protection on a saved PDF                         |
| `/download/{client_id}/{pdf_name}` | GET | Fetch a saved PDF                                     |

Every endpoint that produces a PDF returns the **binary PDF** by default, or
`{"download_url": "/download/<client_id>/<pdf_name>.pdf"}` when both `client_id` and
`pdf_name` are provided. `client_id` and `pdf_name` must be plain names
(`[A-Za-z0-9._-]`, not starting with a dot).

`options` accepts `paper_size` (`a4`, `a3`, `letter`), `orientation`, `margins`,
`page_numbers`, `page_number_format`, `toc`, `toc_depth` and `watermark`.

Errors are always JSON: `{"error": "...", "details": "..."}` (the legacy endpoint keeps
its plain-text errors).

```bash
curl -X POST http://localhost:8000/api/convert \
  -H 'Content-Type: application/json' \
  -d '{"markdown": "# Hello", "options": {"paper_size": "a4", "page_numbers": true}}' \
  --output document.pdf
```

## 🔧 Deployment

Production stack (build, volume for saved PDFs, healthcheck, log rotation):

```bash
make prod         # docker compose -f docker-compose.prod.yml up -d --build
make logs-prod    # follow the logs
make prod-down    # stop
```

`./install.sh` provisions a fresh Debian/Ubuntu host end to end (Docker, NGINX, network,
build, health check).

### Configuration

| Variable                   | Default | Description                                                          |
|----------------------------|---------|----------------------------------------------------------------------|
| `API_KEY`                  | *unset* | When set, `/api/*` requires `X-API-Key` or `Authorization: Bearer`. Unset keeps the API open. |
| `PDF_PROCESS_TIMEOUT_SECS` | `60`    | Wall-clock limit for pandoc / weasyprint / qpdf / pdftoppm            |
| `RUST_LOG`                 | `info`  | Log level                                                            |

> ⚠️ `/api/html-to-pdf` and `/api/render` render arbitrary HTML: WeasyPrint will fetch any
> URL the document references. Keep the service on a private network or set `API_KEY`.

Saved PDFs live in `public/pdf/` inside the container — the production compose file mounts
the `pdf-storage` volume there so `download_url`s survive a redeploy.

For local development:

```bash
make dev          # build, compile and serve with the dev containers
make check        # cargo check + clippy
```

## 🌐 Web Interface

The service root (`http://localhost:8000`) serves a self-contained page — no CDN, works
offline — with three things:

- **Landing** — what the engine does, live status and installed engines pulled from `/api/health`
- **API reference** — every endpoint with its parameters, request/response examples and status codes
- **Console** — a real client for all ten endpoints: fill the form, send, and see the PDF, the
  PNG preview or the JSON inline. Generated PDFs stay in a side list to feed `merge`,
  `watermark` and `protect`. Each request can be copied as a ready-to-run `curl` command
  (the API key is never copied in clear — it comes out as `$API_KEY`).

Reference and console are generated from a single spec in `static/console.js`, so the
documented payload is exactly the one the console sends.

The original CodeMirror markdown editor is still available at `/static/editor.html`.

## 🔄 Compatibility

- Works across all major browsers and operating systems
- PDFs can be viewed in any standard PDF reader
- API can be integrated with any system that supports HTTP requests

## 🙏 Acknowledgements

This project is a fork of the original `md-to-pdf` created by [Spawnia](https://github.com/Spawnia/md-to-pdf). We extend our gratitude to the original contributors while adding significant new functionality.

## 📝 License

This project maintains the same license as the original repository.

---

Developed with ❤️ by [AI SmartTalk](https://aismarttalk.tech)