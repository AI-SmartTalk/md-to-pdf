#!/bin/bash
#
# Provisionne un hôte Debian/Ubuntu et démarre md-to-pdf en production.
# Rejouable sans risque : chaque étape est idempotente.
#
# Ce que le script met en place :
#   - Docker Engine + Compose v2, activés au démarrage de la machine
#   - le réseau partagé ai-toolkit-network
#   - un .env avec une API_KEY générée si absente
#   - la pile de production (restart: unless-stopped)
#   - un watchdog systemd qui redémarre le service s'il se fige
#   - une purge nocturne des PDFs au-delà de la rétention
#
# Le reverse proxy TLS n'est volontairement pas installé : des vhosts prêts à
# l'emploi sont fournis dans deploy/ (nginx-md-to-pdf.conf, apache-md-to-pdf.conf)
# et doivent être adaptés au serveur web déjà en place sur l'hôte.

set -euo pipefail

GREEN="\033[0;32m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
NC="\033[0m"

info() { echo -e "${GREEN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
fail() { echo -e "${RED}$1${NC}" >&2; exit 1; }

NETWORK_NAME="ai-toolkit-network"
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_SYSTEMD="${INSTALL_SYSTEMD:-1}"

# Docker n'est pas utilisable sans sudo tant que la session n'a pas repris le
# nouveau groupe : on bascule automatiquement.
docker_cmd() {
    if docker info > /dev/null 2>&1; then
        docker "$@"
    else
        sudo docker "$@"
    fi
}

info "Installation de md-to-pdf depuis $PROJECT_DIR"

info "Mise à jour de l'index des paquets..."
sudo apt-get update

info "Installation des dépendances : git, make, curl, ca-certificates, gnupg, lsb-release, openssl..."
sudo apt-get install -y git make curl ca-certificates gnupg lsb-release openssl

# ─────────────────────────────── Docker ────────────────────────────────
if command -v docker &> /dev/null; then
    info "Docker est déjà installé."
else
    info "Installation de Docker Engine..."
    sudo install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "https://download.docker.com/linux/$(. /etc/os-release && echo "$ID")/gpg" \
        | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    sudo chmod a+r /etc/apt/keyrings/docker.gpg

    echo \
      "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/$(. /etc/os-release && echo "$ID") \
      $(lsb_release -cs) stable" \
      | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

    sudo apt-get update
    sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

    sudo systemctl enable docker
    sudo systemctl start docker

    sudo usermod -aG docker "$USER"
    info "Docker installé. Reconnectez-vous pour l'utiliser sans sudo."
fi

docker_cmd compose version &> /dev/null \
    || fail "Docker Compose v2 introuvable. L'installation a échoué."
info "Docker Compose v2 est prêt."

# Le démon doit être activé au boot, sinon rien ne remonte après un redémarrage
# de la machine, quelle que soit la politique de restart des conteneurs.
if ! systemctl is-enabled docker &> /dev/null; then
    warn "Le service docker n'était pas activé au démarrage — activation."
    sudo systemctl enable docker
fi

# ─────────────────────────────── réseau ────────────────────────────────
if docker_cmd network inspect "$NETWORK_NAME" &> /dev/null; then
    info "Le réseau $NETWORK_NAME existe déjà."
else
    info "Création du réseau $NETWORK_NAME..."
    docker_cmd network create "$NETWORK_NAME"
fi

# ──────────────────────────── configuration ────────────────────────────
ENV_FILE="$PROJECT_DIR/.env"

if [ ! -f "$ENV_FILE" ]; then
    info "Génération de $ENV_FILE avec une API_KEY aléatoire..."
    cp "$PROJECT_DIR/.env.example" "$ENV_FILE"
    GENERATED_KEY="$(openssl rand -hex 32)"
    sed -i "s|^API_KEY=.*|API_KEY=$GENERATED_KEY|" "$ENV_FILE"
    chmod 600 "$ENV_FILE"
    warn "API_KEY générée : $GENERATED_KEY"
    warn "Notez-la : les clients de l'API en ont besoin, elle ne sera plus affichée."
else
    # Une clé vide ferait démarrer un service ouvert. Le compose refuse déjà de
    # démarrer dans ce cas, autant le dire ici plutôt qu'après le build.
    if ! grep -qE '^API_KEY=.+' "$ENV_FILE"; then
        fail "$ENV_FILE existe mais API_KEY est vide. Renseignez-la (openssl rand -hex 32) puis relancez."
    fi
    info "$ENV_FILE présent, API_KEY renseignée."
fi

# ──────────────────────────── build et démarrage ───────────────────────
info "Construction de l'image de production (quelques minutes)..."
docker_cmd compose -f "$PROJECT_DIR/docker-compose.prod.yml" build --pull

info "Démarrage du service..."
docker_cmd compose -f "$PROJECT_DIR/docker-compose.prod.yml" up -d

info "Attente de la sonde de santé..."
for _ in $(seq 1 45); do
    if curl -fsS http://127.0.0.1:8000/api/health > /dev/null 2>&1; then
        info "Service opérationnel : $(curl -fsS http://127.0.0.1:8000/api/health)"
        break
    fi
    sleep 2
done

curl -fsS http://127.0.0.1:8000/api/health > /dev/null 2>&1 \
    || fail "Le service n'est pas devenu sain. Voir : docker compose -f docker-compose.prod.yml logs"

# ───────────────────────── watchdog et purge ───────────────────────────
# `restart: unless-stopped` couvre le crash du processus et le reboot, mais pas
# un service figé qui reste « up (unhealthy) » : ce trou est comblé ici.
if [ "$INSTALL_SYSTEMD" = "1" ] && command -v systemctl &> /dev/null; then
    info "Installation du watchdog et de la purge (systemd)..."
    chmod +x "$PROJECT_DIR/deploy/md-to-pdf-watchdog.sh" "$PROJECT_DIR/deploy/pdf-purge.sh"

    for unit in md-to-pdf-watchdog md-to-pdf-purge; do
        sudo cp "$PROJECT_DIR/deploy/$unit.service" /etc/systemd/system/
        sudo cp "$PROJECT_DIR/deploy/$unit.timer" /etc/systemd/system/
        # Les units référencent /opt/md-to-pdf : les aligner sur l'emplacement réel
        sudo sed -i "s|/opt/md-to-pdf|$PROJECT_DIR|g" "/etc/systemd/system/$unit.service"
    done

    sudo systemctl daemon-reload
    sudo systemctl enable --now md-to-pdf-watchdog.timer md-to-pdf-purge.timer
    info "Watchdog actif (sonde chaque minute), purge nocturne programmée."
else
    warn "systemd absent ou INSTALL_SYSTEMD=0 : ni watchdog ni purge installés."
    warn "Sans watchdog, un service figé ne sera pas redémarré automatiquement."
fi

echo
info "Installation terminée."
echo "  Service          : http://127.0.0.1:8000 (boucle locale uniquement)"
echo "  Reverse proxy    : adapter deploy/nginx-md-to-pdf.conf ou deploy/apache-md-to-pdf.conf"
echo "  Logs             : docker compose -f docker-compose.prod.yml logs -f"
echo "  État du watchdog : systemctl status md-to-pdf-watchdog.timer"
