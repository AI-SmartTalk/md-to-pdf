"""Check the published product page set against the locale route catalog."""

from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
import unittest

from scripts.blog_pipeline import LOCALE_CONFIG


ROOT = Path(__file__).resolve().parents[1]
SITE_URL = "https://pdf.aismarttalk.tech"
LOCALES = {item["code"]: item for item in LOCALE_CONFIG["languages"]}


class LandingParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.html_language = None
        self.canonical = None
        self.alternates = {}
        self.language_links = {}
        self.ids = set()
        self.scripts = set()
        self.text = []
        self.in_language_nav = 0

    def handle_starttag(self, tag, attrs) -> None:
        attributes = dict(attrs)
        if tag == "html":
            self.html_language = attributes.get("lang")
        if tag == "link":
            rel = attributes.get("rel")
            if rel == "canonical":
                self.canonical = attributes.get("href")
            elif rel == "alternate":
                self.alternates[attributes.get("hreflang")] = attributes.get("href")
        if tag == "nav" and "language-nav" in attributes.get("class", "").split():
            self.in_language_nav += 1
        if tag == "a" and self.in_language_nav and attributes.get("lang"):
            self.language_links[attributes["lang"]] = attributes.get("href")
        if "id" in attributes:
            self.ids.add(attributes["id"])
        if tag == "script" and attributes.get("src"):
            self.scripts.add(attributes["src"])

    def handle_endtag(self, tag) -> None:
        if tag == "nav" and self.in_language_nav:
            self.in_language_nav -= 1

    def handle_data(self, data) -> None:
        self.text.append(" ".join(data.split()))


def file_for_route(language: str, route: str, kind: str) -> Path:
    slug = Path(route).name
    if language == "fr":
        filename = "markdown-to-pdf.html" if kind == "converter" else "markdown-to-pdf-api.html"
        return ROOT / "static" / "seo" / filename
    return ROOT / "static" / "seo" / language / f"{slug}.html"


class PublicLandingTests(unittest.TestCase):
    def test_all_localized_pages_match_catalog_and_link_to_each_other(self) -> None:
        for kind in ("converter", "api"):
            expected_alternates = {
                language: f"{SITE_URL}{locale['landing_paths'][kind]}"
                for language, locale in LOCALES.items()
            }
            expected_alternates["x-default"] = expected_alternates[LOCALE_CONFIG["default_language"]]

            for language, locale in LOCALES.items():
                route = locale["landing_paths"][kind]
                path = file_for_route(language, route, kind)
                self.assertTrue(path.is_file(), f"missing published route {route}: {path}")
                parser = LandingParser()
                parser.feed(path.read_text(encoding="utf-8"))

                self.assertEqual(parser.html_language, language, route)
                self.assertEqual(parser.canonical, f"{SITE_URL}{route}", route)
                self.assertEqual(parser.alternates, expected_alternates, route)
                self.assertEqual(
                    parser.language_links,
                    {code: item["landing_paths"][kind] for code, item in LOCALES.items()},
                    route,
                )
                self.assertTrue(any("description" in line for line in parser.text) or "name=\"description\"" in path.read_text(encoding="utf-8"))

                html = path.read_text(encoding="utf-8")
                if kind == "converter":
                    self.assertIn('id="converter"', html, route)
                    self.assertIn('id="markdown"', html, route)
                    self.assertIn("/static/seo-converter.js", html, route)
                else:
                    self.assertIn("X-API-Key", html, route)
                    self.assertIn("/api/convert", html, route)


if __name__ == "__main__":
    unittest.main()
