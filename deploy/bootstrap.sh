#!/bin/bash
#
# Déploie md-to-pdf sur l'hôte courant. Exécuté par .github/workflows/deploy.yml
# après avoir synchronisé le dépôt, mais utilisable à la main :
#
#   cd /opt/md-to-pdf && ./deploy/bootstrap.sh
#
# Idempotent et sans hypothèse sur l'état de la machine : il fonctionne aussi
# bien sur un VPS où le projet tourne déjà que sur une machine où Docker vient
# d'être installé. Chaque étape vérifie l'existant avant d'agir.
#
# Il ne touche jamais au .env : celui-ci est écrit par le workflow depuis le
# secret PDF_ENV, ou généré par install.sh lors de la toute première mise en
# place. Écraser une API_KEY en service couperait tous les clients d'un coup.

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$PROJECT_DIR/docker-compose.prod.yml"
ENV_FILE="$PROJECT_DIR/.env"
NETWORK_NAME="ai-toolkit-network"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:8000/api/health}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-120}"

GREEN="\033[0;32m"; YELLOW="\033[0;33m"; RED="\033[0;31m"; NC="\033[0m"
info() { echo -e "${GREEN}[deploy]${NC} $1"; }
warn() { echo -e "${YELLOW}[deploy]${NC} $1"; }
fail() { echo -e "${RED}[deploy]${NC} $1" >&2; exit 1; }

cd "$PROJECT_DIR"

# ───────────────────────────── prérequis ──────────────────────────────
command -v docker > /dev/null 2>&1 \
    || fail "Docker absent. Lancez ./install.sh une première fois sur cette machine."

docker compose version > /dev/null 2>&1 \
    || fail "Docker Compose v2 absent. Lancez ./install.sh une première fois."

docker info > /dev/null 2>&1 \
    || fail "Docker injoignable pour l'utilisateur $(whoami). Ajoutez-le au groupe docker : sudo usermod -aG docker $(whoami)"

# Le script tourne sans terminal depuis la CI : un sudo qui attend un mot de
# passe bloquerait le job jusqu'au timeout. On détecte ce qui est possible et on
# dégrade avec un avertissement plutôt que de pendre.
if [ "$(id -u)" -eq 0 ]; then
    SUDO=""
    PRIVILEGED=1
elif sudo -n true 2>/dev/null; then
    SUDO="sudo"
    PRIVILEGED=1
else
    SUDO=""
    PRIVILEGED=0
    warn "sudo indisponible sans mot de passe : les étapes privilégiées seront ignorées."
fi

# Rien ne remonte après un redémarrage de la machine si le démon n'est pas activé
if [ "$PRIVILEGED" -eq 1 ] && command -v systemctl > /dev/null 2>&1 \
   && ! systemctl is-enabled docker > /dev/null 2>&1; then
    warn "Le démon docker n'était pas activé au démarrage — activation."
    $SUDO systemctl enable docker || warn "Activation impossible, à faire à la main."
fi

if ! docker network inspect "$NETWORK_NAME" > /dev/null 2>&1; then
    info "Création du réseau $NETWORK_NAME."
    docker network create "$NETWORK_NAME"
fi

# ──────────────────────────── configuration ───────────────────────────
[ -f "$ENV_FILE" ] || fail "$ENV_FILE absent. Le workflow doit l'écrire depuis PDF_ENV, ou lancez ./install.sh."

# Le compose refuse déjà de démarrer sans clé, mais autant échouer avant d'avoir
# arrêté quoi que ce soit : un déploiement qui casse la prod sur une variable
# vide est exactement ce qu'on veut éviter.
grep -qE '^API_KEY=.+' "$ENV_FILE" \
    || fail "API_KEY absente ou vide dans $ENV_FILE — déploiement interrompu, le service en place n'a pas été touché."

chmod 600 "$ENV_FILE"

# ───────────────────────────── construction ───────────────────────────
# Construire AVANT de toucher au conteneur en service : si le build échoue,
# l'instance actuelle continue de répondre.
info "Construction de l'image…"
docker compose -f "$COMPOSE_FILE" build

# ───────────────────────── bascule et vérification ────────────────────
PREVIOUS_IMAGE="$(docker inspect md-to-pdf --format '{{.Image}}' 2>/dev/null || echo "")"

info "Bascule du service…"
docker compose -f "$COMPOSE_FILE" up -d

info "Attente de la sonde de santé (${HEALTH_TIMEOUT}s max)…"
deadline=$((SECONDS + HEALTH_TIMEOUT))
healthy=0
while [ $SECONDS -lt $deadline ]; do
    if curl -fsS -m 5 "$HEALTH_URL" > /dev/null 2>&1; then
        healthy=1
        break
    fi
    sleep 3
done

if [ "$healthy" -ne 1 ]; then
    warn "Le service ne répond pas après déploiement. Logs :"
    docker compose -f "$COMPOSE_FILE" logs --tail 60 md-to-pdf || true

    if [ -n "$PREVIOUS_IMAGE" ]; then
        warn "Retour à l'image précédente ($PREVIOUS_IMAGE)…"
        docker tag "$PREVIOUS_IMAGE" md-to-pdf:local
        docker compose -f "$COMPOSE_FILE" up -d --no-build

        for _ in $(seq 1 20); do
            if curl -fsS -m 5 "$HEALTH_URL" > /dev/null 2>&1; then
                fail "Déploiement échoué — l'image précédente a été remise en service."
            fi
            sleep 3
        done
        fail "Déploiement échoué ET le retour arrière n'a pas rétabli le service. Intervention nécessaire."
    fi

    fail "Déploiement échoué, aucune image précédente pour revenir en arrière."
fi

info "Service opérationnel : $(curl -fsS -m 5 "$HEALTH_URL")"

# ────────────────────── watchdog, purge, nettoyage ────────────────────
# Réinstallés à chaque déploiement : les units peuvent avoir changé dans le
# dépôt, et un hôte provisionné avant leur existence doit les recevoir.
if [ "$PRIVILEGED" -eq 1 ] && command -v systemctl > /dev/null 2>&1; then
    chmod +x "$PROJECT_DIR/deploy/md-to-pdf-watchdog.sh" "$PROJECT_DIR/deploy/pdf-purge.sh"

    for unit in md-to-pdf-watchdog md-to-pdf-purge; do
        $SUDO cp "$PROJECT_DIR/deploy/$unit.service" /etc/systemd/system/
        $SUDO cp "$PROJECT_DIR/deploy/$unit.timer" /etc/systemd/system/
        $SUDO sed -i "s|/opt/md-to-pdf|$PROJECT_DIR|g" "/etc/systemd/system/$unit.service"
    done

    $SUDO systemctl daemon-reload
    $SUDO systemctl enable --now md-to-pdf-watchdog.timer md-to-pdf-purge.timer
    info "Watchdog et purge à jour."
elif command -v systemctl > /dev/null 2>&1; then
    warn "Watchdog et purge non installés faute de droits — un service figé ne sera pas redémarré."
    warn "À lancer une fois sur le VPS : sudo ./install.sh"
else
    warn "systemd absent : ni watchdog ni purge."
fi

# Les images intermédiaires s'accumulent à chaque build et finissent par saturer
# le disque — ce qui arrête le service aussi sûrement qu'un crash.
info "Nettoyage des images orphelines…"
docker image prune -f > /dev/null 2>&1 || true

info "Déploiement terminé."
