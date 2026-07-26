#!/bin/bash
#
# Redémarre md-to-pdf quand il ne répond plus alors que son conteneur tourne.
#
# `restart: unless-stopped` couvre le crash du processus et le reboot de l'hôte,
# mais Docker Compose ne fait rien d'un healthcheck en échec : un service figé
# (deadlock, saturation mémoire, pandoc bloqué) reste « up (unhealthy) » et
# continue de recevoir du trafic indéfiniment. Ce script comble ce trou.
#
# Installé par install.sh comme service systemd déclenché toutes les minutes.
# Il n'a pas besoin d'accéder au socket Docker depuis un conteneur : il tourne
# sur l'hôte, ce qui évite d'exposer un accès root déguisé.

set -uo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-/opt/md-to-pdf/docker-compose.prod.yml}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:8000/api/health}"
CONTAINER="${CONTAINER:-md-to-pdf}"
# Nombre d'échecs consécutifs avant redémarrage : une sonde isolée qui échoue
# pendant un pic de charge ne doit pas provoquer une coupure.
FAILURES_BEFORE_RESTART="${FAILURES_BEFORE_RESTART:-3}"
DELAY_BETWEEN_PROBES="${DELAY_BETWEEN_PROBES:-5}"

log() { echo "[md-to-pdf-watchdog] $*"; }

# Un conteneur volontairement arrêté (maintenance, déploiement) ne doit pas être
# relancé dans le dos de l'opérateur.
status="$(docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null)" || {
    log "conteneur $CONTAINER introuvable — rien à faire"
    exit 0
}

if [ "$status" != "running" ]; then
    log "conteneur $status — arrêt volontaire supposé, aucune action"
    exit 0
fi

failures=0
for _ in $(seq 1 "$FAILURES_BEFORE_RESTART"); do
    if curl -fsS -m 5 "$HEALTH_URL" > /dev/null 2>&1; then
        exit 0
    fi
    failures=$((failures + 1))
    [ "$failures" -lt "$FAILURES_BEFORE_RESTART" ] && sleep "$DELAY_BETWEEN_PROBES"
done

log "$failures sondes en échec sur $HEALTH_URL alors que le conteneur tourne — redémarrage"
docker compose -f "$COMPOSE_FILE" restart md-to-pdf

# Laisser le service revenir avant de rendre la main, pour que l'état du timer
# reflète le résultat réel du redémarrage.
for _ in $(seq 1 30); do
    if curl -fsS -m 5 "$HEALTH_URL" > /dev/null 2>&1; then
        log "service rétabli"
        exit 0
    fi
    sleep 2
done

log "le service ne répond toujours pas après redémarrage — intervention nécessaire"
exit 1
