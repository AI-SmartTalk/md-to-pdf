#!/usr/bin/env python3
"""Boot the compiled service in an isolated directory and verify public routes."""

from __future__ import annotations

import argparse
import hashlib
import hmac
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parent.parent
SITE_URL = "https://pdf.aismarttalk.tech"


class PageMetadata(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.language = None
        self.canonical = None
        self.robots = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attributes = dict(attrs)
        if tag == "html":
            self.language = attributes.get("lang")
        elif tag == "link" and "canonical" in (attributes.get("rel") or "").split():
            self.canonical = attributes.get("href")
        elif tag == "meta" and attributes.get("name") == "robots":
            self.robots = attributes.get("content")


def get(base_url: str, route: str) -> tuple[int, str, str]:
    request = Request(f"{base_url}{route}", headers={"Connection": "close"})
    try:
        with urlopen(request, timeout=3) as response:
            body = response.read().decode("utf-8", errors="replace")
            return response.status, response.headers.get("Content-Type", ""), body
    except HTTPError as response:
        body = response.read().decode("utf-8", errors="replace")
        return response.code, response.headers.get("Content-Type", ""), body


def check_html(base_url: str, route: str, language: str) -> bool:
    status, content_type, body = get(base_url, route)
    if status != 200 or "text/html" not in content_type:
        raise SystemExit(f"{route}: expected HTML 200, got {status} {content_type}")
    metadata = PageMetadata()
    metadata.feed(body)
    canonical = f"{SITE_URL}{route}"
    if metadata.language != language or metadata.canonical != canonical:
        raise SystemExit(
            f"{route}: wrong metadata (lang={metadata.language!r}, canonical={metadata.canonical!r})"
        )
    return metadata.robots != "noindex,follow"


def check_download_requires_capability(base_url: str) -> None:
    status, _, _ = get(base_url, "/download/blog/runtime-check.pdf")
    if status != 404:
        raise SystemExit(f"Unsigned saved-PDF route should be concealed with 404, got {status}")
    status, _, _ = get(
        base_url,
        "/download/blog/runtime-check.pdf?signature=" + "0" * 64,
    )
    if status != 404:
        raise SystemExit(f"Invalid saved-PDF capability should be concealed with 404, got {status}")
    payload = b"md-to-pdf:download:v1\nruntime-check\nruntime-check.pdf"
    signature = hmac.new(b"ci-runtime-signing-key", payload, hashlib.sha256).hexdigest()
    status, content_type, body = get(
        base_url,
        f"/download/runtime-check/runtime-check.pdf?signature={signature}",
    )
    if status != 200 or "application/pdf" not in content_type or body != "%PDF-runtime-check":
        raise SystemExit("Valid signed download URL did not return its private PDF fixture")


def check_public_pages(base_url: str) -> None:
    catalog = json.loads((ROOT / "content/blog/locales.json").read_text(encoding="utf-8"))
    indexed_routes: set[str] = set()

    for locale in catalog["languages"]:
        language = locale["code"]
        for route in locale["landing_paths"].values():
            check_html(base_url, route, language)
            indexed_routes.add(f"{SITE_URL}{route}")

        blog_index = f"/{language}/blog"
        if check_html(base_url, blog_index, language):
            indexed_routes.add(f"{SITE_URL}{blog_index}")

    for path in sorted((ROOT / "content/blog/published").glob("*/*.json")):
        article = json.loads(path.read_text(encoding="utf-8"))
        route = f"/{article['language']}/blog/{article['slug']}"
        check_html(base_url, route, article["language"])
        indexed_routes.add(f"{SITE_URL}{route}")

    sitemap_status, sitemap_type, sitemap_body = get(base_url, "/sitemap.xml")
    if sitemap_status != 200 or "xml" not in sitemap_type:
        raise SystemExit(f"/sitemap.xml: expected XML 200, got {sitemap_status} {sitemap_type}")
    try:
        root = ET.fromstring(sitemap_body)
    except ET.ParseError as error:
        raise SystemExit(f"/sitemap.xml is malformed: {error}") from None
    namespace = {"sm": "http://www.sitemaps.org/schemas/sitemap/0.9"}
    locations = {
        element.text
        for element in root.findall("sm:url/sm:loc", namespace)
        if element.text
    }
    missing = indexed_routes - locations
    if missing:
        raise SystemExit("Sitemap is missing public routes: " + ", ".join(sorted(missing)))

    robots_status, _, robots_body = get(base_url, "/robots.txt")
    if robots_status != 200 or f"Sitemap: {SITE_URL}/sitemap.xml" not in robots_body:
        raise SystemExit("/robots.txt does not advertise the generated sitemap")
    check_download_requires_capability(base_url)
    print(f"Verified {len(indexed_routes)} indexable public routes, sitemap, robots and download auth.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blog-output", type=Path, required=True)
    args = parser.parse_args()
    binary = ROOT / "target/debug/md-to-pdf"
    if not binary.is_file():
        raise SystemExit(f"Compiled service not found: {binary}")
    if not args.blog_output.is_dir():
        raise SystemExit(f"Generated blog pages not found: {args.blog_output}")

    with tempfile.TemporaryDirectory(prefix="md-to-pdf-runtime-") as temporary:
        app_root = Path(temporary)
        fixture = app_root / "public/pdf/runtime-check/runtime-check.pdf"
        fixture.parent.mkdir(parents=True)
        fixture.write_bytes(b"%PDF-runtime-check")
        shutil.copytree(
            ROOT / "static",
            app_root / "static",
            ignore=shutil.ignore_patterns("blog-generated"),
        )
        shutil.copytree(args.blog_output, app_root / "static/blog-generated")
        for directory in ("themes", "templates"):
            shutil.copytree(ROOT / directory, app_root / directory)
        shutil.copy2(ROOT / "Rocket.toml", app_root / "Rocket.toml")

        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        base_url = f"http://127.0.0.1:{port}"
        log_path = app_root / "rocket.log"
        environment = {
            **os.environ,
            "API_KEY": "ci-runtime-smoke-key",
            "API_KEYS_FILE": str(app_root / "api-keys.json"),
            "DOWNLOAD_SIGNING_KEY": "ci-runtime-smoke-signing-key",
            "ATTESTATION_SECRET": "ci-runtime-signing-key",
            "ROCKET_ADDRESS": "127.0.0.1",
            "ROCKET_PORT": str(port),
        }
        with log_path.open("wb") as log:
            process = subprocess.Popen(
                [str(binary)], cwd=app_root, env=environment, stdout=log, stderr=log
            )
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        detail = log_path.read_text(encoding="utf-8", errors="replace")[-4000:]
                        raise SystemExit(f"Service exited before listening. Startup log:\n{detail}")
                    try:
                        status, _, _ = get(base_url, "/api/health")
                        if status == 200:
                            break
                    except (URLError, TimeoutError, OSError):
                        pass
                    time.sleep(0.25)
                else:
                    detail = log_path.read_text(encoding="utf-8", errors="replace")[-4000:]
                    raise SystemExit(f"Service did not become healthy. Startup log:\n{detail}")

                check_public_pages(base_url)
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)


if __name__ == "__main__":
    main()
