#!/bin/bash
#
# Provisions a fresh Debian/Ubuntu host and starts md-to-pdf in production mode.
# Safe to re-run: every step is idempotent.

set -euo pipefail

# COLORS
GREEN="\033[0;32m"
RED="\033[0;31m"
NC="\033[0m"

info() { echo -e "${GREEN}$1${NC}"; }
fail() { echo -e "${RED}$1${NC}" >&2; exit 1; }

NETWORK_NAME="ai-toolkit-network"

# Docker may not be usable without sudo until the user's session picks up the new group
docker_cmd() {
    if docker info > /dev/null 2>&1; then
        docker "$@"
    else
        sudo docker "$@"
    fi
}

info "Starting installation script..."

# Update system
info "Updating system packages..."
sudo apt-get update && sudo apt-get upgrade -y

# Install dependencies
info "Installing dependencies: git, make, curl, ca-certificates, gnupg, lsb-release..."
sudo apt-get install -y git make curl ca-certificates gnupg lsb-release

# Install NGINX
info "Installing NGINX..."
sudo apt-get install -y nginx
sudo systemctl enable nginx
sudo systemctl start nginx

# Install Docker
if command -v docker &> /dev/null; then
    info "Docker is already installed."
else
    info "Installing Docker Engine..."
    # Add Docker's official GPG key
    sudo install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "https://download.docker.com/linux/$(. /etc/os-release && echo "$ID")/gpg" \
        | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    sudo chmod a+r /etc/apt/keyrings/docker.gpg

    # Set up the repository
    echo \
      "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/$(. /etc/os-release && echo "$ID") \
      $(lsb_release -cs) stable" \
      | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

    # Install Docker packages
    sudo apt-get update
    sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

    # Enable Docker service
    sudo systemctl enable docker
    sudo systemctl start docker

    # Add current user to docker group (takes effect on the next login)
    sudo usermod -aG docker "$USER"
    info "Docker installed. Log out and back in to use docker without sudo."
fi

# Check Docker Compose v2
docker_cmd compose version &> /dev/null \
    || fail "Docker Compose v2 not found. Something went wrong."
info "Docker Compose v2 is ready."

# Shared network used by docker-compose.prod.yml
if docker_cmd network inspect "$NETWORK_NAME" &> /dev/null; then
    info "Network $NETWORK_NAME already exists."
else
    info "Creating network $NETWORK_NAME..."
    docker_cmd network create "$NETWORK_NAME"
fi

# Build and start the production stack
info "Building the production image (this takes a few minutes)..."
docker_cmd compose -f docker-compose.prod.yml build --pull

info "Starting the service..."
docker_cmd compose -f docker-compose.prod.yml up -d

# Wait for the health endpoint
info "Waiting for the service to become healthy..."
for _ in $(seq 1 30); do
    if curl -fsS http://127.0.0.1:8000/api/health > /dev/null 2>&1; then
        info "Service is up: $(curl -fsS http://127.0.0.1:8000/api/health)"
        info "Installation complete."
        exit 0
    fi
    sleep 2
done

fail "Service did not become healthy in time. Check: docker compose -f docker-compose.prod.yml logs"
