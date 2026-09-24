#!/usr/bin/env python3
"""Generate localized blog drafts with a configured LLM endpoint."""

from __future__ import annotations

from datetime import date
import argparse
import json
import os
from pathlib import Path
import re
import tempfile
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener


ROOT = Path(__file__).resolve().parent.parent
BRIEFS = ROOT / "content" / "blog" / "briefs.json"
LOCALES_FILE = ROOT / "content" / "blog" / "locales.json"
DRAFTS = ROOT / "content" / "blog" / "drafts"
PUBLISHED = ROOT / "content" / "blog" / "published"
SLUG = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")
MIN_WORDS = 450
MAX_WORDS = 1800
DRAFT_MIN_WORDS = 600
DRAFT_MAX_WORDS = 1200
MAX_PROVIDER_RESPONSE_BYTES = 2 * 1024 * 1024


class NoRedirect(HTTPRedirectHandler):
    """Never forward the provider credential to a redirect destination."""

    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def read_json(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"Impossible de lire {path}: {exc}") from exc


def load_locales() -> dict:
    config = read_json(LOCALES_FILE)
    languages = config.get("languages")
    if not isinstance(languages, list) or not languages:
        raise SystemExit("content/blog/locales.json doit déclarer au moins une langue.")
    codes = [item.get("code") for item in languages if isinstance(item, dict)]
    if len(codes) != len(languages) or len(set(codes)) != len(codes):
        raise SystemExit("Langues invalides ou dupliquées dans content/blog/locales.json.")
    if any(not isinstance(code, str) or not re.fullmatch(r"[a-z]{2}", code) for code in codes):
        raise SystemExit("Utilisez des codes de langue ISO à deux lettres en minuscules.")
    for field in ("source_language", "default_language", "cta_fallback_language"):
        if config.get(field) not in codes:
            raise SystemExit(f"{field} doit désigner une langue déclarée.")
    if not all(
        isinstance(item.get("name"), str)
        and item["name"].strip()
        and isinstance(item.get("native_name"), str)
        and item["native_name"].strip()
        for item in languages
    ):
        raise SystemExit("Chaque langue doit avoir un nom et un libellé natif.")
    return config


LOCALE_CONFIG = load_locales()
LANGUAGES = [item["code"] for item in LOCALE_CONFIG["languages"]]
LOCALE_BY_CODE = {item["code"]: item for item in LOCALE_CONFIG["languages"]}
SOURCE_LANGUAGE = LOCALE_CONFIG["source_language"]
DEFAULT_LANGUAGE = LOCALE_CONFIG["default_language"]
CTA_FALLBACK_LANGUAGE = LOCALE_CONFIG["cta_fallback_language"]
LANDING_PATHS = {
    language: item.get("landing_paths", {})
    for language, item in LOCALE_BY_CODE.items()
}


def localize_product_url(value: str, language: str) -> str:
    """Keep translated drafts pointed at a live product page in their language."""
    fallback = LANDING_PATHS.get(CTA_FALLBACK_LANGUAGE, {})
    target = LANDING_PATHS.get(language, {})
    for source_paths in LANDING_PATHS.values():
        for kind in ("converter", "api"):
            source = source_paths.get(kind)
            if source == value:
                return target.get(kind) or fallback.get(kind) or value
    return value


def brief_cta(brief: dict, language: str) -> str:
    direct = brief.get(f"cta_{language}")
    if isinstance(direct, str) and direct.strip():
        return direct.strip()
    fallback = brief.get(f"cta_{CTA_FALLBACK_LANGUAGE}")
    if isinstance(fallback, str) and fallback.strip():
        return localize_product_url(fallback.strip(), language)
    raise SystemExit(f"CTA absente pour {brief.get('translation_group', 'ce brief')}.")


def published_by_group() -> dict[str, dict[str, Path]]:
    groups: dict[str, dict[str, Path]] = {}
    for path in PUBLISHED.rglob("*.json"):
        article = read_json(path)
        group, language = article.get("translation_group"), article.get("language")
        if isinstance(group, str) and isinstance(language, str):
            groups.setdefault(group, {})[language] = path
    return groups


def completed_group(group: str, published: dict[str, dict[str, Path]] | None = None) -> bool:
    published = published if published is not None else published_by_group()
    present = set(published.get(group, {}))
    present.update(
        language for language in LANGUAGES
        if (DRAFTS / group / f"{language}.json").is_file()
    )
    return set(LANGUAGES) <= present


def pending_groups() -> list[str]:
    published = published_by_group()
    briefs = read_json(BRIEFS)
    brief_groups = [item["translation_group"] for item in briefs]
    existing = sorted(
        group for group in published
        if group not in brief_groups and not completed_group(group, published)
    )
    queued = existing + [
        group for group in brief_groups
        if not completed_group(group, published)
    ]
    return list(dict.fromkeys(queued))


def next_brief(requested: str | None) -> dict | None:
    briefs = read_json(BRIEFS)
    by_group = {item["translation_group"]: item for item in briefs}
    if requested:
        if requested not in by_group and requested not in published_by_group():
            raise SystemExit(f"Brief ou groupe publié inconnu : {requested}")
        return by_group.get(requested)
    groups = pending_groups()
    return by_group.get(groups[0]) if groups and groups[0] in by_group else None


def endpoint() -> tuple[str, str, str]:
    url = os.environ.get("BLOG_LLM_CHAT_COMPLETIONS_URL", "").strip()
    key = os.environ.get("BLOG_LLM_API_KEY", "").strip()
    model = os.environ.get("BLOG_LLM_MODEL", "").strip()
    if not url or not key or not model:
        raise SystemExit(
            "Configurez BLOG_LLM_CHAT_COMPLETIONS_URL, BLOG_LLM_API_KEY et BLOG_LLM_MODEL."
        )
    parsed = urlsplit(url)
    local = parsed.hostname in {"localhost", "127.0.0.1", "::1"}
    if (parsed.scheme != "https" and not local) or parsed.username or parsed.password:
        raise SystemExit("L’endpoint LLM doit utiliser HTTPS hors d’une machine locale.")
    return url, key, model


def call_llm(system: str, prompt: str) -> dict:
    url, key, model = endpoint()
    body = json.dumps(
        {
            "model": model,
            "temperature": 0.35,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
        }
    ).encode("utf-8")
    request = Request(
        url,
        data=body,
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with build_opener(NoRedirect).open(request, timeout=180) as response:
            raw = response.read(MAX_PROVIDER_RESPONSE_BYTES + 1)
        if len(raw) > MAX_PROVIDER_RESPONSE_BYTES:
            raise SystemExit("Réponse du fournisseur LLM supérieure à 2 Mio.")
        payload = json.loads(raw.decode("utf-8"))
    except HTTPError as exc:
        raise SystemExit(f"Le fournisseur LLM a répondu HTTP {exc.code}.") from None
    except URLError as exc:
        raise SystemExit(f"Impossible de joindre le fournisseur LLM : {exc.reason}.") from None
    except (TimeoutError, json.JSONDecodeError) as exc:
        raise SystemExit(f"Réponse LLM invalide ou expirée : {exc}.") from None

    try:
        content = payload["choices"][0]["message"]["content"]
        if isinstance(content, list):
            content = "".join(part.get("text", "") for part in content if isinstance(part, dict))
        if not isinstance(content, str):
            raise TypeError("message content is not text")
        content = re.sub(r"\A```(?:json)?\s*|\s*```\Z", "", content.strip())
        result = json.loads(content)
    except (KeyError, IndexError, TypeError, json.JSONDecodeError) as exc:
        raise SystemExit(f"Le modèle n’a pas renvoyé le JSON attendu : {exc}.") from None
    if not isinstance(result, dict):
        raise SystemExit("La réponse du modèle doit être un objet JSON.")
    return result


def word_count(markdown: str) -> int:
    return len(re.findall(r"\b[\wÀ-ÿ'-]+\b", markdown, flags=re.UNICODE))


def localize_internal_links(markdown: str, language: str) -> str:
    """Point product and blog links at available pages in the target locale."""
    replacements: dict[str, str] = {}
    fallback = LANDING_PATHS.get(CTA_FALLBACK_LANGUAGE, {})
    target = LANDING_PATHS.get(language, {})
    for source_paths in LANDING_PATHS.values():
        for kind in ("converter", "api"):
            source = source_paths.get(kind)
            destination = target.get(kind) or fallback.get(kind)
            if isinstance(source, str) and isinstance(destination, str) and source != destination:
                replacements[source] = destination
    if replacements:
        pattern = re.compile(
            "|".join(re.escape(path) for path in sorted(replacements, key=len, reverse=True))
            + r"(?=$|[\s)#?,.!;:])"
        )
        markdown = pattern.sub(lambda match: replacements[match.group(0)], markdown)

    slugs_by_group: dict[str, dict[str, str]] = {}
    for group, variants in published_by_group().items():
        for article_language, path in variants.items():
            article = read_json(path)
            if isinstance(article.get("slug"), str):
                slugs_by_group.setdefault(group, {})[article_language] = article["slug"]
    if DRAFTS.exists():
        for path in DRAFTS.glob("*/*.json"):
            article = read_json(path)
            group = article.get("translation_group")
            article_language = article.get("language")
            slug = article.get("slug")
            if (
                isinstance(group, str)
                and article_language in LANGUAGES
                and isinstance(slug, str)
            ):
                slugs_by_group.setdefault(group, {}).setdefault(article_language, slug)

    source_articles = {
        (article_language, slug): variants
        for variants in slugs_by_group.values()
        for article_language, slug in variants.items()
    }
    blog_pattern = re.compile(
        rf"/(?P<source>{'|'.join(map(re.escape, LANGUAGES))})/blog"
        rf"(?:/(?P<slug>[a-z0-9-]+))?(?=$|[\s)#?])"
    )

    def localize_blog_link(match: re.Match[str]) -> str:
        source_language = match.group("source")
        slug = match.group("slug")
        if source_language == language:
            return match.group(0)
        if slug:
            target_slug = source_articles.get((source_language, slug), {}).get(language)
            if target_slug:
                return f"/{language}/blog/{target_slug}"
        return f"/{language}/blog"

    markdown = blog_pattern.sub(localize_blog_link, markdown)
    return markdown


def validate_article(
    article: dict, language: str, group: str, cta: str, *, generated_draft: bool = False
) -> dict:
    for field in ("slug", "title", "description", "article_markdown"):
        if not isinstance(article.get(field), str) or not article[field].strip():
            raise SystemExit(f"Le modèle n’a pas fourni le champ {field}.")
    article["slug"] = article["slug"].strip().lower()
    article["title"] = article["title"].strip()
    article["description"] = re.sub(r"\s+", " ", article["description"]).strip()
    article["article_markdown"] = article["article_markdown"].strip()
    if not SLUG.fullmatch(article["slug"]):
        raise SystemExit(f"Slug invalide : {article['slug']}")
    if not 45 <= len(article["description"]) <= 170:
        raise SystemExit("La meta description doit contenir entre 45 et 170 caractères.")
    words = word_count(article["article_markdown"])
    minimum = DRAFT_MIN_WORDS if generated_draft else MIN_WORDS
    maximum = DRAFT_MAX_WORDS if generated_draft else MAX_WORDS
    if not minimum <= words <= maximum:
        raise SystemExit(f"Article trop court ou long : {words} mots (cible {minimum}–{maximum}).")
    minimum_sections = 4 if generated_draft else 3
    if len(re.findall(r"^##\s+", article["article_markdown"], flags=re.MULTILINE)) < minimum_sections:
        raise SystemExit(f"L’article doit avoir au moins {minimum_sections} sections de niveau 2.")
    if generated_draft:
        fenced_example = re.search(
            r"(?ms)^```[^\n]*\n.+?^```\s*$", article["article_markdown"]
        )
        numbered_steps = re.findall(
            r"^\s*\d+[.)]\s+\S", article["article_markdown"], flags=re.MULTILINE
        )
        if not fenced_example and len(numbered_steps) < 3:
            raise SystemExit("Le brouillon doit inclure un exemple concret ou au moins trois étapes numérotées.")
    if cta not in article["article_markdown"]:
        raise SystemExit("L’article doit inclure son lien d’action interne.")
    if re.search(r"<\s*/?\s*[a-zA-Z][^>]*>", article["article_markdown"]):
        raise SystemExit("HTML brut refusé dans un brouillon ; utilisez Markdown.")

    article.update(
        {
            "translation_group": group,
            "language": language,
            "published_at": date.today().isoformat(),
            "updated_at": date.today().isoformat(),
            "author": "AI SmartTalk",
            "cta_url": cta,
        }
    )
    return article


def store_draft(group: str, language: str, article: dict) -> Path:
    path = DRAFTS / group / f"{language}.json"
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, temporary = tempfile.mkstemp(prefix=".draft-", dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as output:
            json.dump(article, output, ensure_ascii=False, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    except Exception:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise
    return path


def draft_prompt(brief: dict, language: str) -> tuple[str, str]:
    primary_query = brief[f"primary_query_{language}"]
    audience = brief[f"audience_{language}"]
    search_intent = brief[f"search_intent_{language}"]
    editorial_angle = (
        brief.get(f"editorial_angle_{language}")
        or brief.get("editorial_angle_en")
        or "Answer the stated search intent with a specific, practical workflow."
    )
    cta = brief[f"cta_{language}"]
    system = (
        "You are a senior technical editor for md-to-pdf by AI SmartTalk. Write genuinely useful, "
        "specific material for people solving a real document workflow. Never invent capabilities, "
        "performance numbers, prices, certifications, customer names, or guarantees. Use only the "
        "verified product facts supplied. Avoid keyword repetition and generic SEO filler. Return only "
        "a JSON object with slug, title, description, article_markdown. The article must be in "
        f"{language}, 600-1200 words, with a practical opening and at least 4 useful H2 sections. "
        "Include one concrete example as a fenced code sample or a numbered workflow of at least "
        "three steps, plus a relevant internal action link. Markdown only; no raw HTML."
    )
    prompt = json.dumps(
        {
            "language": language,
            "primary_query": primary_query,
            "search_intent": search_intent,
            "editorial_angle": editorial_angle,
            "audience": audience,
            "required_action_link": cta,
            "verified_product_facts": brief["product_facts"],
            "editorial_requirements": [
                "Answer the actual task in the first two paragraphs.",
                "Keep the article distinct from related topics by following its editorial angle.",
                "Show a real step-by-step workflow or code example where the topic warrants it.",
                "Explain limits and review checks; do not promise perfect PDF layout.",
                "Link to the action page and, where helpful, the localized API or blog page.",
                "Do not create an FAQ list just to repeat search terms.",
            ],
        },
        ensure_ascii=False,
    )
    return system, prompt


def translate_prompt(brief: dict | None, source: dict, language: str, cta: str) -> tuple[str, str]:
    locale_name = next(item["name"] for item in LOCALE_CONFIG["languages"] if item["code"] == language)
    system = (
        "You are a careful technical translator and editor. Translate and localize the source article "
        f"into natural {locale_name} ({language}) for the audience described. Preserve meaning, code, product facts, "
        "caveats and internal linking intent. Do not add claims, examples or "
        "capabilities. Return only a JSON object with slug, title, description, article_markdown. "
        "Markdown only; no raw HTML."
    )
    reference_language = "en" if "en" in LANGUAGES else SOURCE_LANGUAGE
    title = source.get("title", "").strip()
    if brief:
        audience = brief.get(f"audience_{language}") or brief.get(f"audience_{reference_language}") or title
        intent = brief.get(f"search_intent_{language}") or brief.get(f"search_intent_{reference_language}") or source.get("description", "")
        query = brief.get(f"primary_query_{language}") or brief.get(f"primary_query_{reference_language}") or title
        editorial_angle = brief.get(f"editorial_angle_{language}") or brief.get("editorial_angle_en") or ""
    else:
        audience = f"Readers seeking practical help with: {title}"
        intent = source.get("description", title)
        query = title
        editorial_angle = ""
    prompt = json.dumps(
        {
            "target_language": language,
            "audience_reference": audience,
            "search_intent_reference": intent,
            "primary_query_reference": query,
            "editorial_angle_reference": editorial_angle,
            "required_action_link": cta,
            "source_article": source,
            "localization_requirements": [
                "Write naturally for readers in the target language; do not translate word for word.",
                "Localize the search phrase, title, examples and terminology while preserving product facts.",
                "Preserve the source article's distinct editorial angle and do not broaden it into a neighboring topic.",
                "Use the required action link and adapt source-language blog-index links to the target-language blog route.",
                "Keep useful links to specific articles; the pipeline resolves an available translation or falls back to the target blog index.",
            ],
        },
        ensure_ascii=False,
    )
    return system, prompt


def generate(group: str | None) -> None:
    brief = next_brief(group)
    if group is None and brief is None:
        groups = pending_groups()
        if groups:
            group = groups[0]
            brief = next((item for item in read_json(BRIEFS) if item["translation_group"] == group), None)
    if group is None and brief is None:
        print("Tous les groupes sont complets. Ajoutez un brief pour reprendre la génération.")
        return
    if group is None:
        group = brief["translation_group"]
    published = published_by_group().get(group, {})
    draft_dir = DRAFTS / group
    draft_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    paths = {language: draft_dir / f"{language}.json" for language in LANGUAGES}
    if completed_group(group):
        raise SystemExit(f"Toutes les langues sont déjà publiées ou en brouillon pour {group}.")
    if paths[SOURCE_LANGUAGE].exists():
        source = read_json(paths[SOURCE_LANGUAGE])
    elif CTA_FALLBACK_LANGUAGE in published:
        source = read_json(published[CTA_FALLBACK_LANGUAGE])
    elif SOURCE_LANGUAGE in published:
        source = read_json(published[SOURCE_LANGUAGE])
    elif published:
        source = read_json(next(iter(published.values())))
    elif brief:
        source_cta = brief_cta(brief, SOURCE_LANGUAGE)
        system, prompt = draft_prompt(brief, SOURCE_LANGUAGE)
        source = validate_article(
            call_llm(system, prompt), SOURCE_LANGUAGE, group, source_cta, generated_draft=True
        )
        store_draft(group, SOURCE_LANGUAGE, source)
        print(f"Brouillon {SOURCE_LANGUAGE.upper()} créé : {paths[SOURCE_LANGUAGE].relative_to(ROOT)}")
    else:
        raise SystemExit(f"Aucun article source disponible pour le groupe {group}.")

    for language in LANGUAGES:
        if language == source.get("language") or paths[language].exists() or language in published:
            continue
        if brief:
            cta = brief_cta(brief, language)
        else:
            cta = localize_product_url(source.get("cta_url", "/en/markdown-to-pdf"), language)
        system, prompt = translate_prompt(brief, source, language, cta)
        localized = call_llm(system, prompt)
        localized["article_markdown"] = localize_internal_links(
            localized.get("article_markdown", ""), language
        )
        localized = validate_article(localized, language, group, cta, generated_draft=True)
        store_draft(group, language, localized)
        print(f"Brouillon {language.upper()} créé : {paths[language].relative_to(ROOT)}")

    print(f"Relisez chaque nouvelle variante avant `publish`; aucune traduction n'est indexée automatiquement.")


def list_available_groups() -> None:
    for group in pending_groups():
        print(group)


def publish(group: str) -> None:
    if not SLUG.fullmatch(group):
        raise SystemExit("Groupe invalide.")
    briefs = read_json(BRIEFS)
    brief = next((item for item in briefs if item["translation_group"] == group), None)
    published = published_by_group().get(group, {})
    paths = {lang: DRAFTS / group / f"{lang}.json" for lang in LANGUAGES}
    missing = [lang for lang in LANGUAGES if lang not in published and not paths[lang].is_file()]
    if missing:
        raise SystemExit("Publication refusée : variantes manquantes : " + ", ".join(missing))
    drafts = {lang: read_json(path) for lang, path in paths.items() if path.is_file()}
    conflict = sorted(set(drafts) & set(published))
    if conflict:
        raise SystemExit("Publication refusée : un brouillon tenterait de remplacer une langue déjà publiée : " + ", ".join(conflict))
    articles = {lang: read_json(path) for lang, path in published.items()}
    articles.update(drafts)
    for lang, article in articles.items():
        if lang not in LANGUAGES:
            raise SystemExit(f"Langue non déclarée dans le groupe publié : {lang}")
        if article.get("translation_group") != group or article.get("language") != lang:
            raise SystemExit(f"Publication refusée : variante {lang} incohérente.")
        if brief:
            cta = brief_cta(brief, lang)
        else:
            cta = article.get("cta_url", "/en/markdown-to-pdf")
        validate_article(article, lang, group, cta, generated_draft=lang in drafts)
        destination = PUBLISHED / lang / f"{article['slug']}.json"
        if lang not in published and destination.exists():
            raise SystemExit(f"Publication refusée : le slug existe déjà ({lang}/{article['slug']}).")

    today = date.today().isoformat()
    destinations = {lang: PUBLISHED / lang / f"{article['slug']}.json" for lang, article in drafts.items()}
    for article in drafts.values():
        article["published_at"] = article.get("published_at", today)
        article["updated_at"] = today
    staged: list[tuple[Path, Path]] = []
    try:
        for lang, article in drafts.items():
            destination = destinations[lang]
            destination.parent.mkdir(parents=True, exist_ok=True)
            fd, temporary = tempfile.mkstemp(prefix=".publish-", dir=destination.parent)
            with os.fdopen(fd, "w", encoding="utf-8") as output:
                json.dump(article, output, ensure_ascii=False, indent=2)
                output.write("\n")
                output.flush()
                os.fsync(output.fileno())
            staged.append((Path(temporary), destination))
        for temporary, destination in staged:
            os.replace(temporary, destination)
    except Exception:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)
        raise
    for lang, destination in destinations.items():
        destination.chmod(0o644)
        print(f"Publié dans le site statique ({lang}) : {destination.relative_to(ROOT)}")
    print("Générez les pages, puis déployez pour rendre les routes localisées publiques.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    generate_parser = commands.add_parser("generate", help="générer toutes les variantes locales du prochain brouillon")
    generate_parser.add_argument("--group", help="translation_group précis, sinon le prochain brief")
    commands.add_parser("available", help="lister les groupes sans brouillon ni publication")
    publish_parser = commands.add_parser("publish", help="publier localement un groupe dont toutes les variantes ont été relues")
    publish_parser.add_argument("group", help="translation_group du brief")
    args = parser.parse_args()
    if args.command == "generate":
        generate(args.group)
    elif args.command == "available":
        list_available_groups()
    else:
        publish(args.group)


if __name__ == "__main__":
    main()
