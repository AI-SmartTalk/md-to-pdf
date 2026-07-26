#!/bin/sh
# Banc d'essai du Layout Doctor, de bout en bout : rendu reel par pandoc + weasyprint, puis
# analyse par le service lui-meme. A lancer depuis la racine du depot, dans le conteneur qui
# porte pandoc, weasyprint et poppler :
#
#   docker compose exec -T rust cargo build
#   docker compose exec -T pandoc sh tests/layout-fixtures/run.sh
#
# Le cache est desactive : le banc doit mesurer le rendu, pas une entree deja calculee.
set -e

API_KEY="${API_KEY:-layout-bench}"
PORT="${PORT:-8317}"
export API_KEY
export ROCKET_PORT="$PORT"
export ROCKET_ADDRESS=127.0.0.1
export PDF_CACHE_ENABLED=false
export LAYOUT_BENCH_URL="http://127.0.0.1:$PORT"

# Un service deja en ecoute sur ce port repondrait a la sonde de demarrage et le banc
# mesurerait un autre binaire que celui qu'on vient de compiler.
if curl -sf "$LAYOUT_BENCH_URL/api/health" >/dev/null 2>&1; then
  echo "Le port $PORT est deja occupe."
  exit 1
fi

./target/debug/md-to-pdf >/tmp/layout-bench.log 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT

attempt=0
until curl -sf "$LAYOUT_BENCH_URL/api/health" >/dev/null; do
  # Un port deja pris repondrait a la sonde depuis un autre service : c'est notre propre
  # processus qui doit etre vivant, pas n'importe lequel.
  if ! kill -0 "$server" 2>/dev/null; then
    echo "Le service s'est arrete :"
    cat /tmp/layout-bench.log
    exit 1
  fi
  attempt=$((attempt + 1))
  if [ "$attempt" -gt 40 ]; then
    echo "Le service n'a pas demarre :"
    cat /tmp/layout-bench.log
    exit 1
  fi
  sleep 0.5
done

python3 tests/layout-fixtures/check.py
