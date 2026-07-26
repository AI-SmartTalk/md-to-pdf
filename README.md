# 🚀 md-to-pdf by AI SmartTalk

**A powerful fork of the original md-to-pdf with advanced features for professional document generation.**

This enhanced version, maintained by [AI SmartTalk](https://aismarttalk.tech), extends the original markdown-to-PDF converter with sophisticated features including customizable headers/footers and intelligent content censoring capabilities.

## ✨ Enhanced Features

- **🔒 Document Censoring:** Remove confidential sections — the hidden text never reaches the PDF
- **🎨 Themes:** Named, versioned brand kits (`aismarttalk`, `report`, `minimal`) with optional cover pages
- **📊 Charts & Diagrams:** ` ```chart ` and ` ```mermaid ` fenced blocks become inline SVG
- **📐 Layout Doctor:** Audit a PDF for overflow, blank pages, orphan headings and split tables — and fix them
- **🖍️ Redaction:** `POST /api/redact` paints out patterns and PII, then rebuilds the pages from pixels
- **🔬 Visual diff:** `POST /api/diff` compares two renders pixel by pixel, for CI
- **📄 Customizable Headers & Footers:** Apply professional templates to your documents
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

The historical point tag can be written `{{CENSOR}}`, `<CENSOR>`, `{{ CENSOR }}`,
`{{CENSOR }}` or `{{ CENSOR}}`. It renders exactly what it always rendered.

Regions are the modern form. Whatever the syntax, the censored text is **removed** before
pandoc sees it: it cannot be selected, searched or extracted from the resulting PDF.

| Tag                                | Effect                                                        |
|------------------------------------|---------------------------------------------------------------|
| `{{CENSOR}}` / `<CENSOR>`          | Point replacement: the historical blurred block                |
| `{{CENSOR:premium}}`               | Same, labelled with that level                                 |
| `{{CENSOR:start}}` … `{{CENSOR:end}}` | Removes the region; the block is sized after what was removed |
| `{{CENSOR:teaser=25}}` … `:end`    | Keeps the first 25 words, removes the rest                     |
| `{{CENSOR:inline}}` … `:end`       | Removes a run of text without breaking the paragraph           |

Levels are free-form names; `premium`, `confidentiel`, `interne` and `secret` are the ones
in common use. Modifiers combine with a comma: `{{CENSOR:start,confidentiel}}`.

Labels cascade: the level named on the tag wins, then `options.censor_label`, then the
historical wording.

Malformed input always errs towards censoring — a syntax mistake must not be a way to
reveal what was meant to be hidden. An unclosed region is censored to the end of the
document; nested regions are depth-counted; a level tag followed by a `:end` opens a region
(that is what a misspelled `start` looks like); a `:end` with no open region is dropped.

## 🎨 Themes

`options.theme` takes `"name"` (latest version) or `"name@2"` (pinned). Three kits ship
with the service:

| Theme         | For                                                                 |
|---------------|---------------------------------------------------------------------|
| `aismarttalk` | The house brand kit                                                 |
| `report`      | Corporate report: numbered sections, justified text, cover page      |
| `minimal`     | Plain typography, greyscale code                                    |

- `GET /api/themes` — every theme with its label, fonts, colour tokens and `preview_url`
- `GET /api/themes/{name}/{version}/preview.png[?cover=true]` — first page of the sample
  document rendered with that theme (`version` may be `latest`)

Themes recognise two utility classes in a pandoc div: `::: wide` (a table with many
columns, rendered at a reduced body size) and `::: success|warning|danger|info|note`
(callout boxes).

`options.cover` (`title`, `subtitle`, `logo`, `date`) inserts a cover page, styled by the
theme when it ships a `cover.html` and by a built-in template otherwise.

> **Themes are immutable.** The cache key contains `name@version`, so changing a published
> `theme.css` without creating a `v<n+1>` directory keeps serving PDFs rendered with the old
> stylesheet until the cache TTL expires. Any visual change means a new version directory.

## 📊 Charts and diagrams

Two fenced block languages are expanded into inline SVG before the document is rendered.
Neither can fail a request: a block that cannot be rendered stays in the document as a code
block and the reason comes back in the `warnings` field of the JSON response.

````markdown
```chart
{
  "type": "bar",
  "title": "Revenue",
  "labels": ["Q1", "Q2", "Q3"],
  "series": [{"name": "2025", "data": [12000, 18000, 15000]}]
}
```
````

`type` is one of `bar`, `hbar`, `stacked-bar`, `grouped-bar`, `line`, `area`, `pie`,
`donut` (aliases `column`, `doughnut`, `stacked`, `grouped` are tolerated). Other fields:
`title`, `subtitle`, `labels`, `series[{name, data, color}]`, `x_label`, `y_label`,
`value_format` (`compact` | `percent` | `eur` | `plain`, default `compact`), `width`
(default 640, 240–2000), `height` (default 360, 160–2000), `legend` (default: present from
two series), `grid` (default true) and `texture` (default true — hatch patterns so the
chart survives being printed in black and white). Limits: 8 series, 2000 points, 8 pie
slices (the tail is folded into an "Other" slice).

````markdown
```mermaid theme=forest
graph TD; A[Start] --> B[End];
```
````

Attributes may follow the language (`theme=dark`, `title="…"`), including in pandoc form
(`{.mermaid theme=forest}`). The HTML spelling `<pre><code class="language-mermaid">` works
too. Accepted themes: `default`, `dark`, `forest`, `neutral`. `click` directives lose their
hyperlink on purpose.

> **Diagrams depend on an external service.** Rendering is delegated to Mermaid Studio
> (`MERMAID_API_URL`), so the container needs outbound access to it. If it is unreachable,
> times out, or `MERMAID_API_URL` is explicitly empty, every diagram degrades to its code
> block plus one warning and the document still comes out — a report is never lost over a
> diagram. Charts have no such dependency: they are drawn in-process.

`options.charts: false` turns off the expansion of **both** block types.

## 📐 Layout Doctor

`options.autolayout: true` on any rendering endpoint analyses the produced PDF, re-renders
it with corrective CSS while that improves the score, and returns the report in the
`layout` field. A corrective pass that does not improve the score is thrown away: the
client never receives a PDF worse than the one it would have had.

> **It costs a full extra render per corrective pass** (`PDF_LAYOUT_MAX_PASSES`, default 1),
> plus the analysis itself — roughly double the latency of the same request without it. Turn
> it on for documents a human will read, not for a batch of ten thousand invoices.

`POST /api/layout` runs the same analysis on a PDF that already exists, without touching
it: `{"pdf": "/download/<client_id>/<name>.pdf"}` → a `LayoutReport`.

| `kind`           | Severity        | Meaning                                              |
|------------------|-----------------|------------------------------------------------------|
| `overflow`       | `error`/`warn`  | Content past the page edge (`error`) or the margin    |
| `blank_page`     | `warn`          | A page with no ink at all — reported, not fixable     |
| `widow_page`     | `warn`          | A last page carrying almost nothing                   |
| `orphan_heading` | `warn`          | A heading alone at the bottom of a page               |
| `split_table`    | `warn`          | A table cut across a page break                       |
| `long_table`     | `info`          | A table taller than a page; suppresses the anti-break |

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
| `engine`           | ❌       | `weasyprint` only on this open endpoint (see `PDF_UNAUTHENTICATED_ENGINES`); the three engines are available on `/api/convert` |
| `header_template`  | ❌       | Specify a custom header template from the `templates` folder          |
| `footer_template`  | ❌       | Specify a custom footer template from the `templates` folder          |
| `client_id`        | ❌       | Optional client identifier for document tracking                      |
| `pdf_name`         | ❌       | Custom name for the generated PDF                                     |

### JSON API — `/api/*`

| Endpoint              | Method | Description                                                        |
|-----------------------|--------|--------------------------------------------------------------------|
| `/api/health`         | GET    | Status, version and the PDF engines actually installed. **The only unauthenticated `/api` route** (container probe) |
| `/api/convert`        | POST   | Markdown → PDF (`markdown`, `css`, `engine`, `options`, header/footer) |
| `/api/render`         | POST   | Tera template + `data` → PDF                                       |
| `/api/html-to-pdf`    | POST   | Raw HTML → PDF                                                     |
| `/api/preview`        | POST   | Pages as PNG (`markdown`, `html`, or `template` + `data`)          |
| `/api/merge`          | POST   | Merge ≥ 2 previously saved PDFs                                    |
| `/api/watermark`      | POST   | Overlay a text watermark on a saved PDF                            |
| `/api/protect`        | POST   | AES-256 password protection on a saved PDF                         |
| `/api/redact`         | POST   | Paint out patterns and PII, then rebuild the pages from pixels     |
| `/api/layout`         | POST   | Audit an existing PDF's layout                                     |
| `/api/diff`           | POST   | Pixel diff of two saved PDFs                                       |
| `/api/themes`         | GET    | Themes available, with their tokens and preview URLs               |
| `/api/themes/{name}/{version}/preview.png` | GET | Sample document rendered with that theme      |
| `/api/metrics`        | GET    | Prometheus exposition (`text/plain; version=0.0.4`)                |
| `/download/{client_id}/{pdf_name}` | GET | Fetch a saved PDF                                     |

Every endpoint that produces a PDF returns the **binary PDF** by default, or
`{"download_url": "/download/<client_id>/<pdf_name>.pdf"}` when both `client_id` and
`pdf_name` are provided. `client_id` and `pdf_name` must be plain names
(`[A-Za-z0-9._-]`, not starting with a dot).

`options` accepts `paper_size` (`a4`, `a3`, `letter`), `orientation`, `margins`,
`page_numbers`, `page_number_format`, `toc`, `toc_depth`, `watermark`, `theme`,
`cover`, `autolayout`, `censor_label` and `charts`.

The JSON response is additive: a request that asked for nothing new still gets exactly
`{"download_url": "…"}`. Three fields appear only when they apply:

| Field      | When                                                                   |
|------------|------------------------------------------------------------------------|
| `cached`   | `true` when the PDF came from the render cache                          |
| `layout`   | The `LayoutReport`, when `options.autolayout` was asked for              |
| `warnings` | A chart or diagram block that could not be rendered was left as-is       |

Errors are always JSON: `{"error": "...", "details": "..."}` (the legacy endpoint keeps
its plain-text errors).

Every response carries an `X-Request-Id` header. Send your own on the way in — printable
ASCII, 128 characters at most — and it is echoed back and attached to every log event, which
is what ties a support ticket to what the service actually did.

#### `POST /api/preview`

Historical behaviour is unchanged: a body with no new field returns the PNG of page 1.

| Field    | Default | Description                                                    |
|----------|---------|----------------------------------------------------------------|
| `pages`  | `"1"`   | `"3"`, `"2-5"` or `"all"` (capped by `PDF_PREVIEW_MAX_PAGES`)  |
| `dpi`    | `150`   | 36–300                                                         |
| `layout` | *see below* | `"png"` (raw image), `"images"` (JSON, one PNG per page), `"sheet"` (a single contact-sheet image) |

Without an explicit `layout`, one page returns a raw PNG and several return the `images`
JSON. `images` reports `truncated: true` when the page cap cut the answer short; `sheet`
cannot, since it is just an image.

#### `POST /api/redact`

`{"pdf", "patterns", "entities", "dpi", "client_id", "pdf_name"}`. Entities:
`email`, `iban`, `phone`, `siret`, `credit_card` — each validated by its check digits, so a
14-digit invoice number is not mistaken for a SIRET.

**`patterns` are literal strings, not regular expressions.** Comparison ignores case and
normalises whitespace (so a match may span a line break). A pattern that *looks* like a
regex (`\d{4}`, `[A-Z]+`, `.*`, `(?i)`) is rejected with a 400 rather than matched
literally — receiving a document where nothing was blacked out while believing the job was
done is the failure this prevents.

The output is rebuilt from images: the redacted strings are gone from the file, and so is
every text layer, bookmark, annotation and metadata entry of the source. The result is
neither selectable nor searchable, and it is larger. The JSON form answers
`{download_url, redactions, pages, mode, notice}`; the binary form (no `client_id`) cannot
carry the redaction counts.

> A PDF that was **scanned** — no text layer — comes back flattened with `redactions: []`.
> Nothing could be located; there is no OCR. An empty array is the only signal.

#### `POST /api/diff`

`{"before", "after", "dpi", "threshold", "images"}`, both PDFs being download paths of this
service. Answers `pages_total`, `pages_before`, `pages_after`, `pages_changed`,
`changed_ratio`, `per_page` and a `verdict` of `identical` or `changed`.

**`threshold` defaults to 0**: any page that visibly moved makes the verdict `changed`.
Raise it to tolerate noise — a CI that defaulted to tolerance would report `identical` on a
renamed heading.

> Run a diff right after a deployment that changed the renderer without bumping the cache
> version and it will compare a stale cached PDF against a fresh one, or worse, call two
> different renders `identical`. `CACHE_VERSION` in `src/cache.rs` exists for exactly this.

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
| `PDF_SSH_PORT`      | `22`             | SSH port — set it when the host listens elsewhere |
| `PDF_DEPLOY_PATH`   | `/opt/md-to-pdf` | Where the repository lives on the VPS      |
| `PDF_PUBLIC_URL`    | *unset*          | Public URL — when set, the workflow checks `/api/health` through the proxy after deploying |

The public key matching `PDF_SSH_KEY` must be present in `~/.ssh/authorized_keys`
of `PDF_SSH_USER` on the target host — GitHub cannot authenticate otherwise. Derive
it from the private key with `ssh-keygen -y -f <keyfile>`, or read it from the
workflow log: the *Vérifier l'accès SSH* step prints the loaded public key when the
connection is refused, along with what to check.

`PDF_ENV` is the source of truth for the **whole** production `.env`, not just the key: the
workflow overwrites the file on every deployment. A variable you add by hand to `/opt/md-to-pdf/.env`
on the VPS works until the next merge to `master`, then silently disappears — this has
already cost a debugging session. Add it to the `PDF_ENV` secret, always. The workflow
refuses to run if `PDF_ENV` does not contain a non-empty `API_KEY=`, since writing it as-is
would take the service down.

Every variable in `.env.example` has a default in `docker-compose.prod.yml` except
`API_KEY`, so a `.env` written before this version keeps working untouched: nothing new is
mandatory.

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
both with body-size and timeout limits aligned on the service. The proxy timeout is 180 s,
not 60: the worst case is `PDF_QUEUE_TIMEOUT_SECS` (30 s) in the queue plus
`PDF_RENDER_DEADLINE_SECS` (120 s) for the whole job — every process and every
corrective pass together, whatever `PDF_LAYOUT_MAX_PASSES` says, and the same ceiling for
`/api/redact`, `/api/diff` and `/api/preview` — so 150 s and never more.
Per-process limits do not compose, which is why the deadline exists at all: without it a
render could outlive the proxy and keep holding its slot while the client got a 503. A
proxy that cuts earlier turns a clean applicative 504 into an opaque gateway error.

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
| `PDF_RENDER_DEADLINE_SECS` | `120`     | Wall-clock limit for a whole job — render, preview, redaction or diff — every process and pass together |
| `RUST_LOG`                 | `info`    | Log level                                                            |
| `MEM_LIMIT` / `CPUS`       | `1g` / `2.0` | Container resource ceiling                                        |
| `PDF_RETENTION_DAYS`       | `180`     | Age past which the purge deletes saved PDFs                          |
| `PDF_MAX_CONCURRENCY`      | cores, ≤ 8 | Simultaneous renders; the rest queue                                |
| `PDF_QUEUE_TIMEOUT_SECS`   | `30`      | Wait in the queue before a `429`                                     |
| `PDF_CACHE_ENABLED`        | `true`    | Content-addressed render cache in `public/cache`                     |
| `PDF_CACHE_MAX_MB`         | `512`     | Ceiling before LRU eviction                                          |
| `PDF_CACHE_TTL_SECS`       | `604800`  | Age past which an entry is dropped                                   |
| `PDF_LAYOUT_MAX_PASSES`    | `1`       | Corrective passes for `options.autolayout` (0–3)                     |
| `PDF_PREVIEW_MAX_PAGES`    | `20`      | Page cap of `/api/preview`                                           |
| `PDF_ALLOWED_URL_HOSTS`    | *empty*   | Hosts a document may fetch from. **Empty means any public host**, not "none" |
| `PDF_URL_STRICT_HOSTS`     | `false`   | `true` reads an empty allowlist as "no remote asset at all"           |
| `PDF_ALLOW_LOCAL_ASSETS`   | `true`    | `false` refuses `file://` and relative paths too                     |
| `PDF_UNAUTHENTICATED_ENGINES` | `weasyprint` | Engines `POST /` may select; the others need a key                |
| `MERMAID_API_URL`          | *Mermaid Studio* | Explicitly empty disables ` ```mermaid ` blocks cleanly       |
| `MERMAID_API_KEY`          | —         | Sent as `X-Api-Key` when present                                     |
| `MERMAID_TIMEOUT_SECS`     | `15`      | Per-diagram timeout                                                  |
| `LOG420_INGEST_TOKEN`      | —         | **Without it nothing is shipped**: observability stays local          |
| `LOG420_URL` / `LOG420_REGION` / `SERVICE_NAME` | *see `.env.example`* | Log shipping target and identity |

`.env.example` documents the remaining `PDF_URLGUARD_*` knobs (fetch timeout, byte ceiling,
redirect budget, extra readable files, refusal log) and the `PDF_URL_GUARD=off` escape
hatch, which is for diagnosis only.

#### The render cache

`public/cache` is **content-addressed**: the key hashes the expanded source, the client CSS,
the engine, the resolved `options`, the header and footer, the theme `name@version` and an
internal `CACHE_VERSION`. Two consequences worth knowing before you go looking for a purge
endpoint — there is none.

- Changing anything that reaches the key **invalidates nothing and evicts nothing**; it
  simply produces a different key. Switching a document from `theme: minimal` to
  `theme: report` renders afresh and both entries coexist until TTL or LRU drops them.
- Changing a *published* `theme.css` in place does **not** change the key. The old PDF keeps
  being served until `PDF_CACHE_TTL_SECS` expires. Any visual change to a theme means a new
  `v<n+1>` directory; any change to the renderer itself means bumping `CACHE_VERSION` in
  `src/cache.rs`.
- The key is computed on the **expanded** source, so a document containing ` ```mermaid `
  calls Mermaid Studio *before* it can hit the disk cache. An in-process diagram cache keeps
  that cheap, but it is emptied when the process restarts — the first render of a diagram
  after a deployment always goes over the network.

The nightly purge (`deploy/pdf-purge.sh`) deliberately does not touch this directory — the
service evicts it by TTL and by `PDF_CACHE_MAX_MB`. The script reports its size on every run
and shouts when it exceeds the ceiling, which is how a broken eviction surfaces before a
full disk does.

> ⚠️ The legacy `POST /` endpoint is **not** covered by `API_KEY` — it never was, by design,
> so existing FormData integrations keep working. Anyone who can reach the service can
> still generate a PDF through it. Migrate callers to `POST /api/convert` with a key, or
> block `POST /` at the reverse proxy, if that matters to you.
>
> Being open is what shapes what it offers. It renders with `weasyprint` only, since that
> is the engine that goes through the guarded fetcher (`PDF_UNAUTHENTICATED_ENGINES`
> reopens the others); block expansion is off, so a ` ```mermaid ` fence stays a code
> listing and an anonymous body of them cannot turn one request into thousands of outbound
> calls; and a blocked URL is reported rather than fatal, because this endpoint has always
> answered with a PDF missing its asset. `/api/*` behaves the other way round on all three
> counts.

> ⚠️ `/api/html-to-pdf` and `/api/render` render arbitrary HTML, and WeasyPrint dereferences
> every URL a document points at. Two layers stand in the way: the request is rejected with a
> **400 naming the offending URL** when the document (or the client CSS) references `file://`,
> a private address, a metadata endpoint or a host outside the allowlist; and at render time
> the fetcher re-checks every URL, re-resolving DNS and pinning the validated IP, so a remote
> resource that is refused is simply **absent from the PDF** rather than an error.
>
> `wkhtmltopdf` and `pdflatex` only get the first layer — the fetcher has no equivalent
> there — which is why neither is selectable without a key. `pdflatex` additionally runs
> under kpathsea's paranoid mode (`openin_any=p`, `openout_any=p`, no shell escape), so
> `\input{/etc/passwd}` fails the render instead of embedding the file: the URL scan sees
> URLs, never TeX commands.

The container publishes on `127.0.0.1:8000` only — never on `0.0.0.0`, since Docker writes
its own iptables rules and would bypass the host firewall along with the proxy's TLS.

Saved PDFs live in `public/pdf/` inside the container; the `pdf-storage` volume is mounted
there so `download_url`s survive a redeploy. The render cache lives in `public/cache/` on
the `pdf-cache` volume — without it the cache restarts empty on every deployment. Neither
volume is backed up: add them to the host's backup scope if those links must outlive the
machine.

### Observability

`GET /api/metrics` (behind the API key) exposes Prometheus text: request counts and
latency histograms labelled by Rocket route pattern and status code only — never a
`client_id` or a file name. With `LOG420_INGEST_TOKEN` set, structured events are also
shipped to log420 in NDJSON batches, each carrying the `X-Request-Id` of its request.

## 🌐 Web Interface

The service root (`http://localhost:8000`) serves a self-contained app — no CDN, works
offline — split into four hash-routed views, each filling the window instead of stacking
into one endless page:

- **`#/` Landing** — what the engine does, live status and installed engines pulled from `/api/health`
- **`#/api` API reference** — searchable sidebar, one endpoint at a time: parameters,
  request/response examples and status codes
- **`#/console` Console** — a real client for every endpoint, in three independently
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