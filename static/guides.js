/* ==========================================================================
   md-to-pdf — guides
   The part of the documentation the endpoint reference cannot carry: what the
   engine is capable of, what ships with it, and how features combine into a
   real document. Every code block is runnable as-is; every example uses a
   feature that actually exists in this build.

   Content is bilingual in place ({en, fr}) rather than in a key catalogue: a
   guide is prose, and prose translated three files away drifts within a month.
   ========================================================================== */
"use strict";

// ══════════════════════════════════════════════════════════ contenu

const GUIDES = [

  // ───────────────────────────────────────────── démarrer ─────────────────

  {
    key: "overview",
    group: { en: "Start here", fr: "Démarrer" },
    icon: "M4 4h11l5 5v11H4zM14 4v5h5",
    title: { en: "What the engine does", fr: "Ce que fait le moteur" },
    lede: {
      en: "One container turns text into a finished document: paginated, branded, illustrated, censored where it must be, and checked before it leaves.",
      fr: "Un seul conteneur transforme du texte en document fini : paginé, à votre marque, illustré, censuré là où il le faut, et vérifié avant de sortir.",
    },
    blocks: [
      ["p", {
        en: "Three input formats, one rendering pipeline, one contract. Whatever you send — Markdown, raw HTML or a Tera template with its data — the document goes through the same expansion, the same stylesheet assembly and the same engine, so a feature documented here works on all three.",
        fr: "Trois formats d'entrée, un seul pipeline de rendu, un seul contrat. Quoi que vous envoyiez — Markdown, HTML brut ou template Tera avec ses données — le document traverse la même expansion, le même assemblage de feuilles de style et le même moteur : une fonctionnalité documentée ici marche sur les trois.",
      }],

      ["h", { en: "The pipeline, in order", fr: "Le pipeline, dans l'ordre" }],
      ["table",
        [{ en: "Stage", fr: "Étape" }, { en: "What happens", fr: "Ce qui se passe" }, { en: "Guide", fr: "Guide" }],
        [
          [{ en: "1 · Censoring", fr: "1 · Censure" },
           { en: "<code>CENSOR</code> tags and regions are <b>removed from the source</b>, before anything else sees it.", fr: "Les tags et régions <code>CENSOR</code> sont <b>retirés de la source</b>, avant que quoi que ce soit d'autre ne la voie." },
           "→ censor"],
          [{ en: "2 · Blocks", fr: "2 · Blocs" },
           { en: "<code>```chart</code> and <code>```mermaid</code> fences become inline SVG.", fr: "Les blocs <code>```chart</code> et <code>```mermaid</code> deviennent du SVG inline." },
           "→ charts"],
          [{ en: "3 · Cover", fr: "3 · Couverture" },
           { en: "<code>options.cover</code> prepends a cover page, styled by the theme.", fr: "<code>options.cover</code> ajoute une page de garde, habillée par le thème." },
           "→ themes"],
          [{ en: "4 · Styling", fr: "4 · Habillage" },
           { en: "<code>default.css</code>, then the theme, then your <code>css</code>, then the layout <code>options</code>.", fr: "<code>default.css</code>, puis le thème, puis votre <code>css</code>, puis les <code>options</code> de mise en page." },
           "→ templates"],
          [{ en: "5 · Render", fr: "5 · Rendu" },
           { en: "pandoc and/or WeasyPrint produce the PDF. Identical inputs hit the cache.", fr: "pandoc et/ou WeasyPrint produisent le PDF. Des entrées identiques touchent le cache." },
           "→ limits"],
          [{ en: "6 · Audit", fr: "6 · Contrôle" },
           { en: "<code>options.autolayout</code> inspects the result and re-renders it if that improves it.", fr: "<code>options.autolayout</code> inspecte le résultat et le re-rend si cela l'améliore." },
           "→ layout"],
        ],
      ],

      ["h", { en: "Sources and destinations", fr: "Sources et destinations" }],
      ["cards", [
        { t: { en: "Markdown → PDF", fr: "Markdown → PDF" },
          d: { en: "<code>POST /api/convert</code>. pandoc, tables, footnotes, table of contents, code highlighting.", fr: "<code>POST /api/convert</code>. pandoc, tableaux, notes de bas de page, sommaire, coloration du code." } },
        { t: { en: "HTML → PDF", fr: "HTML → PDF" },
          d: { en: "<code>POST /api/html-to-pdf</code>. For markup you already build client-side.", fr: "<code>POST /api/html-to-pdf</code>. Pour du HTML que vous construisez déjà côté client." } },
        { t: { en: "Template → PDF", fr: "Template → PDF" },
          d: { en: "<code>POST /api/render</code>. A Tera (Jinja2-like) template plus a JSON context — the invoice case.", fr: "<code>POST /api/render</code>. Un template Tera (proche de Jinja2) et un contexte JSON — le cas facture." } },
        { t: { en: "→ PNG pages", fr: "→ Pages PNG" },
          d: { en: "<code>POST /api/preview</code>. One page, a range, or a contact sheet.", fr: "<code>POST /api/preview</code>. Une page, une plage, ou une planche-contact." } },
        { t: { en: "→ Merged, watermarked, encrypted", fr: "→ Fusionné, filigrané, chiffré" },
          d: { en: "<code>merge</code>, <code>watermark</code>, <code>protect</code> chain on saved PDFs.", fr: "<code>merge</code>, <code>watermark</code>, <code>protect</code> s'enchaînent sur des PDFs sauvegardés." } },
        { t: { en: "→ Redacted, compared", fr: "→ Caviardé, comparé" },
          d: { en: "<code>redact</code> paints out and flattens; <code>diff</code> compares two renders pixel by pixel.", fr: "<code>redact</code> masque et aplatit ; <code>diff</code> compare deux rendus pixel par pixel." } },
      ]],

      ["h", { en: "The response contract", fr: "Le contrat de réponse" }],
      ["p", {
        en: "Every endpoint that produces a PDF returns the <b>binary PDF</b> — unless you send both <code>client_id</code> and <code>pdf_name</code>, in which case the file is saved and you get a JSON download URL instead. Nothing else changes.",
        fr: "Tout endpoint qui produit un PDF renvoie le <b>PDF binaire</b> — sauf si vous envoyez à la fois <code>client_id</code> et <code>pdf_name</code>, auquel cas le fichier est sauvegardé et vous recevez une URL de téléchargement JSON. Rien d'autre ne change.",
      }],
      ["code", "json", '{\n  "download_url": "/download/demo-client/report-2026.pdf",\n  "cached": true,\n  "warnings": [{ "kind": "mermaid", "message": "…", "line": 42 }],\n  "layout": { "pages": 12, "score": 94, "issues": [], "passes": 1 }\n}'],
      ["p", {
        en: "The three optional fields appear only when they apply — <code>cached</code> when the PDF came from the render cache, <code>warnings</code> when a chart or diagram could not be drawn, <code>layout</code> when you asked for <code>autolayout</code>. A client written before they existed still parses the response.",
        fr: "Les trois champs optionnels n'apparaissent que lorsqu'ils s'appliquent — <code>cached</code> quand le PDF vient du cache de rendu, <code>warnings</code> quand un graphique ou un diagramme n'a pas pu être dessiné, <code>layout</code> quand vous avez demandé <code>autolayout</code>. Un client écrit avant leur existence continue de lire la réponse.",
      }],
      ["try", "convert", { en: "Send a first document", fr: "Envoyer un premier document" }],
    ],
  },

  {
    key: "quickstart",
    group: { en: "Start here", fr: "Démarrer" },
    icon: "M13 2L3 14h7l-1 8 10-12h-7z",
    title: { en: "First call in three minutes", fr: "Premier appel en trois minutes" },
    lede: {
      en: "No SDK, no build step: HTTP, JSON, and a token in a header.",
      fr: "Aucun SDK, aucune étape de build : de l'HTTP, du JSON, et un token dans un en-tête.",
    },
    blocks: [
      ["h", { en: "Authenticate", fr: "S'authentifier" }],
      ["p", {
        en: 'Every <code>/api/*</code> route needs your token, as <code>X-API-Key</code> or <code>Authorization: Bearer</code>. Two routes stay open: <code>GET /api/health</code> (the container probe) and <code>GET /download/…</code> (fetching a PDF that was already produced).',
        fr: 'Toutes les routes <code>/api/*</code> demandent votre token, en <code>X-API-Key</code> ou en <code>Authorization: Bearer</code>. Deux routes restent ouvertes : <code>GET /api/health</code> (la sonde du conteneur) et <code>GET /download/…</code> (récupérer un PDF déjà produit).',
      }],
      ["code", "shell", {
        en: "# The token is given to you by the team — never hard-coded in a repository\nexport MDTOPDF_KEY='your-token'\n\ncurl -s $BASE/api/health\n# {\"status\":\"ok\",\"version\":\"0.2.0\",\"engines\":[\"weasyprint\",\"wkhtmltopdf\",\"pdflatex\"]}",
        fr: "# Le token vous est fourni par l'équipe — jamais en dur dans un dépôt\nexport MDTOPDF_KEY='votre-token'\n\ncurl -s $BASE/api/health\n# {\"status\":\"ok\",\"version\":\"0.2.0\",\"engines\":[\"weasyprint\",\"wkhtmltopdf\",\"pdflatex\"]}",
      }],

      ["h", { en: "Get a PDF back", fr: "Récupérer un PDF" }],
      ["code", "shell", {
        en: "curl -X POST \"$BASE/api/convert\" \\\n  -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"markdown\": \"# Hello\\n\\nA **document**.\", \"options\": {\"page_numbers\": true}}' \\\n  --output document.pdf",
        fr: "curl -X POST \"$BASE/api/convert\" \\\n  -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"markdown\": \"# Bonjour\\n\\nUn **document**.\", \"options\": {\"page_numbers\": true}}' \\\n  --output document.pdf",
      }],

      ["h", { en: "Or keep it on the server", fr: "Ou le garder sur le serveur" }],
      ["p", {
        en: "Add <code>client_id</code> and <code>pdf_name</code> and the answer becomes a URL. This is what you want whenever the PDF will be chained into <code>merge</code>, <code>watermark</code>, <code>protect</code>, <code>redact</code> or <code>diff</code> — those endpoints take download paths, not uploads.",
        fr: "Ajoutez <code>client_id</code> et <code>pdf_name</code> et la réponse devient une URL. C'est ce qu'il vous faut dès que le PDF sera enchaîné dans <code>merge</code>, <code>watermark</code>, <code>protect</code>, <code>redact</code> ou <code>diff</code> — ces endpoints prennent des chemins de téléchargement, pas des envois de fichier.",
      }],
      ["code", "shell", {
        en: "curl -X POST \"$BASE/api/convert\" \\\n  -H \"X-API-Key: $MDTOPDF_KEY\" -H 'Content-Type: application/json' \\\n  -d '{\"markdown\": \"# Hello\", \"client_id\": \"demo\", \"pdf_name\": \"hello\"}'\n# {\"download_url\":\"/download/demo/hello.pdf\"}",
        fr: "curl -X POST \"$BASE/api/convert\" \\\n  -H \"X-API-Key: $MDTOPDF_KEY\" -H 'Content-Type: application/json' \\\n  -d '{\"markdown\": \"# Bonjour\", \"client_id\": \"demo\", \"pdf_name\": \"bonjour\"}'\n# {\"download_url\":\"/download/demo/bonjour.pdf\"}",
      }],
      ["note", "info", {
        en: "<code>client_id</code> and <code>pdf_name</code> are plain names — <code>[A-Za-z0-9._-]</code>, 128 characters at most, not starting with a dot. They build a path on disk, so anything else is refused with a 400.",
        fr: "<code>client_id</code> et <code>pdf_name</code> sont des noms simples — <code>[A-Za-z0-9._-]</code>, 128 caractères au plus, ne commençant pas par un point. Ils construisent un chemin sur le disque : tout le reste est refusé en 400.",
      }],

      ["h", { en: "Layout options", fr: "Options de mise en page" }],
      ["p", {
        en: "Every field of <code>options</code> is optional, and an options object that does not mention a field renders exactly as it did before that field existed. Nothing here is breaking.",
        fr: "Chaque champ d'<code>options</code> est optionnel, et un objet options qui ne mentionne pas un champ rend exactement comme avant l'existence de ce champ. Rien ici n'est cassant.",
      }],
      ["table",
        [{ en: "Field", fr: "Champ" }, { en: "Values", fr: "Valeurs" }, { en: "Effect", fr: "Effet" }],
        [
          ["paper_size", "a4 · a3 · letter", { en: "Page format, <code>a4</code> by default.", fr: "Format de page, <code>a4</code> par défaut." }],
          ["orientation", "portrait · landscape", { en: "Wide tables usually want <code>landscape</code>.", fr: "Les tableaux larges veulent en général <code>landscape</code>." }],
          ["margins", '{top, right, bottom, left}', { en: "Any CSS length: <code>\"2cm\"</code>, <code>\"18mm\"</code>.", fr: "Toute longueur CSS : <code>\"2cm\"</code>, <code>\"18mm\"</code>." }],
          ["page_numbers", "true · false", { en: "Numbering in the page footer.", fr: "Numérotation en pied de page." }],
          ["page_number_format", { en: "CSS <code>content</code>", fr: "<code>content</code> CSS" }, { en: 'e.g. <code>counter(page) " / " counter(pages)</code>.', fr: 'ex. <code>counter(page) " / " counter(pages)</code>.' }],
          ["toc / toc_depth", "true · 1–6", { en: "Table of contents built from the headings.", fr: "Sommaire construit à partir des titres." }],
          ["watermark", { en: "text", fr: "texte" }, { en: "Diagonal CSS watermark, drawn at render time.", fr: "Filigrane CSS en diagonale, dessiné au rendu." }],
          ["theme", '"report" · "report@1"', { en: "Named, versioned brand kit.", fr: "Kit de marque nommé et versionné." }],
          ["cover", "{title, subtitle, logo, date}", { en: "Cover page, styled by the theme.", fr: "Page de garde, habillée par le thème." }],
          ["censor_label", { en: "text", fr: "texte" }, { en: "Wording of censored blocks with no named level.", fr: "Libellé des blocs censurés sans niveau nommé." }],
          ["charts", "false", { en: "Turns off <code>chart</code> <b>and</b> <code>mermaid</code> expansion.", fr: "Coupe l'expansion des blocs <code>chart</code> <b>et</b> <code>mermaid</code>." }],
          ["autolayout", "true", { en: "Audits the output and corrects it. Costs a full extra render.", fr: "Analyse la sortie et la corrige. Coûte un rendu complet de plus." }],
        ],
      ],

      ["h", { en: "Errors and tracing", fr: "Erreurs et traçabilité" }],
      ["p", {
        en: 'Errors are always JSON: <code>{"error": "…", "details": "…"}</code>. Every response carries an <code>X-Request-Id</code>; send your own on the way in and it is echoed back and attached to every log event — that is what ties a support ticket to what the service actually did.',
        fr: 'Les erreurs sont toujours en JSON : <code>{"error": "…", "details": "…"}</code>. Chaque réponse porte un <code>X-Request-Id</code> ; envoyez le vôtre à l\'aller et il est renvoyé et attaché à chaque événement de log — c\'est ce qui relie un ticket de support à ce que le service a réellement fait.',
      }],
      ["try", "convert", { en: "Open the console on /api/convert", fr: "Ouvrir la console sur /api/convert" }],
    ],
  },

  // ───────────────────────────────────────────── habillage ────────────────

  {
    key: "themes",
    group: { en: "Look and feel", fr: "Habillage" },
    icon: "M12 3a9 9 0 100 18 3 3 0 002-5 3 3 0 013-3h2a3 3 0 003-3 9 9 0 00-10-7z",
    title: { en: "Themes and cover pages", fr: "Thèmes et pages de garde" },
    lede: {
      en: "Three brand kits ship with the service. They are named, versioned and immutable — a contract rendered with report@1 still comes out identical next year.",
      fr: "Trois kits de marque sont livrés avec le service. Ils sont nommés, versionnés et immuables — un contrat rendu avec report@1 sortira identique l'an prochain.",
    },
    blocks: [
      ["p", {
        en: 'A theme is a whole stylesheet, not a colour: typography, headings, tables, callouts, footnotes, table of contents and — when the kit ships one — a cover page. Ask for it with <code>options.theme</code>, as <code>"name"</code> for the latest version or <code>"name@2"</code> to pin one.',
        fr: 'Un thème est une feuille de style complète, pas une couleur : typographie, titres, tableaux, encadrés, notes de bas de page, sommaire et — quand le kit en livre une — une page de garde. Demandez-le avec <code>options.theme</code>, sous la forme <code>"nom"</code> pour la dernière version ou <code>"nom@2"</code> pour en épingler une.',
      }],

      ["h", { en: "Installed kits", fr: "Kits installés" }],
      ["p", {
        en: "Pulled live from <code>GET /api/themes</code> on the service you are pointing at, previews included. What you see below is what this deployment will actually render.",
        fr: "Chargés en direct depuis <code>GET /api/themes</code> sur le service que vous ciblez, aperçus compris. Ce que vous voyez ci-dessous est ce que ce déploiement rendra réellement.",
      }],
      ["themes"],

      ["code", "json", '{\n  "markdown": "# Quarterly report\\n\\nBody text.",\n  "options": {\n    "theme": "report@1",\n    "toc": true,\n    "page_numbers": true,\n    "cover": {\n      "title": "Quarterly report",\n      "subtitle": "Q4 2026 — internal",\n      "logo": "https://example.com/logo.png",\n      "date": "31 December 2026"\n    }\n  }\n}'],

      ["h", { en: "Utility classes", fr: "Classes utilitaires" }],
      ["p", {
        en: "Every kit understands the same two pandoc div conventions, so a document can move from one theme to another without being rewritten.",
        fr: "Chaque kit comprend les deux mêmes conventions de div pandoc : un document peut donc passer d'un thème à l'autre sans être réécrit.",
      }],
      ["code", "markdown", "::: success\n**Target met.** Median render time is back under two seconds.\n:::\n\n::: warning\nThe figures for December are still provisional.\n:::\n\n::: wide\n| A wide table | rendered | at a reduced | body size |\n|--------------|----------|--------------|-----------|\n| …            | …        | …            | …         |\n:::"],
      ["table",
        [{ en: "Class", fr: "Classe" }, { en: "Use", fr: "Usage" }],
        [
          ["<code>wide</code>", { en: "A table with many columns — rendered at a reduced body size so it still fits.", fr: "Un tableau à nombreuses colonnes — rendu à une taille de corps réduite pour qu'il tienne encore." }],
          ["<code>success</code> · <code>warning</code> · <code>danger</code> · <code>info</code> · <code>note</code>", { en: "Callout boxes, coloured by the theme's tokens.", fr: "Encadrés, colorés par les tokens du thème." }],
        ],
      ],

      ["h", { en: "Cover pages", fr: "Pages de garde" }],
      ["p", {
        en: "<code>options.cover</code> inserts a cover page in front of the document. When the theme ships a <code>cover.html</code> it is used and styled by the kit; otherwise a built-in, self-contained template takes over so a cover asked for without a theme is still a cover page. Every value is escaped, and a <code>logo</code> URL goes through the same fetch policy as any other asset in the document.",
        fr: "<code>options.cover</code> insère une page de garde devant le document. Quand le thème livre un <code>cover.html</code>, c'est lui qui est utilisé et habillé par le kit ; sinon un template intégré et autonome prend le relais, de sorte qu'une couverture demandée sans thème reste une page de garde. Chaque valeur est échappée, et une URL de <code>logo</code> passe par la même politique de récupération que n'importe quel autre média du document.",
      }],
      ["p", {
        en: "Preview a kit's cover without rendering anything of your own: <code>GET /api/themes/report/latest/preview.png?cover=true</code>.",
        fr: "Prévisualisez la couverture d'un kit sans rien rendre de votre côté : <code>GET /api/themes/report/latest/preview.png?cover=true</code>.",
      }],

      ["h", { en: "Adding a kit", fr: "Ajouter un kit" }],
      ["p", {
        en: "A theme is a directory on the server, read once at startup. A kit that does not validate is dropped with a warning rather than keeping the service from starting.",
        fr: "Un thème est un répertoire sur le serveur, lu une seule fois au démarrage. Un kit qui ne valide pas est ignoré avec un avertissement plutôt que d'empêcher le service de démarrer.",
      }],
      ["code", "shell", {
        en: "themes/\n└── acme/\n    └── v1/                 # exactly v1, v2, … — v01 is refused\n        ├── theme.json      # required: label, description, fonts, colors\n        ├── theme.css       # required, non-empty, 512 KB max\n        ├── cover.html      # optional — Tera: title, subtitle, logo, date\n        ├── header.html     # optional — used unless the request overrides it\n        └── footer.html     # optional",
        fr: "themes/\n└── acme/\n    └── v1/                 # exactement v1, v2, … — v01 est refusé\n        ├── theme.json      # requis : label, description, fonts, colors\n        ├── theme.css       # requis, non vide, 512 Ko max\n        ├── cover.html      # optionnel — Tera : title, subtitle, logo, date\n        ├── header.html     # optionnel — utilisé sauf si la requête l'écrase\n        └── footer.html     # optionnel",
      }],
      ["code", "json", '{\n  "label": "ACME",\n  "description": "ACME brand kit: sans-serif, red accent, dark table headers.",\n  "author": "ACME",\n  "fonts": ["TeX Gyre Heros", "DejaVu Sans Mono"],\n  "colors": { "accent": "#c0322f", "ink": "#101a2b", "line": "#dbe3ef" }\n}'],
      ["note", "warn", {
        en: "<b>Themes are immutable.</b> The render cache key contains <code>name@version</code>, so changing a published <code>theme.css</code> in place does not invalidate anything — the old PDF keeps being served until the cache TTL expires. Any visual change means a new <code>v&lt;n+1&gt;</code> directory.",
        fr: "<b>Les thèmes sont immuables.</b> La clé du cache de rendu contient <code>nom@version</code> : modifier un <code>theme.css</code> déjà publié n'invalide rien — l'ancien PDF continue d'être servi jusqu'à expiration du TTL. Tout changement visuel implique un nouveau répertoire <code>v&lt;n+1&gt;</code>.",
      }],
      ["p", {
        en: "The fonts a kit declares all ship inside the image. Naming a family that is not installed does not fail the render — it silently falls back, which is exactly the kind of difference nobody notices until the contract is printed.",
        fr: "Les polices qu'un kit déclare sont toutes présentes dans l'image. Nommer une famille non installée ne fait pas échouer le rendu — elle retombe silencieusement sur une autre, exactement le genre d'écart que personne ne remarque avant l'impression du contrat.",
      }],
      ["try", "themes", { en: "List the themes", fr: "Lister les thèmes" }],
    ],
  },

  {
    key: "templates",
    group: { en: "Look and feel", fr: "Habillage" },
    icon: "M4 5h16M4 12h16M4 19h10",
    title: { en: "Headers, footers and CSS", fr: "En-têtes, pieds de page et CSS" },
    lede: {
      en: "Four ways to reach the same stylesheet, in a defined order. Knowing that order is the difference between styling a document and fighting it.",
      fr: "Quatre manières d'atteindre la même feuille de style, dans un ordre défini. Connaître cet ordre, c'est la différence entre habiller un document et se battre avec lui.",
    },
    blocks: [
      ["h", { en: "Order of application", fr: "Ordre d'application" }],
      ["p", {
        en: "Stylesheets are concatenated, later wins on equal specificity:",
        fr: "Les feuilles de style sont concaténées, la dernière l'emporte à spécificité égale :",
      }],
      ["code", "text", "templates/default.css  →  theme.css  →  your `css` field  →  `options` (paper, margins, numbering, watermark)"],
      ["p", {
        en: "<code>templates/default.css</code> is the floor every document stands on: readable typography, tables, page-break rules, and the geometry of censored blocks. It is applied even when no theme is asked for, which is why an unstyled request still comes out looking deliberate.",
        fr: "<code>templates/default.css</code> est le socle sur lequel repose tout document : typographie lisible, tableaux, règles de saut de page, et la géométrie des blocs censurés. Elle est appliquée même sans thème demandé, ce qui explique qu'une requête sans habillage sorte quand même avec une allure voulue.",
      }],

      ["h", { en: "Headers and footers", fr: "En-têtes et pieds de page" }],
      ["p", {
        en: "Four fields, resolved in a strict priority: inline HTML beats a named template, and a named template beats the fragment the theme ships. That order is what lets one document override a brand kit for one page without abandoning the kit.",
        fr: "Quatre champs, résolus selon une priorité stricte : l'HTML en ligne l'emporte sur un template nommé, et un template nommé l'emporte sur le fragment livré par le thème. C'est cet ordre qui permet à un document d'écraser un kit de marque pour une page sans abandonner le kit.",
      }],
      ["table",
        [{ en: "Field", fr: "Champ" }, { en: "Priority", fr: "Priorité" }, { en: "Content", fr: "Contenu" }],
        [
          ["<code>header_html</code> / <code>footer_html</code>", "1", { en: "Raw HTML, sent in the request.", fr: "HTML brut, envoyé dans la requête." }],
          ["<code>header_template</code> / <code>footer_template</code>", "2", { en: "The name of a file in <code>templates/</code>, e.g. <code>header.html</code>.", fr: "Le nom d'un fichier de <code>templates/</code>, par ex. <code>header.html</code>." }],
          [{ en: "The theme's <code>header.html</code> / <code>footer.html</code>", fr: "Les <code>header.html</code> / <code>footer.html</code> du thème" }, "3", { en: "Used when the request names neither.", fr: "Utilisés quand la requête n'en nomme aucun." }],
        ],
      ],

      ["h", { en: "Templates shipped", fr: "Templates livrés" }],
      ["p", {
        en: "These sit in <code>templates/</code> and are ready to be named in a request. They are examples as much as they are assets — copy one, change the logo and the wording, drop it next to them.",
        fr: "Ils se trouvent dans <code>templates/</code> et sont prêts à être nommés dans une requête. Ce sont autant des exemples que des ressources — copiez-en un, changez le logo et le libellé, posez-le à côté.",
      }],
      ["table",
        [{ en: "File", fr: "Fichier" }, { en: "What it draws", fr: "Ce qu'il dessine" }],
        [
          ["<code>header.html</code>", { en: "The neutral starting point: a logo slot (empty <code>src</code>) and a company name, on a ruled line. Copy this one.", fr: "Le point de départ neutre : un emplacement de logo (<code>src</code> vide) et un nom de société, sur un filet. C'est celui à copier." }],
          ["<code>footer.html</code>", { en: "A centred page number above a rule, fixed to the bottom of the page.", fr: "Un numéro de page centré au-dessus d'un filet, fixé en bas de page." }],
          ["<code>mdp.header.html</code>", { en: "A filled-in example: MDP Data Protection branding.", fr: "Un exemple rempli : identité MDP Data Protection." }],
          ["<code>symexo.header.html</code>", { en: "A filled-in example: Symexo branding.", fr: "Un exemple rempli : identité Symexo." }],
          ["<code>default.css</code>", { en: "Not a header — the base stylesheet described above.", fr: "Pas un en-tête — la feuille de style de base décrite plus haut." }],
        ],
      ],
      ["code", "json", '{\n  "markdown": "# Service agreement",\n  "header_template": "header.html",\n  "footer_template": "footer.html",\n  "options": { "page_numbers": true }\n}'],
      ["note", "info", {
        en: "For page numbering, prefer <code>options.page_numbers</code> over a custom footer: it uses the CSS paged-media counters the engine already sets up, and it survives a change of theme. A footer template is for what numbering cannot express — a legal mention, a document reference, a classification level.",
        fr: "Pour la numérotation, préférez <code>options.page_numbers</code> à un pied de page maison : il s'appuie sur les compteurs CSS paged-media que le moteur met déjà en place, et il survit à un changement de thème. Un template de pied de page sert à ce que la numérotation ne peut pas exprimer — une mention légale, une référence de document, un niveau de classification.",
      }],
      ["note", "warn", {
        en: "HTML headers and footers only make sense for the HTML-based engines. With <code>engine: \"pdflatex\"</code> they are ignored and the service says so in its logs rather than silently producing a document without them.",
        fr: "Les en-têtes et pieds de page HTML n'ont de sens que pour les moteurs HTML. Avec <code>engine: \"pdflatex\"</code> ils sont ignorés, et le service le dit dans ses logs plutôt que de produire silencieusement un document sans eux.",
      }],

      ["h", { en: "Your own CSS", fr: "Votre propre CSS" }],
      ["p", {
        en: "The <code>css</code> field is appended last, so it wins. Everything CSS Paged Media offers is available — running headers, named pages, breaks:",
        fr: "Le champ <code>css</code> est ajouté en dernier : il l'emporte. Tout ce qu'offre CSS Paged Media est disponible — en-têtes courants, pages nommées, sauts :",
      }],
      ["code", "text", '@page { size: A4; margin: 25mm 20mm; }\n@page :first { margin-top: 60mm; }\n\nh1 { break-before: page; }\ntable { break-inside: avoid; }\n\n/* A running footer on every page but the first */\n@page { @bottom-center { content: "Confidential — do not circulate"; font-size: 8pt; } }\n@page :first { @bottom-center { content: ""; } }'],
      ["try", "convert", { en: "Try a header template", fr: "Essayer un template d'en-tête" }],
    ],
  },

  // ───────────────────────────────────────────── contenu ──────────────────

  {
    key: "censor",
    group: { en: "Document content", fr: "Contenu du document" },
    icon: "M3 3l18 18M10.6 10.6a3 3 0 004.2 4.2M9.4 5.2A9.6 9.6 0 0112 5c5 0 9 4.5 9 7a12 12 0 01-2.2 3.2M6.2 6.2A12.6 12.6 0 003 12c0 2.5 4 7 9 7 1.4 0 2.7-.3 3.9-.9",
    title: { en: "Censoring: hiding what must not ship", fr: "Censure : masquer ce qui ne doit pas partir" },
    lede: {
      en: "The censored text is removed from the source before pandoc ever sees it. It cannot be selected, searched or extracted from the PDF, because it is not in the PDF.",
      fr: "Le texte censuré est retiré de la source avant même que pandoc ne la voie. Il ne peut être ni sélectionné, ni recherché, ni extrait du PDF, parce qu'il n'est pas dans le PDF.",
    },
    blocks: [
      ["p", {
        en: "This is the feature the fork was built for: publishing one document to two audiences. The premium reader gets everything; everyone else gets the same document with a sized, labelled hole where the paid content was — which is a sales pitch, not an error page.",
        fr: "C'est la fonctionnalité pour laquelle le fork a été construit : publier un même document pour deux audiences. Le lecteur premium a tout ; les autres ont le même document avec un trou dimensionné et libellé à la place du contenu payant — ce qui est un argument commercial, pas une page d'erreur.",
      }],

      ["h", { en: "The tags", fr: "Les tags" }],
      ["table",
        [{ en: "Tag", fr: "Tag" }, { en: "Effect", fr: "Effet" }],
        [
          ["<code>{{CENSOR}}</code> / <code>&lt;CENSOR&gt;</code>", { en: "Point replacement: the historical blurred block. Unchanged since day one.", fr: "Remplacement ponctuel : le bloc flouté historique. Inchangé depuis le premier jour." }],
          ["<code>{{CENSOR:premium}}</code>", { en: "The same, labelled with that level.", fr: "Le même, libellé avec ce niveau." }],
          ["<code>{{CENSOR:start}}</code> … <code>{{CENSOR:end}}</code>", { en: "Removes the region; the block is <b>sized after what was removed</b>, so the page keeps its shape.", fr: "Retire la région ; le bloc est <b>dimensionné d'après ce qui a été retiré</b>, la page garde donc sa forme." }],
          ["<code>{{CENSOR:teaser=25}}</code> … <code>:end</code>", { en: "Keeps the first 25 words, removes the rest. The teaser.", fr: "Garde les 25 premiers mots, retire le reste. L'accroche." }],
          ["<code>{{CENSOR:inline}}</code> … <code>:end</code>", { en: "Removes a run of text without breaking the paragraph around it.", fr: "Retire un passage sans casser le paragraphe autour." }],
        ],
      ],
      ["p", {
        en: "The historical point tag tolerates spacing: <code>{{CENSOR}}</code>, <code>{{ CENSOR }}</code>, <code>{{CENSOR }}</code> and <code>{{ CENSOR}}</code> all render exactly what they always rendered. Levels are free-form names — <code>premium</code>, <code>confidentiel</code>, <code>interne</code>, <code>secret</code> are the ones in common use. Modifiers combine with a comma: <code>{{CENSOR:start,confidentiel}}</code>.",
        fr: "Le tag ponctuel historique tolère les espaces : <code>{{CENSOR}}</code>, <code>{{ CENSOR }}</code>, <code>{{CENSOR }}</code> et <code>{{ CENSOR}}</code> rendent tous exactement ce qu'ils ont toujours rendu. Les niveaux sont des noms libres — <code>premium</code>, <code>confidentiel</code>, <code>interne</code>, <code>secret</code> sont les plus courants. Les modificateurs se combinent par une virgule : <code>{{CENSOR:start,confidentiel}}</code>.",
      }],

      ["h", { en: "A full example", fr: "Un exemple complet" }],
      ["code", "markdown", {
        en: "# Market study 2026\n\n## Public summary\n\nThe market grew by 18% over the quarter.\n\n## Margins by segment\n\n{{CENSOR:teaser=20,premium}}\nThe hardware segment carries a gross margin of 34%, well above the\nservices segment, whose contracts are priced on a three-year horizon and\nabsorb the onboarding cost in the first year.\n{{CENSOR:end}}\n\n## Method\n\nOur estimate rests on {{CENSOR:inline}}a proprietary panel of 400 firms{{CENSOR:end}}\nweighted by revenue.",
        fr: "# Étude de marché 2026\n\n## Synthèse publique\n\nLe marché progresse de 18 % sur le trimestre.\n\n## Marges par segment\n\n{{CENSOR:teaser=20,premium}}\nLe segment matériel porte une marge brute de 34 %, très au-dessus du\nsegment services, dont les contrats sont valorisés sur trois ans et\nabsorbent le coût d'accueil la première année.\n{{CENSOR:end}}\n\n## Méthode\n\nNotre estimation repose sur {{CENSOR:inline}}un panel propriétaire de 400 entreprises{{CENSOR:end}}\npondéré par le chiffre d'affaires."
      }],
      ["p", {
        en: "Two documents from one source: send it as-is for the free edition, strip the tags before sending for the paid one. Same rendering path, same pagination, same theme.",
        fr: "Deux documents depuis une seule source : envoyez-la telle quelle pour l'édition gratuite, retirez les tags avant l'envoi pour l'édition payante. Même chaîne de rendu, même pagination, même thème.",
      }],

      ["h", { en: "Labels", fr: "Libellés" }],
      ["p", {
        en: "Labels cascade: the level named on the tag wins, then <code>options.censor_label</code>, then the historical wording. A long label wraps rather than being clipped — it is the sales pitch, and half of it would sell nothing.",
        fr: "Les libellés cascadent : le niveau nommé sur le tag l'emporte, puis <code>options.censor_label</code>, puis le libellé historique. Un libellé long passe à la ligne au lieu d'être coupé — c'est l'argument commercial, et la moitié d'un argument ne vend rien.",
      }],
      ["code", "json", '{\n  "markdown": "…",\n  "options": { "censor_label": "Reserved for subscribers — aismarttalk.tech" }\n}'],

      ["h", { en: "What happens when you get it wrong", fr: "Ce qui arrive quand vous vous trompez" }],
      ["p", {
        en: "Malformed input always errs <b>towards</b> censoring. A syntax mistake must not be a way to reveal what was meant to be hidden:",
        fr: "Une entrée malformée penche toujours <b>vers</b> la censure. Une faute de syntaxe ne doit pas être un moyen de révéler ce qui devait être caché :",
      }],
      ["table",
        [{ en: "Mistake", fr: "Erreur" }, { en: "Behaviour", fr: "Comportement" }],
        [
          [{ en: "An unclosed region", fr: "Une région non fermée" }, { en: "Censored to the end of the document.", fr: "Censurée jusqu'à la fin du document." }],
          [{ en: "Nested regions", fr: "Régions imbriquées" }, { en: "Depth-counted, so the outer one still closes correctly.", fr: "Comptées en profondeur : la région extérieure se ferme correctement." }],
          [{ en: "A level tag followed by <code>:end</code>", fr: "Un tag de niveau suivi d'un <code>:end</code>" }, { en: "Opens a region — that is what a misspelled <code>start</code> looks like.", fr: "Ouvre une région — c'est à cela que ressemble un <code>start</code> mal orthographié." }],
          [{ en: "An <code>:end</code> with no open region", fr: "Un <code>:end</code> sans région ouverte" }, { en: "Dropped.", fr: "Ignoré." }],
        ],
      ],
      ["note", "info", {
        en: "Censoring hides text you control, in a source you are about to render. To black out text in a <b>PDF that already exists</b> — someone else's contract, a scanned invoice — you want <code>/api/redact</code>, which works on pixels. See the redaction guide.",
        fr: "La censure masque un texte que vous maîtrisez, dans une source que vous êtes sur le point de rendre. Pour masquer du texte dans un <b>PDF qui existe déjà</b> — le contrat d'un tiers, une facture scannée — c'est <code>/api/redact</code> qu'il vous faut, qui travaille sur les pixels. Voir le guide caviardage.",
      }],
      ["try", "convert", { en: "Render a censored document", fr: "Rendre un document censuré" }],
    ],
  },

  {
    key: "charts",
    group: { en: "Document content", fr: "Contenu du document" },
    icon: "M4 20V10M10 20V4M16 20v-7M22 20H2",
    title: { en: "Charts, drawn in-process", fr: "Graphiques, dessinés dans le processus" },
    lede: {
      en: "A fenced ```chart block becomes inline SVG. No browser, no headless Chrome, no external service — and no chart can fail your request.",
      fr: "Un bloc ```chart devient du SVG inline. Pas de navigateur, pas de Chrome headless, pas de service externe — et aucun graphique ne peut faire échouer votre requête.",
    },
    blocks: [
      ["code", "markdown", '```chart\n{\n  "type": "bar",\n  "title": "Revenue",\n  "labels": ["Q1", "Q2", "Q3", "Q4"],\n  "series": [{"name": "2026", "data": [12000, 18000, 15000, 21000]}]\n}\n```'],

      ["h", { en: "Types", fr: "Types" }],
      ["table",
        [{ en: "type", fr: "type" }, { en: "For", fr: "Pour" }],
        [
          ["<code>bar</code> · <code>column</code>", { en: "Vertical bars — the default reflex for a few categories.", fr: "Barres verticales — le réflexe par défaut pour quelques catégories." }],
          ["<code>hbar</code>", { en: "Horizontal bars — when the labels are long.", fr: "Barres horizontales — quand les libellés sont longs." }],
          ["<code>stacked-bar</code> · <code>stacked</code>", { en: "Composition of a total.", fr: "Composition d'un total." }],
          ["<code>grouped-bar</code> · <code>grouped</code>", { en: "Several series compared category by category.", fr: "Plusieurs séries comparées catégorie par catégorie." }],
          ["<code>line</code>", { en: "A trend over an ordered axis.", fr: "Une tendance sur un axe ordonné." }],
          ["<code>area</code>", { en: "The same, when the volume under the curve matters.", fr: "La même, quand le volume sous la courbe compte." }],
          ["<code>pie</code> · <code>donut</code> · <code>doughnut</code>", { en: "A share of a whole, up to 8 slices.", fr: "Une part d'un tout, jusqu'à 8 parts." }],
        ],
      ],

      ["h", { en: "Every field", fr: "Tous les champs" }],
      ["table",
        [{ en: "Field", fr: "Champ" }, { en: "Default", fr: "Défaut" }, { en: "Description", fr: "Description" }],
        [
          ["<code>type</code>", "<code>bar</code>", { en: "One of the values above.", fr: "L'une des valeurs ci-dessus." }],
          ["<code>title</code> / <code>subtitle</code>", "—", { en: "Drawn inside the SVG, so they travel with the figure.", fr: "Dessinés dans le SVG : ils voyagent avec la figure." }],
          ["<code>labels</code>", "—", { en: "Categories, one per point.", fr: "Catégories, une par point." }],
          ["<code>series</code>", "—", { en: "<code>[{name, data, color}]</code> — 8 series max, 2000 points.", fr: "<code>[{name, data, color}]</code> — 8 séries max, 2000 points." }],
          ["<code>x_label</code> / <code>y_label</code>", "—", { en: "Axis titles.", fr: "Titres des axes." }],
          ["<code>value_format</code>", "<code>compact</code>", { en: "<code>compact</code> · <code>percent</code> · <code>eur</code> · <code>plain</code>.", fr: "<code>compact</code> · <code>percent</code> · <code>eur</code> · <code>plain</code>." }],
          ["<code>width</code> / <code>height</code>", "640 / 360", { en: "240–2000 and 160–2000 respectively.", fr: "240–2000 et 160–2000 respectivement." }],
          ["<code>legend</code>", { en: "from 2 series", fr: "dès 2 séries" }, { en: "Force it on or off.", fr: "Forcer son affichage ou son absence." }],
          ["<code>grid</code>", "<code>true</code>", { en: "Background grid lines.", fr: "Lignes de grille en fond." }],
          ["<code>texture</code>", "<code>true</code>", { en: "Hatch patterns, so the chart survives being printed in black and white.", fr: "Motifs hachurés, pour que le graphique survive à une impression en noir et blanc." }],
        ],
      ],
      ["note", "info", {
        en: "<code>texture</code> is on by default on purpose. A report is printed, photocopied and faxed; two series that differ only by hue become one series the moment the colour is gone.",
        fr: "<code>texture</code> est actif par défaut, volontairement. Un rapport est imprimé, photocopié, faxé ; deux séries qui ne diffèrent que par la teinte n'en font plus qu'une dès que la couleur disparaît.",
      }],

      ["h", { en: "A denser example", fr: "Un exemple plus dense" }],
      ["code", "markdown", '```chart\n{\n  "type": "grouped-bar",\n  "title": "Documents converted",\n  "subtitle": "by quarter and by engine",\n  "labels": ["Q1", "Q2", "Q3", "Q4"],\n  "series": [\n    {"name": "weasyprint", "data": [12000, 18000, 15000, 21000], "color": "#0e8ea3"},\n    {"name": "pdflatex",   "data": [3000, 2600, 4100, 3900]}\n  ],\n  "y_label": "documents",\n  "value_format": "compact",\n  "height": 420\n}\n```'],

      ["h", { en: "Failure is never fatal", fr: "L'échec n'est jamais fatal" }],
      ["p", {
        en: "A block that cannot be rendered — invalid JSON, an unknown type, too many series — <b>stays in the document as a code block</b>, and the reason comes back in the <code>warnings</code> field of the JSON response. A hundred-page report is never lost over one malformed figure.",
        fr: "Un bloc qui ne peut pas être rendu — JSON invalide, type inconnu, trop de séries — <b>reste dans le document sous forme de bloc de code</b>, et la raison revient dans le champ <code>warnings</code> de la réponse JSON. Un rapport de cent pages n'est jamais perdu à cause d'une figure malformée.",
      }],
      ["code", "json", '{\n  "download_url": "/download/demo/report.pdf",\n  "warnings": [\n    { "kind": "chart", "message": "unknown chart type \\"radar\\"", "line": 42 }\n  ]\n}'],
      ["p", {
        en: "<code>options.charts: false</code> turns off the expansion of <b>both</b> block types — useful when you are rendering untrusted input and want no block expansion at all.",
        fr: "<code>options.charts: false</code> coupe l'expansion des <b>deux</b> types de blocs — utile quand vous rendez une entrée non fiable et ne voulez aucune expansion.",
      }],
      ["try", "convert", { en: "Render a chart", fr: "Rendre un graphique" }],
    ],
  },

  {
    key: "diagrams",
    group: { en: "Document content", fr: "Contenu du document" },
    icon: "M5 3h6v6H5zM13 15h6v6h-6zM8 9v6h5",
    title: { en: "Mermaid diagrams", fr: "Diagrammes Mermaid" },
    lede: {
      en: "Flowcharts, sequences and state machines written as text, rendered to SVG inside the page.",
      fr: "Organigrammes, séquences et machines à états écrits en texte, rendus en SVG dans la page.",
    },
    blocks: [
      ["code", "markdown", '```mermaid theme=forest\ngraph TD;\n  A[Request] --> B{Cached?};\n  B -- yes --> C[Serve from cache];\n  B -- no --> D[Render];\n  D --> C;\n```'],
      ["p", {
        en: "Attributes may follow the language — <code>theme=dark</code>, <code>title=\"…\"</code> — including in pandoc form (<code>{.mermaid theme=forest}</code>). The HTML spelling <code>&lt;pre&gt;&lt;code class=\"language-mermaid\"&gt;</code> works too, which matters when the Markdown was produced by another tool. Accepted themes: <code>default</code>, <code>dark</code>, <code>forest</code>, <code>neutral</code>.",
        fr: "Des attributs peuvent suivre le langage — <code>theme=dark</code>, <code>title=\"…\"</code> — y compris en forme pandoc (<code>{.mermaid theme=forest}</code>). L'écriture HTML <code>&lt;pre&gt;&lt;code class=\"language-mermaid\"&gt;</code> fonctionne aussi, ce qui compte quand le Markdown a été produit par un autre outil. Thèmes acceptés : <code>default</code>, <code>dark</code>, <code>forest</code>, <code>neutral</code>.",
      }],
      ["p", {
        en: "<code>click</code> directives lose their hyperlink on purpose: a diagram in a printed report should not carry a live link to somewhere the reader cannot see.",
        fr: "Les directives <code>click</code> perdent leur lien, volontairement : un diagramme dans un rapport imprimé ne doit pas porter un lien vivant vers un endroit que le lecteur ne voit pas.",
      }],
      ["note", "warn", {
        en: "<b>Diagrams depend on an external service.</b> Rendering is delegated to Mermaid Studio, so the container needs outbound access to it. If it is unreachable, times out, or is explicitly disabled, every diagram degrades to its code block plus one warning and the document still comes out. Charts have no such dependency — they are drawn in-process.",
        fr: "<b>Les diagrammes dépendent d'un service externe.</b> Le rendu est délégué à Mermaid Studio : le conteneur a besoin d'un accès sortant vers lui. S'il est injoignable, en timeout ou explicitement désactivé, chaque diagramme retombe sur son bloc de code avec un avertissement, et le document sort quand même. Les graphiques n'ont pas cette dépendance — ils sont dessinés dans le processus.",
      }],
      ["p", {
        en: "One consequence worth knowing: the render cache key is computed on the <b>expanded</b> source, so a document containing a diagram calls Mermaid Studio <i>before</i> it can hit the disk cache. An in-process diagram cache keeps that cheap, but it empties on restart — the first render of a diagram after a deployment always goes over the network.",
        fr: "Une conséquence utile à connaître : la clé du cache de rendu est calculée sur la source <b>expansée</b>, donc un document contenant un diagramme appelle Mermaid Studio <i>avant</i> de pouvoir toucher le cache disque. Un cache de diagrammes en mémoire rend cela peu coûteux, mais il se vide au redémarrage — le premier rendu d'un diagramme après un déploiement passe toujours par le réseau.",
      }],
      ["try", "convert", { en: "Render a diagram", fr: "Rendre un diagramme" }],
    ],
  },

  // ───────────────────────────────────────────── qualité ──────────────────

  {
    key: "layout",
    group: { en: "Quality control", fr: "Contrôle qualité" },
    icon: "M4 4h16v16H4zM4 9h16M9 9v11",
    title: { en: "Layout Doctor", fr: "Layout Doctor" },
    lede: {
      en: "The service reads back the PDF it just produced, finds what a proofreader would find, and fixes it — but only if fixing it actually helped.",
      fr: "Le service relit le PDF qu'il vient de produire, trouve ce qu'un relecteur trouverait, et le corrige — mais seulement si la correction a réellement aidé.",
    },
    blocks: [
      ["p", {
        en: "<code>options.autolayout: true</code> on any rendering endpoint analyses the produced PDF, re-renders it with corrective CSS while that improves the score, and returns the report in the <code>layout</code> field. <b>A corrective pass that does not improve the score is thrown away</b>: the client never receives a PDF worse than the one it would have had.",
        fr: "<code>options.autolayout: true</code> sur n'importe quel endpoint de rendu analyse le PDF produit, le re-rend avec du CSS correctif tant que cela améliore le score, et renvoie le rapport dans le champ <code>layout</code>. <b>Une passe correctrice qui n'améliore pas le score est jetée</b> : le client ne reçoit jamais un PDF pire que celui qu'il aurait eu.",
      }],
      ["code", "json", '{\n  "markdown": "…",\n  "options": { "theme": "report", "autolayout": true }\n}'],
      ["code", "json", '{\n  "download_url": "/download/demo/report.pdf",\n  "layout": {\n    "pages": 12,\n    "score": 94,\n    "passes": 1,\n    "issues": [\n      { "kind": "orphan_heading", "page": 7, "severity": "warn",\n        "detail": "heading alone at the bottom of the page", "bbox": [72, 60, 523, 84] }\n    ]\n  }\n}'],

      ["h", { en: "What it looks for", fr: "Ce qu'il cherche" }],
      ["table",
        [{ en: "kind", fr: "kind" }, { en: "Severity", fr: "Sévérité" }, { en: "Meaning", fr: "Signification" }],
        [
          ["<code>overflow</code>", "<code>error</code> / <code>warn</code>", { en: "Content past the page edge (error) or past the margin (warn).", fr: "Contenu au-delà du bord de page (error) ou de la marge (warn)." }],
          ["<code>blank_page</code>", "<code>warn</code>", { en: "A page with no ink at all — reported, not fixable.", fr: "Une page sans la moindre encre — signalée, pas corrigeable." }],
          ["<code>widow_page</code>", "<code>warn</code>", { en: "A last page carrying almost nothing.", fr: "Une dernière page qui ne porte presque rien." }],
          ["<code>orphan_heading</code>", "<code>warn</code>", { en: "A heading alone at the bottom of a page.", fr: "Un titre seul en bas de page." }],
          ["<code>split_table</code>", "<code>warn</code>", { en: "A table cut across a page break.", fr: "Un tableau coupé par un saut de page." }],
          ["<code>long_table</code>", "<code>info</code>", { en: "A table taller than a page; suppresses the anti-break rule.", fr: "Un tableau plus haut qu'une page ; supprime la règle anti-coupure." }],
        ],
      ],

      ["h", { en: "Auditing a PDF you already have", fr: "Auditer un PDF que vous avez déjà" }],
      ["p", {
        en: "<code>POST /api/layout</code> runs the same analysis on an existing PDF without touching it — a useful gate in a pipeline that renders elsewhere.",
        fr: "<code>POST /api/layout</code> lance la même analyse sur un PDF existant sans y toucher — un bon garde-fou dans un pipeline qui rend ailleurs.",
      }],
      ["code", "json", '{ "pdf": "/download/demo/report.pdf" }'],
      ["note", "warn", {
        en: "<b>It costs a full extra render per corrective pass</b>, plus the analysis itself — roughly double the latency of the same request without it. Turn it on for documents a human will read, not for a batch of ten thousand invoices.",
        fr: "<b>Cela coûte un rendu complet de plus par passe correctrice</b>, plus l'analyse elle-même — environ le double de la latence de la même requête sans lui. Activez-le pour les documents qu'un humain va lire, pas pour un lot de dix mille factures.",
      }],
      ["try", "layout", { en: "Audit a PDF", fr: "Auditer un PDF" }],
    ],
  },

  {
    key: "redaction",
    group: { en: "Quality control", fr: "Contrôle qualité" },
    icon: "M4 6h16v12H4zM7 10h6M7 14h10",
    title: { en: "Redaction and visual diff", fr: "Caviardage et diff visuel" },
    lede: {
      en: "Black out a PDF so the text is genuinely gone, and compare two renders pixel by pixel so a rendering regression cannot ship unnoticed.",
      fr: "Masquer un PDF pour que le texte ait réellement disparu, et comparer deux rendus pixel par pixel pour qu'une régression de rendu ne passe pas inaperçue.",
    },
    blocks: [
      ["h", { en: "POST /api/redact", fr: "POST /api/redact" }],
      ["p", {
        en: "Paints out patterns and personal data, then <b>rebuilds every page from pixels</b>. The redacted strings are gone from the file, and so is every text layer, bookmark, annotation and metadata entry of the source. The result is neither selectable nor searchable, and it is larger — that is the trade, and it is the only honest one.",
        fr: "Masque des motifs et des données personnelles, puis <b>reconstruit chaque page à partir des pixels</b>. Les chaînes caviardées ont disparu du fichier, ainsi que toute couche de texte, signet, annotation et métadonnée de la source. Le résultat n'est ni sélectionnable ni recherchable, et il est plus lourd — c'est le compromis, et c'est le seul honnête.",
      }],
      ["code", "json", '{\n  "pdf": "/download/demo/contract.pdf",\n  "patterns": ["Jean Dupont", "FR76 3000 1007 9412 3456 7890 185"],\n  "entities": ["email", "iban", "phone", "siret", "credit_card"],\n  "dpi": 150,\n  "client_id": "demo",\n  "pdf_name": "contract-redacted"\n}'],
      ["p", {
        en: "Entities are validated by their check digits, so a 14-digit invoice number is not mistaken for a SIRET and a random 16-digit reference is not blacked out as a card number.",
        fr: "Les entités sont validées par leurs clés de contrôle : un numéro de facture à 14 chiffres n'est pas pris pour un SIRET, et une référence à 16 chiffres n'est pas masquée comme un numéro de carte.",
      }],
      ["note", "warn", {
        en: "<b><code>patterns</code> are literal strings, not regular expressions.</b> Comparison ignores case and normalises whitespace, so a match may span a line break. A pattern that <i>looks</i> like a regex (<code>\\d{4}</code>, <code>[A-Z]+</code>, <code>.*</code>, <code>(?i)</code>) is rejected with a 400 rather than matched literally — receiving a document where nothing was blacked out while believing the job was done is the failure this prevents.",
        fr: "<b>Les <code>patterns</code> sont des chaînes littérales, pas des expressions régulières.</b> La comparaison ignore la casse et normalise les espaces : une correspondance peut donc franchir un retour à la ligne. Un motif qui <i>ressemble</i> à une regex (<code>\\d{4}</code>, <code>[A-Z]+</code>, <code>.*</code>, <code>(?i)</code>) est refusé en 400 plutôt que cherché littéralement — recevoir un document où rien n'a été masqué en croyant le travail fait est précisément l'échec que cela évite.",
      }],
      ["code", "json", '{\n  "download_url": "/download/demo/contract-redacted.pdf",\n  "redactions": [{ "page": 1, "count": 3 }, { "page": 2, "count": 1 }],\n  "pages": 4,\n  "mode": "flatten",\n  "notice": "…"\n}'],
      ["p", {
        en: "<code>mode</code> is always <code>flatten</code>: there is deliberately no overlay mode that would keep the text layer under a black rectangle. The binary form of the endpoint (no <code>client_id</code>) cannot carry the redaction counts — use the JSON form whenever you need proof of what was painted.",
        fr: "<code>mode</code> vaut toujours <code>flatten</code> : il n'existe volontairement aucun mode « superposition » qui garderait la couche de texte sous un rectangle noir. La forme binaire de l'endpoint (sans <code>client_id</code>) ne peut pas porter le compte des masquages — utilisez la forme JSON dès que vous avez besoin d'une preuve de ce qui a été masqué.",
      }],
      ["note", "info", {
        en: "A PDF that was <b>scanned</b> — no text layer — comes back flattened with <code>redactions: []</code>. Nothing could be located; there is no OCR. An empty array is the only signal, and it means \"nothing was found\", not \"nothing was there\".",
        fr: "Un PDF <b>scanné</b> — sans couche de texte — revient aplati avec <code>redactions: []</code>. Rien n'a pu être localisé ; il n'y a pas d'OCR. Le tableau vide est le seul signal, et il veut dire « rien n'a été trouvé », pas « il n'y avait rien ».",
      }],

      ["h", { en: "POST /api/diff", fr: "POST /api/diff" }],
      ["p", {
        en: "Compares two saved PDFs page by page and tells you whether anything moved. This is the endpoint that turns \"the report still looks right\" into a CI check.",
        fr: "Compare deux PDFs sauvegardés page par page et vous dit si quelque chose a bougé. C'est l'endpoint qui transforme « le rapport a toujours l'air correct » en test d'intégration continue.",
      }],
      ["code", "json", '{\n  "before": "/download/ci/report-main.pdf",\n  "after": "/download/ci/report-branch.pdf",\n  "dpi": 150,\n  "threshold": 0,\n  "images": true\n}'],
      ["code", "json", '{\n  "verdict": "changed",\n  "pages_total": 12, "pages_before": 12, "pages_after": 12,\n  "pages_changed": 2,\n  "changed_ratio": 0.167,\n  "per_page": [{ "page": 7, "ratio": 0.031 }, { "page": 8, "ratio": 0.004 }]\n}'],
      ["p", {
        en: "<b><code>threshold</code> defaults to 0</b>: any page that visibly moved makes the verdict <code>changed</code>. Raise it to tolerate noise — a CI that defaulted to tolerance would report <code>identical</code> on a renamed heading, which is the one thing it exists to catch.",
        fr: "<b><code>threshold</code> vaut 0 par défaut</b> : toute page qui a visiblement bougé rend le verdict <code>changed</code>. Augmentez-le pour tolérer du bruit — une CI qui tolérerait par défaut annoncerait <code>identical</code> sur un titre renommé, c'est-à-dire la seule chose qu'elle existe pour attraper.",
      }],
      ["note", "warn", {
        en: "Run a diff right after a deployment that changed the renderer without bumping the internal cache version and it will compare a stale cached PDF against a fresh one — or worse, call two different renders <code>identical</code>. Render both sides of a comparison in the same session.",
        fr: "Lancez un diff juste après un déploiement qui a changé le moteur de rendu sans incrémenter la version interne du cache, et il comparera un PDF caché périmé à un PDF frais — ou pire, déclarera <code>identical</code> deux rendus différents. Rendez les deux côtés d'une comparaison dans la même session.",
      }],
      ["try", "redact", { en: "Redact a PDF", fr: "Caviarder un PDF" }],
    ],
  },

  // ───────────────────────────────────────────── boîte à outils ───────────

  {
    key: "toolbox",
    group: { en: "PDF toolbox", fr: "Boîte à outils PDF" },
    icon: "M3 7h18v13H3zM8 7V4h8v3M8 12h8",
    title: { en: "Preview, merge, watermark, protect", fr: "Aperçu, fusion, filigrane, protection" },
    lede: {
      en: "Four endpoints that operate on PDFs the service has already saved. They chain: the output of one is the input of the next.",
      fr: "Quatre endpoints qui opèrent sur des PDFs déjà sauvegardés par le service. Ils s'enchaînent : la sortie de l'un est l'entrée du suivant.",
    },
    blocks: [
      ["h", { en: "Preview — pages as PNG", fr: "Aperçu — pages en PNG" }],
      ["p", {
        en: "<code>POST /api/preview</code> takes the same body as a render (<code>markdown</code>, <code>html</code>, or <code>template</code> + <code>data</code>) and answers with images instead of a PDF. A body with no new field returns the PNG of page 1 — historical behaviour, unchanged.",
        fr: "<code>POST /api/preview</code> prend le même corps qu'un rendu (<code>markdown</code>, <code>html</code>, ou <code>template</code> + <code>data</code>) et répond avec des images plutôt qu'un PDF. Un corps sans champ nouveau renvoie le PNG de la page 1 — comportement historique, inchangé.",
      }],
      ["table",
        [{ en: "Field", fr: "Champ" }, { en: "Default", fr: "Défaut" }, { en: "Description", fr: "Description" }],
        [
          ["<code>pages</code>", '<code>"1"</code>', { en: '<code>"3"</code>, <code>"2-5"</code> or <code>"all"</code>, capped by the deployment.', fr: '<code>"3"</code>, <code>"2-5"</code> ou <code>"all"</code>, plafonné par le déploiement.' }],
          ["<code>dpi</code>", "<code>150</code>", { en: "36–300.", fr: "36–300." }],
          ["<code>layout</code>", { en: "see below", fr: "voir ci-dessous" }, { en: "<code>png</code> (raw image), <code>images</code> (JSON, one PNG per page), <code>sheet</code> (one contact-sheet image).", fr: "<code>png</code> (image brute), <code>images</code> (JSON, un PNG par page), <code>sheet</code> (une planche-contact)." }],
        ],
      ],
      ["p", {
        en: "Without an explicit <code>layout</code>, one page returns a raw PNG and several return the <code>images</code> JSON — so an existing caller keeps getting exactly what it got. <code>images</code> reports <code>truncated: true</code> when the page cap cut the answer short; <code>sheet</code> cannot, since it is just an image.",
        fr: "Sans <code>layout</code> explicite, une page renvoie un PNG brut et plusieurs renvoient le JSON <code>images</code> — un appelant existant reçoit donc exactement ce qu'il recevait. <code>images</code> signale <code>truncated: true</code> quand le plafond de pages a écourté la réponse ; <code>sheet</code> ne le peut pas, puisque ce n'est qu'une image.",
      }],
      ["code", "json", '{\n  "markdown": "# Report\\n\\n…",\n  "pages": "1-4",\n  "dpi": 120,\n  "layout": "sheet",\n  "options": { "theme": "report" }\n}'],

      ["h", { en: "Merge", fr: "Fusion" }],
      ["p", {
        en: "<code>POST /api/merge</code> concatenates two or more previously saved PDFs, in the order given.",
        fr: "<code>POST /api/merge</code> concatène deux PDFs sauvegardés ou plus, dans l'ordre donné.",
      }],
      ["code", "json", '{\n  "pdfs": [\n    "/download/demo/cover.pdf",\n    "/download/demo/body.pdf",\n    "/download/demo/appendix.pdf"\n  ],\n  "client_id": "demo",\n  "pdf_name": "complete-file"\n}'],

      ["h", { en: "Watermark", fr: "Filigrane" }],
      ["p", {
        en: "<code>POST /api/watermark</code> overlays a diagonal text watermark on every page of a saved PDF.",
        fr: "<code>POST /api/watermark</code> superpose un filigrane textuel en diagonale sur chaque page d'un PDF sauvegardé.",
      }],
      ["code", "json", '{\n  "pdf": "/download/demo/contract.pdf",\n  "text": "DRAFT — DO NOT SIGN",\n  "opacity": 0.06,\n  "angle": -45,\n  "client_id": "demo",\n  "pdf_name": "contract-draft"\n}'],
      ["p", {
        en: "<code>opacity</code> defaults to <code>0.06</code> (0–1) and <code>angle</code> to <code>-45</code> (−360 to 360). The default is deliberately faint: a watermark that fights the text makes the document unreadable, and an unreadable draft gets re-exported without the watermark.",
        fr: "<code>opacity</code> vaut <code>0.06</code> par défaut (0–1) et <code>angle</code> vaut <code>-45</code> (−360 à 360). Le défaut est volontairement discret : un filigrane qui lutte avec le texte rend le document illisible, et un brouillon illisible finit ré-exporté sans filigrane.",
      }],
      ["note", "info", {
        en: "Two watermarks, two purposes. <code>options.watermark</code> is drawn <b>at render time</b>, in CSS, under the text — it belongs to the document. <code>/api/watermark</code> stamps an <b>existing</b> PDF, including one the service did not produce.",
        fr: "Deux filigranes, deux usages. <code>options.watermark</code> est dessiné <b>au moment du rendu</b>, en CSS, sous le texte — il appartient au document. <code>/api/watermark</code> tamponne un PDF <b>existant</b>, y compris un PDF que le service n'a pas produit.",
      }],

      ["h", { en: "Protect", fr: "Protection" }],
      ["p", {
        en: "<code>POST /api/protect</code> applies AES-256 password protection to a saved PDF. Do this <b>last</b>: an encrypted PDF cannot be merged, watermarked, previewed or diffed afterwards.",
        fr: "<code>POST /api/protect</code> applique une protection par mot de passe AES-256 à un PDF sauvegardé. Faites-le en <b>dernier</b> : un PDF chiffré ne peut plus être fusionné, filigrané, prévisualisé ni comparé ensuite.",
      }],
      ["code", "json", '{\n  "pdf": "/download/demo/contract-draft.pdf",\n  "password": "a-real-passphrase",\n  "client_id": "demo",\n  "pdf_name": "contract-sealed"\n}'],
      ["try", "preview", { en: "Preview a document", fr: "Prévisualiser un document" }],
    ],
  },

  // ───────────────────────────────────────────── recettes ─────────────────

  {
    key: "recipes",
    group: { en: "End to end", fr: "De bout en bout" },
    icon: "M4 4h16v4H4zM4 12h10M4 17h7M17 12l3 3-3 3",
    title: { en: "Five complete recipes", fr: "Cinq recettes complètes" },
    lede: {
      en: "Whole workflows, copy-pasteable, each combining several features the way a real integration does.",
      fr: "Des enchaînements complets, copiables tels quels, combinant chacun plusieurs fonctionnalités comme le fait une vraie intégration.",
    },
    blocks: [
      ["h", { en: "1 · A branded, sealed contract", fr: "1 · Un contrat à votre marque, scellé" }],
      ["p", {
        en: "Cover page, brand kit, numbering, table of contents, draft watermark, then AES-256. Four calls, each feeding the next.",
        fr: "Page de garde, kit de marque, numérotation, sommaire, filigrane brouillon, puis AES-256. Quatre appels, chacun alimentant le suivant.",
      }],
      ["code", "shell", {
        en: "# 1. Render, save server-side\ncurl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"markdown\": \"# Service agreement\\n\\n## 1. Scope\\n…\",\n    \"options\": {\n      \"theme\": \"report@1\",\n      \"toc\": true, \"toc_depth\": 2,\n      \"page_numbers\": true,\n      \"page_number_format\": \"counter(page) \\\" / \\\" counter(pages)\",\n      \"cover\": { \"title\": \"Service agreement\", \"subtitle\": \"ACME × AI SmartTalk\", \"date\": \"2026-07-26\" },\n      \"autolayout\": true\n    },\n    \"client_id\": \"acme\", \"pdf_name\": \"agreement\"\n  }'\n# {\"download_url\":\"/download/acme/agreement.pdf\",\"layout\":{\"score\":97,…}}\n\n# 2. Stamp it as a draft\ncurl -s -X POST \"$BASE/api/watermark\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/acme/agreement.pdf\", \"text\": \"DRAFT\",\n    \"client_id\": \"acme\", \"pdf_name\": \"agreement-draft\"\n  }'\n\n# 3. Seal it — always last\ncurl -s -X POST \"$BASE/api/protect\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/acme/agreement-draft.pdf\", \"password\": \"…\",\n    \"client_id\": \"acme\", \"pdf_name\": \"agreement-sealed\"\n  }'",
        fr: "# 1. Rendre, sauvegarder côté serveur\ncurl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"markdown\": \"# Contrat de service\\n\\n## 1. Objet\\n…\",\n    \"options\": {\n      \"theme\": \"report@1\",\n      \"toc\": true, \"toc_depth\": 2,\n      \"page_numbers\": true,\n      \"page_number_format\": \"counter(page) \\\" / \\\" counter(pages)\",\n      \"cover\": { \"title\": \"Contrat de service\", \"subtitle\": \"ACME × AI SmartTalk\", \"date\": \"26/07/2026\" },\n      \"autolayout\": true\n    },\n    \"client_id\": \"acme\", \"pdf_name\": \"contrat\"\n  }'\n# {\"download_url\":\"/download/acme/contrat.pdf\",\"layout\":{\"score\":97,…}}\n\n# 2. Tamponner « brouillon »\ncurl -s -X POST \"$BASE/api/watermark\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/acme/contrat.pdf\", \"text\": \"BROUILLON\",\n    \"client_id\": \"acme\", \"pdf_name\": \"contrat-brouillon\"\n  }'\n\n# 3. Sceller — toujours en dernier\ncurl -s -X POST \"$BASE/api/protect\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/acme/contrat-brouillon.pdf\", \"password\": \"…\",\n    \"client_id\": \"acme\", \"pdf_name\": \"contrat-scelle\"\n  }'",
      }],

      ["h", { en: "2 · A two-audience study", fr: "2 · Une étude pour deux audiences" }],
      ["p", {
        en: "One source, two PDFs. The free edition keeps the CENSOR tags; the paid edition has them stripped before the call. Same theme, same pagination, same figures — which is what makes the hole read as a paywall rather than as a broken document.",
        fr: "Une source, deux PDFs. L'édition gratuite garde les tags CENSOR ; l'édition payante les retire avant l'appel. Même thème, même pagination, mêmes figures — c'est ce qui fait lire le trou comme un mur payant et non comme un document cassé.",
      }],
      ["code", "shell", {
        en: "# Free edition — the source ships with its tags\ncurl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d @study.json \\\n  --output study-free.pdf\n\n# Paid edition — same JSON, tags removed from the `markdown` field first\njq '.markdown |= gsub(\"\\\\{\\\\{CENSOR:[^}]*\\\\}\\\\}\"; \"\")' study.json \\\n  | curl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n      -H 'Content-Type: application/json' -d @- --output study-full.pdf",
        fr: "# Édition gratuite — la source part avec ses tags\ncurl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d @etude.json \\\n  --output etude-gratuite.pdf\n\n# Édition payante — même JSON, tags retirés du champ `markdown` d'abord\njq '.markdown |= gsub(\"\\\\{\\\\{CENSOR:[^}]*\\\\}\\\\}\"; \"\")' etude.json \\\n  | curl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n      -H 'Content-Type: application/json' -d @- --output etude-complete.pdf",
      }],

      ["h", { en: "3 · Invoices in bulk", fr: "3 · Des factures en lot" }],
      ["p", {
        en: "One Tera template, one JSON context per invoice, no theme and no <code>autolayout</code> — this is the path built for volume. The template is stored on your side and sent with each call, so a change of wording is a deploy of your service, not of ours.",
        fr: "Un template Tera, un contexte JSON par facture, sans thème et sans <code>autolayout</code> — c'est le chemin fait pour le volume. Le template est chez vous et part à chaque appel : un changement de formulation est un déploiement de votre service, pas du nôtre.",
      }],
      ["code", "json", '{\n  "template": "<h1>Invoice {{ number }}</h1>\\n<p>{{ customer.name }}</p>\\n<table>{% for l in lines %}<tr><td>{{ l.label }}</td><td>{{ l.amount }}</td></tr>{% endfor %}</table>",\n  "data": {\n    "number": "2026-0412",\n    "customer": { "name": "ACME" },\n    "lines": [{ "label": "Annual subscription", "amount": "1 200,00 €" }]\n  },\n  "options": { "paper_size": "a4", "margins": { "top": "20mm", "bottom": "20mm" } },\n  "client_id": "acme", "pdf_name": "invoice-2026-0412"\n}'],
      ["note", "info", {
        en: "Identical inputs hit the content-addressed render cache and come back with <code>cached: true</code> without re-rendering. Two invoices differ, so they will not — but a re-run of the same batch after a failure costs almost nothing.",
        fr: "Des entrées identiques touchent le cache de rendu adressé par contenu et reviennent avec <code>cached: true</code> sans re-rendu. Deux factures diffèrent, donc non — mais rejouer le même lot après un échec ne coûte presque rien.",
      }],

      ["h", { en: "4 · Visual regression in CI", fr: "4 · Régression visuelle en CI" }],
      ["p", {
        en: "Render the same document on both branches, compare, fail the build if anything moved. The threshold stays at 0 so a renamed heading is caught.",
        fr: "Rendre le même document sur les deux branches, comparer, faire échouer le build si quelque chose a bougé. Le seuil reste à 0 pour qu'un titre renommé soit attrapé.",
      }],
      ["code", "shell", {
        en: "render() {  # $1 = name — renders the reference document and saves it\n  curl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n    -H 'Content-Type: application/json' \\\n    -d \"$(jq -n --rawfile md fixtures/reference.md --arg n \"$1\" \\\n         '{markdown: $md, options: {theme: \"report@1\"}, client_id: \"ci\", pdf_name: $n}')\" \\\n    | jq -r .download_url\n}\n\nBEFORE=$(git stash && render main && git stash pop)\nAFTER=$(render branch)\n\nVERDICT=$(curl -s -X POST \"$BASE/api/diff\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' \\\n  -d \"{\\\"before\\\": \\\"$BEFORE\\\", \\\"after\\\": \\\"$AFTER\\\", \\\"threshold\\\": 0}\" | jq -r .verdict)\n\n[ \"$VERDICT\" = identical ] || { echo \"rendering changed\"; exit 1; }",
        fr: "render() {  # $1 = nom — rend le document de référence et le sauvegarde\n  curl -s -X POST \"$BASE/api/convert\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n    -H 'Content-Type: application/json' \\\n    -d \"$(jq -n --rawfile md fixtures/reference.md --arg n \"$1\" \\\n         '{markdown: $md, options: {theme: \"report@1\"}, client_id: \"ci\", pdf_name: $n}')\" \\\n    | jq -r .download_url\n}\n\nBEFORE=$(git stash && render main && git stash pop)\nAFTER=$(render branche)\n\nVERDICT=$(curl -s -X POST \"$BASE/api/diff\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' \\\n  -d \"{\\\"before\\\": \\\"$BEFORE\\\", \\\"after\\\": \\\"$AFTER\\\", \\\"threshold\\\": 0}\" | jq -r .verdict)\n\n[ \"$VERDICT\" = identical ] || { echo \"le rendu a changé\"; exit 1; }",
      }],

      ["h", { en: "5 · Sharing a third-party document safely", fr: "5 · Partager sans risque un document tiers" }],
      ["p", {
        en: "A supplier contract has to go to a partner, minus the personal data and the bank details. Redact first, then compare the result against the source: the pages the diff reports as changed must be exactly the pages the redaction says it painted. Two independent answers agreeing is what makes it a check rather than a hope.",
        fr: "Un contrat fournisseur doit partir chez un partenaire, sans les données personnelles ni les coordonnées bancaires. Caviarder d'abord, puis comparer le résultat à la source : les pages que le diff signale comme modifiées doivent être exactement celles que le caviardage dit avoir masquées. Deux réponses indépendantes qui concordent, c'est ce qui fait une vérification plutôt qu'un espoir.",
      }],
      ["code", "shell", {
        en: "# 1. Redact — JSON form, because we want the counts back\ncurl -s -X POST \"$BASE/api/redact\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/legal/supplier-contract.pdf\",\n    \"entities\": [\"email\", \"iban\", \"phone\"],\n    \"patterns\": [\"Jean Dupont\"],\n    \"client_id\": \"legal\", \"pdf_name\": \"supplier-contract-public\"\n  }'\n# {\"redactions\":[{\"page\":1,\"count\":4},{\"page\":3,\"count\":2}],\"mode\":\"flatten\",…}\n#\n# An empty `redactions` array here means nothing was FOUND — check the source\n# has a text layer before concluding it had nothing to hide.\n\n# 2. Compare source and result: pages 1 and 3 must be the ones that moved\ncurl -s -X POST \"$BASE/api/diff\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"before\": \"/download/legal/supplier-contract.pdf\",\n    \"after\":  \"/download/legal/supplier-contract-public.pdf\",\n    \"threshold\": 0\n  }' | jq '.per_page'",
        fr: "# 1. Caviarder — forme JSON, parce qu'on veut les comptes en retour\ncurl -s -X POST \"$BASE/api/redact\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"pdf\": \"/download/legal/contrat-fournisseur.pdf\",\n    \"entities\": [\"email\", \"iban\", \"phone\"],\n    \"patterns\": [\"Jean Dupont\"],\n    \"client_id\": \"legal\", \"pdf_name\": \"contrat-fournisseur-public\"\n  }'\n# {\"redactions\":[{\"page\":1,\"count\":4},{\"page\":3,\"count\":2}],\"mode\":\"flatten\",…}\n#\n# Un tableau `redactions` vide ici veut dire que rien n'a été TROUVÉ — vérifiez\n# que la source a une couche de texte avant de conclure qu'elle n'avait rien à cacher.\n\n# 2. Comparer source et résultat : les pages 1 et 3 doivent être celles qui bougent\ncurl -s -X POST \"$BASE/api/diff\" -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'Content-Type: application/json' -d '{\n    \"before\": \"/download/legal/contrat-fournisseur.pdf\",\n    \"after\":  \"/download/legal/contrat-fournisseur-public.pdf\",\n    \"threshold\": 0\n  }' | jq '.per_page'",
      }],
      ["note", "info", {
        en: "The redacted file is rebuilt from pixels, so <b>every</b> page differs from its source at the byte level. The diff still tells you what you need: <code>per_page</code> ranks the pages by how much visibly changed, and a page that only lost a text layer barely moves while a page that gained a black rectangle does.",
        fr: "Le fichier caviardé est reconstruit à partir des pixels : <b>toutes</b> les pages diffèrent de leur source au niveau des octets. Le diff vous dit quand même l'essentiel — <code>per_page</code> classe les pages par ampleur du changement visible, et une page qui n'a perdu qu'une couche de texte bouge à peine, là où une page qui a gagné un rectangle noir bouge nettement.",
      }],
    ],
  },

  // ───────────────────────────────────────────── exploitation ─────────────

  {
    key: "limits",
    group: { en: "Operating", fr: "Exploitation" },
    icon: "M12 3l8 4v6c0 5-3.5 7.5-8 8-4.5-.5-8-3-8-8V7z",
    title: { en: "Limits, caching and safety", fr: "Limites, cache et sécurité" },
    lede: {
      en: "What the service refuses, what it remembers, and what it does when it is busy. Reading this before going to production is cheaper than reading it after.",
      fr: "Ce que le service refuse, ce dont il se souvient, et ce qu'il fait quand il est chargé. Lire ceci avant la production coûte moins cher que de le lire après.",
    },
    blocks: [
      ["h", { en: "Timeouts and queueing", fr: "Délais et file d'attente" }],
      ["p", {
        en: "Renders run in a bounded pool. Past the pool, requests queue; past the queue timeout, you get a <code>429</code> with a <code>Retry-After</code> header telling you when a retry has a chance. A whole job — render, preview, redaction or diff, every process and every corrective pass together — is bounded by a single wall-clock deadline, so a request can never outlive the proxy in front of it.",
        fr: "Les rendus tournent dans un pool borné. Au-delà du pool, les requêtes font la queue ; au-delà du délai de file, vous recevez un <code>429</code> avec un en-tête <code>Retry-After</code> qui indique quand un nouvel essai a une chance. Un travail complet — rendu, aperçu, caviardage ou diff, tous les processus et toutes les passes correctrices ensemble — est borné par une seule échéance en temps réel : une requête ne peut donc jamais survivre au proxy placé devant elle.",
      }],
      ["table",
        [{ en: "Status", fr: "Statut" }, { en: "Meaning", fr: "Signification" }, { en: "What to do", fr: "Quoi faire" }],
        [
          ["<code>400</code>", { en: "Malformed request, or a refused URL named explicitly.", fr: "Requête malformée, ou une URL refusée nommée explicitement." }, { en: "Read <code>details</code> — it names the offending field or URL.", fr: "Lisez <code>details</code> — il nomme le champ ou l'URL en cause." }],
          ["<code>401</code>", { en: "Token missing or refused.", fr: "Token absent ou refusé." }, { en: "Check the <code>X-API-Key</code> header.", fr: "Vérifiez l'en-tête <code>X-API-Key</code>." }],
          ["<code>404</code>", { en: "Unknown theme, template or saved PDF.", fr: "Thème, template ou PDF sauvegardé inconnu." }, { en: "An unknown theme lists the ones available.", fr: "Un thème inconnu liste ceux qui sont disponibles." }],
          ["<code>429</code>", { en: "No render slot within the queue timeout.", fr: "Aucun créneau de rendu dans le délai de file." }, { en: "Honour <code>Retry-After</code>; do not hammer.", fr: "Respectez <code>Retry-After</code> ; n'insistez pas en boucle." }],
          ["<code>502</code>", { en: "A service we depend on failed — Mermaid Studio, typically.", fr: "Un service dont nous dépendons a échoué — Mermaid Studio, typiquement." }, { en: "Diagrams normally degrade instead; this is the rest.", fr: "Les diagrammes se dégradent normalement ; ceci est le reste." }],
          ["<code>504</code>", { en: "The job hit its deadline.", fr: "Le travail a atteint son échéance." }, { en: "Split the document, or drop <code>autolayout</code>.", fr: "Découpez le document, ou retirez <code>autolayout</code>." }],
        ],
      ],

      ["h", { en: "The render cache", fr: "Le cache de rendu" }],
      ["p", {
        en: "Rendering is <b>content-addressed</b>: the key hashes the expanded source, your CSS, the engine, the resolved options, the header and footer, and the theme <code>name@version</code>. Identical inputs come back instantly with <code>cached: true</code>. Two consequences are worth knowing before you go looking for a purge endpoint — there is none.",
        fr: "Le rendu est <b>adressé par contenu</b> : la clé hache la source expansée, votre CSS, le moteur, les options résolues, l'en-tête et le pied de page, et le <code>nom@version</code> du thème. Des entrées identiques reviennent instantanément avec <code>cached: true</code>. Deux conséquences méritent d'être connues avant d'aller chercher un endpoint de purge — il n'y en a pas.",
      }],
      ["p", {
        en: "Changing anything that reaches the key <b>invalidates nothing and evicts nothing</b>; it simply produces a different key, and both entries coexist until TTL or LRU drops them. And changing a <i>published</i> <code>theme.css</code> in place does not change the key at all — which is why theme versions exist.",
        fr: "Modifier quoi que ce soit qui atteint la clé <b>n'invalide rien et n'évince rien</b> ; cela produit simplement une clé différente, et les deux entrées coexistent jusqu'à ce que le TTL ou le LRU les supprime. Et modifier un <code>theme.css</code> <i>déjà publié</i> ne change pas la clé du tout — c'est exactement pour cela que les versions de thème existent.",
      }],

      ["h", { en: "What a document may fetch", fr: "Ce qu'un document peut aller chercher" }],
      ["p", {
        en: "WeasyPrint dereferences every URL a document points at, so two layers stand in the way. The request is rejected with a <b>400 naming the offending URL</b> when the document or your CSS references <code>file://</code>, a private address, a cloud metadata endpoint, or a host outside the allowlist. At render time the fetcher re-checks every URL, re-resolving DNS and pinning the validated IP — a remote resource that is refused is simply <b>absent from the PDF</b> rather than an error.",
        fr: "WeasyPrint déréférence chaque URL vers laquelle pointe un document : deux couches se tiennent donc sur le chemin. La requête est rejetée par un <b>400 nommant l'URL fautive</b> quand le document ou votre CSS référence <code>file://</code>, une adresse privée, un endpoint de métadonnées cloud, ou un hôte hors liste blanche. Au rendu, le récupérateur revérifie chaque URL, re-résout le DNS et épingle l'IP validée — une ressource distante refusée est simplement <b>absente du PDF</b> plutôt qu'une erreur.",
      }],
      ["note", "info", {
        en: "This is why <code>weasyprint</code> is the default and the only engine available without a token. <code>wkhtmltopdf</code> and <code>pdflatex</code> only get the first layer — the guarded fetcher has no equivalent there.",
        fr: "C'est pour cela que <code>weasyprint</code> est le moteur par défaut et le seul disponible sans token. <code>wkhtmltopdf</code> et <code>pdflatex</code> n'ont que la première couche — le récupérateur gardé n'a pas d'équivalent chez eux.",
      }],

      ["h", { en: "Tracing a request", fr: "Tracer une requête" }],
      ["p", {
        en: "Every response carries an <code>X-Request-Id</code>. Send your own on the way in — printable ASCII, 128 characters at most — and it is echoed back and attached to every log event the request produces, including the render event with its duration, engine and cache status. That identifier is what turns \"a PDF came out wrong yesterday\" into a line in a log.",
        fr: "Chaque réponse porte un <code>X-Request-Id</code>. Envoyez le vôtre à l'aller — ASCII imprimable, 128 caractères au plus — et il est renvoyé et attaché à chaque événement de log produit par la requête, y compris l'événement de rendu avec sa durée, son moteur et son état de cache. Cet identifiant, c'est ce qui transforme « un PDF est sorti de travers hier » en une ligne de log.",
      }],
      ["code", "shell", "curl -i -X POST \"$BASE/api/convert\" \\\n  -H \"X-API-Key: $MDTOPDF_KEY\" \\\n  -H 'X-Request-Id: invoice-batch-2026-07-26-0412' \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"markdown\": \"# Invoice\"}'"],

      ["h", { en: "The open legacy endpoint", fr: "L'endpoint legacy ouvert" }],
      ["p", {
        en: "<code>POST /</code> takes FormData and is <b>not</b> covered by the API key — it never was, by design, so existing integrations keep working. Being open is what shapes what it offers: it renders with <code>weasyprint</code> only, block expansion is off (a <code>```mermaid</code> fence stays a code listing, so an anonymous body of them cannot turn one request into thousands of outbound calls), and a blocked URL is reported rather than fatal. <code>/api/*</code> behaves the other way round on all three counts.",
        fr: "<code>POST /</code> prend du FormData et n'est <b>pas</b> couvert par la clé d'API — il ne l'a jamais été, par conception, pour que les intégrations existantes continuent de fonctionner. Son ouverture façonne ce qu'il propose : il rend uniquement avec <code>weasyprint</code>, l'expansion des blocs est coupée (un bloc <code>```mermaid</code> reste une liste de code, de sorte qu'un corps anonyme rempli de ces blocs ne peut pas transformer une requête en milliers d'appels sortants), et une URL bloquée est signalée plutôt que fatale. <code>/api/*</code> fait l'inverse sur les trois points.",
      }],
      ["try", "health", { en: "Check the service", fr: "Vérifier le service" }],
    ],
  },
];

// ══════════════════════════════════════════════════════════ rendu

const Guides = (() => {
  const BY_KEY = {};
  GUIDES.forEach((g) => (BY_KEY[g.key] = g));

  let filter = "";
  let current = GUIDES[0].key;

  /* Search runs over the rendered language only: filtering French prose with an
     English query would return everything on an empty match. */
  function haystack(guide) {
    const parts = [guide.key, t(guide.title), t(guide.lede), t(guide.group)];
    guide.blocks.forEach((b) => {
      if (b[0] === "h" || b[0] === "p") parts.push(t(b[1]));
    });
    return parts.join(" ").toLowerCase();
  }

  function matches(guide) {
    return !filter || haystack(guide).includes(filter.toLowerCase());
  }

  // ─────────────────────────── sidebar ───────────────────────────

  function renderNav() {
    const kept = GUIDES.filter(matches);

    if (!kept.length) {
      $("#guideNav").innerHTML = `<p class="nav-empty">${escapeHtml(k("guides.nomatch"))}</p>`;
      return;
    }

    // Groups keep the declaration order rather than being sorted: the list is a
    // reading order, from "what is this" to "how do I operate it".
    const groups = [];
    kept.forEach((g) => {
      const label = t(g.group);
      let group = groups.find((x) => x.label === label);
      if (!group) groups.push((group = { label, guides: [] }));
      group.guides.push(g);
    });

    $("#guideNav").innerHTML = groups.map((group) => `
      <div class="nav-group">
        <h4>${escapeHtml(group.label)}</h4>
        ${group.guides.map((g) => `
          <button class="nav-item guide-item${g.key === current ? " active" : ""}" data-key="${g.key}">
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
              <path d="${g.icon}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            <span class="label">${escapeHtml(t(g.title))}</span>
          </button>`).join("")}
      </div>`).join("");

    $$("#guideNav .nav-item").forEach((btn) => {
      btn.onclick = () => { location.hash = "#/guides/" + btn.dataset.key; };
    });
  }

  // ─────────────────────────── blocs ───────────────────────────

  let codeSeq = 0;

  function cell(value) {
    // Cells carry <code> and <b> on purpose, so they are inserted as markup.
    return t(value);
  }

  function renderBlock(block) {
    const [kind] = block;

    if (kind === "h") return `<h3 class="guide-h">${escapeHtml(t(block[1]))}</h3>`;

    if (kind === "p") return `<p class="guide-p">${t(block[1])}</p>`;

    if (kind === "code") {
      const id = "gcode" + ++codeSeq;
      const lang = block[1];
      const code = t(block[2]);
      return `
        <div class="code-wrap">
          <button class="copy-btn" data-copy="#${id}">${escapeHtml(k("guides.copy"))}</button>
          <pre id="${id}" data-raw="${escapeHtml(code)}">${highlightCode(code, lang)}</pre>
        </div>`;
    }

    if (kind === "table") {
      return `
        <div class="table-wrap">
          <table class="grid">
            <thead><tr>${block[1].map((h) => `<th>${escapeHtml(t(h))}</th>`).join("")}</tr></thead>
            <tbody>
              ${block[2].map((row) => `<tr>${row.map((c) => `<td>${cell(c)}</td>`).join("")}</tr>`).join("")}
            </tbody>
          </table>
        </div>`;
    }

    if (kind === "note") {
      return `<div class="callout${block[1] === "warn" ? " warn" : ""}">${t(block[2])}</div>`;
    }

    if (kind === "cards") {
      return `<div class="guide-cards">${block[1].map((c) => `
        <div class="guide-card">
          <h4>${escapeHtml(t(c.t))}</h4>
          <p>${t(c.d)}</p>
        </div>`).join("")}</div>`;
    }

    if (kind === "try") {
      return `<p class="guide-try"><button class="btn primary sm" data-try="${block[1]}">${escapeHtml(t(block[2]))} →</button></p>`;
    }

    if (kind === "themes") return '<div class="theme-gallery" id="themeGallery"></div>';

    return "";
  }

  // ─────────────────────────── galerie de thèmes ───────────────────────────

  // The gallery is the one part of the guides that talks to the service: a
  // screenshot in a repository is a promise about a deployment it cannot see.
  async function loadThemes() {
    const host = $("#themeGallery");
    if (!host) return;

    host.innerHTML = `<p class="nav-empty">${escapeHtml(k("chrome.health.connecting"))}</p>`;

    let themes;
    try {
      const res = await fetch(apiBase() + "/api/themes", {
        headers: state.apiKey ? { "X-API-Key": state.apiKey } : {},
      });
      if (!res.ok) throw new Error(res.status === 401 ? k("key.none") : "HTTP " + res.status);
      themes = await res.json();
    } catch (e) {
      host.innerHTML = `<div class="callout warn">${escapeHtml(k("chrome.health.offline"))} — ${escapeHtml(e.message)}</div>`;
      return;
    }

    if (!Array.isArray(themes) || !themes.length) {
      host.innerHTML = `<p class="nav-empty">—</p>`;
      return;
    }

    host.innerHTML = themes.map((th) => {
      const tokens = Object.entries(th.colors || {}).slice(0, 8);
      return `
        <figure class="theme-card">
          <img loading="lazy" src="${escapeHtml(apiBase() + th.preview_url)}"
               alt="${escapeHtml(th.label)}">
          <figcaption>
            <div class="theme-head">
              <strong>${escapeHtml(th.label)}</strong>
              <code>${escapeHtml(th.name)}@${th.version}</code>
              ${th.latest ? '<span class="tag">latest</span>' : ""}
              ${th.cover ? '<span class="tag dashed">cover</span>' : ""}
            </div>
            <p>${escapeHtml(th.description)}</p>
            <div class="theme-tokens">
              ${tokens.map(([name, hex]) => `
                <span class="token" title="${escapeHtml(name + " " + hex)}">
                  <i style="background:${escapeHtml(hex)}"></i>${escapeHtml(name)}
                </span>`).join("")}
            </div>
            <p class="theme-fonts">${(th.fonts || []).map((f) => escapeHtml(f)).join(" · ")}</p>
          </figcaption>
        </figure>`;
    }).join("");
  }

  // ─────────────────────────── détail ───────────────────────────

  function neighbours(key) {
    const i = GUIDES.findIndex((g) => g.key === key);
    return {
      prev: i > 0 ? GUIDES[i - 1] : null,
      next: i >= 0 && i < GUIDES.length - 1 ? GUIDES[i + 1] : null,
    };
  }

  function render(key) {
    const guide = BY_KEY[key];
    if (!guide) return;
    current = key;

    const { prev, next } = neighbours(key);

    $("#guideBody").innerHTML = `
      <div class="doc-head">
        <button class="btn sm ghost doc-back" id="guideBack">${escapeHtml(k("guides.back"))}</button>
        <span class="tag dashed">${escapeHtml(t(guide.group))}</span>
        <span class="spacer"></span>
      </div>

      <h2 class="doc-title">${escapeHtml(t(guide.title))}</h2>
      <p class="doc-desc">${escapeHtml(t(guide.lede))}</p>

      <div class="guide-body">
        ${guide.blocks.map(renderBlock).join("")}
      </div>

      <div class="doc-prevnext">
        ${prev ? `<button class="btn sm" data-goto="${prev.key}">← ${escapeHtml(t(prev.title))}</button>` : "<span></span>"}
        ${next ? `<button class="btn sm" data-goto="${next.key}">${escapeHtml(t(next.title))} →</button>` : "<span></span>"}
      </div>`;

    $("#guidePane").scrollTop = 0;

    $("#guideBack").onclick = () => { $("#guideSplit").dataset.mobile = "list"; };
    $$("#guideBody [data-goto]").forEach((b) => {
      b.onclick = () => { location.hash = "#/guides/" + b.dataset.goto; };
    });
    $$("#guideBody [data-try]").forEach((b) => {
      b.onclick = () => { location.hash = "#/console/" + b.dataset.try; };
    });

    $$("#guideNav .nav-item").forEach((b) => b.classList.toggle("active", b.dataset.key === key));

    loadThemes();
  }

  function initSearch() {
    const input = $("#guideSearch");
    input.oninput = () => { filter = input.value.trim(); renderNav(); };
    input.onkeydown = (e) => {
      if (e.key !== "Enter") return;
      const first = $("#guideNav .nav-item");
      if (first) location.hash = "#/guides/" + first.dataset.key;
    };
  }

  function init() {
    renderNav();
    initSearch();
    // Le détail est re-rendu par le routeur ; ici seule la liste, qui reste
    // visible depuis les autres vues, a besoin d'être retraduite.
    I18n.onChange(renderNav);
  }

  return { init, render, renderNav, keys: () => GUIDES.map((g) => g.key), byKey: BY_KEY, all: () => GUIDES };
})();
