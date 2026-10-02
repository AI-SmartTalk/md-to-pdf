#!/usr/bin/env bash
# Deployment contract: auth, rendering, persistent files and preview.
set -euo pipefail
base=${1:-http://127.0.0.1:18000}
: "${API_KEY:?API_KEY is required}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
api() { curl -fsS --max-time 150 -H "X-API-Key: $API_KEY" "$@"; }
curl -fsS "$base/api/health" >/dev/null
code=$(curl -sS -o "$work/unauthorized.json" -w '%{http_code}' -H 'Content-Type: application/json' -d '{"markdown":"# fixture"}' "$base/api/convert")
[[ "$code" = 401 ]]
api -H 'Content-Type: application/json' -d '{"markdown":"# Core deployment fixture"}' "$base/api/convert" -o "$work/direct.pdf"
[[ $(head -c 4 "$work/direct.pdf") = '%PDF' ]]
api -H 'Content-Type: application/json' -d '{"markdown":"# Saved deployment fixture","client_id":"core-ci","pdf_name":"fixture"}' "$base/api/convert" -o "$work/saved.json"
link=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["download_url"])' "$work/saved.json")
[[ "$link" = /download/* ]]
api "$base$link" -o "$work/download.pdf"
[[ $(head -c 4 "$work/download.pdf") = '%PDF' ]]
api -H 'Content-Type: application/json' -d '{"markdown":"# Preview deployment fixture"}' "$base/api/preview" -o "$work/preview.png"
[[ $(od -An -tx1 -N8 "$work/preview.png" | tr -d ' \n') = 89504e470d0a1a0a ]]
echo 'Core PDF API contract passed (health, auth, render, save/download, preview).'
