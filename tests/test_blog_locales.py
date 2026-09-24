"""Regression checks for localized blog routes and translation backfill ordering."""

from __future__ import annotations

import json
import io
from pathlib import Path
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch
import xml.etree.ElementTree as ET

import scripts.blog_pipeline as pipeline
import scripts.build_public_pages as site_builder


ROOT = Path(__file__).resolve().parents[1]
LANGUAGES = pipeline.LANGUAGES


class LocalizedBlogRouteTests(unittest.TestCase):
    def test_builder_emits_routes_indexes_and_complete_hreflang_group(self) -> None:
        with tempfile.TemporaryDirectory(prefix="blog-locales-") as temporary:
            root = Path(temporary)
            content = root / "published"
            output = root / "generated"
            for language in LANGUAGES:
                article = {
                    "slug": f"route-check-{language}",
                    "translation_group": "route-check",
                    "language": language,
                    "title": f"Route check ({language})",
                    "description": f"A localized route validation for {language}.",
                    "published_at": "2026-09-23",
                    "article_markdown": "A localized guide body.",
                    "cta_url": "/en/markdown-to-pdf",
                }
                destination = content / language / f"{article['slug']}.json"
                destination.parent.mkdir(parents=True)
                destination.write_text(json.dumps(article), encoding="utf-8")

            with patch.object(site_builder, "CONTENT", content), patch.object(
                site_builder, "render_markdown", return_value="<p>Localized guide body.</p>"
            ):
                site_builder.build(output)

            for language in LANGUAGES:
                self.assertTrue((output / language / "index.html").is_file())
                page = output / language / f"route-check-{language}.html"
                self.assertIn(f'<html lang="{language}">', page.read_text(encoding="utf-8"))

            sitemap = ET.parse(output / "sitemap.xml").getroot()
            ns = {
                "sm": "http://www.sitemaps.org/schemas/sitemap/0.9",
                "xhtml": "http://www.w3.org/1999/xhtml",
            }
            urls = {
                item.find("sm:loc", ns).text: item
                for item in sitemap.findall("sm:url", ns)
            }
            source_url = f"{site_builder.SITE_URL}/fr/blog/route-check-fr"
            alternate_urls = {
                item.attrib["href"]
                for item in urls[source_url].findall("xhtml:link", ns)
                if item.attrib.get("hreflang") != "x-default"
            }
            self.assertEqual(
                alternate_urls,
                {
                    f"{site_builder.SITE_URL}/{language}/blog/route-check-{language}"
                    for language in LANGUAGES
                },
            )
            for kind in ("tools", "api"):
                landing_routes = {
                    language: site_builder.LANDING_PATHS[language][0 if kind == "tools" else 1]
                    for language in LANGUAGES
                }
                landing = urls[f"{site_builder.SITE_URL}{landing_routes['es']}"]
                localized_urls = {
                    item.attrib["hreflang"]: item.attrib["href"]
                    for item in landing.findall("xhtml:link", ns)
                }
                self.assertEqual(
                    localized_urls,
                    {
                        **{
                            language: f"{site_builder.SITE_URL}{route}"
                            for language, route in landing_routes.items()
                        },
                        "x-default": f"{site_builder.SITE_URL}{landing_routes[site_builder.DEFAULT_LANGUAGE]}",
                    },
                )

    def test_empty_locale_index_is_served_but_not_indexed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="blog-empty-locale-") as temporary:
            root = Path(temporary)
            content = root / "published"
            output = root / "generated"
            article = {
                "slug": "english-only-guide",
                "translation_group": "english-only-guide",
                "language": "en",
                "title": "An English guide",
                "description": "A published English guide for checking empty locale indexing.",
                "published_at": "2026-09-23",
                "article_markdown": "A useful English guide body.",
                "cta_url": "/en/markdown-to-pdf",
            }
            destination = content / "en" / "english-only-guide.json"
            destination.parent.mkdir(parents=True)
            destination.write_text(json.dumps(article), encoding="utf-8")

            with patch.object(site_builder, "CONTENT", content), patch.object(
                site_builder, "render_markdown", return_value="<p>A useful English guide body.</p>"
            ):
                site_builder.build(output)

            spanish_index = (output / "es" / "index.html").read_text(encoding="utf-8")
            english_index = (output / "en" / "index.html").read_text(encoding="utf-8")
            sitemap = (output / "sitemap.xml").read_text(encoding="utf-8")
            self.assertIn('name="robots" content="noindex,follow"', spanish_index)
            self.assertIn('name="robots" content="index,follow"', english_index)
            self.assertNotIn(f"{site_builder.SITE_URL}/es/blog", sitemap)
            self.assertNotIn('hreflang="es"', english_index)


class TranslationBackfillQueueTests(unittest.TestCase):
    def test_translation_links_use_the_target_blog_and_live_product_routes(self) -> None:
        markdown = (
            "[Converter](/fr/convertir-markdown-pdf) "
            "[API](/fr/api-generation-pdf) [Guides](/fr/blog) "
            "[Older guide](/en/blog/existing-guide)"
        )
        translated = pipeline.localize_internal_links(markdown, "es")
        self.assertIn("/es/convertidor-markdown-pdf", translated)
        self.assertIn("/es/api-generacion-pdf", translated)
        self.assertIn("/es/blog", translated)
        self.assertNotIn("/en/blog/existing-guide", translated)

    def test_brief_cta_uses_localized_product_pages_when_available(self) -> None:
        brief = {
            "translation_group": "localized-cta",
            "cta_fr": "/fr/api-generation-pdf",
            "cta_en": "/en/pdf-generation-api",
        }
        for language in LANGUAGES:
            self.assertEqual(
                pipeline.brief_cta(brief, language),
                pipeline.LOCALE_BY_CODE[language]["landing_paths"]["api"],
            )

    def test_article_links_resolve_to_matching_translation_before_blog_index_fallback(self) -> None:
        with tempfile.TemporaryDirectory(prefix="blog-link-localization-") as temporary:
            root = Path(temporary)
            published = root / "published"
            drafts = root / "drafts"
            for language, slug in (("fr", "guide-fr"), ("en", "guide-en")):
                path = published / language / f"{slug}.json"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    json.dumps({
                        "translation_group": "related-guide",
                        "language": language,
                        "slug": slug,
                    }),
                    encoding="utf-8",
                )
            draft = drafts / "related-guide" / "es.json"
            draft.parent.mkdir(parents=True)
            draft.write_text(
                json.dumps({
                    "translation_group": "related-guide",
                    "language": "es",
                    "slug": "guia-relacionada",
                }),
                encoding="utf-8",
            )

            with patch.object(pipeline, "PUBLISHED", published), patch.object(
                pipeline, "DRAFTS", drafts
            ):
                localized = pipeline.localize_internal_links(
                    "[Related guide](/fr/blog/guide-fr) [Unavailable](/de/blog/missing-guide)",
                    "es",
                )

            self.assertIn("/es/blog/guia-relacionada", localized)
            self.assertIn("/es/blog", localized)
            self.assertNotIn("/de/blog/missing-guide", localized)

    def test_existing_partial_groups_are_queued_until_all_locales_exist(self) -> None:
        with tempfile.TemporaryDirectory(prefix="blog-backfill-") as temporary:
            root = Path(temporary)
            published = root / "published"
            drafts = root / "drafts"
            briefs = root / "briefs.json"
            briefs.write_text(
                json.dumps([{"translation_group": "new-topic"}]), encoding="utf-8"
            )
            legacy = published / "legacy-group" / "en.json"
            legacy.parent.mkdir(parents=True)
            legacy.write_text(
                json.dumps({"translation_group": "legacy-group", "language": "en"}),
                encoding="utf-8",
            )

            with patch.object(pipeline, "PUBLISHED", published), patch.object(
                pipeline, "DRAFTS", drafts
            ), patch.object(pipeline, "BRIEFS", briefs):
                self.assertEqual(pipeline.pending_groups(), ["legacy-group", "new-topic"])
                for language in LANGUAGES:
                    if language == "en":
                        continue
                    draft = drafts / "legacy-group" / f"{language}.json"
                    draft.parent.mkdir(parents=True, exist_ok=True)
                    draft.write_text("{}", encoding="utf-8")
                self.assertEqual(pipeline.pending_groups(), ["new-topic"])


class EditorialAngleTests(unittest.TestCase):
    def test_overlapping_layout_topics_keep_distinct_draft_and_translation_angles(self) -> None:
        briefs = {item["translation_group"]: item for item in pipeline.read_json(pipeline.BRIEFS)}
        automated = briefs["automatically-check-pdf-layout"]
        inspection = briefs["inspect-multipage-pdf-layout"]

        automated_prompt = json.loads(pipeline.draft_prompt(automated, "fr")[1])
        inspection_prompt = json.loads(pipeline.draft_prompt(inspection, "fr")[1])
        self.assertIn("POST /api/layout", automated_prompt["editorial_angle"])
        self.assertIn("vérification humaine", inspection_prompt["editorial_angle"])
        self.assertNotEqual(automated_prompt["editorial_angle"], inspection_prompt["editorial_angle"])

        source = {
            "title": "Review a multi-page PDF report",
            "description": "A practical review workflow for a generated PDF.",
            "article_markdown": "## Review\n\nInspect the affected page.",
        }
        translated_prompt = json.loads(
            pipeline.translate_prompt(inspection, source, "es", "/es/api-generacion-pdf")[1]
        )
        self.assertEqual(
            translated_prompt["editorial_angle_reference"],
            inspection["editorial_angle_en"],
        )


class DraftPublicationTests(unittest.TestCase):
    def test_complete_mocked_group_is_staged_as_publication_candidates(self) -> None:
        with tempfile.TemporaryDirectory(prefix="blog-publication-") as temporary:
            root = Path(temporary)
            drafts = root / "drafts"
            published = root / "published"
            briefs = root / "briefs.json"
            brief = {
                "translation_group": "workflow-check",
                "primary_query_fr": "créer un rapport PDF avec une API",
                "primary_query_en": "create a PDF report with an API",
                "search_intent_fr": "Créer puis vérifier un rapport PDF.",
                "search_intent_en": "Create and review a generated PDF report.",
                "audience_fr": "Équipe produit",
                "audience_en": "Product team",
                "cta_fr": "/fr/api-generation-pdf",
                "cta_en": "/en/pdf-generation-api",
                "product_facts": ["The API generates PDFs from Markdown."],
            }
            briefs.write_text(json.dumps([brief]), encoding="utf-8")

            def mock_llm(_system: str, prompt: str) -> dict:
                request = json.loads(prompt)
                language = request.get("language") or request["target_language"]
                cta = request["required_action_link"]
                prose = "Review each page and compare the rendered document with the requested output. " * 20
                markdown = "\n\n".join(
                    f"## Workflow step {number}\n\n{prose}"
                    for number in range(1, 5)
                ) + f"\n\n```bash\ncurl -X POST {cta}\n```\n\n[Continue with the API]({cta})"
                return {
                    "slug": f"workflow-check-{language}",
                    "title": f"PDF workflow check ({language})",
                    "description": "A practical guide to create and inspect an application PDF report.",
                    "article_markdown": markdown,
                }

            with patch.object(pipeline, "ROOT", root), patch.object(
                pipeline, "DRAFTS", drafts
            ), patch.object(
                pipeline, "PUBLISHED", published
            ), patch.object(pipeline, "BRIEFS", briefs), patch.object(
                pipeline, "call_llm", side_effect=mock_llm
            ), redirect_stdout(io.StringIO()):
                pipeline.generate("workflow-check")
                pipeline.publish("workflow-check")

                by_language = pipeline.published_by_group()["workflow-check"]
                self.assertEqual(set(by_language), set(LANGUAGES))
                self.assertEqual(pipeline.pending_groups(), [])
                for language, path in by_language.items():
                    article = json.loads(path.read_text(encoding="utf-8"))
                    self.assertEqual(article["language"], language)
                    self.assertEqual(article["translation_group"], "workflow-check")
                    self.assertIn(article["cta_url"], article["article_markdown"])

    def test_legacy_article_bounds_remain_compatible(self) -> None:
        cta = "/en/pdf-generation-api"
        legacy = {
            "slug": "older-guide",
            "title": "Older guide",
            "description": "A practical older article retained while localized variants are added.",
            "article_markdown": (
                "## First section\n\n" + "Useful information about the workflow. " * 100
                + "\n\n## Second section\n\nMore review details."
                + "\n\n## Third section\n\n[Continue](/en/pdf-generation-api)"
            ),
        }
        pipeline.validate_article(dict(legacy), "en", "older-guide", cta)
        with self.assertRaisesRegex(SystemExit, "600–1200"):
            pipeline.validate_article(dict(legacy), "en", "older-guide", cta, generated_draft=True)

    def test_generated_article_requires_a_concrete_example(self) -> None:
        cta = "/en/pdf-generation-api"
        prose = "Review every rendered page and compare it against the expected layout before delivery. " * 15
        article = {
            "slug": "example-required",
            "title": "A sufficiently detailed generated article",
            "description": "A practical guide to reviewing pages and improving a generated PDF workflow.",
            "article_markdown": "\n\n".join(
                f"## Workflow section {number}\n\n{prose}" for number in range(1, 5)
            ) + f"\n\n[Continue]({cta})",
        }
        with self.assertRaisesRegex(SystemExit, "exemple concret"):
            pipeline.validate_article(article, "en", "example-required", cta, generated_draft=True)


if __name__ == "__main__":
    unittest.main()
