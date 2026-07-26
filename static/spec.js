/* ==========================================================================
   md-to-pdf — spécification de l'API
   Source unique : elle alimente la référence, la console et les exemples de
   code. La doc ne peut donc pas diverger de ce que la console envoie.
   ========================================================================== */
"use strict";

// ══════════════════════════════════════════════════════════ exemples

// Vitrine : le document par défaut de la console exerce réellement les trois
// nouveautés (graphique, diagramme, région censurée) plutôt que de les décrire.
const SAMPLE_MD = () => t({ fr: `# Rapport d'analyse

Un paragraphe en **markdown** avec une liste :

- premier point
- deuxième point

## Chiffre d'affaires

\`\`\`chart
{
  "type": "bar",
  "title": "Chiffre d'affaires par trimestre",
  "labels": ["Q1", "Q2", "Q3", "Q4"],
  "series": [{"name": "2026", "data": [12000, 18000, 15000, 21000]}]
}
\`\`\`

## Parcours d'une requête

\`\`\`mermaid
graph TD;
  A[Requête] --> B[Rendu];
  B --> C[PDF];
\`\`\`

## Section premium

{{CENSOR:start,premium}}
Le détail des marges par segment n'est pas public : ce texte est retiré du
document avant que pandoc ne le voie.
{{CENSOR:end}}

## Conclusion

Contenu de nouveau visible.`, en: `# Analysis report

A paragraph in **markdown** with a list:

- first point
- second point

## Revenue

\`\`\`chart
{
  "type": "bar",
  "title": "Revenue by quarter",
  "labels": ["Q1", "Q2", "Q3", "Q4"],
  "series": [{"name": "2026", "data": [12000, 18000, 15000, 21000]}]
}
\`\`\`

## Path of a request

\`\`\`mermaid
graph TD;
  A[Request] --> B[Render];
  B --> C[PDF];
\`\`\`

## Premium section

{{CENSOR:start,premium}}
The margin breakdown by segment is not public: this text is removed from the
document before pandoc ever sees it.
{{CENSOR:end}}

## Conclusion

Visible content again.` });

const SAMPLE_HTML = () => t({
  fr: `<h1>Facture #123</h1>
<p>Client : <strong>ACME</strong></p>
<table border="1" cellpadding="6" cellspacing="0">
  <tr><th>Article</th><th>Prix</th></tr>
  <tr><td>Abonnement</td><td>49,00 €</td></tr>
</table>`,
  en: `<h1>Invoice #123</h1>
<p>Customer: <strong>ACME</strong></p>
<table border="1" cellpadding="6" cellspacing="0">
  <tr><th>Item</th><th>Price</th></tr>
  <tr><td>Subscription</td><td>49.00 €</td></tr>
</table>`,
});

const SAMPLE_TEMPLATE = () => t({
  fr: `<h1>{{ title }}</h1>
<p>Bonjour {{ name }},</p>
<ul>
  {% for line in lines %}<li>{{ line }}</li>{% endfor %}
</ul>`,
  en: `<h1>{{ title }}</h1>
<p>Hello {{ name }},</p>
<ul>
  {% for line in lines %}<li>{{ line }}</li>{% endfor %}
</ul>`,
});

const SAMPLE_DATA = () => t({
  fr: `{
  "title": "Facture #123",
  "name": "Jean Dupont",
  "lines": ["Abonnement annuel", "Support premium"]
}`,
  en: `{
  "title": "Invoice #123",
  "name": "Jane Doe",
  "lines": ["Annual subscription", "Premium support"]
}`,
});

// ══════════════════════════════════════════════════════════ statuts partagés

// Les mêmes quatre ou cinq statuts reviennent sur presque tous les endpoints :
// les nommer une fois évite quinze traductions divergentes de « Token absent ».
const ST = {
  pdfOrUrl: { en: "PDF or download_url", fr: "PDF ou download_url" },
  badRequest: { en: "Invalid request", fr: "Requête invalide" },
  unauthorized: { en: "Token missing or refused", fr: "Token absent ou refusé" },
  notFound: { en: "PDF not found", fr: "PDF introuvable" },
  timeout: { en: "Timeout", fr: "Timeout" },
};

// ══════════════════════════════════════════════════════════ blocs de champs

const cssField = {
  name: "css", type: "textarea", rows: 3, label: "css",
  hint: { en: "appended after templates/default.css", fr: "concaténé après templates/default.css" }, value: "",
};

const engineField = {
  name: "engine", type: "select", label: "engine",
  options: ["", "weasyprint", "wkhtmltopdf", "pdflatex"], value: "",
};

const optionsBlock = () => ({
  type: "fieldset", legend: { en: "options (layout)", fr: "options (mise en page)" }, collapsed: true, fields: [
    { type: "row", fields: [
      { name: "options.paper_size", type: "select", label: "paper_size", options: ["", "a4", "a3", "letter"], value: "" },
      { name: "options.orientation", type: "select", label: "orientation", options: ["", "portrait", "landscape"], value: "" },
    ]},
    { type: "row", fields: [
      { name: "options.margins.top", type: "text", label: { en: "margin top", fr: "marge haut" }, placeholder: "2cm", value: "" },
      { name: "options.margins.right", type: "text", label: { en: "right", fr: "droite" }, placeholder: "2cm", value: "" },
      { name: "options.margins.bottom", type: "text", label: { en: "bottom", fr: "bas" }, placeholder: "2cm", value: "" },
      { name: "options.margins.left", type: "text", label: { en: "left", fr: "gauche" }, placeholder: "2cm", value: "" },
    ]},
    { name: "options.page_numbers", type: "checkbox", label: "page_numbers", value: false },
    { name: "options.page_number_format", type: "text", label: "page_number_format",
      hint: { en: 'CSS content value, e.g. counter(page) " / " counter(pages)', fr: 'valeur CSS content, ex. counter(page) " / " counter(pages)' }, value: "" },
    { type: "row", fields: [
      { name: "options.toc", type: "checkbox", label: "toc", value: false },
      { name: "options.toc_depth", type: "number", label: "toc_depth", value: "" },
    ]},
    { name: "options.watermark", type: "text", label: { en: "watermark (CSS)", fr: "watermark (filigrane CSS)" }, value: "" },
    { type: "row", fields: [
      { name: "options.theme", type: "select", label: "theme",
        options: ["", "aismarttalk", "report", "minimal"], value: "" },
      { name: "options.autolayout", type: "checkbox", label: "autolayout", value: false },
    ]},
    { name: "options.censor_label", type: "text", label: "censor_label",
      hint: { en: "wording of CENSOR blocks with no named level", fr: "libellé des blocs CENSOR sans niveau nommé" }, value: "" },
    { name: "options.charts", type: "bool", label: "charts",
      hint: { en: "false turns off chart and mermaid expansion", fr: "false coupe l'expansion des blocs chart et mermaid" }, value: "" },
    { type: "row", fields: [
      { name: "options.cover.title", type: "text", label: "cover.title", value: "" },
      { name: "options.cover.subtitle", type: "text", label: "cover.subtitle", value: "" },
    ]},
    { type: "row", fields: [
      { name: "options.cover.logo", type: "text", label: "cover.logo", placeholder: "https://…/logo.png", value: "" },
      { name: "options.cover.date", type: "text", label: "cover.date", value: "" },
    ]},
  ],
});

const saveBlock = () => ({
  type: "fieldset", legend: { en: "server-side save — otherwise a binary PDF comes back", fr: "sauvegarde serveur — sinon PDF binaire en réponse" }, collapsed: false, fields: [
    { type: "row", fields: [
      { name: "client_id", type: "text", label: "client_id", placeholder: "demo-client", value: "" },
      { name: "pdf_name", type: "text", label: "pdf_name", placeholder: "mon-document", value: "" },
    ]},
  ],
});

const headerFooterBlock = () => ({
  type: "fieldset", legend: { en: "header / footer", fr: "en-tête / pied de page" }, collapsed: true, fields: [
    { name: "header_html", type: "textarea", rows: 2, label: "header_html", hint: { en: "takes priority over header_template", fr: "prioritaire sur header_template" }, value: "" },
    { name: "footer_html", type: "textarea", rows: 2, label: "footer_html", value: "" },
    { type: "row", fields: [
      { name: "header_template", type: "text", label: "header_template", placeholder: "header.html", value: "" },
      { name: "footer_template", type: "text", label: "footer_template", placeholder: "footer.html", value: "" },
    ]},
  ],
});

// Paramètres communs, pour les tables de la référence
const OPTIONS_PARAM = {
  name: "options", type: "object", desc: {
    en: "Layout: <code>paper_size</code> (a4, a3, letter), <code>orientation</code>, " +
      "<code>margins</code> {top, right, bottom, left}, <code>page_numbers</code>, " +
      "<code>page_number_format</code>, <code>toc</code>, <code>toc_depth</code>, <code>watermark</code>.\n" +
      "Presentation: <code>theme</code> (\"name\" or \"name@2\"), <code>cover</code> {title, subtitle, logo, date}, " +
      "<code>censor_label</code>, <code>charts</code> (<code>false</code> turns off chart and mermaid blocks), " +
      "<code>autolayout</code> (audits the output and corrects it, report in the <code>layout</code> field).",
    fr: "Mise en page : <code>paper_size</code> (a4, a3, letter), <code>orientation</code>, " +
      "<code>margins</code> {top, right, bottom, left}, <code>page_numbers</code>, " +
      "<code>page_number_format</code>, <code>toc</code>, <code>toc_depth</code>, <code>watermark</code>.\n" +
      "Habillage : <code>theme</code> (\"nom\" ou \"nom@2\"), <code>cover</code> {title, subtitle, logo, date}, " +
      "<code>censor_label</code>, <code>charts</code> (<code>false</code> coupe les blocs chart et mermaid), " +
      "<code>autolayout</code> (analyse la sortie et la recorrige, rapport dans le champ <code>layout</code>).",
  },
};

const SAVE_PARAMS = [
  { name: "client_id", type: "string", desc: {
    en: "Destination folder. Plain name: <code>[A-Za-z0-9._-]</code>, 128 characters at most, not starting with a dot.",
    fr: "Dossier de destination. Nom simple : <code>[A-Za-z0-9._-]</code>, 128 caractères max, ne commence pas par un point." } },
  { name: "pdf_name", type: "string", desc: {
    en: "File name (the <code>.pdf</code> suffix is added when missing). Same constraints as <code>client_id</code>.",
    fr: "Nom du fichier (le suffixe <code>.pdf</code> est ajouté s'il manque). Mêmes contraintes que <code>client_id</code>." } },
];

const CSS_PARAM = { name: "css", type: "string", desc: {
  en: "Extra stylesheet, applied after <code>templates/default.css</code>.",
  fr: "Feuille de style additionnelle, appliquée après <code>templates/default.css</code>." } };

// ══════════════════════════════════════════════════════════ endpoints

const ENDPOINTS = [
  {
    key: "health", method: "GET", path: "/api/health", auth: false, group: "service",
    title: { en: "Service health", fr: "Santé du service" },
    icon: "M12 21s-7-4.4-7-10a7 7 0 0114 0c0 5.6-7 10-7 10z",
    card: { en: "Status, version and the PDF engines actually installed in the image. No token: this is the healthcheck probe.",
      fr: "Statut, version et moteurs PDF réellement installés dans l'image. Sans token : c'est la sonde du healthcheck." },
    desc: { en: "Returns the service status, its version and the list of PDF engines actually present in the image.\n\"status\" is \"degraded\" when pandoc or WeasyPrint are missing.",
      fr: "Retourne le statut du service, sa version et la liste des moteurs PDF réellement présents dans l'image.\n« status » vaut « degraded » si pandoc ou WeasyPrint manquent." },
    params: [],
    fields: [],
    example: { response: { status: "ok", version: "0.2.0", engines: ["weasyprint", "wkhtmltopdf", "pdflatex"] } },
    statuses: [["200", { en: "Service reachable", fr: "Service joignable" }]],
  },

  {
    key: "convert", method: "POST", path: "/api/convert", json: true, group: "generate",
    title: { en: "Markdown → PDF", fr: "Markdown → PDF" },
    icon: "M4 4h11l5 5v11H4z",
    card: { en: "Markdown conversion through pandoc, with CSS, headers, table of contents and CENSOR block replacement.",
      fr: "Conversion Markdown via pandoc, avec CSS, en-têtes, sommaire et remplacement des blocs CENSOR." },
    desc: { en: "Converts Markdown to PDF through pandoc.\n" +
        "Point CENSOR tags ({{CENSOR}}, <CENSOR>) and regions ({{CENSOR:start}} … {{CENSOR:end}}) are removed from the document before pandoc: the hidden text never reaches the PDF.\n" +
        "```chart and ```mermaid blocks become inline SVG; a block that cannot be rendered stays a code block and the reason comes back in the \"warnings\" field.",
      fr: "Convertit du Markdown en PDF via pandoc.\n" +
        "Les tags CENSOR ponctuels ({{CENSOR}}, <CENSOR>) et les régions ({{CENSOR:start}} … {{CENSOR:end}}) sont retirés du document avant pandoc : le texte caché n'atteint jamais le PDF.\n" +
        "Les blocs ```chart et ```mermaid deviennent du SVG inline ; un bloc qui ne peut pas être rendu reste un bloc de code et la raison revient dans le champ « warnings »." },
    params: [
      { name: "markdown", type: "string", required: true, desc: { en: "The source document.", fr: "Le document source." } },
      CSS_PARAM,
      { name: "engine", type: "enum", desc: { en: "<code>weasyprint</code> (default), <code>wkhtmltopdf</code> or <code>pdflatex</code>.", fr: "<code>weasyprint</code> (défaut), <code>wkhtmltopdf</code> ou <code>pdflatex</code>." } },
      OPTIONS_PARAM,
      { name: "header_html", type: "string", desc: { en: "HTML injected into the header. Takes priority over <code>header_template</code>.", fr: "HTML injecté dans l'en-tête. Prioritaire sur <code>header_template</code>." } },
      { name: "footer_html", type: "string", desc: { en: "HTML injected after the document body.", fr: "HTML injecté après le corps du document." } },
      { name: "header_template", type: "string", desc: { en: "Name of a file in <code>templates/</code>, e.g. <code>header.html</code>.", fr: "Nom d'un fichier du dossier <code>templates/</code>, par ex. <code>header.html</code>." } },
      { name: "footer_template", type: "string", desc: { en: "Same, for the footer.", fr: "Idem pour le pied de page." } },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "markdown", type: "textarea", rows: 12, label: "markdown", required: true, value: SAMPLE_MD },
      cssField, engineField, optionsBlock(), headerFooterBlock(), saveBlock(),
    ],
    example: {
      request: () => ({
        markdown: t({ en: "# Report", fr: "# Rapport" }) + "\n\n```chart\n" +
          "{\"type\": \"bar\", \"labels\": [\"Q1\", \"Q2\"], " +
          "\"series\": [{\"name\": \"2026\", \"data\": [12000, 18000]}]}\n```\n\n" +
          "```mermaid\ngraph TD; A[" + t({ en: "Request", fr: "Requête" }) + "] --> B[PDF];\n```\n\n" +
          "{{CENSOR:start,premium}}\n" + t({ en: "Subscribers only.", fr: "Réservé aux abonnés." }) + "\n{{CENSOR:end}}\n",
        options: { paper_size: "a4", page_numbers: true, theme: "report" },
        client_id: "demo-client", pdf_name: t({ en: "report-2026", fr: "rapport-2026" }),
      }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "report-2026", fr: "rapport-2026" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", ST.badRequest], ["401", ST.unauthorized], ["404", { en: "Template not found", fr: "Template introuvable" }], ["500", { en: "pandoc failed", fr: "Échec pandoc" }], ["504", ST.timeout]],
  },

  {
    key: "render", method: "POST", path: "/api/render", json: true, group: "generate",
    title: { en: "Tera template → PDF", fr: "Template Tera → PDF" },
    icon: "M4 6h16M4 12h10M4 18h7",
    card: { en: "Renders a Tera template (Jinja2-like syntax) with your JSON data, then converts through WeasyPrint.",
      fr: "Rendu d'un template Tera (syntaxe Jinja2) avec vos données JSON, puis conversion via WeasyPrint." },
    desc: { en: "Renders a Tera HTML template with the data provided, then converts the result to PDF through WeasyPrint.\nThe \"data\" field must be a JSON object.",
      fr: "Rend un template HTML au format Tera avec les données fournies, puis convertit le résultat en PDF via WeasyPrint.\nLe champ « data » doit être un objet JSON." },
    params: [
      { name: "template", type: "string", required: true, desc: { en: "HTML template, Tera syntax: <code>{{ variable }}</code>, <code>{% for %}</code>, filters.", fr: "Template HTML, syntaxe Tera : <code>{{ variable }}</code>, <code>{% for %}</code>, filtres." } },
      { name: "data", type: "object", required: true, desc: { en: "Render context. Must be an object (an array or a string answers 400).", fr: "Contexte de rendu. Doit être un objet (un tableau ou une chaîne renvoie 400)." } },
      CSS_PARAM, OPTIONS_PARAM, ...SAVE_PARAMS,
    ],
    fields: [
      { name: "template", type: "textarea", rows: 8, label: "template", required: true, value: SAMPLE_TEMPLATE },
      { name: "data", type: "json", rows: 6, label: "data", required: true, value: SAMPLE_DATA },
      cssField, optionsBlock(), saveBlock(),
    ],
    example: {
      request: () => ({ template: "<h1>{{ title }}</h1>", data: { title: t({ en: "Invoice #123", fr: "Facture #123" }) } }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "invoice-123", fr: "facture-123" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", { en: "Invalid template or data", fr: "Template ou data invalide" }], ["401", ST.unauthorized], ["500", { en: "WeasyPrint failed", fr: "Échec WeasyPrint" }], ["504", ST.timeout]],
  },

  {
    key: "html-to-pdf", method: "POST", path: "/api/html-to-pdf", json: true, group: "generate",
    title: { en: "HTML → PDF", fr: "HTML → PDF" },
    icon: "M8 6l-4 6 4 6M16 6l4 6-4 6",
    card: { en: "For HTML you already build client-side. Relative paths resolve from the service directory.",
      fr: "Pour du HTML déjà construit côté client. Les chemins relatifs sont résolus depuis le répertoire du service." },
    desc: { en: "Converts raw HTML to PDF through WeasyPrint, without going through pandoc.\nRelative paths (images, static/…) resolve from the server working directory.",
      fr: "Convertit du HTML brut en PDF via WeasyPrint, sans passer par pandoc.\nLes chemins relatifs (images, static/…) sont résolus depuis le répertoire de travail du serveur." },
    params: [
      { name: "html", type: "string", required: true, desc: { en: "Full HTML document or fragment.", fr: "Document HTML complet ou fragment." } },
      CSS_PARAM, OPTIONS_PARAM, ...SAVE_PARAMS,
    ],
    fields: [
      { name: "html", type: "textarea", rows: 12, label: "html", required: true, value: SAMPLE_HTML },
      cssField, optionsBlock(), saveBlock(),
    ],
    example: {
      request: () => ({ html: "<h1>" + t({ en: "Invoice", fr: "Facture" }) + "</h1>", options: { paper_size: "a4" } }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "invoice", fr: "facture" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", ST.badRequest], ["401", ST.unauthorized], ["500", { en: "WeasyPrint failed", fr: "Échec WeasyPrint" }], ["504", ST.timeout]],
  },

  {
    key: "preview", method: "POST", path: "/api/preview", json: true, group: "generate",
    title: { en: "PNG preview", fr: "Aperçu PNG" },
    icon: "M4 5h16v14H4zM8 13l3-3 3 3 2-2 2 2",
    card: { en: "The first page as a 150 dpi PNG — a thumbnail, or a look before generating.",
      fr: "La première page en PNG 150 dpi, pour une vignette ou un aperçu avant génération." },
    desc: { en: "Renders the first page as PNG (150 dpi).\nExclusive modes, evaluated in this order: markdown, template + data, html.",
      fr: "Rend la première page en PNG (150 dpi).\nModes exclusifs, évalués dans cet ordre : markdown, template + data, html." },
    params: [
      { name: "markdown", type: "string", desc: { en: "Markdown mode (takes priority).", fr: "Mode markdown (prioritaire)." } },
      { name: "template", type: "string", desc: { en: "Template mode, to combine with <code>data</code>.", fr: "Mode template, à combiner avec <code>data</code>." } },
      { name: "data", type: "object", desc: { en: "Template context. Required when <code>template</code> is given.", fr: "Contexte du template. Obligatoire si <code>template</code> est fourni." } },
      { name: "html", type: "string", desc: { en: "Raw HTML mode.", fr: "Mode HTML brut." } },
      CSS_PARAM,
      { name: "engine", type: "enum", desc: { en: "Engine used in markdown mode.", fr: "Moteur utilisé en mode markdown." } },
      OPTIONS_PARAM,
      { name: "pages", type: "string", desc: { en: "<code>\"3\"</code>, <code>\"2-5\"</code> or <code>\"all\"</code>. Absent: page 1 alone.", fr: "<code>\"3\"</code>, <code>\"2-5\"</code> ou <code>\"all\"</code>. Absent : la page 1 seule." } },
      { name: "dpi", type: "number", desc: { en: "36 to 300. Default <code>150</code>.", fr: "36 à 300. Défaut <code>150</code>." } },
      { name: "layout", type: "enum", desc: {
        en: "<code>png</code> (raw image), <code>images</code> (JSON, one PNG per page) or <code>sheet</code> " +
          "(contact sheet in a single image). Without a value: <code>png</code> for one page, <code>images</code> beyond.",
        fr: "<code>png</code> (image brute), <code>images</code> (JSON, un PNG par page) ou <code>sheet</code> " +
          "(planche contact en une image). Sans valeur : <code>png</code> pour une page, <code>images</code> au-delà." } },
    ],
    fields: [
      { name: "__mode", type: "select", label: "mode", options: ["markdown", "template", "html"], value: "markdown" },
      { name: "markdown", type: "textarea", rows: 8, label: "markdown", value: SAMPLE_MD, showFor: "markdown" },
      { name: "template", type: "textarea", rows: 6, label: "template", value: SAMPLE_TEMPLATE, showFor: "template" },
      { name: "data", type: "json", rows: 5, label: "data", value: SAMPLE_DATA, showFor: "template" },
      { name: "html", type: "textarea", rows: 8, label: "html", value: SAMPLE_HTML, showFor: "html" },
      cssField, engineField,
      { type: "row", fields: [
        { name: "pages", type: "text", label: "pages", placeholder: "1, 2-5, all", value: "" },
        { name: "dpi", type: "number", label: "dpi", hint: "36 → 300", value: "" },
        { name: "layout", type: "select", label: "layout", options: ["", "png", "images", "sheet"], value: "" },
      ]},
      optionsBlock(),
    ],
    example: {
      request: () => ({ markdown: t({ en: "# Preview", fr: "# Aperçu" }) }),
      responseNote: { en: "Binary image/png body, or JSON { pages: [{ page, png, width, height }], pages_total }", fr: "Corps binaire image/png, ou JSON { pages: [{ page, png, width, height }], pages_total }" },
    },
    statuses: [["200", { en: "image/png or JSON", fr: "image/png ou JSON" }], ["400", { en: "No mode given / range outside the document", fr: "Aucun mode fourni / plage hors document" }], ["401", ST.unauthorized], ["500", { en: "pdftoppm failed", fr: "Échec pdftoppm" }], ["504", ST.timeout]],
  },

  {
    key: "merge", method: "POST", path: "/api/merge", json: true, group: "process",
    title: { en: "Merge PDFs", fr: "Fusion de PDFs" },
    icon: "M7 4h7l4 4v12H7zM3 8h3v12h9",
    card: { en: "Concatenates already-saved PDFs, in the order given (pdfunite).",
      fr: "Concatène des PDFs déjà sauvegardés, dans l'ordre fourni (pdfunite)." },
    desc: { en: "Merges at least two previously saved PDFs, in array order.\nThe paths are the ones the other endpoints return: /download/<client_id>/<pdf_name>.",
      fr: "Fusionne au moins deux PDFs précédemment sauvegardés, dans l'ordre du tableau.\nLes chemins sont ceux renvoyés par les autres endpoints : /download/<client_id>/<pdf_name>." },
    params: [
      { name: "pdfs", type: "string[]", required: true, desc: { en: "At least 2 <code>/download/…</code> paths. Any path escaping <code>public/pdf</code> is rejected.", fr: "Au moins 2 chemins <code>/download/…</code>. Tout chemin sortant de <code>public/pdf</code> est rejeté." } },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdfs", type: "pdflist", label: "pdfs", required: true,
        hint: { en: "one path per line", fr: "un chemin par ligne" }, value: "" },
      saveBlock(),
    ],
    example: {
      request: () => ({ pdfs: ["/download/demo-client/a.pdf", "/download/demo-client/b.pdf"], client_id: "demo-client", pdf_name: t({ en: "complete-file", fr: "dossier-complet" }) }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "complete-file", fr: "dossier-complet" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", { en: "Fewer than 2 PDFs / invalid path", fr: "Moins de 2 PDFs / chemin invalide" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "pdfunite failed", fr: "Échec pdfunite" }], ["504", ST.timeout]],
  },

  {
    key: "watermark", method: "POST", path: "/api/watermark", json: true, group: "process",
    title: { en: "Watermark", fr: "Filigrane" },
    icon: "M12 3l7 4v6c0 4-3 7-7 8-4-1-7-4-7-8V7z",
    card: { en: "Overlays diagonal text on every page of an existing PDF (qpdf overlay).",
      fr: "Superpose un texte en diagonale sur toutes les pages d'un PDF existant (overlay qpdf)." },
    desc: { en: "Builds a layer holding the text, then overlays it on the source PDF with qpdf.\nThe text is escaped: no injection risk in the HTML layer.",
      fr: "Génère un calque contenant le texte puis le superpose au PDF source avec qpdf.\nLe texte est échappé : aucun risque d'injection dans le calque HTML." },
    params: [
      { name: "pdf", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the source PDF.", fr: "Chemin <code>/download/…</code> du PDF source." } },
      { name: "text", type: "string", required: true, desc: { en: "Watermark text.", fr: "Texte du filigrane." } },
      { name: "opacity", type: "number", desc: { en: "Between 0 and 1. Default <code>0.06</code>.", fr: "Entre 0 et 1. Défaut <code>0.06</code>." } },
      { name: "angle", type: "number", desc: { en: "Between -360 and 360 degrees. Default <code>-45</code>.", fr: "Entre -360 et 360 degrés. Défaut <code>-45</code>." } },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "text", type: "text", label: "text", required: true, value: { en: "DRAFT", fr: "BROUILLON" } },
      { type: "row", fields: [
        { name: "opacity", type: "number", step: "0.01", label: "opacity", hint: "0 → 1", value: "0.06" },
        { name: "angle", type: "number", step: "1", label: "angle", hint: "-360 → 360", value: "-45" },
      ]},
      saveBlock(),
    ],
    example: {
      request: () => ({ pdf: "/download/demo-client/report.pdf", text: t({ en: "CONFIDENTIAL", fr: "CONFIDENTIEL" }), opacity: 0.08 }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "report-watermarked", fr: "report-filigrane" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", { en: "opacity / angle out of range", fr: "opacity / angle hors bornes" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "qpdf failed", fr: "Échec qpdf" }], ["504", ST.timeout]],
  },

  {
    key: "protect", method: "POST", path: "/api/protect", json: true, group: "process",
    title: { en: "Password protection", fr: "Protection par mot de passe" },
    icon: "M7 11V8a5 5 0 0110 0v3M5 11h14v9H5z",
    card: { en: "AES-256 encryption through qpdf. The password travels in an argument file, never on the command line.",
      fr: "Chiffrement AES-256 via qpdf. Le mot de passe transite par un fichier d'arguments, jamais par la ligne de commande." },
    desc: { en: "Encrypts a saved PDF with AES-256 (user password = owner password).",
      fr: "Chiffre un PDF sauvegardé en AES-256 (mot de passe utilisateur = mot de passe propriétaire)." },
    params: [
      { name: "pdf", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the source PDF.", fr: "Chemin <code>/download/…</code> du PDF source." } },
      { name: "password", type: "string", required: true, desc: { en: "Non-empty, no line break.", fr: "Non vide, sans retour à la ligne." } },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "password", type: "text", label: "password", required: true, value: "secret123" },
      saveBlock(),
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf", password: "s3cr3t" },
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "report-protected", fr: "report-protege" }) + ".pdf" }),
    },
    statuses: [["200", ST.pdfOrUrl], ["400", { en: "Empty password", fr: "Mot de passe vide" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "qpdf failed", fr: "Échec qpdf" }], ["504", ST.timeout]],
  },

  {
    key: "redact", method: "POST", path: "/api/redact", json: true, group: "process",
    title: { en: "Redaction", fr: "Caviardage" },
    icon: "M4 7h16v4H4zM4 14h10v4H4z",
    card: { en: "Blacks out strings and entities (e-mail, IBAN, phone, SIRET, card) then rebuilds the pages as images.",
      fr: "Noircit des chaînes et des entités (e-mail, IBAN, téléphone, SIRET, carte) puis reconstruit les pages en image." },
    desc: { en: "Locates the text to hide, paints black rectangles, then REBUILDS every page from pixels.\nThe redacted text is gone from the file — as are the text layer, bookmarks, annotations and metadata of the source. The result is neither selectable nor searchable, and it is larger.",
      fr: "Localise le texte à masquer, pose des rectangles noirs, puis RECONSTRUIT chaque page à partir de pixels.\nLe texte caviardé disparaît du fichier — comme la couche texte, les signets, les annotations et les métadonnées de la source. Le résultat n'est ni sélectionnable ni cherchable, et il est plus lourd." },
    params: [
      { name: "pdf", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the source PDF.", fr: "Chemin <code>/download/…</code> du PDF source." } },
      { name: "patterns", type: "string[]", desc: {
        en: "LITERAL strings (not regular expressions), compared case-insensitively with normalised whitespace. " +
          "A pattern that looks like a regex (<code>\\d{4}</code>, <code>[A-Z]+</code>, <code>.*</code>) is refused with a 400.",
        fr: "Chaînes LITTÉRALES (pas des expressions régulières), comparées sans casse et à blancs normalisés. " +
          "Un motif qui ressemble à une regex (<code>\\d{4}</code>, <code>[A-Z]+</code>, <code>.*</code>) est refusé en 400." } },
      { name: "entities", type: "string[]", desc: {
        en: "<code>email</code>, <code>iban</code>, <code>phone</code>, <code>siret</code>, <code>credit_card</code>. " +
          "Each is validated by its check digits.",
        fr: "<code>email</code>, <code>iban</code>, <code>phone</code>, <code>siret</code>, <code>credit_card</code>. " +
          "Chacune est validée par sa clé de contrôle." } },
      { name: "dpi", type: "number", desc: { en: "72 to 400. Default <code>200</code>.", fr: "72 à 400. Défaut <code>200</code>." } },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "patterns", type: "list", rows: 3, label: "patterns",
        hint: { en: "one literal string per line — not a regular expression", fr: "une chaîne littérale par ligne — pas une expression régulière" }, value: "" },
      { name: "entities", type: "list", rows: 2, label: "entities",
        hint: { en: "one per line: email, iban, phone, siret, credit_card", fr: "une par ligne : email, iban, phone, siret, credit_card" }, value: "email" },
      { name: "dpi", type: "number", label: "dpi", hint: "72 → 400", value: "" },
      saveBlock(),
    ],
    example: {
      request: () => ({ pdf: "/download/demo-client/contract.pdf", patterns: ["Jean Dupont"], entities: ["email", "iban"], client_id: "demo-client", pdf_name: t({ en: "contract-redacted", fr: "contract-caviarde" }) }),
      response: () => ({ download_url: "/download/demo-client/" + t({ en: "contract-redacted", fr: "contract-caviarde" }) + ".pdf", redactions: [{ page: 1, count: 3 }], pages: 2, mode: "flatten" }),
    },
    statuses: [["200", { en: "PDF or JSON { download_url, redactions, pages, mode, notice }", fr: "PDF ou JSON { download_url, redactions, pages, mode, notice }" }], ["400", { en: "Neither patterns nor entities / regex-looking pattern / unknown entity / dpi out of range", fr: "Ni patterns ni entities / motif regex / entité inconnue / dpi hors bornes" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "Non-conforming output (residual text)", fr: "Sortie non conforme (texte résiduel)" }], ["504", ST.timeout]],
  },

  {
    key: "layout", method: "POST", path: "/api/layout", json: true, group: "process",
    title: { en: "Layout audit", fr: "Audit de mise en page" },
    icon: "M4 4h16v16H4zM4 9h16M9 9v11",
    card: { en: "Analyses an existing PDF: overflow, blank pages, orphan headings, split tables.",
      fr: "Analyse un PDF existant : débordements, pages blanches, titres orphelins, tableaux coupés." },
    desc: { en: "The same analysis as options.autolayout, applied to a PDF already produced — including one made elsewhere.\nChanges nothing: it returns a report and a score out of 100.",
      fr: "Même analyse que options.autolayout, appliquée à un PDF déjà produit — y compris fabriqué ailleurs.\nNe modifie rien : elle rend un rapport et un score sur 100." },
    params: [
      { name: "pdf", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the PDF to audit.", fr: "Chemin <code>/download/…</code> du PDF à auditer." } },
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf" },
      response: { pages: 4, score: 88, issues: [{ kind: "orphan_heading", severity: "warn", page: 2 }] },
    },
    statuses: [["200", "LayoutReport"], ["400", { en: "Invalid path", fr: "Chemin invalide" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "poppler failed", fr: "Échec poppler" }], ["504", ST.timeout]],
  },

  {
    key: "diff", method: "POST", path: "/api/diff", json: true, group: "process",
    title: { en: "Visual diff", fr: "Comparaison visuelle" },
    icon: "M9 4H5v16h4zM15 4h4v16h-4zM12 4v16",
    card: { en: "Compares two PDFs pixel by pixel and says which pages moved. Built for continuous integration.",
      fr: "Compare deux PDFs pixel à pixel et dit quelles pages ont bougé. Fait pour l'intégration continue." },
    desc: { en: "Rasterises both documents and compares the pixels page by page.\nThe threshold defaults to 0: any page that visibly moves makes the verdict \"changed\". That is what a CI expects; a tolerant default would report \"identical\" on a renamed heading.",
      fr: "Rastérise les deux documents et compare les pixels page par page.\nLe seuil vaut 0 par défaut : toute page qui bouge visiblement rend le verdict « changed ». C'est ce qu'attend une CI ; un défaut tolérant annoncerait « identical » sur un titre renommé." },
    params: [
      { name: "before", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the reference version.", fr: "Chemin <code>/download/…</code> de la version de référence." } },
      { name: "after", type: "string", required: true, desc: { en: "<code>/download/…</code> path of the version to check.", fr: "Chemin <code>/download/…</code> de la version à contrôler." } },
      { name: "dpi", type: "number", desc: { en: "36 to 300. Default <code>100</code>.", fr: "36 à 300. Défaut <code>100</code>." } },
      { name: "threshold", type: "number", desc: { en: "Share of changed pixels tolerated. Default <code>0</code>.", fr: "Part de pixels changés tolérée. Défaut <code>0</code>." } },
      { name: "images", type: "boolean", desc: { en: "Returns one highlight image per changed page.", fr: "Renvoie une image de surlignage par page changée." } },
    ],
    fields: [
      { name: "before", type: "pdfpick", label: "before", required: true, value: "" },
      { name: "after", type: "pdfpick", label: "after", required: true, value: "" },
      { type: "row", fields: [
        { name: "dpi", type: "number", label: "dpi", hint: "36 → 300", value: "" },
        { name: "threshold", type: "number", step: "0.001", label: "threshold", hint: { en: "0 = no tolerance", fr: "0 = aucune tolérance" }, value: "" },
      ]},
      { name: "images", type: "checkbox", label: { en: "images (highlight the changed areas)", fr: "images (surlignage des zones changées)" }, value: false },
    ],
    example: {
      request: { before: "/download/demo-client/v1.pdf", after: "/download/demo-client/v2.pdf" },
      response: { pages_total: 6, pages_changed: [4], changed_ratio: 0.00032, verdict: "changed", threshold: 0, dpi: 100 },
    },
    statuses: [["200", "DiffResponse"], ["400", { en: "Invalid path / dpi out of range", fr: "Chemin invalide / dpi hors bornes" }], ["401", ST.unauthorized], ["404", ST.notFound], ["500", { en: "poppler failed", fr: "Échec poppler" }], ["504", ST.timeout]],
  },

  {
    key: "themes", method: "GET", path: "/api/themes", group: "service",
    title: { en: "Available themes", fr: "Thèmes disponibles" },
    icon: "M12 3a9 9 0 100 18h2a3 3 0 003-3 3 3 0 013-3h1a2 2 0 002-2 9 9 0 00-11-10z",
    card: { en: "The brand kits shipped with the service, with their fonts, colour tokens and preview URL.",
      fr: "Les kits de marque livrés avec le service, avec leurs polices, leurs jetons de couleur et l'URL de leur aperçu." },
    desc: { en: "Lists the themes options.theme accepts.\nA theme is immutable: any visual change means a new version, otherwise the cache would keep serving PDFs from the old one.",
      fr: "Liste les thèmes que options.theme accepte.\nUn thème est immuable : toute évolution visuelle donne une nouvelle version, sinon le cache resservirait les PDF de l'ancienne." },
    params: [],
    fields: [],
    example: {
      response: { themes: [{ name: "report", version: 1, label: "Rapport", latest: true, cover: true, preview_url: "/api/themes/report/1/preview.png" }] },
    },
    statuses: [["200", { en: "Theme list", fr: "Liste des thèmes" }], ["401", ST.unauthorized]],
  },

  {
    key: "theme-preview", method: "GET", path: "/api/themes/{name}/{version}/preview.png", group: "service",
    title: { en: "Theme preview", fr: "Aperçu d'un thème" },
    icon: "M4 5h16v14H4zM4 15l4-4 3 3 3-3 6 6",
    card: { en: "The first page of the sample document rendered with a theme. ?cover=true shows the cover page.",
      fr: "La première page du document d'exemple rendue avec un thème. ?cover=true montre la page de couverture." },
    desc: { en: "Renders the sample document with the requested theme and returns the first page as PNG.\nversion accepts \"latest\". The image is cached: the first call costs about a second, the next ones a few milliseconds.",
      fr: "Rend le document d'exemple avec le thème demandé et renvoie la première page en PNG.\nversion accepte « latest ». L'image est mise en cache : le premier appel coûte environ une seconde, les suivants quelques millisecondes." },
    params: [
      { name: "name", type: "path", required: true, desc: { en: "Theme name.", fr: "Nom du thème." } },
      { name: "version", type: "path", required: true, desc: { en: "Version number, or <code>latest</code>.", fr: "Numéro de version, ou <code>latest</code>." } },
      { name: "cover", type: "boolean", desc: { en: "<code>true</code> to render the cover instead of the body.", fr: "<code>true</code> pour rendre la couverture au lieu du corps." } },
    ],
    fields: [
      { type: "row", fields: [
        { name: "name", type: "select", label: "name", options: ["aismarttalk", "report", "minimal"], value: "report" },
        { name: "version", type: "text", label: "version", value: "latest" },
      ]},
      { name: "cover", type: "checkbox", label: "cover", value: false },
    ],
    buildPath: (v) => `/api/themes/${encodeURIComponent(v.name || "")}/${encodeURIComponent(v.version || "latest")}/preview.png` + (v.cover ? "?cover=true" : ""),
    example: { responseNote: { en: "Binary image/png body", fr: "Corps binaire image/png" } },
    statuses: [["200", "image/png"], ["401", ST.unauthorized], ["404", { en: "Unknown theme or version", fr: "Thème ou version inconnus" }], ["504", ST.timeout]],
  },

  {
    key: "metrics", method: "GET", path: "/api/metrics", group: "service",
    title: { en: "Prometheus metrics", fr: "Métriques Prometheus" },
    icon: "M4 19h16M7 16V9M12 16V5M17 16v-6",
    card: { en: "Prometheus exposition: request counters and latency histograms, labelled by route and status code.",
      fr: "Exposition Prometheus : compteurs de requêtes et histogrammes de latence, étiquetés par route et par code." },
    desc: { en: "Returns text/plain; version=0.0.4.\nLabels are limited to the Rocket route pattern and the status code: never a client_id nor a file name.",
      fr: "Renvoie du text/plain; version=0.0.4.\nLes étiquettes se limitent au motif de route Rocket et au code de statut : jamais de client_id ni de nom de fichier." },
    params: [],
    fields: [],
    example: { responseNote: { en: "text/plain body in the Prometheus exposition format", fr: "Corps text/plain au format d'exposition Prometheus" } },
    statuses: [["200", "text/plain"], ["401", ST.unauthorized]],
  },

  {
    key: "download", method: "GET", path: "/download/{client_id}/{pdf_name}", auth: false, group: "files",
    title: { en: "Download", fr: "Téléchargement" },
    icon: "M12 4v10m0 0l-4-4m4 4l4-4M5 20h14",
    card: { en: "Fetches a saved PDF, served as an attachment with the right Content-Disposition.",
      fr: "Récupère un PDF sauvegardé, servi en pièce jointe avec le bon Content-Disposition." },
    desc: { en: "Serves a previously saved PDF. Both segments are validated: escaping public/pdf is impossible.",
      fr: "Sert un PDF précédemment sauvegardé. Les deux segments sont validés : impossible de sortir de public/pdf." },
    params: [
      { name: "client_id", type: "path", required: true, desc: { en: "URL segment.", fr: "Segment d'URL." } },
      { name: "pdf_name", type: "path", required: true, desc: { en: "URL segment, with the extension.", fr: "Segment d'URL, avec l'extension." } },
    ],
    fields: [
      { type: "row", fields: [
        { name: "client_id", type: "text", label: "client_id", required: true, value: "demo-client" },
        { name: "pdf_name", type: "text", label: "pdf_name", required: true, value: "mon-document.pdf" },
      ]},
    ],
    buildPath: (v) => `/download/${encodeURIComponent(v.client_id || "")}/${encodeURIComponent(v.pdf_name || "")}`,
    example: { responseNote: { en: "Binary application/pdf body", fr: "Corps binaire application/pdf" } },
    statuses: [["200", "application/pdf"], ["404", { en: "Unknown file", fr: "Fichier inconnu" }]],
  },

  {
    key: "legacy", method: "POST", path: "/", form: true, auth: false, group: "legacy",
    title: { en: "Legacy endpoint (FormData)", fr: "Endpoint historique (FormData)" },
    icon: "M4 7h16M4 12h16M4 17h10",
    card: { en: "The original API, preserved exactly: FormData in, plain-text errors out.",
      fr: "L'API d'origine, préservée à l'identique : FormData en entrée, erreurs en texte brut." },
    desc: { en: "The original endpoint, kept for backward compatibility. multipart/form-data body, errors returned as plain text.\nNeeds no token, unlike the /api/* routes.",
      fr: "Endpoint d'origine conservé pour la rétro-compatibilité. Corps en multipart/form-data, erreurs renvoyées en texte brut.\nN'exige pas de token, contrairement aux routes /api/*." },
    params: [
      { name: "markdown", type: "field", required: true, desc: { en: "Source document.", fr: "Document source." } },
      { name: "css", type: "field", desc: { en: "Extra CSS.", fr: "CSS additionnel." } },
      { name: "engine", type: "field", desc: { en: "weasyprint, wkhtmltopdf or pdflatex.", fr: "weasyprint, wkhtmltopdf ou pdflatex." } },
      { name: "header_template", type: "field", desc: { en: "File from the <code>templates/</code> folder.", fr: "Fichier du dossier <code>templates/</code>." } },
      { name: "footer_template", type: "field", desc: { en: "Same, for the footer.", fr: "Idem pied de page." } },
      { name: "client_id", type: "field", desc: { en: "Server-side save.", fr: "Sauvegarde serveur." } },
      { name: "pdf_name", type: "field", desc: { en: "Server-side save.", fr: "Sauvegarde serveur." } },
    ],
    fields: [
      { name: "markdown", type: "textarea", rows: 10, label: "markdown", required: true, value: SAMPLE_MD },
      cssField, engineField,
      { type: "row", fields: [
        { name: "header_template", type: "text", label: "header_template", placeholder: "header.html", value: "" },
        { name: "footer_template", type: "text", label: "footer_template", placeholder: "footer.html", value: "" },
      ]},
      saveBlock(),
    ],
    example: { responseNote: { en: "Binary PDF, or {\"download_url\": \"…\"} when client_id and pdf_name are given", fr: "PDF binaire, ou {\"download_url\": \"…\"} si client_id et pdf_name sont fournis" } },
    statuses: [["200", ST.pdfOrUrl], ["400", { en: "pandoc error (plain text)", fr: "Erreur pandoc (texte brut)" }], ["500", { en: "Internal error", fr: "Erreur interne" }]],
  },
];

const BY_KEY = Object.fromEntries(ENDPOINTS.map((e) => [e.key, e]));

// Regroupement des endpoints dans les navigations. L'ordre des groupes est
// celui de la liste ci-dessous ; un endpoint sans groupe connu finit en fin.
const GROUPS = [
  { id: "generate", title: { en: "Generate", fr: "Génération" } },
  { id: "process", title: { en: "Post-processing", fr: "Post-traitement" } },
  { id: "files", title: { en: "Files", fr: "Fichiers" } },
  { id: "service", title: { en: "Service", fr: "Service" } },
  { id: "legacy", title: { en: "Compatibility", fr: "Compatibilité" } },
];

// [{ id, title, endpoints: [...] }] — les groupes vides sont écartés.
const GROUPED = GROUPS
  .map((g) => ({ ...g, endpoints: ENDPOINTS.filter((e) => e.group === g.id) }))
  .filter((g) => g.endpoints.length);

// Ordre de parcours (précédent / suivant dans la référence).
const ORDERED_KEYS = GROUPED.flatMap((g) => g.endpoints.map((e) => e.key));
