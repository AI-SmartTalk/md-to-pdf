#!/bin/bash
#
# Purge les PDFs sauvegardés plus vieux que PDF_RETENTION_DAYS.
#
# Le volume pdf-storage n'a aucune rotation : chaque appel avec client_id +
# pdf_name écrit un fichier qui reste indéfiniment. Sur un service qui génère
# des documents en continu, le disque finit par se remplir et le conteneur
# s'arrête sur une écriture impossible.
#
# À manier avec la conscience de ce que ça implique : les download_url déjà
# distribués aux utilisateurs cessent de répondre passé le délai. Choisir une
# rétention cohérente avec la durée de vie attendue des liens.

set -euo pipefail

RETENTION_DAYS="${PDF_RETENTION_DAYS:-180}"
CONTAINER="${CONTAINER:-md-to-pdf}"
PDF_ROOT="${PDF_ROOT:-/home/rocket/public/pdf}"

log() { echo "[pdf-purge] $*"; }

if ! docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null | grep -q running; then
    log "conteneur $CONTAINER non démarré — purge annulée"
    exit 0
fi

before=$(docker exec "$CONTAINER" sh -c "du -sh $PDF_ROOT 2>/dev/null | cut -f1" || echo "?")
count=$(docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -type f -name '*.pdf' -mtime +$RETENTION_DAYS | wc -l" || echo 0)

if [ "${count:-0}" -eq 0 ]; then
    log "aucun PDF de plus de $RETENTION_DAYS jours (occupation : $before)"
    exit 0
fi

docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -type f -name '*.pdf' -mtime +$RETENTION_DAYS -delete"
# Les dossiers client_id vidés par la purge n'ont plus de raison d'être
docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -mindepth 1 -type d -empty -delete" || true

after=$(docker exec "$CONTAINER" sh -c "du -sh $PDF_ROOT 2>/dev/null | cut -f1" || echo "?")
log "$count PDF supprimés (plus de $RETENTION_DAYS jours) — occupation $before → $after"
