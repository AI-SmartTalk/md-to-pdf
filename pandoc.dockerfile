FROM debian:12-slim

# Keep this list in sync with the runtime stage of Dockerfile, otherwise endpoints work in
# production but not in dev (preview needs poppler-utils, merge/watermark/protect need qpdf).
RUN apt-get update \
 && apt-get install --yes \
      pandoc \
      wkhtmltopdf \
      texlive \
      build-essential python3-dev python3-pip python3-setuptools python3-wheel python3-cffi libcairo2 libpango-1.0-0 libpangocairo-1.0-0 libgdk-pixbuf2.0-0 libffi-dev shared-mime-info \
      poppler-utils \
      qpdf \
      curl \
 # https://stackoverflow.com/questions/75608323/how-do-i-solve-error-externally-managed-environment-every-time-i-use-pip-3
 && pip3 install --break-system-packages weasyprint \
 && pandoc --version

# Même enveloppe anti-SSRF qu'en production : sans elle le développement rend sans
# garde et un écart dev/prod ne se voit qu'une fois déployé.
COPY deploy/weasyprint-safe.py /usr/local/lib/weasyprint-safe.py

RUN set -eu \
 && if [ -e /usr/local/bin/weasyprint ] \
    && ! grep -q '^# urlguard-wrapper' /usr/local/bin/weasyprint; then \
      mv /usr/local/bin/weasyprint /usr/local/bin/weasyprint-real; \
    fi \
 && install -m 0755 /usr/local/lib/weasyprint-safe.py /usr/local/bin/weasyprint \
 && weasyprint --version \
 && printf '# titre\n\ntexte\n' > /tmp/smoke.md \
 && printf '@page { size: A5 }\n' > /tmp/.tmpSm0ke1.css \
 && pandoc --standalone --to=html5 --css=/tmp/.tmpSm0ke1.css --pdf-engine=weasyprint \
      --output=/tmp/smoke.pdf /tmp/smoke.md \
 && pdfinfo /tmp/smoke.pdf | grep -q '419.528 x 595.276' \
 && rm -f /tmp/smoke.md /tmp/.tmpSm0ke1.css /tmp/smoke.pdf

EXPOSE 8000

WORKDIR /workdir
