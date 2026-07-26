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

# Authentication (only meaningful when the server runs with API_KEY)
if [ -n "$API_KEY" ]; then
  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"markdown": "# x"}' \
    "$BASE_URL/api/convert")
  check "convert without API key → 401" 401 "$CODE"

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $API_KEY" \
    -d '{"markdown": "# x"}' \
    "$BASE_URL/api/convert")
  check "convert with bearer token → 200" 200 "$CODE"

  # The new routes are behind the key too — /api/health is the only exception
  for path in themes metrics; do
    CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/$path")
    check "GET /api/$path without API key → 401" 401 "$CODE"
  done

  CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Content-Type: application/json" \
    -d '{"before": "/download/a/b.pdf", "after": "/download/a/c.pdf"}' \
    "$BASE_URL/api/diff")
  check "POST /api/diff without API key → 401" 401 "$CODE"

  CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/health")
  check "GET /api/health stays open (container probe)" 200 "$CODE"

  # The legacy endpoint was never covered by the key, by design
  CODE=$(curl -s -o /dev/null -w "%{http_code}" -F "markdown=# legacy" "$BASE_URL/")
  check "POST / stays open without a key (legacy contract)" 200 "$CODE"
fi

echo
echo "========================================="
if [ "$SKIP" -gt 0 ]; then
  echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
else
  echo "Results: $PASS passed, $FAIL failed"
fi
echo "========================================="

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
