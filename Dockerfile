FROM rustlang/rust:nightly-bookworm-slim AS builder

WORKDIR /usr/src/md-to-pdf

# Les dépendances sont compilées dans leur propre couche, avant que le code du
# projet n'entre dans l'image. Sans cette séparation, `COPY . .` invalidait tout
# dès qu'un fichier changeait — modifier une ligne de CSS recompilait Rocket et
# ses 300 crates, soit ~8 minutes à chaque déploiement.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
 && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY src ./src
# cargo ne recompile que ce dont l'empreinte a changé : toucher le binaire
# du projet force sa recompilation sans repartir des dépendances.
RUN touch src/main.rs \
 && cargo install --path . --locked

FROM debian:12-slim

RUN apt-get update \
 && apt-get install --yes \
      pandoc \
      wkhtmltopdf \
      texlive \
      build-essential python3-dev python3-pip python3-setuptools python3-wheel python3-cffi libcairo2 libpango-1.0-0 libpangocairo-1.0-0 libgdk-pixbuf2.0-0 libffi-dev shared-mime-info \
      poppler-utils \
      qpdf \
      curl \
 && rm -rf /var/lib/apt/lists/* \
 # https://stackoverflow.com/questions/75608323/how-do-i-solve-error-externally-managed-environment-every-time-i-use-pip-3
 && pip3 install --no-cache-dir --break-system-packages weasyprint \
 && pandoc --version

# WeasyPrint résout toute URL référencée par le document (img, url(), @import, SVG) :
# sans garde, le service relaie un SSRF vers le réseau interne et lit les fichiers
# locaux, et POST / est atteignable sans clé d'API. Le CLI n'accepte pas d'url_fetcher,
# la politique vit donc dans une enveloppe Python installée à la place du binaire.
COPY deploy/weasyprint-safe.py /usr/local/lib/weasyprint-safe.py

# Le déplacement est conditionné au CONTENU du fichier, pas à son existence : une
# reconstruction, ou une réinstallation pip de weasyprint, ne doit jamais renommer
# l'enveloppe en weasyprint-real — elle s'appellerait elle-même sans fin. Le nom
# `weasyprint` est imposé : pandoc choisit la forme de ses arguments d'après le nom
# de base passé à --pdf-engine et refuse tout autre nom.
RUN set -eu \
 && if [ -e /usr/local/bin/weasyprint ] \
    && ! grep -q '^# urlguard-wrapper' /usr/local/bin/weasyprint; then \
      mv /usr/local/bin/weasyprint /usr/local/bin/weasyprint-real; \
    fi \
 && install -m 0755 /usr/local/lib/weasyprint-safe.py /usr/local/bin/weasyprint \
 && weasyprint --version \
 # La conversion pandoc est le point de rupture le plus probable de l'enveloppe :
 # la vérifier ici fait échouer la construction plutôt que la production. La feuille
 # de style porte le nom que le service donne à ses fichiers temporaires, et le
 # format A5 sert d'assertion : une enveloppe qui perdrait le CSS rendrait un A4.
 && printf '# titre\n\ntexte\n' > /tmp/smoke.md \
 && printf '@page { size: A5 }\n' > /tmp/.tmpSm0ke1.css \
 && pandoc --standalone --to=html5 --css=/tmp/.tmpSm0ke1.css --pdf-engine=weasyprint \
      --output=/tmp/smoke.pdf /tmp/smoke.md \
 && pdfinfo /tmp/smoke.pdf | grep -q '419.528 x 595.276' \
 && rm -f /tmp/smoke.md /tmp/.tmpSm0ke1.css /tmp/smoke.pdf

COPY --from=builder /usr/local/cargo/bin/md-to-pdf /usr/local/bin/md-to-pdf

RUN useradd --create-home rocket

WORKDIR /home/rocket

COPY --chown=rocket:rocket static /home/rocket/static
COPY --chown=rocket:rocket Rocket.toml /home/rocket/Rocket.toml
COPY --chown=rocket:rocket templates /home/rocket/templates
# Sans ce répertoire le service démarre en annonçant « No theme loaded » et toute
# requête portant "theme" répond 404.
COPY --chown=rocket:rocket themes /home/rocket/themes

# Les PDF générés sont écrits ici ; monter un volume dessus garde les URL de
# téléchargement valides d'un déploiement à l'autre. public/cache contient les rendus
# adressés par contenu : sans volume, le cache repart à zéro à chaque déploiement.
RUN mkdir -p /home/rocket/public/pdf /home/rocket/public/cache \
 && chown -R rocket:rocket /home/rocket/public

USER rocket

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8000/api/health || exit 1

CMD ["md-to-pdf"]
