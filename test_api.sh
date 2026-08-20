#!/usr/bin/env bash
# Integration test script for md-to-pdf Document Engine API
# Usage: ./test_api.sh [base_url]
#
# Set API_KEY when the server runs with authentication enabled.

set -euo pipefail

BASE_URL="${1:-http://localhost:8000}"
API_KEY="${API_KEY:-}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Makes every document of this run unique, so the content-addressed render cache
# cannot turn a second run into a cache hit and add a "cached" field where a test
# asserts the exact response shape.
RUN_ID="$(date +%s)-$$"

PASS=0
FAIL=0
SKIP=0

green()  { echo -e "\033[0;32m$1\033[0m"; }
red()    { echo -e "\033[0;31m$1\033[0m"; }
yellow() { echo -e "\033[0;33m$1\033[0m"; }

# curl wrapper adding the API key when one is configured
api() {
  if [ -n "$API_KEY" ]; then
    curl -s -H "X-API-Key: $API_KEY" "$@"
  else
    curl -s "$@"
  fi
}

check() {
  local name="$1"
  local expected_code="$2"
  local actual_code="$3"
  if [ "$actual_code" = "$expected_code" ]; then
    green "  ✓ $name (HTTP $actual_code)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $name — expected HTTP $expected_code, got $actual_code"
    FAIL=$((FAIL + 1))
  fi
}

check_pdf() {
  local name="$1"
  local file="$2"
  if [ -s "$file" ] && head -c 4 "$file" | grep -q "%PDF"; then
    green "  ✓ $name (valid PDF, $(wc -c < "$file" | tr -d ' ') bytes)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $name — not a PDF body"
    FAIL=$((FAIL + 1))
  fi
}

check_png() {
  local name="$1"
  local file="$2"
  # The PNG signature starts with a non-ASCII byte, which makes grep treat the
  # whole thing as binary and refuse to match: compare the three ASCII bytes.
  if [ -s "$file" ] && [ "$(head -c 4 "$file" | tail -c 3)" = "PNG" ]; then
    green "  ✓ $name (valid PNG, $(wc -c < "$file" | tr -d ' ') bytes)"
    PASS=$((PASS + 1))
  else
    red "  ✗ $name — not a PNG body"
    FAIL=$((FAIL + 1))
  fi
}

# The response fields are additive, so most assertions are "this substring is
# there" / "this substring is not there" on the raw body — no jq dependency.
check_contains() {
  local name="$1" file="$2" needle="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    green "  ✓ $name"
    PASS=$((PASS + 1))
  else
    red "  ✗ $name — expected $needle in the body, got: $(head -c 200 "$file")"
    FAIL=$((FAIL + 1))
  fi
}

check_absent() {
  local name="$1" file="$2" needle="$3"
  if grep -qF -- "$needle" "$file" 2>/dev/null; then
    red "  ✗ $name — unexpected $needle in the body: $(head -c 200 "$file")"
    FAIL=$((FAIL + 1))
  else
    green "  ✓ $name"
    PASS=$((PASS + 1))
  fi
}

# For behaviour that depends on the server's configuration: a server with the
# cache turned off must not be reported as broken.
skip() {
  yellow "  ~ $1"
  SKIP=$((SKIP + 1))
}

echo "=== md-to-pdf API Integration Tests ==="
echo "Base URL: $BASE_URL"
[ -n "$API_KEY" ] && echo "Auth: X-API-Key" || echo "Auth: none"
echo

# -----------------------------------------------------------
# 0. The public site — routing, SEO, and the pages a visitor lands on
#
# These assertions exist because an SEO regression is silent: a canonical that
# disappears, a redirect that becomes a copy, a title that loses its keyword —
# nothing breaks, the traffic just stops three weeks later.
# -----------------------------------------------------------
echo "--- The public site ---"

for path in / /en /tarifs /pricing /outils/compresser-pdf /tools/compress-pdf /dev /sitemap.xml /robots.txt /og.png; do
  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL$path")
  check "GET $path" 200 "$CODE"
done

# The root belongs to the public; the console kept every one of its views
curl -s -o "$TMP_DIR/home.html" "$BASE_URL/"
check_contains "the root is the public site, not the console" "$TMP_DIR/home.html" "AI SmartTalk <strong>Documents</strong>"
check_absent "and no longer announces the stack to a visitor" "$TMP_DIR/home.html" "SERVICE INTERNE"
curl -s -o "$TMP_DIR/console.html" "$BASE_URL/dev"
check_contains "the console is intact at its own address" "$TMP_DIR/console.html" 'id="view-console"'

# The console moved from /console to /dev when the two front doors became one. The old
# address has to keep answering: it is in READMEs, in bookmarks, and in people's habits.
CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/console")
check "the old console address redirects rather than 404s" 308 "$CODE"
LOCATION=$(curl -s -o /dev/null -w "%{redirect_url}" "$BASE_URL/console")
if [ "${LOCATION%/dev}" != "$LOCATION" ]; then
  green "  ✓ and it redirects to /dev"; PASS=$((PASS + 1))
else
  red "  ✗ and it redirects to /dev — got: $LOCATION"; FAIL=$((FAIL + 1))
fi

# Where the home pages used to live must redirect, never serve a second copy
for pair in "/outils 308" "/tools 308"; do
  set -- $pair
  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL$1")
  check "$1 redirects instead of duplicating" "$2" "$CODE"
done

# A tool page carries what a search engine needs to rank and display it
curl -s -o "$TMP_DIR/tool.html" "$BASE_URL/outils/compresser-pdf"
check_contains "the title leads with the search phrase" "$TMP_DIR/tool.html" "<title>Compresser PDF en ligne"
check_contains "a canonical, absolute" "$TMP_DIR/tool.html" 'rel="canonical" href="https://'
check_contains "an alternate pointing at the same tool in English" "$TMP_DIR/tool.html" '/tools/compress-pdf'
check_contains "structured data saying it is a tool" "$TMP_DIR/tool.html" '"@type":"SoftwareApplication"'
check_contains "a breadcrumb for the results page" "$TMP_DIR/tool.html" '"@type":"BreadcrumbList"'
check_contains "questions answered in the page" "$TMP_DIR/tool.html" '"@type":"FAQPage"'
check_contains "an image for when the link is shared" "$TMP_DIR/tool.html" 'og:image'
check_absent "and no HTML entity mangling the URLs" "$TMP_DIR/tool.html" 'href="https:&#x2F;'

# The action comes before the prose: the drop zone must precede the explanation
python3 - "$TMP_DIR/tool.html" <<'PY' && { green "  ✓ the drop zone comes before the how-it-works text"; PASS=$((PASS + 1)); } || { red "  ✗ the explanation still precedes the drop zone"; FAIL=$((FAIL + 1)); }
import sys
html = open(sys.argv[1], encoding='utf-8').read()
drop, steps = html.find('data-role="drop"'), html.find('Comment ça marche')
sys.exit(0 if drop != -1 and (steps == -1 or drop < steps) else 1)
PY

# The sitemap is how forty-odd pages get discovered at all
curl -s -o "$TMP_DIR/sitemap.xml" "$BASE_URL/sitemap.xml"
check_contains "the sitemap declares its alternates" "$TMP_DIR/sitemap.xml" 'hreflang="en"'
check_contains "and a modification date" "$TMP_DIR/sitemap.xml" "<lastmod>"
URLS=$(grep -c "<loc>" "$TMP_DIR/sitemap.xml")
if [ "$URLS" -ge 44 ]; then
  green "  ✓ the sitemap lists $URLS URLs"; PASS=$((PASS + 1))
else
  red "  ✗ the sitemap lists only $URLS URLs, expected at least 44"; FAIL=$((FAIL + 1))
fi

# Documents produced for identified callers are not public pages
curl -s -o "$TMP_DIR/robots.txt" "$BASE_URL/robots.txt"
check_contains "robots.txt keeps /download out of every index" "$TMP_DIR/robots.txt" "Disallow: /download/"
check_contains "and points at the sitemap" "$TMP_DIR/robots.txt" "Sitemap: https://"

# A dead link is a person who wanted a tool, not a JSON error
CODE=$(curl -s -o "$TMP_DIR/404.html" -w "%{http_code}" -H "Accept: text/html" "$BASE_URL/outils/nexiste-pas")
check "an unknown tool page → 404" 404 "$CODE"
check_contains "and offers the other tools instead" "$TMP_DIR/404.html" "tool-card"
check_contains "while asking not to be indexed" "$TMP_DIR/404.html" "noindex"
CODE=$(curl -s -o "$TMP_DIR/404.json" -w "%{http_code}" -H "Accept: application/json" "$BASE_URL/api/nexiste-pas")
check "an unknown API path → 404" 404 "$CODE"
check_contains "and stays JSON for an integration" "$TMP_DIR/404.json" '"error"'

# The card that shows up when a link is shared, rendered by the engine itself
curl -s -o "$TMP_DIR/og.png" "$BASE_URL/og.png"
check_png "the social card is a real image" "$TMP_DIR/og.png"
echo

# -----------------------------------------------------------
# 0bis. One source of truth for the API
#
# tests/api_surface.rs already compares src/main.rs, swagger.yaml and spec.js at
# build time — that is where a drift is caught first, before anything ships.
# This block covers what a build-time test cannot see: the documents actually
# SERVED by this instance. An image built from a stale static/ passes cargo test
# on the source tree and still hands visitors a reference for another service.
# -----------------------------------------------------------
echo "--- One source of truth for the API ---"

curl -s -o "$TMP_DIR/spec.js" "$BASE_URL/static/spec.js"
curl -s -o "$TMP_DIR/swagger.yaml" "$BASE_URL/static/swagger.yaml"

python3 - "$TMP_DIR/spec.js" "$TMP_DIR/swagger.yaml" <<'PY' && { green "  ✓ the served spec.js and swagger.yaml describe the same endpoints"; PASS=$((PASS + 1)); } || { red "  ✗ the two served documents disagree (see above)"; FAIL=$((FAIL + 1)); }
import sys

METHODS = ("get", "post", "put", "delete", "patch", "head", "options")


def from_spec_js(path):
    """One endpoint per object, with method and path on the line that carries the key."""
    found = set()
    for line in open(path, encoding="utf-8"):
        method, endpoint = quoted(line, "method: "), quoted(line, "path: ")
        if method and endpoint:
            found.add(f"{method} {endpoint}")
    return found


def quoted(line, key):
    start = 0
    while (at := line.find(key, start)) != -1:
        before = line[at - 1] if at else ""
        if not (before.isalnum() or before == "_"):
            rest = line[at + len(key):]
            if rest.startswith('"'):
                return rest[1:].split('"')[0]
            return None
        start = at + len(key)
    return None


def from_swagger(path):
    """Two spaces name a path, four an operation on it."""
    found, current, inside = set(), None, False
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if line.startswith("paths:"):
            inside = True
            continue
        if not inside:
            continue
        if line and not line.startswith((" ", "#")):
            break
        if line.startswith("  /") and line.endswith(":"):
            current = line.strip().rstrip(":")
        elif current and line[:4] == "    " and line[4:] in [m + ":" for m in METHODS]:
            found.add(f"{line.strip().rstrip(':').upper()} {current}")
    return found


spec, swagger = from_spec_js(sys.argv[1]), from_swagger(sys.argv[2])
if not spec or not swagger:
    print(f"  one of the documents came back empty: spec.js {len(spec)}, swagger.yaml {len(swagger)}")
    sys.exit(1)

for missing, where, other in ((spec - swagger, "swagger.yaml", "spec.js"),
                              (swagger - spec, "spec.js", "swagger.yaml")):
    for endpoint in sorted(missing):
        print(f"  missing from {where} but declared by {other}: {endpoint}")

print(f"  {len(spec)} endpoints documented, both documents agreeing")
sys.exit(0 if spec == swagger else 1)
PY

# The agent surface used to be mounted and documented nowhere. It is open by
# design: an integrator who cannot see the endpoint cannot configure the key.
CODE=$(curl -s -o "$TMP_DIR/mcp.json" -w "%{http_code}" "$BASE_URL/mcp")
check "GET /mcp describes the agent surface without a key" 200 "$CODE"
check_contains "and names the protocol it speaks" "$TMP_DIR/mcp.json" "Model Context Protocol"
check_contains "and lists its tools" "$TMP_DIR/mcp.json" "document_render"
echo

# -----------------------------------------------------------
# 1. Health check
# -----------------------------------------------------------
echo "--- GET /api/health ---"
CODE=$(curl -s -o "$TMP_DIR/health.json" -w "%{http_code}" "$BASE_URL/api/health")
check "health check" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/health.json")"
echo

# -----------------------------------------------------------
# 2. Legacy POST / (FormData, backward compat) — returns PDF
# -----------------------------------------------------------
echo "--- POST / (legacy FormData) ---"
CODE=$(curl -s -o "$TMP_DIR/legacy.pdf" -w "%{http_code}" \
  -F "markdown=# Hello World" \
  "$BASE_URL/")
check "legacy convert (PDF response)" 200 "$CODE"
check_pdf "legacy convert body" "$TMP_DIR/legacy.pdf"
echo

# -----------------------------------------------------------
# 3. Legacy POST / with client_id/pdf_name — returns JSON
# -----------------------------------------------------------
echo "--- POST / (legacy FormData, save) ---"
CODE=$(curl -s -o "$TMP_DIR/legacy_save.json" -w "%{http_code}" \
  -F "markdown=# Saved Document" \
  -F "client_id=test-client" \
  -F "pdf_name=test-legacy" \
  "$BASE_URL/")
check "legacy convert (save → JSON)" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/legacy_save.json")"
echo

# -----------------------------------------------------------
# 3b. POST / is open to anyone: what it accepts is what it can be trusted with
# -----------------------------------------------------------
echo "--- POST / (open endpoint, engines and defaults) ---"

# pdflatex reads local files with \input, wkhtmltopdf follows redirects itself:
# neither is covered by the URL scan, so neither is offered without a key.
for ENGINE in pdflatex wkhtmltopdf; do
  CODE=$(curl -s -o "$TMP_DIR/legacy_engine.txt" -w "%{http_code}" \
    -F "markdown=Bonjour

\\input{/etc/passwd}" \
    -F "engine=$ENGINE" \
    "$BASE_URL/")
  check "engine=$ENGINE refused without a key" 400 "$CODE"
done

# A ```mermaid fence has always come out as a code listing here, and each one would be
# an outbound call: expansion stays off on the endpoint that cannot ask for it.
CODE=$(curl -s -o "$TMP_DIR/legacy_mermaid.pdf" -w "%{http_code}" \
  -F "markdown=# Diagramme $RUN_ID

\`\`\`mermaid
graph TD; A-->B;
\`\`\`" \
  "$BASE_URL/")
check "legacy mermaid fence still renders" 200 "$CODE"
if command -v pdftotext > /dev/null 2>&1; then
  pdftotext "$TMP_DIR/legacy_mermaid.pdf" "$TMP_DIR/legacy_mermaid.txt" 2>/dev/null || true
  check_contains "and stays a code block, not a diagram" "$TMP_DIR/legacy_mermaid.txt" "graph TD"
fi

# One unreachable asset used to cost the asset, never the document
CODE=$(curl -s -o "$TMP_DIR/legacy_asset.pdf" -w "%{http_code}" \
  -F "markdown=# Rapport $RUN_ID

<img src=\"http://nas.local/logo.png\">" \
  "$BASE_URL/")
check "a blocked asset does not lose the whole document" 200 "$CODE"
check_pdf "and the body is still a PDF" "$TMP_DIR/legacy_asset.pdf"
echo

# -----------------------------------------------------------
# 4. POST /api/convert — Markdown → PDF (JSON)
# -----------------------------------------------------------
echo "--- POST /api/convert ---"
CODE=$(api -o "$TMP_DIR/convert.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "markdown": "# API Convert Test\n\nThis is a **test** document.",
    "client_id": "test-client",
    "pdf_name": "test-convert",
    "options": {
      "paper_size": "a4",
      "page_numbers": true
    }
  }' \
  "$BASE_URL/api/convert")
check "convert markdown → PDF" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/convert.json")"
echo

# -----------------------------------------------------------
# 5. POST /api/html-to-pdf — HTML → PDF (JSON)
# -----------------------------------------------------------
echo "--- POST /api/html-to-pdf ---"
CODE=$(api -o "$TMP_DIR/html2pdf.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "html": "<html><body><h1>HTML to PDF</h1><p>Direct HTML conversion.</p>{{CENSOR}}</body></html>",
    "client_id": "test-client",
    "pdf_name": "test-html2pdf"
  }' \
  "$BASE_URL/api/html-to-pdf")
check "html-to-pdf (with CENSOR tag)" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/html2pdf.json")"
echo

# -----------------------------------------------------------
# 6. POST /api/render — Tera template → PDF
# -----------------------------------------------------------
echo "--- POST /api/render ---"
CODE=$(api -o "$TMP_DIR/render.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "template": "<html><body><h1>{{ title }}</h1><p>Dear {{ name }},</p><p>{{ body }}</p></body></html>",
    "data": {
      "title": "Invoice #123",
      "name": "John Doe",
      "body": "Thank you for your purchase."
    },
    "client_id": "test-client",
    "pdf_name": "test-render"
  }' \
  "$BASE_URL/api/render")
check "render template → PDF" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/render.json")"
echo

# -----------------------------------------------------------
# 7. POST /api/preview — Markdown → PNG
# -----------------------------------------------------------
echo "--- POST /api/preview ---"
CODE=$(api -o "$TMP_DIR/preview.png" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "markdown": "# Preview Test\n\nThis should produce a PNG."
  }' \
  "$BASE_URL/api/preview")
check "preview markdown → PNG" 200 "$CODE"
# Non-regression: a body with none of the new fields must still be a raw PNG of page 1
check_png "preview body is a raw PNG (no pages/dpi/layout)" "$TMP_DIR/preview.png"
echo

# -----------------------------------------------------------
# 8. POST /api/merge — Merge PDFs
# -----------------------------------------------------------
echo "--- POST /api/merge ---"
CODE=$(api -o "$TMP_DIR/merge.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "pdfs": [
      "/download/test-client/test-convert.pdf",
      "/download/test-client/test-html2pdf.pdf"
    ],
    "client_id": "test-client",
    "pdf_name": "test-merged"
  }' \
  "$BASE_URL/api/merge")
check "merge PDFs (save → JSON)" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/merge.json")"

# Without client_id/pdf_name the merged file comes back as a binary body
CODE=$(api -o "$TMP_DIR/merge.pdf" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "pdfs": [
      "/download/test-client/test-convert.pdf",
      "/download/test-client/test-html2pdf.pdf"
    ]
  }' \
  "$BASE_URL/api/merge")
check "merge PDFs (no client → PDF body)" 200 "$CODE"
check_pdf "merged body" "$TMP_DIR/merge.pdf"
echo

# -----------------------------------------------------------
# 9. POST /api/watermark — Add watermark
# -----------------------------------------------------------
echo "--- POST /api/watermark ---"
CODE=$(api -o "$TMP_DIR/watermark.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "pdf": "/download/test-client/test-convert.pdf",
    "text": "DRAFT <&>",
    "opacity": 0.1,
    "angle": -45,
    "client_id": "test-client",
    "pdf_name": "test-watermarked"
  }' \
  "$BASE_URL/api/watermark")
check "watermark PDF" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/watermark.json")"
echo

# -----------------------------------------------------------
# 10. POST /api/protect — Password protect
# -----------------------------------------------------------
echo "--- POST /api/protect ---"
CODE=$(api -o "$TMP_DIR/protect.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "pdf": "/download/test-client/test-convert.pdf",
    "password": "secret123",
    "client_id": "test-client",
    "pdf_name": "test-protected"
  }' \
  "$BASE_URL/api/protect")
check "protect PDF" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/protect.json")"
echo

# -----------------------------------------------------------
# 11. GET /download — Download saved PDF
# -----------------------------------------------------------
echo "--- GET /download (saved PDF) ---"
CODE=$(curl -s -o "$TMP_DIR/downloaded.pdf" -w "%{http_code}" \
  "$BASE_URL/download/test-client/test-convert.pdf")
check "download saved PDF" 200 "$CODE"
check_pdf "downloaded body" "$TMP_DIR/downloaded.pdf"
echo

# -----------------------------------------------------------
# 12. Backward compatibility — a request asking for nothing new
#     must get exactly {"download_url": "…"}, as it always did
# -----------------------------------------------------------
echo "--- POST /api/convert (response shape, no new option) ---"
PLAIN_PAYLOAD="{\"markdown\": \"# Shape check $RUN_ID\", \"client_id\": \"test-client\", \"pdf_name\": \"test-shape\"}"
CODE=$(api -o "$TMP_DIR/shape.json" -w "%{http_code}" \
  -H "Content-Type: application/json" -d "$PLAIN_PAYLOAD" \
  "$BASE_URL/api/convert")
check "convert without any new option" 200 "$CODE"
check_contains "response carries download_url" "$TMP_DIR/shape.json" '"download_url"'
check_absent "no layout field (autolayout not asked)" "$TMP_DIR/shape.json" '"layout"'
check_absent "no warnings field (no chart, no diagram)" "$TMP_DIR/shape.json" '"warnings"'
check_absent "no cached field on a first render" "$TMP_DIR/shape.json" '"cached"'

# The same document again: the content-addressed cache should answer it
CODE=$(api -o "$TMP_DIR/shape2.json" -w "%{http_code}" \
  -H "Content-Type: application/json" -d "$PLAIN_PAYLOAD" \
  "$BASE_URL/api/convert")
check "convert the same document again" 200 "$CODE"
if grep -q '"cached":true' "$TMP_DIR/shape2.json"; then
  green "  ✓ second render served from the cache (cached: true)"
  PASS=$((PASS + 1))
else
  skip "no cache hit — the server likely runs with PDF_CACHE_ENABLED=false"
fi
echo

# -----------------------------------------------------------
# 13. Themes
# -----------------------------------------------------------
echo "--- Themes ---"
CODE=$(api -o "$TMP_DIR/themes.json" -w "%{http_code}" "$BASE_URL/api/themes")
check "list themes" 200 "$CODE"
check_contains "the report theme is published" "$TMP_DIR/themes.json" '"name":"report"'
check_contains "each theme advertises a preview_url" "$TMP_DIR/themes.json" '"preview_url"'

CODE=$(api -o "$TMP_DIR/theme.png" -w "%{http_code}" \
  "$BASE_URL/api/themes/report/latest/preview.png")
check "theme preview (version latest)" 200 "$CODE"
check_png "theme preview body" "$TMP_DIR/theme.png"

CODE=$(api -o "$TMP_DIR/theme_cover.png" -w "%{http_code}" \
  "$BASE_URL/api/themes/report/latest/preview.png?cover=true")
check "theme preview of the cover page" 200 "$CODE"

# A pinned version and a cover, through the render pipeline this time
CODE=$(api -o "$TMP_DIR/theme_convert.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# Themed $RUN_ID\n\nA paragraph.\",
    \"options\": {
      \"theme\": \"report@1\",
      \"cover\": {\"title\": \"Rapport\", \"subtitle\": \"Interne\", \"date\": \"2026\"}
    },
    \"client_id\": \"test-client\", \"pdf_name\": \"test-themed\"
  }" \
  "$BASE_URL/api/convert")
check "convert with a pinned theme and a cover" 200 "$CODE"
echo "  Response: $(cat "$TMP_DIR/theme_convert.json")"
echo

# -----------------------------------------------------------
# 14. Charts, diagrams and censoring
# -----------------------------------------------------------
echo "--- Charts, diagrams, CENSOR ---"
CODE=$(api -o "$TMP_DIR/blocks.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# Blocks $RUN_ID\n\n\`\`\`chart\n{\\\"type\\\": \\\"bar\\\", \\\"labels\\\": [\\\"Q1\\\", \\\"Q2\\\"], \\\"series\\\": [{\\\"name\\\": \\\"2026\\\", \\\"data\\\": [12000, 18000]}]}\n\`\`\`\n\n\`\`\`mermaid\ngraph TD; A[Start] --> B[End];\n\`\`\`\n\n{{CENSOR:start,premium}}\nSECRETSAUCE\n{{CENSOR:end}}\n\",
    \"options\": {\"censor_label\": \"RÉSERVÉ\"},
    \"client_id\": \"test-client\", \"pdf_name\": \"test-blocks\"
  }" \
  "$BASE_URL/api/convert")
check "convert with chart + mermaid + CENSOR region" 200 "$CODE"
check_contains "the PDF was produced" "$TMP_DIR/blocks.json" '"download_url"'
echo "  Response: $(cat "$TMP_DIR/blocks.json")"

# A block that cannot be rendered never fails the request — it warns
CODE=$(api -o "$TMP_DIR/badchart.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# Bad chart $RUN_ID\n\n\`\`\`chart\n{\\\"type\\\": \\\"nope\\\"}\n\`\`\`\n\",
    \"client_id\": \"test-client\", \"pdf_name\": \"test-badchart\"
  }" \
  "$BASE_URL/api/convert")
check "an invalid chart spec still produces a PDF" 200 "$CODE"
check_contains "and reports it in warnings" "$TMP_DIR/badchart.json" '"warnings"'

# options.charts:false turns both block types off, silently and on purpose
CODE=$(api -o "$TMP_DIR/nocharts.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# No blocks $RUN_ID\n\n\`\`\`mermaid\ngraph TD; A --> B;\n\`\`\`\n\",
    \"options\": {\"charts\": false},
    \"client_id\": \"test-client\", \"pdf_name\": \"test-nocharts\"
  }" \
  "$BASE_URL/api/convert")
check "options.charts:false → blocks left as code" 200 "$CODE"
check_absent "and no warning, it was an explicit choice" "$TMP_DIR/nocharts.json" '"warnings"'
echo

# -----------------------------------------------------------
# 15. Layout Doctor — options.autolayout and POST /api/layout
# -----------------------------------------------------------
echo "--- Layout Doctor ---"
CODE=$(api -o "$TMP_DIR/autolayout.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# Autolayout $RUN_ID\n\nA short paragraph.\",
    \"options\": {\"autolayout\": true},
    \"client_id\": \"test-client\", \"pdf_name\": \"test-autolayout\"
  }" \
  "$BASE_URL/api/convert")
check "convert with options.autolayout" 200 "$CODE"
check_contains "the report comes back in the layout field" "$TMP_DIR/autolayout.json" '"layout"'
check_contains "with a score" "$TMP_DIR/autolayout.json" '"score"'
echo "  Response: $(cat "$TMP_DIR/autolayout.json")"

CODE=$(api -o "$TMP_DIR/layout.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf"}' \
  "$BASE_URL/api/layout")
check "audit an existing PDF" 200 "$CODE"
check_contains "report has pages, score and issues" "$TMP_DIR/layout.json" '"issues"'
echo "  Response: $(cat "$TMP_DIR/layout.json")"
echo

# -----------------------------------------------------------
# 16. Redaction
# -----------------------------------------------------------
echo "--- POST /api/redact ---"
CODE=$(api -o "$TMP_DIR/redact_src.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "{
    \"markdown\": \"# Contract $RUN_ID\n\nContact: jean.dupont@example.com\n\nIBAN FR7630006000011234567890189\n\",
    \"client_id\": \"test-client\", \"pdf_name\": \"test-redact-src\"
  }" \
  "$BASE_URL/api/convert")
check "source document for redaction" 200 "$CODE"

CODE=$(api -o "$TMP_DIR/redact.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "pdf": "/download/test-client/test-redact-src.pdf",
    "patterns": ["Contact"],
    "entities": ["email", "iban"],
    "client_id": "test-client",
    "pdf_name": "test-redacted"
  }' \
  "$BASE_URL/api/redact")
check "redact patterns and entities" 200 "$CODE"
check_contains "counts the areas painted per page" "$TMP_DIR/redact.json" '"redactions"'
check_contains "and says the pages were flattened" "$TMP_DIR/redact.json" '"mode":"flatten"'
echo "  Response: $(head -c 200 "$TMP_DIR/redact.json")"

# Without client_id the redacted file comes back as a binary body
CODE=$(api -o "$TMP_DIR/redact.pdf" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-redact-src.pdf", "entities": ["email"]}' \
  "$BASE_URL/api/redact")
check "redact (no client → PDF body)" 200 "$CODE"
check_pdf "redacted body" "$TMP_DIR/redact.pdf"
echo

# -----------------------------------------------------------
# 17. Visual diff
# -----------------------------------------------------------
echo "--- POST /api/diff ---"
CODE=$(api -o "$TMP_DIR/diff_same.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "before": "/download/test-client/test-convert.pdf",
    "after": "/download/test-client/test-convert.pdf",
    "dpi": 72
  }' \
  "$BASE_URL/api/diff")
check "diff a document against itself" 200 "$CODE"
check_contains "verdict is identical" "$TMP_DIR/diff_same.json" '"verdict":"identical"'

CODE=$(api -o "$TMP_DIR/diff_changed.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{
    "before": "/download/test-client/test-convert.pdf",
    "after": "/download/test-client/test-render.pdf",
    "dpi": 72
  }' \
  "$BASE_URL/api/diff")
check "diff two different documents" 200 "$CODE"
check_contains "verdict is changed" "$TMP_DIR/diff_changed.json" '"verdict":"changed"'
check_contains "and names the pages that moved" "$TMP_DIR/diff_changed.json" '"pages_changed"'
echo "  Response: $(head -c 200 "$TMP_DIR/diff_changed.json")"
echo

# -----------------------------------------------------------
# 18. Preview — pages, dpi and layout
# -----------------------------------------------------------
echo "--- POST /api/preview (pages / dpi / layout) ---"
CODE=$(api -o "$TMP_DIR/preview_images.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# One\n\nText.\n\n# Two", "pages": "all", "dpi": 72, "layout": "images"}' \
  "$BASE_URL/api/preview")
check "preview pages=all, layout=images" 200 "$CODE"
check_contains "one entry per page, base64 encoded" "$TMP_DIR/preview_images.json" '"pages"'

CODE=$(api -o "$TMP_DIR/preview_sheet.png" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# One\n\nText.", "pages": "all", "dpi": 72, "layout": "sheet"}' \
  "$BASE_URL/api/preview")
check "preview layout=sheet (contact sheet)" 200 "$CODE"
check_png "contact sheet body" "$TMP_DIR/preview_sheet.png"
echo

# -----------------------------------------------------------
# 19. Metrics and request correlation
# -----------------------------------------------------------
echo "--- Observability ---"
CODE=$(api -o "$TMP_DIR/metrics.txt" -w "%{http_code}" "$BASE_URL/api/metrics")
check "Prometheus exposition" 200 "$CODE"
check_contains "request counter is exposed" "$TMP_DIR/metrics.txt" "mdtopdf_requests_total"
check_absent "no client_id leaks into the labels" "$TMP_DIR/metrics.txt" "test-client"

# The request id sent in is the one echoed back and attached to every log event
curl -s -D "$TMP_DIR/headers.txt" -o /dev/null \
  -H "X-Request-Id: test-api-$RUN_ID" "$BASE_URL/api/health"
check_contains "X-Request-Id is echoed back" "$TMP_DIR/headers.txt" "test-api-$RUN_ID"
echo

# -----------------------------------------------------------
# Error cases
# -----------------------------------------------------------
echo "--- Error cases ---"

# Bad request: merge with < 2 PDFs
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdfs": ["/download/test-client/test-convert.pdf"]}' \
  "$BASE_URL/api/merge")
check "merge with 1 PDF → 400" 400 "$CODE"

# Bad request: render with non-object data
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"template": "<h1>test</h1>", "data": "not an object"}' \
  "$BASE_URL/api/render")
check "render with bad data → 400" 400 "$CODE"

# Bad request: preview with a template but no data
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"template": "<h1>{{ title }}</h1>"}' \
  "$BASE_URL/api/preview")
check "preview template without data → 400" 400 "$CODE"

# Bad request: empty password
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf", "password": ""}' \
  "$BASE_URL/api/protect")
check "protect with empty password → 400" 400 "$CODE"

# Bad request: out of range opacity
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf", "text": "X", "opacity": 42}' \
  "$BASE_URL/api/watermark")
check "watermark with opacity 42 → 400" 400 "$CODE"

# Bad request: CSS injection through page_number_format
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# x", "options": {"page_numbers": true, "page_number_format": "\"a\"} body { x: url(http://evil) } @page {"}}' \
  "$BASE_URL/api/convert")
check "CSS injection in page_number_format → 400" 400 "$CODE"

# Path traversal: writing outside public/pdf
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# x", "client_id": "../../etc", "pdf_name": "evil"}' \
  "$BASE_URL/api/convert")
check "path traversal in client_id → 400" 400 "$CODE"

# Path traversal: reading outside public/pdf
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/../../../etc/passwd", "password": "x"}' \
  "$BASE_URL/api/protect")
check "path traversal in pdf path → 400" 400 "$CODE"

# Not found: download non-existent file
CODE=$(curl -s -o /dev/null -w "%{http_code}" \
  "$BASE_URL/download/no-such-client/no-such-file.pdf")
check "download non-existent → 404" 404 "$CODE"

# Encoded traversal on the download route
CODE=$(curl -s -o /dev/null -w "%{http_code}" --path-as-is \
  "$BASE_URL/download/test-client/..%2F..%2F..%2Fetc%2Fpasswd")
check "encoded traversal on /download → 404" 404 "$CODE"

# Unknown theme: 404, and the message lists what does exist
CODE=$(api -o "$TMP_DIR/badtheme.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# x", "options": {"theme": "no-such-theme"}}' \
  "$BASE_URL/api/convert")
check "unknown theme → 404" 404 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  "$BASE_URL/api/themes/no-such-theme/1/preview.png")
check "preview of an unknown theme → 404" 404 "$CODE"

# A pattern that looks like a regex is refused rather than matched literally:
# believing the job was done while nothing was blacked out is the real failure
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf", "patterns": ["\\d{4}"]}' \
  "$BASE_URL/api/redact")
check "redact with a regex-looking pattern → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf"}' \
  "$BASE_URL/api/redact")
check "redact with neither patterns nor entities → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/test-client/test-convert.pdf", "entities": ["ssn"]}' \
  "$BASE_URL/api/redact")
check "redact with an unknown entity → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# x", "dpi": 9999}' \
  "$BASE_URL/api/preview")
check "preview with dpi 9999 → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"before": "/download/test-client/test-convert.pdf", "after": "/download/test-client/test-render.pdf", "dpi": 5}' \
  "$BASE_URL/api/diff")
check "diff with dpi 5 → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"pdf": "/download/../../etc/passwd"}' \
  "$BASE_URL/api/layout")
check "path traversal on /api/layout → 400" 400 "$CODE"

# An internal target in the client CSS is refused before anything is rendered
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "# x", "css": "body { background: url(http://169.254.169.254/latest/meta-data/) }"}' \
  "$BASE_URL/api/convert")
check "SSRF through the client CSS → 400" 400 "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"html": "<img src=\"file:///etc/passwd\">"}' \
  "$BASE_URL/api/html-to-pdf")
check "file:// reference in a document → 400" 400 "$CODE"

# The fence heuristic belongs to Markdown: in HTML a backtick run hides nothing
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"html": "<div>\n```\n<img src=\"http://169.254.169.254/latest/\">\n```\n</div>"}' \
  "$BASE_URL/api/html-to-pdf")
check "a code fence in HTML hides nothing from the guard → 400" 400 "$CODE"

# An unterminated fence is the shape an attacker controls, not a code block
CODE=$(api -o /dev/null -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "<div>\n```\n<img src=\"http://10.1.2.3/internal.png\">\n</div>"}' \
  "$BASE_URL/api/convert")
check "an unterminated fence does not hide a URL → 400" 400 "$CODE"

# pdflatex is reachable with a key, but not the filesystem: \input is refused by
# kpathsea, so the engine fails instead of embedding the file in the PDF
CODE=$(api -o "$TMP_DIR/latex_lfi.json" -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d '{"markdown": "Bonjour\n\n\\input{/etc/passwd}\n\nFin", "engine": "pdflatex"}' \
  "$BASE_URL/api/convert")
check "pdflatex \\input of a local file → 500, not a PDF" 500 "$CODE"
check_absent "and nothing of the file came back" "$TMP_DIR/latex_lfi.json" "root:x:"

# -----------------------------------------------------------
# Ingestion — the way a caller's own file gets in
# -----------------------------------------------------------
echo
echo "Ingestion (POST /api/files)"

# The fixture is a PDF this service produced: the suite must not depend on a
# binary file committed to the repository.
api -o "$TMP_DIR/upload_src.pdf" \
  -H "Content-Type: application/json" \
  -d "{\"markdown\": \"# Ingestion $RUN_ID\n\nUne page, du texte, un tableau.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n\\\\newpage\n\n## Deuxieme page\n\nSuite.\"}" \
  "$BASE_URL/api/convert" > /dev/null
check_pdf "fixture produced for the upload tests" "$TMP_DIR/upload_src.pdf"

CODE=$(api -o "$TMP_DIR/upload.json" -w "%{http_code}" \
  -F "file=@$TMP_DIR/upload_src.pdf;filename=fixture.pdf" \
  "$BASE_URL/api/files")
check "POST /api/files → 201" 201 "$CODE"
check_contains "the asset reports its detected kind" "$TMP_DIR/upload.json" '"kind":"pdf"'
check_contains "and when it expires" "$TMP_DIR/upload.json" '"expires_at"'

ASSET=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/upload.json" | head -1)
if [ -z "$ASSET" ]; then
  red "  ✗ no asset id came back — skipping the tool suite"
  FAIL=$((FAIL + 1))
else
  green "  ✓ asset id: $ASSET"
  PASS=$((PASS + 1))

  CODE=$(api -o "$TMP_DIR/meta.json" -w "%{http_code}" "$BASE_URL/api/files/$ASSET/meta")
  check "GET /api/files/{id}/meta → 200" 200 "$CODE"
  check_contains "the page count was read from the file" "$TMP_DIR/meta.json" '"pages"'

  CODE=$(api -o "$TMP_DIR/fetched.pdf" -w "%{http_code}" "$BASE_URL/api/files/$ASSET")
  check "GET /api/files/{id} → 200" 200 "$CODE"
  check_pdf "and the bytes come back unchanged" "$TMP_DIR/fetched.pdf"

  # The whole point of the socle: a route that predates uploads accepts one
  CODE=$(api -o "$TMP_DIR/wm_asset.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"text\": \"CONFIDENTIEL\"}" \
    "$BASE_URL/api/watermark")
  check "an existing route accepts asset:// → 200" 200 "$CODE"
  check_pdf "and still answers a PDF body" "$TMP_DIR/wm_asset.pdf"

  # -----------------------------------------------------------
  # The toolbelt
  # -----------------------------------------------------------
  echo
  echo "Toolbelt"

  CODE=$(api -o "$TMP_DIR/compress.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"level\": \"ebook\", \"output\": \"asset\"}" \
    "$BASE_URL/api/compress")
  check "POST /api/compress → 200" 200 "$CODE"
  check_contains "compression returns a verdict" "$TMP_DIR/compress.json" '"verdict"'
  check_contains "and the verdict names its checks" "$TMP_DIR/compress.json" '"checks"'
  check_contains "including whether the text survived" "$TMP_DIR/compress.json" 'text-preserved'

  CODE=$(api -o "$TMP_DIR/extract_pages.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"op\": \"extract\", \"pages\": \"1\"}" \
    "$BASE_URL/api/pages")
  check "POST /api/pages (extract) → 200" 200 "$CODE"
  check_pdf "and hands back the extracted page" "$TMP_DIR/extract_pages.pdf"

  CODE=$(api -o "$TMP_DIR/pages_bad.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"op\": \"extract\", \"pages\": \"9-3\"}" \
    "$BASE_URL/api/pages")
  check "a reversed page range → 400, not a silent empty PDF" 400 "$CODE"

  CODE=$(api -o "$TMP_DIR/rotate.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"op\": \"rotate\", \"angle\": 90}" \
    "$BASE_URL/api/pages")
  check "POST /api/pages (rotate) → 200" 200 "$CODE"
  check_pdf "and the rotated document is a PDF" "$TMP_DIR/rotate.pdf"

  CODE=$(api -o "$TMP_DIR/raster.png" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"pages\": \"1\", \"dpi\": 72}" \
    "$BASE_URL/api/rasterize")
  check "POST /api/rasterize → 200" 200 "$CODE"
  check_png "one page comes back as a raw PNG" "$TMP_DIR/raster.png"

  # img2pdf is a small dependency, but a slim image may still not carry it
  if api "$BASE_URL/api/health" | grep -q '"images-to-pdf"'; then
    # The image fixture is drawn by the service itself from the PDF above: nothing
    # binary in the repository, and nothing to install to run this suite.
    CODE=$(api -o "$TMP_DIR/raster_asset.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"pages\": \"1\", \"dpi\": 72, \"output\": \"asset\"}" \
      "$BASE_URL/api/rasterize")
    check "a rasterized page kept as an asset → 200" 200 "$CODE"
    check_contains "and stored as a PNG" "$TMP_DIR/raster_asset.json" '"kind":"png"'

    IMG=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/raster_asset.json" | head -1)
    if [ -n "$IMG" ]; then
      CODE=$(api -o "$TMP_DIR/images.pdf" -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"images\": [\"asset://$IMG\", \"asset://$IMG\"], \"paper_size\": \"A4\", \"margin\": 12}" \
        "$BASE_URL/api/images-to-pdf")
      check "POST /api/images-to-pdf → 200" 200 "$CODE"
      check_pdf "two images make one document" "$TMP_DIR/images.pdf"

      CODE=$(api -o "$TMP_DIR/images_asset.json" -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"images\": [\"asset://$IMG\"], \"fit\": \"actual\", \"output\": \"asset\"}" \
        "$BASE_URL/api/images-to-pdf")
      check "one image at its own size → 200" 200 "$CODE"
      check_contains "and the page count is reported" "$TMP_DIR/images_asset.json" '"pages":1'

      # The kind is read from the bytes: a PDF handed in as an image is refused
      # rather than embedded as a page nobody can open
      CODE=$(api -o /dev/null -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"images\": [\"asset://$ASSET\"]}" \
        "$BASE_URL/api/images-to-pdf")
      check "a PDF passed off as an image → 400" 400 "$CODE"
    else
      red "  ✗ no image asset came back from /api/rasterize"
      FAIL=$((FAIL + 1))
    fi
  else
    skip "img2pdf is not installed in this image"
  fi

  # -----------------------------------------------------------
  # An encrypted document is the caller's problem to fix, not a breakdown of ours.
  # The fixture is produced by /api/protect: no password-protected file is committed.
  # -----------------------------------------------------------
  CODE=$(api -o "$TMP_DIR/protected.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"password\": \"secret-$RUN_ID\", \"output\": \"asset\"}" \
    "$BASE_URL/api/protect")
  check "an encrypted fixture is produced → 200" 200 "$CODE"

  LOCKED=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/protected.json" | head -1)
  if [ -n "$LOCKED" ]; then
    LOCKED_ROUTES="compress rasterize pdfa extract"
    # OCR opens the document the same way, but only where its binary exists
    if api "$BASE_URL/api/health" | grep -q '"ocr"'; then
      LOCKED_ROUTES="$LOCKED_ROUTES ocr"
    fi

    for route in $LOCKED_ROUTES; do
      CODE=$(api -o "$TMP_DIR/locked_$route.json" -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"pdf\": \"asset://$LOCKED\"}" \
        "$BASE_URL/api/$route")
      check "an encrypted PDF on /api/$route → 400, not 500" 400 "$CODE"
      check_contains "and the answer sends the caller to /api/unlock" \
        "$TMP_DIR/locked_$route.json" "/api/unlock"
    done

    # And the route it names does take that file
    CODE=$(api -o "$TMP_DIR/unlocked.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$LOCKED\", \"password\": \"secret-$RUN_ID\", \"output\": \"asset\"}" \
      "$BASE_URL/api/unlock")
    check "POST /api/unlock with the right password → 200" 200 "$CODE"
  else
    red "  ✗ /api/protect returned no asset — the encryption cases cannot run"
    FAIL=$((FAIL + 1))
  fi

  CODE=$(api -o "$TMP_DIR/extract.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"format\": \"markdown\"}" \
    "$BASE_URL/api/extract")
  check "POST /api/extract → 200" 200 "$CODE"
  check_contains "the extracted document carries its text" "$TMP_DIR/extract.json" '"content"'

  CODE=$(api -o "$TMP_DIR/repair.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\"}" \
    "$BASE_URL/api/repair")
  check "POST /api/repair on a sound file → 200" 200 "$CODE"

  # A tool that removes a protection must never be a tool that breaks one
  CODE=$(api -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\"}" \
    "$BASE_URL/api/unlock")
  check "POST /api/unlock without a password → 400 (it cracks nothing)" 400 "$CODE"

  CODE=$(api -o "$TMP_DIR/numbered.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"format\": \"{page} / {pages}\"}" \
    "$BASE_URL/api/pages/number")
  check "POST /api/pages/number → 200" 200 "$CODE"

  CODE=$(api -o "$TMP_DIR/crop.pdf" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"box\": \"auto\"}" \
    "$BASE_URL/api/crop")
  check "POST /api/crop (auto) → 200" 200 "$CODE"

  CODE=$(api -o "$TMP_DIR/pdfa.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"variant\": \"pdf/a-2b\", \"output\": \"asset\"}" \
    "$BASE_URL/api/pdfa")
  check "POST /api/pdfa → 200" 200 "$CODE"

  # OCR is slow and its binary may be absent from a slim image: report, do not fail
  if api "$BASE_URL/api/health" | grep -q '"ocr"'; then
    CODE=$(api -o "$TMP_DIR/ocr.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"mode\": \"auto\", \"output\": \"asset\"}" \
      "$BASE_URL/api/ocr")
    check "POST /api/ocr → 200" 200 "$CODE"
    check_contains "OCR reports what it could read" "$TMP_DIR/ocr.json" '"verdict"'
  else
    skip "OCR is not installed in this image"
  fi

  # LibreOffice is a large dependency and a slim image may not carry it
  if api "$BASE_URL/api/health" | grep -q '"office"'; then
    # The other direction: a PDF rebuilt into an editable document. It is a guess, and
    # the response has to say so — that warning is the whole point of the route.
    CODE=$(api -o "$TMP_DIR/pdf_to_office.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"to\": \"docx\", \"output\": \"asset\"}" \
      "$BASE_URL/api/pdf-to-office")
    check "POST /api/pdf-to-office → 200" 200 "$CODE"
    check_contains "the produced asset is a Word document" "$TMP_DIR/pdf_to_office.json" '"kind":"docx"'
    check_contains "the reconstruction is announced as approximate" "$TMP_DIR/pdf_to_office.json" 'approximate by nature'
    check_contains "and how much of the text survived is measured" "$TMP_DIR/pdf_to_office.json" 'text-preserved'

    CODE=$(api -o /dev/null -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"to\": \"odt\"}" \
      "$BASE_URL/api/pdf-to-office")
    check "a target format nobody can produce here → 400" 400 "$CODE"

    # The fixture is built by LibreOffice itself: no binary file in the repository
    docker compose exec -T pandoc sh -c \
      'cd /tmp && printf "Rapport\n\nUn paragraphe.\n" > s.txt && soffice --headless \
       --convert-to docx --outdir /tmp/docxfix /tmp/s.txt' >/dev/null 2>&1 || true
    docker compose exec -T pandoc cat /tmp/docxfix/s.docx > "$TMP_DIR/fixture.docx" 2>/dev/null || true

    if [ -s "$TMP_DIR/fixture.docx" ]; then
      CODE=$(api -o "$TMP_DIR/docx_upload.json" -w "%{http_code}" \
        -F "file=@$TMP_DIR/fixture.docx" "$BASE_URL/api/files")
      check "a Word file uploads → 201" 201 "$CODE"
      check_contains "and is recognised as docx from its bytes" "$TMP_DIR/docx_upload.json" '"kind":"docx"'

      DOCX=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/docx_upload.json" | head -1)
      CODE=$(api -o "$TMP_DIR/office.json" -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"file\": \"asset://$DOCX\", \"output\": \"asset\"}" \
        "$BASE_URL/api/office-to-pdf")
      check "POST /api/office-to-pdf → 200" 200 "$CODE"
      check_contains "and reports what survived the conversion" "$TMP_DIR/office.json" 'text-layer'

      # -----------------------------------------------------------
      # Asynchronous work
      # -----------------------------------------------------------
      echo
      echo "Jobs"

      CODE=$(api -o "$TMP_DIR/job.json" -w "%{http_code}" \
        -H "Content-Type: application/json" \
        -d "{\"endpoint\": \"/api/office-to-pdf\", \"body\": {\"file\": \"asset://$DOCX\"}}" \
        "$BASE_URL/api/jobs")
      check "POST /api/jobs → 202" 202 "$CODE"
      check_contains "and hands back where to poll" "$TMP_DIR/job.json" '"poll_url"'

      JOB=$(sed -n 's/.*"job_id":"\(job_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/job.json" | head -1)
      # Twenty attempts at a second apart: LibreOffice's first start is the slow one
      for _ in $(seq 1 20); do
        api -o "$TMP_DIR/job_state.json" "$BASE_URL/api/jobs/$JOB" >/dev/null 2>&1
        grep -q '"status":"done"\|"status":"failed"' "$TMP_DIR/job_state.json" && break
        sleep 1
      done
      check_contains "the job finishes" "$TMP_DIR/job_state.json" '"status":"done"'
      check_contains "and its result carries the produced asset" "$TMP_DIR/job_state.json" '"asset"'
    else
      skip "could not build a Word fixture with LibreOffice"
    fi
  else
    skip "LibreOffice is not installed in this image"
  fi

  CODE=$(api -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"endpoint": "/api/nonexistent", "body": {}}' \
    "$BASE_URL/api/jobs")
  check "an endpoint that cannot be queued → 400" 400 "$CODE"

  # -----------------------------------------------------------
  # Proof — nobody else answers this question
  # -----------------------------------------------------------
  echo
  echo "Attestation"

  CODE=$(api -o "$TMP_DIR/attest.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"pdf\": \"asset://$ASSET\", \"engine\": \"weasyprint\"}" \
    "$BASE_URL/api/attest")
  check "POST /api/attest → 200" 200 "$CODE"
  check_contains "the sealed record is a v1 line" "$TMP_DIR/attest.json" '"attestation":"v1.'

  SEAL=$(sed -n 's/.*"attestation":"\([^"]*\)".*/\1/p' "$TMP_DIR/attest.json" | head -1)
  if [ -n "$SEAL" ]; then
    CODE=$(api -o "$TMP_DIR/verify.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"attestation\": \"$SEAL\"}" \
      "$BASE_URL/api/verify")
    check "POST /api/verify on the genuine file → 200" 200 "$CODE"
    check_contains "and it verifies" "$TMP_DIR/verify.json" '"verdict":"valid"'

    # The same seal against a different document must say so, and say which way
    api -o "$TMP_DIR/other.json" \
      -H "Content-Type: application/json" \
      -d "{\"markdown\": \"# Autre document $RUN_ID\", \"client_id\": \"test-$RUN_ID\", \"pdf_name\": \"other\"}" \
      "$BASE_URL/api/convert" > /dev/null
    CODE=$(api -o "$TMP_DIR/verify_other.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"/download/test-$RUN_ID/other.pdf\", \"attestation\": \"$SEAL\"}" \
      "$BASE_URL/api/verify")
    check "the same seal on another document → 200" 200 "$CODE"
    check_contains "and it reports an altered match" "$TMP_DIR/verify_other.json" '"verdict":"altered"'

    CODE=$(api -o "$TMP_DIR/verify_forged.json" -w "%{http_code}" \
      -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$ASSET\", \"attestation\": \"v1.YWJj.deadbeef\"}" \
      "$BASE_URL/api/verify")
    check_contains "a rewritten seal is reported as forged" "$TMP_DIR/verify_forged.json" '"verdict":"forged"'
  else
    red "  ✗ no attestation came back"
    FAIL=$((FAIL + 1))
  fi

  # -----------------------------------------------------------
  # The document contract
  # -----------------------------------------------------------
  echo
  echo "Compose (the contract)"

  CODE=$(api -o "$TMP_DIR/compose.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"markdown\": \"# Contrat $RUN_ID\n\nUn paragraphe court.\", \"constraints\": {\"max_pages\": 4, \"min_layout_score\": 60}, \"client_id\": \"test-$RUN_ID\", \"pdf_name\": \"composed\"}" \
    "$BASE_URL/api/compose")
  check "POST /api/compose → 200" 200 "$CODE"
  check_contains "it renders a verdict on the contract" "$TMP_DIR/compose.json" '"verdict":"met"'
  check_contains "and the log of its passes" "$TMP_DIR/compose.json" '"passes"'

  # An impossible contract must be reported, not silently ignored
  CODE=$(api -o "$TMP_DIR/compose_unmet.json" -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d "{\"markdown\": \"# Contrat impossible $RUN_ID\n\nUn paragraphe court.\", \"constraints\": {\"min_pages\": 40}, \"client_id\": \"test-$RUN_ID\", \"pdf_name\": \"composed2\"}" \
    "$BASE_URL/api/compose")
  check "an unreachable contract still returns the document" 200 "$CODE"
  check_contains "and says which constraint is unmet" "$TMP_DIR/compose_unmet.json" '"unmet"'

  # -----------------------------------------------------------
  # Retention
  # -----------------------------------------------------------
  echo
  echo "Retention"

  CODE=$(api -o /dev/null -w "%{http_code}" -X DELETE "$BASE_URL/api/files/$ASSET")
  check "DELETE /api/files/{id} → 204" 204 "$CODE"

  CODE=$(api -o /dev/null -w "%{http_code}" "$BASE_URL/api/files/$ASSET/meta")
  check "and the asset is gone → 404" 404 "$CODE"
fi

# An id that is not one of ours never reaches the filesystem
CODE=$(api -o /dev/null -w "%{http_code}" "$BASE_URL/api/files/as_..%2f..%2fetc%2fpasswd/meta")
check "a traversal in the asset id → 400 or 404" "$( [ "$CODE" = "400" ] && echo 400 || echo 404 )" "$CODE"

CODE=$(api -o /dev/null -w "%{http_code}" "$BASE_URL/api/jobs/job_00000000000000000000000000000000")
check "an unknown job → 404" 404 "$CODE"

# Authentication (only meaningful when the server runs with API_KEY)
if [ -n "$API_KEY" ]; then
  # Ownership: an id is not a capability. What a key uploads, and what a tool produces
  # for it, both carry that key's name — otherwise an id leaked in a log or a support
  # ticket would hand a stranger the document.
  api -o "$TMP_DIR/owned_src.pdf" -H "Content-Type: application/json" \
    -d "{\"markdown\": \"# Propriete $RUN_ID\"}" "$BASE_URL/api/convert" > /dev/null
  api -o "$TMP_DIR/owned_upload.json" -F "file=@$TMP_DIR/owned_src.pdf" "$BASE_URL/api/files" > /dev/null
  check_contains "an upload records the key that made it" "$TMP_DIR/owned_upload.json" '"owner"'

  OWNED=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' "$TMP_DIR/owned_upload.json" | head -1)
  if [ -n "$OWNED" ]; then
    api -o "$TMP_DIR/owned_out.json" -H "Content-Type: application/json" \
      -d "{\"pdf\": \"asset://$OWNED\", \"op\": \"extract\", \"pages\": \"1\", \"output\": \"asset\"}" \
      "$BASE_URL/api/pages" > /dev/null
    check_contains "and so does what a tool produces from it" "$TMP_DIR/owned_out.json" '"owner"'

    # A wrong key must not even learn the asset exists: 404, never 403
    CODE=$(curl -s -o /dev/null -w "%{http_code}" -H "X-API-Key: ${API_KEY}-wrong" \
      "$BASE_URL/api/files/$OWNED/meta")
    check "a key that is not the owner cannot read it" 401 "$CODE"
  fi

  # PUBLIC_TOOLS changes what an anonymous request gets, and the suite has no access to
  # the server's environment: ask the server instead. An anonymous upload that succeeds
  # means the free tier is on, and the endpoints the public pages drive answer without a
  # key by design. Guessing one of the two configurations would make this suite fail on a
  # correctly configured deployment.
  PUBLIC_TIER=no
  if [ "$(curl -s -o /dev/null -w "%{http_code}" -F "file=@$TMP_DIR/legacy.pdf" \
          "$BASE_URL/api/files")" = "201" ]; then
    PUBLIC_TIER=yes
    yellow "  ~ free tier is on (PUBLIC_TOOLS): the tool endpoints answer without a key"
  fi

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"markdown": "# x"}' \
    "$BASE_URL/api/convert")
  if [ "$PUBLIC_TIER" = "yes" ]; then
    check "convert without a key → 200 on the free tier" 200 "$CODE"
  else
    check "convert without API key → 401" 401 "$CODE"
  fi

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $API_KEY" \
    -d '{"markdown": "# x"}' \
    "$BASE_URL/api/convert")
  check "convert with bearer token → 200" 200 "$CODE"

  # `/api/metrics` and `/api/render` are never on the free tier, whatever it is set to:
  # a template engine and an operator's counters do not face the open internet.
  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/metrics")
  check "GET /api/metrics without API key → 401, free tier or not" 401 "$CODE"

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"template": "<p>x</p>", "data": {}}' \
    "$BASE_URL/api/render")
  check "POST /api/render without API key → 401, free tier or not" 401 "$CODE"

  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/themes")
  if [ "$PUBLIC_TIER" = "yes" ]; then
    check "GET /api/themes without a key → 200 on the free tier" 200 "$CODE"
  else
    check "GET /api/themes without API key → 401" 401 "$CODE"
  fi

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"before": "/download/a/b.pdf", "after": "/download/a/c.pdf"}' \
    "$BASE_URL/api/diff")
  if [ "$PUBLIC_TIER" = "yes" ]; then
    # The guard lets it through, so the answer is about the missing document, not the key
    check "POST /api/diff without a key → 404 on the free tier" 404 "$CODE"
  else
    check "POST /api/diff without API key → 401" 401 "$CODE"
  fi

  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/health")
  check "GET /api/health stays open (container probe)" 200 "$CODE"

  # The legacy endpoint was never covered by the key, by design
  CODE=$(curl -s -o /dev/null -w "%{http_code}" -F "markdown=# legacy" "$BASE_URL/")
  check "POST / stays open without a key (legacy contract)" 200 "$CODE"
fi

# =========================================================================
# Accounts: signing up, minting a key, and the quality record
# =========================================================================
# This is the funnel the whole product hangs off, and it broke silently once
# already: on a deployment with no API_KEY the guard answered `open` before it
# ever looked up a member's own key, so no work was ever attributed and the
# quality record stayed permanently empty. These tests are that regression.

echo
yellow "== Accounts and the quality record =="

JAR=$(mktemp)
EMAIL="suite-$$@example.test"

CODE=$(curl -s -o /tmp/api_signup.json -w "%{http_code}" -c "$JAR" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$EMAIL\",\"password\":\"un mot de passe assez long\"}" \
  "$BASE_URL/api/auth/signup")

# `AUTH_ATTEMPTS_PER_MINUTE` guards these routes, and this block spends several attempts.
# Two runs inside the same minute therefore hit the limit — which is the limit working, not
# a regression. Say so and move on rather than reporting a wall of red.
if [ "$CODE" = "429" ]; then
  skip "Accounts: the sign-in rate limit is still counting a previous run — wait a minute"
  ACCOUNTS_LIMITED=yes
else
  ACCOUNTS_LIMITED=no
fi

if [ "$ACCOUNTS_LIMITED" = "no" ]; then
check "POST /api/auth/signup creates an account" 201 "$CODE"
check_contains "signup answers with the account" /tmp/api_signup.json '"email"'
check_absent "signup never echoes the password back" /tmp/api_signup.json 'mot de passe'

CODE=$(curl -s -o /dev/null -w "%{http_code}" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$EMAIL\",\"password\":\"un mot de passe assez long\"}" \
  "$BASE_URL/api/auth/signup")
check "the same address cannot be claimed twice" 409 "$CODE"

CODE=$(curl -s -o /dev/null -w "%{http_code}" \
  -H 'Content-Type: application/json' \
  -d "{\"email\":\"$EMAIL\",\"password\":\"le mauvais\"}" \
  "$BASE_URL/api/auth/login")
check "a wrong password is refused" 401 "$CODE"

# A session cookie with no age dies when the browser closes, which would make a thirty-day
# server session worth exactly one browsing session — everybody signing in again, every
# time, with nothing on screen explaining why.
COOKIE_EXPIRY=$(grep mdpdf_session "$JAR" 2>/dev/null | awk '{print $5}')
if [ -n "$COOKIE_EXPIRY" ] && [ "$COOKIE_EXPIRY" -gt 0 ]; then
  green "  ✓ the session cookie outlives the browser window"
  PASS=$((PASS + 1))
else
  red "  ✗ the session cookie outlives the browser window — got expiry \"$COOKIE_EXPIRY\""
  FAIL=$((FAIL + 1))
fi


CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/auth/me")
check "GET /api/auth/me without a session → 401" 401 "$CODE"

CODE=$(curl -s -o /tmp/api_key.json -w "%{http_code}" -b "$JAR" \
  -H 'Content-Type: application/json' -d '{"name":"suite"}' "$BASE_URL/api/keys")
check "POST /api/keys mints a key for the member" 201 "$CODE"
check_contains "the secret travels once, and says so" /tmp/api_key.json 'cannot be shown again'

MEMBER_KEY=$(sed -n 's/.*"secret":"\([^"]*\)".*/\1/p' /tmp/api_key.json)
if [ -z "$MEMBER_KEY" ]; then
  red "  ✗ no key secret returned — skipping the attribution tests"
  FAIL=$((FAIL + 1))
else
  curl -s -o /tmp/api_owned.pdf -H "X-API-Key: $MEMBER_KEY" \
    -H 'Content-Type: application/json' \
    -d '{"markdown":"# Attribution\n\nUn paragraphe assez long pour porter une couche de texte."}' \
    "$BASE_URL/api/convert" > /dev/null

  curl -s -o /tmp/api_owned.json -H "X-API-Key: $MEMBER_KEY" \
    -F "file=@/tmp/api_owned.pdf" "$BASE_URL/api/files" > /dev/null
  # The regression in one line: the asset must carry the member's name, not "open"
  check_contains "a member's key attributes the work to that member" /tmp/api_owned.json '/suite"'

  ASSET=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' /tmp/api_owned.json)
  curl -s -o /dev/null -H "X-API-Key: $MEMBER_KEY" -H 'Content-Type: application/json' \
    -d "{\"pdf\":\"asset://$ASSET\",\"output\":\"asset\"}" "$BASE_URL/api/compress"

  curl -s -o /tmp/api_history.json -b "$JAR" "$BASE_URL/api/history" > /dev/null
  check_contains "the operation is written to the quality record" /tmp/api_history.json '"tool":"compress"'
  # The record exists to hold the verdict; without it the page is an empty shell
  check_contains "the record keeps the verdict, not just the file" /tmp/api_history.json '"verdict"'
  check_absent "the tool is named, not guessed from the file name" /tmp/api_history.json '"tool":"compressed.pdf"'

  cp /tmp/api_owned.pdf /tmp/api_web.pdf

  # The record has to name the visitor's own document. It named the tool's output instead —
  # every line reading "compressed.pdf" — which turns a history into ten identical rows on the
  # one page this product hangs its argument on.
  cp /tmp/api_owned.pdf /tmp/contrat-de-test.pdf
  curl -s -o /tmp/api_named.json -b "$JAR" -F "file=@/tmp/contrat-de-test.pdf" "$BASE_URL/api/files" > /dev/null
  NAMED_ASSET=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' /tmp/api_named.json)
  curl -s -o /dev/null -b "$JAR" -H 'Content-Type: application/json' \
    -d "{\"pdf\":\"asset://$NAMED_ASSET\",\"output\":\"asset\"}" "$BASE_URL/api/compress"
  curl -s -o /tmp/api_named_history.json -b "$JAR" "$BASE_URL/api/history" > /dev/null
  check_contains "the record names the caller's document, not the tool's output" \
    /tmp/api_named_history.json '"file":"contrat-de-test.pdf"'
  rm -f /tmp/contrat-de-test.pdf /tmp/api_named.json /tmp/api_named_history.json

  # "web" is what a browser session files under. A key of the same name would share its
  # attribution entry, and revoking the key would silently stop recording the member's work.
  CODE=$(curl -s -o /dev/null -w "%{http_code}" -b "$JAR" -H 'Content-Type: application/json' \
    -d '{"name":"web"}' "$BASE_URL/api/keys")
  check "a key cannot be named after the browser session" 400 "$CODE"

  CODE=$(curl -s -o /dev/null -w "%{http_code}" -b "$JAR" -H 'Content-Type: application/json' \
    -d '{"name":"suite"}' "$BASE_URL/api/keys")
  check "two keys of one account cannot share a name" 409 "$CODE"

  # Shown beside every key, and the one signal a member leans on to decide which is safe to
  # revoke — so "never used" about the key running production is worse than no column at all.
  curl -s -o /dev/null -H "X-API-Key: $MEMBER_KEY" -F "file=@/tmp/api_web.pdf" "$BASE_URL/api/files"
  curl -s -o /tmp/api_keys.json -b "$JAR" "$BASE_URL/api/keys" > /dev/null
  check_contains "a key that was just used no longer reads as never used" /tmp/api_keys.json '"last_used"'
  rm -f /tmp/api_keys.json

  CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE -b "$JAR" "$BASE_URL/api/history")
  check "DELETE /api/history clears it" 204 "$CODE"

  KEY_ID=$(sed -n 's/.*"key":{"id":"\([^"]*\)".*/\1/p' /tmp/api_key.json)
  CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE -b "$JAR" "$BASE_URL/api/keys/$KEY_ID")
  check "DELETE /api/keys/<id> revokes it" 204 "$CODE"

  CODE=$(curl -s -o /tmp/api_revoked.json -w "%{http_code}" -H "X-API-Key: $MEMBER_KEY" \
    -F "file=@/tmp/api_owned.pdf" "$BASE_URL/api/files")
  # A revoked key is no longer *anybody's*. Where a key is required that means 401;
  # on a deployment that requires none it means the request passes as anonymous. What
  # must never happen, in either case, is the work still being filed under the member.
  if [ "$CODE" = "401" ]; then
    check "a revoked key stops working" 401 "$CODE"
  else
    check "a revoked key is still accepted where none is required" 201 "$CODE"
    check_absent "but the work is no longer attributed to the member" /tmp/api_revoked.json '/suite"'
  fi
fi

# The case that matters most: a member who never mints a key. The tool pages call this
# API from the browser with a session cookie and no key at all, and that work has to reach
# their record — otherwise the workspace is an empty page for almost everyone who signs up.
curl -s -o /tmp/api_web.json -b "$JAR" -F "file=@/tmp/api_web.pdf" "$BASE_URL/api/files" > /dev/null
check_contains "a browser session attributes the work to the member" /tmp/api_web.json '/web"'

WEB_ASSET=$(sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p' /tmp/api_web.json)
curl -s -o /dev/null -b "$JAR" -H 'Content-Type: application/json' \
  -d "{\"pdf\":\"asset://$WEB_ASSET\",\"output\":\"asset\"}" "$BASE_URL/api/compress"
curl -s -o /tmp/api_web_history.json -b "$JAR" "$BASE_URL/api/history" > /dev/null
check_contains "and it reaches the quality record without any API key" /tmp/api_web_history.json '"tool":"compress"'

# The sign-up page offers longer retention in exchange for an address. That has to be a
# number in the code, not a sentence on a page: a member's file must outlive an anonymous
# one, and the page must quote the figure the service actually applies.
ANON_TTL=$(curl -s -F "file=@/tmp/api_web.pdf" "$BASE_URL/api/files" \
  | sed -n 's/.*"expires_unix":\([0-9]*\).*/\1/p')
MEMBER_TTL=$(curl -s -b "$JAR" -F "file=@/tmp/api_web.pdf" "$BASE_URL/api/files" \
  | sed -n 's/.*"expires_unix":\([0-9]*\).*/\1/p')
if [ -n "$ANON_TTL" ] && [ -n "$MEMBER_TTL" ] && [ "$MEMBER_TTL" -gt "$ANON_TTL" ]; then
  green "  ✓ a member's files really are kept longer than an anonymous visitor's"
  PASS=$((PASS + 1))
else
  red "  ✗ a member's files really are kept longer — anonymous $ANON_TTL, member $MEMBER_TTL"
  FAIL=$((FAIL + 1))
fi

curl -s -o /tmp/api_signup_page.html "$BASE_URL/inscription"
MEMBER_HOURS=$(curl -s -b "$JAR" "$BASE_URL/api/usage" \
  | sed -n 's/.*"retention_hours":\([0-9]*\).*/\1/p')
check_contains "and the sign-up page quotes the figure the service applies" \
  /tmp/api_signup_page.html "$MEMBER_HOURS heures"
rm -f /tmp/api_signup_page.html

# The privacy promise works the other way round: no session, no trace.
curl -s -o /tmp/api_anon.json -F "file=@/tmp/api_web.pdf" "$BASE_URL/api/files" > /dev/null
check_absent "an anonymous visitor is still attributed to nobody" /tmp/api_anon.json '/web"'

rm -f /tmp/api_web.json /tmp/api_web_history.json /tmp/api_anon.json /tmp/api_web.pdf

CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST -b "$JAR" "$BASE_URL/api/auth/logout")
check "POST /api/auth/logout closes the session" 204 "$CODE"

CODE=$(curl -s -o /dev/null -w "%{http_code}" -b "$JAR" "$BASE_URL/api/history")
check "the session is dead afterwards" 401 "$CODE"

rm -f "$JAR" /tmp/api_signup.json /tmp/api_key.json /tmp/api_owned.pdf /tmp/api_owned.json \
  /tmp/api_revoked.json /tmp/api_history.json

# Two protections that only exist because each attempt costs 600 000 PBKDF2 rounds on a
# bounded pool: an unbounded password would hash for minutes, and unlimited attempts would
# be both a brute-force oracle and a denial of service against everyone else's sign-in.
LONG_PASSWORD=$(head -c 4000 /dev/zero | tr '\0' 'a')
CODE=$(curl -s -o /dev/null -w "%{http_code}" -H 'Content-Type: application/json' \
  -d "{\"email\":\"long-$$@example.test\",\"password\":\"$LONG_PASSWORD\"}" \
  "$BASE_URL/api/auth/signup")
# 400 refuses it outright, 429 means the rate limit got there first — both refuse to hash
if [ "$CODE" = "400" ] || [ "$CODE" = "429" ]; then
  green "  ✓ an unbounded password is refused before it is hashed"
  PASS=$((PASS + 1))
else
  red "  ✗ an unbounded password is refused before it is hashed — got HTTP $CODE"
  FAIL=$((FAIL + 1))
fi

# Fired together rather than one after another. Each attempt costs 600 000 PBKDF2 rounds,
# which in a debug build takes seconds — serially, twelve of them span more than the sixty
# second window and it resets underneath the test. Concurrently is also what an attacker
# does.
ATTEMPTS=$(mktemp -d)
for i in $(seq 1 14); do
  ( curl -s -o /dev/null -w "%{http_code}" -H 'Content-Type: application/json' \
      -d '{"email":"nobody@example.test","password":"un mot de passe assez long"}' \
      "$BASE_URL/api/auth/login" > "$ATTEMPTS/$i" ) &
done
wait
LIMITED=no
grep -qs 429 "$ATTEMPTS"/* && LIMITED=yes
rm -rf "$ATTEMPTS"
if [ "$LIMITED" = "yes" ]; then
  green "  ✓ repeated sign-in attempts are rate-limited"
  PASS=$((PASS + 1))
else
  red "  ✗ repeated sign-in attempts are rate-limited — twelve went through untouched"
  FAIL=$((FAIL + 1))
fi

fi  # ACCOUNTS_LIMITED

# =========================================================================
# The sandbox: converters in a container with no network
# =========================================================================
# Skipped when the service runs everything in one container, which is what
# development does. When it is on, /api/health tells the truth about the worker —
# and that matters more than it looks: without it a dead worker leaves an API that
# answers 200 to every probe and 500 to every conversion, so the watchdog restarts
# nothing and the graph stays green through an outage.

echo
yellow "== Sandbox =="

curl -s -o /tmp/api_health.json "$BASE_URL/api/health"
SANDBOX=$(sed -n 's/.*"sandbox":"\([a-z]*\)".*/\1/p' /tmp/api_health.json)

if [ -z "$SANDBOX" ]; then
  skip "Sandbox: converters run in this container (SANDBOX_SPOOL unset)"
else
  check "the worker answers" "ok" "$SANDBOX"
  check_contains "and the service reports itself healthy" /tmp/api_health.json '"status":"ok"'

  # Every family of converters, exercised through the spool: pandoc and WeasyPrint,
  # then Ghostscript. If the round trip were broken these would not merely be slow,
  # they would be impossible.
  CODE=$(api -o /tmp/api_sandbox.pdf -w "%{http_code}" -X POST -H 'Content-Type: application/json' \
    -d '{"markdown":"# Sandbox\n\nUn paragraphe assez long pour porter une couche de texte."}' \
    "$BASE_URL/api/convert")
  check "pandoc and WeasyPrint run over the spool" 200 "$CODE"
  check_pdf "and produce a real PDF" /tmp/api_sandbox.pdf

  ASSET=$(api -X POST -F "file=@/tmp/api_sandbox.pdf" "$BASE_URL/api/files" \
    | sed -n 's/.*"id":"\(as_[0-9a-f]*\)".*/\1/p')
  CODE=$(api -o /tmp/api_sandbox.json -w "%{http_code}" -X POST -H 'Content-Type: application/json' \
    -d "{\"pdf\":\"asset://$ASSET\",\"output\":\"asset\"}" "$BASE_URL/api/compress")
  check "Ghostscript runs over the spool" 200 "$CODE"
  check_contains "and returns its verdict like any other tool" /tmp/api_sandbox.json '"verdict"'

  rm -f /tmp/api_sandbox.pdf /tmp/api_sandbox.json
fi
rm -f /tmp/api_health.json

# =========================================================================
# The pages a visitor without an account actually lands on
# =========================================================================
echo
yellow "== Account pages =="

for PAGE in /connexion /inscription /signin /signup /app /en/app; do
  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL$PAGE")
  check "GET $PAGE renders" 200 "$CODE"
done

# An English visitor who clicks "Sign in" must not land on a page written in French — at
# the exact moment we are asking them for a password.
curl -s -o /tmp/api_en.html "$BASE_URL/en"
check_contains "the English site links to the English sign-in" /tmp/api_en.html 'href="/signin"'
check_absent "and never to the French one" /tmp/api_en.html 'href="/connexion"'
curl -s -o /tmp/api_fr.html "$BASE_URL/"
check_contains "the French site links to the French sign-in" /tmp/api_fr.html 'href="/connexion"'
rm -f /tmp/api_en.html /tmp/api_fr.html

echo
echo "========================================="
if [ "$SKIP" -gt 0 ]; then
  echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
else
  echo "Results: $PASS passed, $FAIL failed"
fi
echo "========================================="

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
