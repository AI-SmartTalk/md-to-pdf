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

COPY --from=builder /usr/local/cargo/bin/md-to-pdf /usr/local/bin/md-to-pdf

RUN useradd --create-home rocket

WORKDIR /home/rocket

COPY --chown=rocket:rocket static /home/rocket/static
COPY --chown=rocket:rocket Rocket.toml /home/rocket/Rocket.toml
COPY --chown=rocket:rocket templates /home/rocket/templates

# Generated PDFs are written here; mount a volume on it to keep download URLs alive
# across deployments.
RUN mkdir -p /home/rocket/public/pdf && chown -R rocket:rocket /home/rocket/public

USER rocket

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8000/api/health || exit 1

CMD ["md-to-pdf"]
