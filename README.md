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

### Continuous deployment

Every merge to `master` deploys to production. The workflow SSHes into the VPS,
syncs the repository to the merged commit, writes `.env` from a secret, then runs
`deploy/bootstrap.sh` — which builds, switches over, waits for the health probe and
**rolls back to the previous image if the new one does not answer**.

Configure once, in *Settings → Secrets and variables → Actions*:

| Secret          | Content                                                        |
|-----------------|----------------------------------------------------------------|
| `PDF_SSH_KEY`   | Private SSH key with access to the VPS                          |
| `PDF_SSH_HOST`  | VPS host or IP                                                  |
| `PDF_SSH_USER`  | SSH user                                                        |
| `PDF_ENV`       | Full contents of the production `.env`, `API_KEY` included      |

| Variable (optional) | Default          | Purpose                                    |
|---------------------|------------------|--------------------------------------------|
| `PDF_DEPLOY_PATH`   | `/opt/md-to-pdf` | Where the repository lives on the VPS      |
| `PDF_PUBLIC_URL`    | *unset*          | Public URL — when set, the workflow checks `/api/health` through the proxy after deploying |

`PDF_ENV` is the source of truth for the production key: it overwrites `.env` on
every deployment. The workflow refuses to run if it does not contain a non-empty
`API_KEY=`, since writing it as-is would take the service down.

`deploy/bootstrap.sh` is idempotent and makes no assumption about the machine: it
clones the repository if missing, creates the network if absent, reinstalls the
systemd units, prunes dangling images. Running it by hand on the VPS is equivalent
to a deployment:

```bash
cd /opt/md-to-pdf && ./deploy/bootstrap.sh
```

### First host

Only needed once, on a machine that has never run the service:

```bash
git clone git@github.com:AI-SmartTalk/md-to-pdf.git /opt/md-to-pdf
cd /opt/md-to-pdf
./install.sh
```

`install.sh` is idempotent and does the whole job: Docker Engine + Compose (enabled at
boot), the `ai-toolkit-network` network, a `.env` with a freshly generated `API_KEY`, the
production stack, a systemd watchdog and a nightly PDF purge. It prints the generated key
once — write it down, it is what clients authenticate with.

It deliberately does **not** install a reverse proxy: the host usually already runs one.
Ready-made vhosts sit in `deploy/` — `nginx-md-to-pdf.conf` and `apache-md-to-pdf.conf`,
both with body-size and timeout limits aligned on the service.

### Day to day

```bash
make prod         # build and start
make logs-prod    # follow the logs
make prod-down    # stop
make check        # cargo check + clippy
make dev          # dev containers with hot reload
```

### Staying up

| Failure                          | What catches it                                                      |
|----------------------------------|----------------------------------------------------------------------|
| Process crashes                  | `restart: unless-stopped` — Docker restarts it immediately           |
| Host reboots                     | `systemctl enable docker` + the same restart policy                  |
| Service hangs but does not exit  | `md-to-pdf-watchdog.timer` — probes `/api/health` every minute, restarts after 3 consecutive failures |
| Disk fills with generated PDFs   | `md-to-pdf-purge.timer` — nightly, drops PDFs older than `PDF_RETENTION_DAYS` |
| A document eats all the RAM      | `mem_limit`, `cpus` and `pids_limit` on the container                |

The watchdog exists because Docker Compose does nothing with a failing healthcheck: a
frozen container stays `up (unhealthy)` and keeps taking traffic forever. It runs on the
host rather than in a container, so the Docker socket is never exposed.

```bash
systemctl status md-to-pdf-watchdog.timer
journalctl -u md-to-pdf-watchdog.service --since today
```

### Configuration

Copy `.env.example` to `.env` (or let `install.sh` generate it).

| Variable                   | Default   | Description                                                          |
|----------------------------|-----------|----------------------------------------------------------------------|
| `API_KEY`                  | *required* | Closes `/api/*`: clients send `X-API-Key` or `Authorization: Bearer`. The production compose file **refuses to start** without it. |
| `PDF_PROCESS_TIMEOUT_SECS` | `60`      | Wall-clock limit for pandoc / weasyprint / qpdf / pdftoppm            |
| `RUST_LOG`                 | `info`    | Log level                                                            |
| `MEM_LIMIT` / `CPUS`       | `1g` / `2.0` | Container resource ceiling                                        |
| `PDF_RETENTION_DAYS`       | `180`     | Age past which the purge deletes saved PDFs                          |

> ⚠️ The legacy `POST /` endpoint is **not** covered by `API_KEY` — it never was, by design,
> so existing FormData integrations keep working. Anyone who can reach the service can
> still generate a PDF through it. Migrate callers to `POST /api/convert` with a key, or
> block `POST /` at the reverse proxy, if that matters to you.

> ⚠️ `/api/html-to-pdf` and `/api/render` render arbitrary HTML: WeasyPrint will fetch any
> URL the document references.

The container publishes on `127.0.0.1:8000` only — never on `0.0.0.0`, since Docker writes
its own iptables rules and would bypass the host firewall along with the proxy's TLS.

Saved PDFs live in `public/pdf/` inside the container; the `pdf-storage` volume is mounted
there so `download_url`s survive a redeploy. The volume itself is not backed up — add it to
the host's backup scope if those links must outlive the machine.

## 🌐 Web Interface

The service root (`http://localhost:8000`) serves a self-contained app — no CDN, works
offline — split into four hash-routed views, each filling the window instead of stacking
into one endless page:

- **`#/` Landing** — what the engine does, live status and installed engines pulled from `/api/health`
- **`#/api` API reference** — searchable sidebar, one endpoint at a time: parameters,
  request/response examples and status codes
- **`#/console` Console** — a real client for all ten endpoints, in three independently
  scrolling panes (endpoints, request, response) with a draggable request/response split.
  Generated PDFs stay in a side list to feed `merge`, `watermark` and `protect`. Each
  request can be copied as a ready-to-run `curl` command (the API key is never copied in
  clear — it comes out as `$API_KEY`).
- **`#/acces` Access** — the service runs for AI SmartTalk teams and products, not as a
  public SaaS: this view explains how to request a token
  (`contact+mdtopdf@aismarttalk.tech`), stores it in the browser and verifies it against
  the running service

The token state is visible everywhere: a marker on the key button in the top bar and a
banner in the console whenever no token is stored, plus an explicit toast when a request
comes back `401`. `⌘K` opens a search palette over every endpoint and view (`⇧⏎` jumps
straight to the console), `⌘⏎` sends the current request.

Front-end files, all under `static/`: `spec.js` (the single API spec), `ui.js` (shared
helpers, theme, toasts), `docs.js` (landing, reference), `console.js` (test console),
`app.js` (router, palette, token, health) and `app.css`. Reference and console are
generated from the same spec, so the documented payload is exactly the one the console
sends.

> Deployment and configuration are documented in this README only — the web app targets
> integrators, not operators. Set `API_KEY` in the environment: without it the service
> starts with `/api/*` open to anyone who can reach it, which contradicts what the app
> tells its users.

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