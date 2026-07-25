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

PASS=0
FAIL=0

green() { echo -e "\033[0;32m$1\033[0m"; }
red()   { echo -e "\033[0;31m$1\033[0m"; }

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
fi

echo
echo "========================================="
echo "Results: $PASS passed, $FAIL failed"
echo "========================================="

[ "$FAIL" -eq 0 ] && exit 0 || exit 1
