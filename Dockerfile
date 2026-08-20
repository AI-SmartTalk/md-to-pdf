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
      # Ingestion of documents the service did not produce. Each one is a parser that will
      # be handed hostile bytes, which is why every call site pins what it can: Ghostscript
      # always runs with -dSAFER, LibreOffice with a throwaway profile seeded to refuse
      # remote references and macros (src/routes/office.rs), ocrmypdf with a page ceiling.
      #
      # None of that isolates the processes. What does is where they run: in production
      # these binaries execute in the md-to-pdf-worker container, which has no network
      # interface at all (docker-compose.prod.yml, `network_mode: none`). See
      # src/sandbox.rs for how the work gets there, and PLAN-METAMORPHOSE.md §5.2.
      ghostscript \
      ocrmypdf tesseract-ocr tesseract-ocr-fra tesseract-ocr-eng tesseract-ocr-deu tesseract-ocr-spa tesseract-ocr-ita \
      libreoffice-writer libreoffice-calc libreoffice-impress \
      img2pdf \
      fonts-dejavu fonts-liberation2 \
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

# Tous les répertoires d'état, créés ici et possédés par `rocket`.
#
# Ce n'est pas de la coquetterie : quand docker-compose monte un volume nommé sur un
# chemin qui N'EXISTE PAS dans l'image, Docker crée le point de montage en root:root,
# et le service — qui tourne en rocket — ne peut plus rien y écrire. C'est ce qui
# faisait échouer POST /api/files avec « Permission denied » sur tout déploiement
# neuf, sans que rien ne le dise au démarrage.
#
#   pdf       les PDF produits, gardés pour que les download_url distribuées vivent
#   cache     les rendus adressés par contenu
#   assets    les fichiers déposés par les appelants
#   accounts  comptes, clés d'API hachées, registre de qualité
#   sessions  les sessions ouvertes
#   work      les temporaires, partagés avec le worker (voir src/sandbox.rs)
#   spool     la boîte aux lettres du worker
RUN mkdir -p \
      /home/rocket/public/pdf \
      /home/rocket/public/cache \
      /home/rocket/public/assets \
      /home/rocket/public/accounts \
      /home/rocket/public/sessions \
      /home/rocket/work \
      /home/rocket/spool \
 && chown -R rocket:rocket /home/rocket/public /home/rocket/work /home/rocket/spool

USER rocket

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8000/api/health || exit 1

CMD ["md-to-pdf"]
