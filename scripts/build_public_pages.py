#!/usr/bin/env python3
"""Render reviewed multilingual articles as crawlable static HTML pages."""

from __future__ import annotations

from html import escape
from html.parser import HTMLParser
from datetime import date
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parent.parent
CONTENT = ROOT / "content" / "blog" / "published"
LOCALES_FILE = ROOT / "content" / "blog" / "locales.json"
DEFAULT_OUTPUT = ROOT / "static" / "blog-generated"
SITE_URL = "https://pdf.aismarttalk.tech"
SLUG = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")
LOCALE_CONFIG = json.loads(LOCALES_FILE.read_text(encoding="utf-8"))
LOCALE_BY_CODE = {item["code"]: item for item in LOCALE_CONFIG["languages"]}
LANGUAGES = list(LOCALE_BY_CODE)
LANGUAGE_NAMES = {code: item["native_name"] for code, item in LOCALE_BY_CODE.items()}
DEFAULT_LANGUAGE = LOCALE_CONFIG["default_language"]
CTA_FALLBACK_LANGUAGE = LOCALE_CONFIG["cta_fallback_language"]
LANDING_PATHS = {
    code: (paths["converter"], paths["api"])
    for code, item in LOCALE_BY_CODE.items()
    if isinstance(paths := item.get("landing_paths"), dict)
    and isinstance(paths.get("converter"), str)
    and isinstance(paths.get("api"), str)
}
LANDING_LANGUAGES = list(LANDING_PATHS)
WORDS = {code: item["blog"] for code, item in LOCALE_BY_CODE.items()}
if not LANGUAGES or DEFAULT_LANGUAGE not in LOCALE_BY_CODE:
    raise SystemExit("Le catalogue de langues doit définir une langue par défaut connue.")
if CTA_FALLBACK_LANGUAGE not in LANDING_PATHS:
    raise SystemExit("La langue de repli des CTA doit avoir des routes produit publiées.")
if any(language not in LOCALE_BY_CODE for language in LANDING_LANGUAGES):
    raise SystemExit("Une route produit référence une langue inconnue.")
ALLOWED_TAGS = {
    "a", "blockquote", "br", "code", "del", "em", "h2", "h3", "h4", "hr",
    "li", "ol", "p", "pre", "s", "strong", "table", "tbody", "td", "th",
    "thead", "tr", "ul",
}
VOID_TAGS = {"br", "hr"}


class SafeHtml(HTMLParser):
    """Keep useful Markdown markup while dropping attributes and unsafe URL schemes."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.output: list[str] = []
        self.stack: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag not in ALLOWED_TAGS:
            return
        if tag == "a":
            href = next((value for name, value in attrs if name == "href" and value), None)
            self.output.append(f'<a href="{escape(href, quote=True)}">' if href and safe_href(href) else "<a>")
        else:
            self.output.append(f"<{tag}>")
        if tag not in VOID_TAGS:
            self.stack.append(tag)

    def handle_endtag(self, tag: str) -> None:
        if tag not in self.stack:
            return
        while self.stack:
            opened = self.stack.pop()
            self.output.append(f"</{opened}>")
            if opened == tag:
                break

    def handle_data(self, data: str) -> None:
        self.output.append(escape(data))

    def close_fragment(self) -> str:
        super().close()
        while self.stack:
            self.output.append(f"</{self.stack.pop()}>")
        return "".join(self.output)


def safe_href(value: str) -> bool:
    value = value.strip()
    if not value or "\\" in value or any(ord(char) < 32 for char in value):
        return False
    try:
        parsed = urlsplit(value)
        if parsed.scheme:
            return (
                parsed.scheme == "https"
                and bool(parsed.hostname)
                and parsed.username is None
                and parsed.password is None
            )
        return not value.startswith("//") and (value.startswith("/") or value.startswith("#"))
    except ValueError:
        return False


def render_markdown(markdown: str) -> str:
    result = subprocess.run(
        ["pandoc", "--from=markdown-raw_html-raw_tex", "--to=html5", "--wrap=none"],
        input=markdown,
        text=True,
        capture_output=True,
        check=True,
    )
    parser = SafeHtml()
    parser.feed(result.stdout)
    return parser.close_fragment()


def alternates(article: dict, groups: dict[str, dict[str, dict]]) -> str:
    variants = groups.get(article["translation_group"], {})
    links = []
    for language, item in sorted(variants.items()):
        links.append(f'<link rel="alternate" hreflang="{language}" href="{SITE_URL}/{language}/blog/{escape(item["slug"], quote=True)}">')
    if variants:
        default_language = DEFAULT_LANGUAGE if DEFAULT_LANGUAGE in variants else next(iter(variants))
        default = variants[default_language]
        links.append(f'<link rel="alternate" hreflang="x-default" href="{SITE_URL}/{default_language}/blog/{escape(default["slug"], quote=True)}">')
    return "\n".join(links)


def article_template(article: dict, body: str, groups: dict[str, dict[str, dict]]) -> str:
    language = article["language"]
    words = WORDS[language]
    slug = article["slug"]
    canonical = f"{SITE_URL}/{language}/blog/{slug}"
    title = escape(article["title"])
    description = escape(article["description"], quote=True)
    date = escape(article["published_at"])
    modified = escape(article.get("updated_at", article["published_at"]))
    converter_path, api_path = LANDING_PATHS.get(language, LANDING_PATHS[CTA_FALLBACK_LANGUAGE])
    variants = groups.get(article["translation_group"], {})
    language_links = " ".join(
        f'<a lang="{code}" href="/{code}/blog/{escape(variant["slug"], quote=True)}">{escape(LANGUAGE_NAMES[code])}</a>'
        for code, variant in sorted(variants.items()) if code != language
    )
    structured = json.dumps(
        {
            "@context": "https://schema.org", "@type": "Article",
            "headline": article["title"], "description": article["description"],
            "datePublished": date, "dateModified": modified, "inLanguage": language,
            "author": {"@type": "Organization", "name": article.get("author", "AI SmartTalk")},
            "publisher": {"@type": "Organization", "name": "AI SmartTalk", "url": "https://aismarttalk.tech"},
            "mainEntityOfPage": canonical,
        }, ensure_ascii=False,
    ).replace("<", "\\u003c")
    return f'''<!doctype html>
<html lang="{language}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} | md-to-pdf by AI SmartTalk</title><meta name="description" content="{description}">
<meta name="robots" content="index,follow,max-image-preview:large"><link rel="canonical" href="{canonical}">
{alternates(article, groups)}<meta property="og:type" content="article"><meta property="og:site_name" content="md-to-pdf by AI SmartTalk">
<meta property="og:title" content="{title}"><meta property="og:description" content="{description}"><meta property="og:url" content="{canonical}">
<meta property="article:published_time" content="{date}"><meta property="article:modified_time" content="{modified}">
<link rel="icon" href="/static/favicon.ico"><link rel="stylesheet" href="/static/marketing.css">
<script type="application/ld+json">{structured}</script></head><body>
<header class="site-header"><a class="brand" href="/">md-to-pdf <span>by AI SmartTalk</span></a>
<nav aria-label="Main navigation"><a href="{converter_path}">{words['convert']}</a><a href="/{language}/blog">{words['blog']}</a><a href="{api_path}">{words['api']}</a></nav>
<nav class="language-nav" aria-label="Language">{language_links}</nav></header>
<main><nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/">{words['home']}</a> / <a href="/{language}/blog">{words['blog']}</a> / {title}</nav>
<article class="article"><p class="eyebrow">{words['eyebrow']} · {date}</p><h1>{title}</h1><p class="lede">{description}</p>
<div class="article-body">{body}</div><aside class="callout article-cta"><h2>{words['cta_title']}</h2><p>{words['cta_copy']}</p>
<p><a class="text-link" href="{escape(article['cta_url'], quote=True)}">{words['cta_link']}</a></p></aside></article></main>
<footer class="site-footer"><span>md-to-pdf by AI SmartTalk</span><nav><a href="{converter_path}">{words['convert']}</a><a href="/{language}/blog">{words['blog']}</a><a href="{api_path}">{words['api']}</a></nav></footer></body></html>'''


def validate_article(path: Path, article: dict) -> None:
    required = ("slug", "translation_group", "language", "title", "description", "published_at", "article_markdown", "cta_url")
    if any(not isinstance(article.get(field), str) or not article[field].strip() for field in required):
        raise SystemExit(f"Champs requis manquants dans {path}")
    if article["language"] not in LANGUAGES or not SLUG.fullmatch(article["slug"]) or not SLUG.fullmatch(article["translation_group"]):
        raise SystemExit(f"Langue ou slug invalide dans {path}")
    try:
        date.fromisoformat(article["published_at"])
        date.fromisoformat(article.get("updated_at", article["published_at"]))
    except (TypeError, ValueError):
        raise SystemExit(f"Date invalide dans {path}") from None
    if not safe_href(article["cta_url"]) or not article["cta_url"].startswith("/"):
        raise SystemExit(f"CTA invalide dans {path}")


def load_articles() -> tuple[list[dict], dict[str, dict[str, dict]]]:
    articles: list[dict] = []
    groups: dict[str, dict[str, dict]] = {}
    seen: set[tuple[str, str]] = set()
    if not CONTENT.exists():
        return articles, groups
    for path in sorted(CONTENT.rglob("*.json")):
        try:
            article = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise SystemExit(f"Article invalide {path}: {exc}") from exc
        validate_article(path, article)
        identity = (article["language"], article["slug"])
        if identity in seen:
            raise SystemExit(f"Slug en double pour {article['language']}: {article['slug']}")
        seen.add(identity)
        language_map = groups.setdefault(article["translation_group"], {})
        if article["language"] in language_map:
            raise SystemExit(f"Traduction en double pour {article['translation_group']} ({article['language']})")
        language_map[article["language"]] = article
        articles.append(article)
    return sorted(articles, key=lambda item: item["published_at"], reverse=True), groups


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(0o644)


def index_template(
    language: str,
    articles: list[dict],
    index_languages: list[str],
    alternate_languages: list[str],
) -> str:
    words = WORDS[language]
    converter_path, api_path = LANDING_PATHS.get(language, LANDING_PATHS[CTA_FALLBACK_LANGUAGE])
    lang_articles = [item for item in articles if item["language"] == language]
    robots = "index,follow" if lang_articles else "noindex,follow"
    default_language = DEFAULT_LANGUAGE if DEFAULT_LANGUAGE in alternate_languages else next(iter(alternate_languages), None)
    language_alternates = "".join(
        f'<link rel="alternate" hreflang="{lang}" href="{SITE_URL}/{lang}/blog">'
        for lang in alternate_languages
    )
    if default_language:
        language_alternates += (
            f'<link rel="alternate" hreflang="x-default" href="{SITE_URL}/{default_language}/blog">'
        )
    cards = "\n".join(
        f'''<article class="post-card"><p class="eyebrow">{escape(item.get("published_at", ""))}</p>
<h2><a href="/{language}/blog/{escape(item["slug"], quote=True)}">{escape(item["title"])}</a></h2>
<p>{escape(item["description"])}</p><a class="text-link" href="/{language}/blog/{escape(item["slug"], quote=True)}">{words['read']}</a></article>'''
        for item in lang_articles
    ) or f"<p>{escape(words['empty'])}</p>"
    language_switch = " ".join(
        f'<a lang="{code}" href="/{code}/blog">{escape(LANGUAGE_NAMES[code])}</a>'
        for code in index_languages if code != language
    )
    canonical = f"{SITE_URL}/{language}/blog"
    return f'''<!doctype html><html lang="{language}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{escape(words['index_title'])}</title><meta name="description" content="{escape(words['index_description'], quote=True)}">
<meta name="robots" content="{robots}"><link rel="canonical" href="{canonical}">
{language_alternates}
<meta property="og:type" content="website"><meta property="og:title" content="{escape(words['index_title'])}"><meta property="og:description" content="{escape(words['index_description'], quote=True)}">
<meta property="og:url" content="{canonical}"><link rel="stylesheet" href="/static/marketing.css"></head><body>
<header class="site-header"><a class="brand" href="/">md-to-pdf <span>by AI SmartTalk</span></a>
<nav><a href="{converter_path}">{words['convert']}</a><a href="{api_path}">{words['api']}</a></nav>
<nav class="language-nav" aria-label="Language">{language_switch}</nav></header>
<main><section class="hero"><p class="eyebrow">AI SMARTTALK</p><h1>{escape(words['index_heading'])}</h1><p class="lede">{escape(words['index_lede'])}</p></section><section class="post-list">{cards}</section>
<section class="related"><h2>{words['try']}</h2><p><a href="{converter_path}">{words['convert_link']}</a> · <a href="{api_path}">{words['api_link']}</a></p></section></main>
<footer class="site-footer"><span>md-to-pdf by AI SmartTalk</span><nav><a href="{converter_path}">{words['convert']}</a><a href="{api_path}">{words['api']}</a></nav></footer></body></html>'''


def build(output: Path) -> None:
    articles, groups = load_articles()
    index_languages = [
        language for language in LANGUAGES
        if language in LANDING_LANGUAGES or any(item["language"] == language for item in articles)
    ]
    published_index_languages = [
        language for language in LANGUAGES
        if any(item["language"] == language for item in articles)
    ]
    unsupported_copy = sorted({item["language"] for item in articles} - WORDS.keys())
    if unsupported_copy:
        raise SystemExit(
            "Ajoutez les libellés du blog pour ces langues avant publication : "
            + ", ".join(unsupported_copy)
        )
    for article in articles:
        path = output / article["language"] / f"{article['slug']}.html"
        body = render_markdown(article["article_markdown"])
        write(path, article_template(article, body, groups))
    for language in index_languages:
        write(
            output / language / "index.html",
            index_template(language, articles, index_languages, published_index_languages),
        )

    latest_date = max((item.get("updated_at", item["published_at"]) for item in articles), default=None)
    entries: list[tuple[str, str | None, str | None]] = [("/", None, None)]
    for language in LANDING_LANGUAGES:
        converter_path, api_path = LANDING_PATHS[language]
        entries.append((converter_path, None, "tools"))
        entries.append((api_path, None, "api"))
    for language in published_index_languages:
        entries.append((f"/{language}/blog", latest_date, "blog"))
    for article in articles:
        entries.append((f"/{article['language']}/blog/{article['slug']}", article.get("updated_at", article["published_at"]), article["translation_group"]))

    sitemap = ['<?xml version="1.0" encoding="UTF-8"?>', '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">']
    sitemap_routes: list[dict[str, object]] = []
    for path, lastmod, group in entries:
        sitemap.append("  <url>")
        sitemap.append(f"    <loc>{escape(SITE_URL + path)}</loc>")
        if lastmod:
            sitemap.append(f"    <lastmod>{escape(lastmod)}</lastmod>")
        route: dict[str, object] = {"path": path, "lastmod": lastmod, "alternates": []}
        if group in {"tools", "api"}:
            position = 0 if group == "tools" else 1
            for lang in LANDING_LANGUAGES:
                alternate_path = LANDING_PATHS[lang][position]
                sitemap.append(f'    <xhtml:link rel="alternate" hreflang="{lang}" href="{SITE_URL}{alternate_path}"/>')
                route["alternates"].append({"language": lang, "path": alternate_path})
            default_path = LANDING_PATHS[DEFAULT_LANGUAGE][position]
            sitemap.append(f'    <xhtml:link rel="alternate" hreflang="x-default" href="{SITE_URL}{default_path}"/>')
            route["x_default"] = default_path
        elif group == "blog":
            for lang in published_index_languages:
                alternate_path = f"/{lang}/blog"
                sitemap.append(f'    <xhtml:link rel="alternate" hreflang="{lang}" href="{SITE_URL}{alternate_path}"/>')
                route["alternates"].append({"language": lang, "path": alternate_path})
            default_language = DEFAULT_LANGUAGE if DEFAULT_LANGUAGE in published_index_languages else next(iter(published_index_languages), None)
            if default_language:
                default_path = f"/{default_language}/blog"
                sitemap.append(f'    <xhtml:link rel="alternate" hreflang="x-default" href="{SITE_URL}{default_path}"/>')
                route["x_default"] = default_path
        elif group:
            for lang, variant in sorted(groups.get(group, {}).items()):
                alternate_path = f"/{lang}/blog/{variant['slug']}"
                sitemap.append(f'    <xhtml:link rel="alternate" hreflang="{lang}" href="{SITE_URL}{escape(alternate_path)}"/>')
                route["alternates"].append({"language": lang, "path": alternate_path})
            default_language = DEFAULT_LANGUAGE if DEFAULT_LANGUAGE in groups[group] else next(iter(groups[group]))
            default = groups[group][default_language]
            default_path = f"/{default_language}/blog/{default['slug']}"
            sitemap.append(f'    <xhtml:link rel="alternate" hreflang="x-default" href="{SITE_URL}{escape(default_path)}"/>')
            route["x_default"] = default_path
        if path != "/":
            sitemap_routes.append(route)
        sitemap.append("  </url>")
    sitemap.append("</urlset>")
    write(output / "sitemap.xml", "\n".join(sitemap) + "\n")
    write(output / "sitemap-routes.json", json.dumps({"entries": sitemap_routes}, ensure_ascii=False, separators=(",", ":")) + "\n")
    write(output / "robots.txt", f"User-agent: *\nAllow: /\nDisallow: /api/\nDisallow: /download/\nSitemap: {SITE_URL}/sitemap.xml\n")


if __name__ == "__main__":
    destination = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_OUTPUT
    try:
        build(destination)
    except FileNotFoundError as exc:
        raise SystemExit("Pandoc doit être installé pour générer les pages publiques.") from exc
