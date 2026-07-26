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
#
# Le cache de rendu (public/cache, volume pdf-cache) n'est PAS purgé ici : le
# service l'évince lui-même par TTL (PDF_CACHE_TTL_SECS) et par plafond de
# taille (PDF_CACHE_MAX_MB), et supprimer un fichier sous ses pieds pendant un
# rendu n'apporterait rien. Ce script se contente de mesurer son occupation et
# d'alerter si elle dépasse le plafond annoncé — c'est le signal que l'éviction
# ne fait pas son travail, et le seul moyen de ne pas découvrir le problème par
# un disque plein.

set -euo pipefail

RETENTION_DAYS="${PDF_RETENTION_DAYS:-180}"
CONTAINER="${CONTAINER:-md-to-pdf}"
PDF_ROOT="${PDF_ROOT:-/home/rocket/public/pdf}"
CACHE_ROOT="${CACHE_ROOT:-/home/rocket/public/cache}"
CACHE_MAX_MB="${PDF_CACHE_MAX_MB:-512}"

log() { echo "[pdf-purge] $*"; }

# Surveillance du cache de rendu — mesure seulement, aucune suppression.
report_cache() {
    local mb
    mb=$(docker exec "$CONTAINER" sh -c \
        "du -sm $CACHE_ROOT 2>/dev/null | cut -f1" 2>/dev/null || echo "")
    [ -z "$mb" ] && return 0
    if [ "$mb" -gt "$CACHE_MAX_MB" ]; then
        log "ATTENTION cache de rendu : ${mb} Mo pour un plafond de ${CACHE_MAX_MB} Mo —" \
            "l'éviction du service ne suit pas, vérifier les logs du conteneur"
    else
        log "cache de rendu : ${mb} Mo / ${CACHE_MAX_MB} Mo (évincé par le service, pas ici)"
    fi
}

if ! docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null | grep -q running; then
    log "conteneur $CONTAINER non démarré — purge annulée"
    exit 0
fi

before=$(docker exec "$CONTAINER" sh -c "du -sh $PDF_ROOT 2>/dev/null | cut -f1" || echo "?")
count=$(docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -type f -name '*.pdf' -mtime +$RETENTION_DAYS | wc -l" || echo 0)

if [ "${count:-0}" -eq 0 ]; then
    log "aucun PDF de plus de $RETENTION_DAYS jours (occupation : $before)"
    report_cache
    exit 0
fi

docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -type f -name '*.pdf' -mtime +$RETENTION_DAYS -delete"
# Les dossiers client_id vidés par la purge n'ont plus de raison d'être
docker exec "$CONTAINER" sh -c \
    "find $PDF_ROOT -mindepth 1 -type d -empty -delete" || true

after=$(docker exec "$CONTAINER" sh -c "du -sh $PDF_ROOT 2>/dev/null | cut -f1" || echo "?")
log "$count PDF supprimés (plus de $RETENTION_DAYS jours) — occupation $before → $after"
report_cache
