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
  // Les outils arrivés avec l'ingestion partagent la même réponse et les mêmes
  // deux façons d'échouer sur leur source.
  toolOk: { en: "Binary file, or JSON { download_url | asset, pages, verdict }",
    fr: "Fichier binaire, ou JSON { download_url | asset, pages, verdict }" },
  sourceMissing: { en: "Unknown asset, or asset expired", fr: "Asset inconnu, ou asset expiré" },
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

// Les outils arrivés avec l'ingestion ont tous la même sortie : le binaire par
// défaut, une download_url si client_id et pdf_name sont donnés, un asset si
// output vaut « asset » — c'est ce dernier qui permet d'enchaîner deux outils
// sans faire redescendre le fichier chez l'appelant.
const outputBlock = () => ({
  type: "fieldset", legend: { en: "output — binary file by default", fr: "sortie — fichier binaire par défaut" }, collapsed: false, fields: [
    { type: "row", fields: [
      { name: "client_id", type: "text", label: "client_id", placeholder: "demo-client", value: "" },
      { name: "pdf_name", type: "text", label: "pdf_name", placeholder: "mon-document", value: "" },
    ]},
    { name: "output", type: "select", label: "output", options: ["", "binary", "asset"], value: "",
      hint: { en: "asset keeps the result in the service, ready for the next tool", fr: "asset garde le résultat dans le service, prêt pour l'outil suivant" } },
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

// Toute route qui consomme un document accepte les deux formes de référence :
// celle d'hier, et celle qu'apporte le dépôt de fichier.
const sourceParam = (name) => ({ name, type: "string", required: true, desc: {
  en: "Source document: <code>asset://as_…</code> returned by <code>POST /api/files</code>, or a <code>/download/&lt;client_id&gt;/&lt;name&gt;.pdf</code> path this service produced.",
  fr: "Document source : <code>asset://as_…</code> renvoyé par <code>POST /api/files</code>, ou un chemin <code>/download/&lt;client_id&gt;/&lt;nom&gt;.pdf</code> produit par ce service." } });

const OUTPUT_PARAM = { name: "output", type: "enum", desc: {
  en: "<code>binary</code> (default) or <code>asset</code>. With <code>asset</code> the answer carries an <code>asset</code> object whose <code>asset://&lt;id&gt;</code> feeds the next tool without a round trip.",
  fr: "<code>binary</code> (défaut) ou <code>asset</code>. Avec <code>asset</code>, la réponse porte un objet <code>asset</code> dont l'<code>asset://&lt;id&gt;</code> alimente l'outil suivant sans aller-retour." } };

const VERDICT_PARAM = { name: "verdict", type: "response", desc: {
  en: "Read-only field of the answer: <code>{ status, score, summary, checks }</code>. It never blocks a response — it says what the tool thinks of what it just produced.",
  fr: "Champ de la réponse, en lecture seule : <code>{ status, score, summary, checks }</code>. Il ne bloque jamais une réponse — il dit ce que l'outil pense de ce qu'il vient de produire." } };

const TOOL_PARAMS = [...SAVE_PARAMS, OUTPUT_PARAM, VERDICT_PARAM];

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

  // ────────────────────────────────────────── ingestion et assets ─────────

  {
    key: "files", method: "POST", path: "/api/files", form: true, multipart: true, group: "files",
    title: { en: "Upload a file", fr: "Déposer un fichier" },
    icon: "M12 16V4m0 0L8 8m4-4l4 4M5 20h14",
    card: { en: "The way in: your own file becomes an asset:// reference every tool accepts.",
      fr: "La porte d'entrée : votre fichier devient une référence asset:// que tous les outils acceptent." },
    desc: { en: "The only endpoint of the API that is not JSON: multipart/form-data, with a repeatable field named \"file\".\nThe type is read from the bytes, never from the extension. What comes back is an id, a type, a size, a page count and an expiry — nothing else is kept.",
      fr: "Le seul endpoint de l'API qui ne soit pas du JSON : multipart/form-data, avec un champ « file » répétable.\nLe type est lu dans les octets, jamais dans l'extension. Ce qui revient : un id, un type, une taille, un nombre de pages et une expiration — rien d'autre n'est conservé." },
    params: [
      { name: "file", type: "file", required: true, desc: {
        en: "One or more files, all under the same field name. PDF, images, Office and OpenDocument, HTML, Markdown, text and CSV are recognised; anything else is stored as <code>binary</code> and no tool will accept it.",
        fr: "Un ou plusieurs fichiers, tous sous le même nom de champ. PDF, images, Office et OpenDocument, HTML, Markdown, texte et CSV sont reconnus ; le reste est stocké en <code>binary</code> et aucun outil ne l'acceptera." } },
    ],
    fields: [
      { name: "file", type: "file", label: "file", required: true, multiple: true,
        hint: { en: "several files at once are allowed", fr: "plusieurs fichiers à la fois sont acceptés" }, value: "" },
    ],
    example: {
      response: { files: [{ id: "as_9f2c1d84e6b74a01bd35c7f0a1e2d3c4", name: "contrat.pdf", kind: "pdf", bytes: 184203, pages: 12, created_at: "2026-08-19T12:02:11Z", expires_at: "2026-08-19T14:02:11Z" }] },
    },
    statuses: [["201", { en: "Files stored", fr: "Fichiers stockés" }], ["400", { en: "No file, or file too large / too many pages", fr: "Aucun fichier, ou fichier trop lourd / trop de pages" }], ["401", ST.unauthorized], ["413", { en: "Body over the upload ceiling", fr: "Corps au-delà du plafond de dépôt" }]],
  },

  {
    key: "file-meta", method: "GET", path: "/api/files/{id}/meta", group: "files",
    title: { en: "Asset description", fr: "Description d'un asset" },
    icon: "M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z",
    card: { en: "Type, size, page count and expiry of a stored file, without downloading it.",
      fr: "Type, taille, nombre de pages et expiration d'un fichier stocké, sans le télécharger." },
    desc: { en: "Returns the record kept about an uploaded file. Reading it also triggers the lazy purge: an expired asset answers 404 rather than being served.",
      fr: "Renvoie la fiche conservée sur un fichier déposé. La lecture déclenche aussi la purge paresseuse : un asset expiré répond 404 plutôt que d'être servi." },
    params: [{ name: "id", type: "path", required: true, desc: { en: "Asset identifier, <code>as_</code> followed by 32 hexadecimal characters.", fr: "Identifiant d'asset, <code>as_</code> suivi de 32 caractères hexadécimaux." } }],
    fields: [{ name: "id", type: "assetpick", label: "id", required: true, placeholder: "as_…", value: "" }],
    buildPath: (v) => `/api/files/${encodeURIComponent(String(v.id || "").replace("asset://", ""))}/meta`,
    example: { response: { id: "as_9f2c1d84e6b74a01bd35c7f0a1e2d3c4", name: "contrat.pdf", kind: "pdf", bytes: 184203, pages: 12, expires_at: "2026-08-19T14:02:11Z" } },
    statuses: [["200", "AssetMeta"], ["401", ST.unauthorized], ["404", ST.sourceMissing]],
  },

  {
    key: "file-get", method: "GET", path: "/api/files/{id}", group: "files",
    title: { en: "Download an asset", fr: "Récupérer un asset" },
    icon: "M12 4v10m0 0l-4-4m4 4l4-4M5 20h14",
    card: { en: "The bytes back, with the content type read from the file itself.",
      fr: "Les octets, avec le type de contenu lu dans le fichier lui-même." },
    desc: { en: "Serves a stored file. This is how the result of a chain asked for as \"output\": \"asset\" is finally fetched.",
      fr: "Sert un fichier stocké. C'est ainsi que le résultat d'une chaîne demandé en « output »: « asset » est finalement récupéré." },
    params: [{ name: "id", type: "path", required: true, desc: { en: "Asset identifier.", fr: "Identifiant d'asset." } }],
    fields: [{ name: "id", type: "assetpick", label: "id", required: true, placeholder: "as_…", value: "" }],
    buildPath: (v) => `/api/files/${encodeURIComponent(String(v.id || "").replace("asset://", ""))}`,
    example: { responseNote: { en: "Binary body, with the Content-Type of the stored kind", fr: "Corps binaire, avec le Content-Type du type stocké" } },
    statuses: [["200", { en: "The file", fr: "Le fichier" }], ["401", ST.unauthorized], ["404", ST.sourceMissing]],
  },

  {
    key: "file-delete", method: "DELETE", path: "/api/files/{id}", group: "files",
    title: { en: "Forget an asset", fr: "Oublier un asset" },
    icon: "M5 7h14M9 7V5h6v2M7 7l1 13h8l1-13",
    card: { en: "Immediate deletion, before the expiry does it. Nothing is kept.",
      fr: "Suppression immédiate, avant que l'expiration ne s'en charge. Rien n'est conservé." },
    desc: { en: "Deletes an uploaded file and its record now, rather than waiting for the TTL.\nAnswers 204 with no body: there is nothing left to describe.",
      fr: "Supprime maintenant un fichier déposé et sa fiche, sans attendre le TTL.\nRépond 204 sans corps : il ne reste rien à décrire." },
    params: [{ name: "id", type: "path", required: true, desc: { en: "Asset identifier.", fr: "Identifiant d'asset." } }],
    fields: [{ name: "id", type: "assetpick", label: "id", required: true, placeholder: "as_…", value: "" }],
    buildPath: (v) => `/api/files/${encodeURIComponent(String(v.id || "").replace("asset://", ""))}`,
    example: { responseNote: { en: "204, empty body", fr: "204, corps vide" } },
    statuses: [["204", { en: "Deleted", fr: "Supprimé" }], ["401", ST.unauthorized], ["404", ST.sourceMissing]],
  },

  // ────────────────────────────────────────── pages ───────────────────────

  {
    key: "pages", method: "POST", path: "/api/pages", json: true, group: "organize",
    title: { en: "Organise pages", fr: "Organiser les pages" },
    icon: "M4 4h9v16H4zM15 8h5v12h-5",
    card: { en: "Extract, delete, reorder, rotate or split — one route, one verb, one range syntax.",
      fr: "Extraire, supprimer, réordonner, pivoter ou découper — une route, un verbe, une syntaxe de plage." },
    desc: { en: "Five operations on the page list of a document, chosen with \"op\".\nA range that names a page the document does not have is a 400, not a silently shorter PDF. \"split\" answers with one asset per part.",
      fr: "Cinq opérations sur la liste des pages d'un document, choisies par « op ».\nUne plage qui nomme une page absente du document donne un 400, pas un PDF silencieusement plus court. « split » répond avec un asset par partie." },
    params: [
      sourceParam("pdf"),
      { name: "op", type: "enum", required: true, desc: {
        en: "<code>extract</code>, <code>delete</code>, <code>reorder</code>, <code>rotate</code> or <code>split</code>.",
        fr: "<code>extract</code>, <code>delete</code>, <code>reorder</code>, <code>rotate</code> ou <code>split</code>." } },
      { name: "pages", type: "string", desc: {
        en: "Range: <code>1,3,5-9</code>, <code>2-</code>, <code>all</code>, <code>odd</code>, <code>even</code>. Used by every op but <code>reorder</code>; on <code>split</code> it gives the cut points.",
        fr: "Plage : <code>1,3,5-9</code>, <code>2-</code>, <code>all</code>, <code>odd</code>, <code>even</code>. Utilisée par tous les op sauf <code>reorder</code> ; sur <code>split</code>, elle donne les points de coupe." } },
      { name: "order", type: "string", desc: {
        en: "For <code>reorder</code>: the complete new order, which must list every page of the document.",
        fr: "Pour <code>reorder</code> : l'ordre complet, qui doit lister toutes les pages du document." } },
      { name: "angle", type: "number", desc: { en: "For <code>rotate</code>: a multiple of 90, positive or negative.", fr: "Pour <code>rotate</code> : un multiple de 90, positif ou négatif." } },
      { name: "every", type: "number", desc: { en: "For <code>split</code> without <code>pages</code>: pages per file. Default <code>1</code>.", fr: "Pour <code>split</code> sans <code>pages</code> : pages par fichier. Défaut <code>1</code>." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "op", type: "select", label: "op", options: ["extract", "delete", "reorder", "rotate", "split"], value: "extract" },
        { name: "pages", type: "text", label: "pages", placeholder: "1,3,5-9 · 2- · all · odd · even", value: "" },
      ]},
      { type: "row", fields: [
        { name: "order", type: "text", label: "order", placeholder: "3,1,2", value: "" },
        { name: "angle", type: "number", step: "90", label: "angle", hint: "90 · 180 · 270", value: "" },
        { name: "every", type: "number", label: "every", hint: { en: "split: pages per file", fr: "split : pages par fichier" }, value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c1d84e6b74a01bd35c7f0a1e2d3c4", op: "extract", pages: "1-3,7", output: "asset" },
      response: { asset: { id: "as_31b0…", name: "pages.pdf", kind: "pdf", bytes: 41208, pages: 4 }, pages: 4 },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown op / range outside the document", fr: "op inconnu / plage hors document" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["500", { en: "qpdf failed", fr: "Échec qpdf" }], ["504", ST.timeout]],
  },

  {
    key: "pages-number", method: "POST", path: "/api/pages/number", json: true, group: "organize",
    title: { en: "Number the pages", fr: "Numéroter les pages" },
    icon: "M4 4h16v16H4zM8 16h2m4 0h2",
    card: { en: "Page numbers on a document rendered elsewhere, drawn on an overlay of the same geometry.",
      fr: "Des numéros de page sur un document rendu ailleurs, dessinés sur un calque de même géométrie." },
    desc: { en: "The renderer numbers what it lays out itself; this stamps a document it never saw. The geometry is read from the source rather than assumed, because an overlay built for A4 lands scaled and off-centre on a letter page.\n\"format\" is plain text with the tokens {page} and {pages} — not HTML.",
      fr: "Le moteur numérote ce qu'il met en page lui-même ; ceci tamponne un document qu'il n'a jamais vu. La géométrie est lue dans la source plutôt que supposée : un calque construit pour de l'A4 arrive mis à l'échelle et décentré sur une page letter.\n« format » est du texte simple avec les jetons {page} et {pages} — pas du HTML." },
    params: [
      sourceParam("pdf"),
      { name: "format", type: "string", desc: { en: 'Tokens <code>{page}</code> and <code>{pages}</code>. Default <code>{page}</code>, 64 characters at most.', fr: 'Jetons <code>{page}</code> et <code>{pages}</code>. Défaut <code>{page}</code>, 64 caractères au plus.' } },
      { name: "position", type: "enum", desc: { en: "<code>bottom-center</code> (default), <code>bottom-left</code>, <code>bottom-right</code>, <code>top-center</code>, <code>top-left</code>, <code>top-right</code>.", fr: "<code>bottom-center</code> (défaut), <code>bottom-left</code>, <code>bottom-right</code>, <code>top-center</code>, <code>top-left</code>, <code>top-right</code>." } },
      { name: "start", type: "number", desc: { en: "First number shown. Default <code>1</code> — an appendix numbered from 900 is legitimate.", fr: "Premier numéro affiché. Défaut <code>1</code> — une annexe numérotée à partir de 900 est légitime." } },
      { name: "from_page", type: "number", desc: { en: "First page that gets a number. The pages before it stay bare.", fr: "Première page qui reçoit un numéro. Les pages précédentes restent nues." } },
      { name: "font_size", type: "number", desc: { en: "4 to 72 points. Default <code>10</code>.", fr: "4 à 72 points. Défaut <code>10</code>." } },
      { name: "margin", type: "string", desc: { en: "CSS length: <code>1.5cm</code> by default.", fr: "Longueur CSS : <code>1.5cm</code> par défaut." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "format", type: "text", label: "format", placeholder: "{page} / {pages}", value: "" },
        { name: "position", type: "select", label: "position",
          options: ["", "bottom-center", "bottom-left", "bottom-right", "top-center", "top-left", "top-right"], value: "" },
      ]},
      { type: "row", fields: [
        { name: "start", type: "number", label: "start", value: "" },
        { name: "from_page", type: "number", label: "from_page", value: "" },
        { name: "font_size", type: "number", label: "font_size", hint: "4 → 72", value: "" },
        { name: "margin", type: "text", label: "margin", placeholder: "1.5cm", value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf", format: "{page} / {pages}", position: "bottom-right", from_page: 2 },
      response: { pages: 12, verdict: { status: "ok", score: 100, summary: "12 pages numbered", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown token in format / position out of the list", fr: "Jeton inconnu dans format / position hors liste" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["500", { en: "qpdf failed", fr: "Échec qpdf" }], ["504", ST.timeout]],
  },

  {
    key: "crop", method: "POST", path: "/api/crop", json: true, group: "organize",
    title: { en: "Crop", fr: "Rogner" },
    icon: "M6 2v16h16M2 6h16v16",
    card: { en: "Rewrites the CropBox, either to a box you give or to the bounding box of the visible text.",
      fr: "Réécrit la CropBox, soit sur un cadre fourni, soit sur la boîte englobante du texte visible." },
    desc: { en: "Two modes. An explicit box is clamped to the page rather than refused; \"auto\" measures the words on each page and keeps a margin around them.\nA page whose content cannot be measured keeps its own box: cropping to nothing would hand back blank pages.",
      fr: "Deux modes. Un cadre explicite est ramené dans la page plutôt que refusé ; « auto » mesure les mots de chaque page et garde une marge autour.\nUne page dont le contenu ne peut pas être mesuré garde son propre cadre : rogner à zéro rendrait des pages blanches." },
    params: [
      sourceParam("pdf"),
      { name: "box", type: "array | string", desc: {
        en: "<code>[left, bottom, right, top]</code> in PostScript points, or the string <code>\"auto\"</code>. Absent means <code>\"auto\"</code>.",
        fr: "<code>[gauche, bas, droite, haut]</code> en points PostScript, ou la chaîne <code>\"auto\"</code>. Absent vaut <code>\"auto\"</code>." } },
      { name: "margin", type: "number", desc: { en: "Points kept around the detected content in <code>auto</code>. Default <code>12</code>.", fr: "Points conservés autour du contenu détecté en <code>auto</code>. Défaut <code>12</code>." } },
      { name: "pages", type: "string", desc: { en: "<code>\"all\"</code>, <code>\"3\"</code>, <code>\"2-5\"</code> or <code>\"1,4,7-9\"</code>. Untargeted pages keep their box.", fr: "<code>\"all\"</code>, <code>\"3\"</code>, <code>\"2-5\"</code> ou <code>\"1,4,7-9\"</code>. Les pages non visées gardent leur cadre." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "box", type: "json", rows: 2, label: "box",
        hint: { en: '"auto", or [left, bottom, right, top] in points', fr: '"auto", ou [gauche, bas, droite, haut] en points' }, value: '"auto"' },
      { type: "row", fields: [
        { name: "margin", type: "number", label: "margin", hint: { en: "points, auto mode", fr: "points, mode auto" }, value: "" },
        { name: "pages", type: "text", label: "pages", placeholder: "all", value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", box: "auto", margin: 18 },
      response: { pages: 4, verdict: { status: "ok", score: 96, summary: "4 pages cropped", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Malformed box / page list", fr: "Cadre ou liste de pages malformés" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  // ────────────────────────────────────────── optimisation ────────────────

  {
    key: "compress", method: "POST", path: "/api/compress", json: true, group: "optimize",
    title: { en: "Compress", fr: "Compresser" },
    icon: "M4 8V4h4M20 8V4h-4M4 16v4h4M20 16v4h-4M8 12h8",
    card: { en: "Four presets, and a verdict that says what the compression cost — pages, text, image dpi.",
      fr: "Quatre préréglages, et un verdict qui dit ce que la compression a coûté — pages, texte, dpi des images." },
    desc: { en: "Ghostscript in -dSAFER, with the four standard presets.\nThe verdict is the point: it compares the page count and the number of selectable characters before and after, so a compression that rasterised a scan says so instead of being discovered on opening the file.",
      fr: "Ghostscript en -dSAFER, avec les quatre préréglages standard.\nLe verdict est l'essentiel : il compare le nombre de pages et le nombre de caractères sélectionnables avant et après, de sorte qu'une compression qui a rastérisé un scan le dit, au lieu d'être découverte à l'ouverture du fichier." },
    params: [
      sourceParam("pdf"),
      { name: "level", type: "enum", desc: { en: "<code>screen</code>, <code>ebook</code> (default), <code>printer</code>, <code>prepress</code>.", fr: "<code>screen</code>, <code>ebook</code> (défaut), <code>printer</code>, <code>prepress</code>." } },
      { name: "dpi", type: "number", desc: { en: "Image downsampling target, 36 to 1200. Absent: the preset decides.", fr: "Cible de rééchantillonnage des images, 36 à 1200. Absent : le préréglage décide." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "level", type: "select", label: "level", options: ["", "screen", "ebook", "printer", "prepress"], value: "" },
        { name: "dpi", type: "number", label: "dpi", hint: "36 → 1200", value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", level: "ebook", output: "asset" },
      response: { asset: { id: "as_71ad…", name: "compressed.pdf", kind: "pdf", bytes: 798231 },
        pages: 12,
        verdict: { status: "ok", score: 92, summary: "4.2 MB → 780 KB, text intact", checks: [{ name: "text-preserved", status: "ok", detail: "100% of the characters kept" }] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown level / dpi out of range", fr: "level inconnu / dpi hors bornes" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["500", { en: "Ghostscript failed", fr: "Échec Ghostscript" }], ["504", ST.timeout]],
  },

  {
    key: "repair", method: "POST", path: "/api/repair", json: true, group: "optimize",
    title: { en: "Repair", fr: "Réparer" },
    icon: "M14 7l3 3-7 7-3-3zM17 3l4 4-2 2-4-4z",
    card: { en: "Rebuilds the internal structure of a file readers refuse to open.",
      fr: "Reconstruit la structure interne d'un fichier que les lecteurs refusent d'ouvrir." },
    desc: { en: "Rewrites the document with qpdf, then falls back to Ghostscript when qpdf itself cannot make sense of it.\nThe verdict says how many pages came back and whether the text layer survived — a repair that silently drops half the document is the failure mode worth naming.",
      fr: "Réécrit le document avec qpdf, puis se rabat sur Ghostscript quand qpdf lui-même n'en tire rien.\nLe verdict dit combien de pages sont revenues et si la couche texte a survécu — une réparation qui perd silencieusement la moitié du document est le mode d'échec qui mérite d'être nommé." },
    params: [sourceParam("pdf"), ...TOOL_PARAMS],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…" },
      response: { pages: 9, verdict: { status: "warn", score: 78, summary: "9 of 10 pages recovered", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unreadable source", fr: "Source illisible" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  {
    key: "unlock", method: "POST", path: "/api/unlock", json: true, group: "optimize",
    title: { en: "Remove a password", fr: "Retirer un mot de passe" },
    icon: "M7 11V8a5 5 0 019.5-2M5 11h14v9H5z",
    card: { en: "Decrypts a PDF with the password you provide. No cracking, ever.",
      fr: "Déchiffre un PDF avec le mot de passe que vous fournissez. Aucun cassage, jamais." },
    desc: { en: "qpdf --decrypt, with the password given in the request.\nThere is no recovery mode and there will not be one: a tool that opens a document without its password is not a tool, it is a liability.",
      fr: "qpdf --decrypt, avec le mot de passe fourni dans la requête.\nIl n'existe aucun mode de récupération et il n'en existera pas : un outil qui ouvre un document sans son mot de passe n'est pas un outil, c'est un risque." },
    params: [
      sourceParam("pdf"),
      { name: "password", type: "string", required: true, desc: { en: "The document's own password. A wrong one is a 400 that says so.", fr: "Le mot de passe du document. Un mauvais mot de passe donne un 400 qui le dit." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "password", type: "text", label: "password", required: true, value: "" },
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", password: "s3cr3t" },
      response: { pages: 12 },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Missing or wrong password", fr: "Mot de passe absent ou faux" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  {
    key: "ocr", method: "POST", path: "/api/ocr", json: true, group: "optimize",
    title: { en: "OCR", fr: "OCR" },
    icon: "M4 8V4h4M20 8V4h-4M4 16v4h4M20 16v4h-4M8 10h8M8 14h5",
    card: { en: "Adds a text layer to a scan, and says which pages it is unsure about.",
      fr: "Ajoute une couche texte à un scan, et dit de quelles pages il n'est pas sûr." },
    desc: { en: "ocrmypdf over the pages that need it. \"auto\" only reads the pages without text, \"force\" rasterises and reads everything again, \"redo\" replaces a previous OCR layer.\nThe verdict names the pages below the confidence threshold — an OCR that half worked is worse than one that failed, because nobody checks.",
      fr: "ocrmypdf sur les pages qui en ont besoin. « auto » ne lit que les pages sans texte, « force » rastérise et relit tout, « redo » remplace une couche OCR précédente.\nLe verdict nomme les pages sous le seuil de confiance — un OCR à moitié réussi est pire qu'un OCR échoué, parce que personne ne vérifie." },
    params: [
      sourceParam("pdf"),
      { name: "languages", type: "string[]", desc: { en: "Among the languages installed in the image: <code>fra</code>, <code>eng</code>, <code>deu</code>, <code>spa</code>, <code>ita</code>. Default <code>[\"fra\", \"eng\"]</code>.", fr: "Parmi les langues installées dans l'image : <code>fra</code>, <code>eng</code>, <code>deu</code>, <code>spa</code>, <code>ita</code>. Défaut <code>[\"fra\", \"eng\"]</code>." } },
      { name: "mode", type: "enum", desc: { en: "<code>auto</code> (default), <code>force</code> or <code>redo</code>.", fr: "<code>auto</code> (défaut), <code>force</code> ou <code>redo</code>." } },
      { name: "pdfa", type: "boolean", desc: { en: "Also produce a PDF/A in the same pass, which ocrmypdf can do for free.", fr: "Produire en plus un PDF/A dans la même passe, ce qu'ocrmypdf sait faire gratuitement." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "languages", type: "list", rows: 2, label: "languages",
        hint: { en: "one per line: fra, eng, deu, spa, ita", fr: "une par ligne : fra, eng, deu, spa, ita" }, value: "" },
      { type: "row", fields: [
        { name: "mode", type: "select", label: "mode", options: ["", "auto", "force", "redo"], value: "" },
        { name: "pdfa", type: "checkbox", label: "pdfa", value: false },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", languages: ["fra"], mode: "auto", output: "asset" },
      response: { pages: 24, verdict: { status: "warn", score: 81, summary: "24 pages read, 2 below the confidence threshold", checks: [{ name: "ocr-confidence", status: "warn", detail: "page 7: 61%", page: 7 }] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown language or mode / too many pages", fr: "Langue ou mode inconnus / trop de pages" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", { en: "Timeout — send it through POST /api/jobs", fr: "Timeout — passez par POST /api/jobs" }]],
  },

  {
    key: "pdfa", method: "POST", path: "/api/pdfa", json: true, group: "optimize",
    title: { en: "PDF/A archiving", fr: "Archivage PDF/A" },
    icon: "M4 4h16v5H4zM6 9v11h12V9M10 13h4",
    card: { en: "Converts to the archival profile a public body will actually accept.",
      fr: "Convertit vers le profil d'archivage qu'une administration acceptera réellement." },
    desc: { en: "Ghostscript to the requested PDF/A profile, fonts embedded.\nThe verdict reports what the conversion changed — a profile is only worth something if you know the document survived it.",
      fr: "Ghostscript vers le profil PDF/A demandé, polices embarquées.\nLe verdict rapporte ce que la conversion a changé — un profil ne vaut quelque chose que si l'on sait que le document y a survécu." },
    params: [
      sourceParam("pdf"),
      { name: "variant", type: "enum", desc: { en: "<code>pdf/a-1b</code>, <code>pdf/a-2b</code> (default) or <code>pdf/a-3b</code>.", fr: "<code>pdf/a-1b</code>, <code>pdf/a-2b</code> (défaut) ou <code>pdf/a-3b</code>." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "variant", type: "select", label: "variant", options: ["", "pdf/a-1b", "pdf/a-2b", "pdf/a-3b"], value: "" },
      outputBlock(),
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf", variant: "pdf/a-2b" },
      response: { pages: 12, verdict: { status: "ok", score: 100, summary: "pdf/a-2b, 12 pages, fonts embedded", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown variant", fr: "Variante inconnue" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["500", { en: "Ghostscript failed", fr: "Échec Ghostscript" }], ["504", ST.timeout]],
  },

  // ────────────────────────────────────────── conversions ─────────────────

  {
    key: "rasterize", method: "POST", path: "/api/rasterize", json: true, group: "convert",
    title: { en: "PDF → images", fr: "PDF → images" },
    icon: "M4 5h16v14H4zM8 13l3-3 3 3 2-2 2 2",
    card: { en: "Pages of an uploaded document as PNG or JPEG, one asset per page.",
      fr: "Les pages d'un document déposé en PNG ou JPEG, un asset par page." },
    desc: { en: "pdftoppm on a document the service did not render — a thumbnail, a page for a viewer, an image for something that cannot read PDF.\nOne page comes back as the image itself; several come back as one asset per page.",
      fr: "pdftoppm sur un document que le service n'a pas rendu — une vignette, une page pour une visionneuse, une image pour ce qui ne sait pas lire le PDF.\nUne page revient comme l'image elle-même ; plusieurs reviennent en un asset par page." },
    params: [
      sourceParam("pdf"),
      { name: "pages", type: "string", desc: { en: "<code>\"1\"</code>, <code>\"2-5\"</code> or <code>\"all\"</code>. Absent: the whole document, up to the per-call ceiling.", fr: "<code>\"1\"</code>, <code>\"2-5\"</code> ou <code>\"all\"</code>. Absent : tout le document, jusqu'au plafond par appel." } },
      { name: "dpi", type: "number", desc: { en: "36 to 300. Default <code>150</code>.", fr: "36 à 300. Défaut <code>150</code>." } },
      { name: "format", type: "enum", desc: { en: "<code>png</code> (default) or <code>jpeg</code>.", fr: "<code>png</code> (défaut) ou <code>jpeg</code>." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "pages", type: "text", label: "pages", placeholder: "1, 2-5, all", value: "" },
        { name: "dpi", type: "number", label: "dpi", hint: "36 → 300", value: "" },
        { name: "format", type: "select", label: "format", options: ["", "png", "jpeg"], value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", pages: "1-3", dpi: 150, format: "png" },
      response: { assets: [{ id: "as_a1…", name: "page-1.png", kind: "png", bytes: 210442 }], pages: 3 },
    },
    statuses: [["200", { en: "Image, or JSON { assets }", fr: "Image, ou JSON { assets }" }], ["400", { en: "Range outside the document / pixel budget exceeded", fr: "Plage hors document / budget de pixels dépassé" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  {
    key: "images-to-pdf", method: "POST", path: "/api/images-to-pdf", json: true, group: "convert",
    title: { en: "Images → PDF", fr: "Images → PDF" },
    icon: "M3 6h13v11H3zM8 12l2-2 2 2 2-2M18 8h3v11H8",
    card: { en: "Uploaded images become one paginated document, in the order given.",
      fr: "Des images déposées deviennent un document paginé, dans l'ordre fourni." },
    desc: { en: "One image per page, laid out by WeasyPrint rather than pasted at native size, so a portrait scan and a landscape photo end up in the same document without one of them overflowing.",
      fr: "Une image par page, mise en page par WeasyPrint plutôt que collée à sa taille native : un scan portrait et une photo paysage finissent dans le même document sans que l'un déborde." },
    params: [
      { name: "images", type: "string[]", required: true, desc: { en: "<code>asset://as_…</code> references, one per page, in page order. 200 at most.", fr: "Références <code>asset://as_…</code>, une par page, dans l'ordre des pages. 200 au plus." } },
      { name: "paper_size", type: "enum", desc: { en: "<code>A3</code>, <code>A4</code> (default), <code>A5</code>, <code>A6</code>, <code>Letter</code>, <code>Legal</code>, <code>Tabloid</code>. Ignored when <code>fit</code> is <code>actual</code>.", fr: "<code>A3</code>, <code>A4</code> (défaut), <code>A5</code>, <code>A6</code>, <code>Letter</code>, <code>Legal</code>, <code>Tabloid</code>. Ignoré quand <code>fit</code> vaut <code>actual</code>." } },
      { name: "orientation", type: "enum", desc: { en: "<code>portrait</code> (default), <code>landscape</code>, or <code>auto</code> to follow each image.", fr: "<code>portrait</code> (défaut), <code>landscape</code>, ou <code>auto</code> pour suivre chaque image." } },
      { name: "fit", type: "enum", desc: { en: "<code>contain</code> (default) or <code>actual</code>.", fr: "<code>contain</code> (défaut) ou <code>actual</code>." } },
      { name: "margin", type: "number", desc: { en: "Points between the image and the page border, 144 at most.", fr: "Points entre l'image et le bord de page, 144 au plus." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "images", type: "assetlist", pick: "image", rows: 4, label: "images", required: true,
        hint: { en: "one asset:// per line, in page order", fr: "un asset:// par ligne, dans l'ordre des pages" }, value: "" },
      { type: "row", fields: [
        { name: "paper_size", type: "select", label: "paper_size", options: ["", "A3", "A4", "A5", "A6", "Letter", "Legal", "Tabloid"], value: "" },
        { name: "orientation", type: "select", label: "orientation", options: ["", "portrait", "landscape", "auto"], value: "" },
        { name: "fit", type: "select", label: "fit", options: ["", "contain", "actual"], value: "" },
        { name: "margin", type: "number", label: "margin", hint: { en: "points", fr: "points" }, value: "" },
      ]},
      outputBlock(),
    ],
    example: {
      request: { images: ["asset://as_a1…", "asset://as_b2…"], paper_size: "A4", fit: "contain", margin: 24 },
      response: { pages: 2 },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Empty list / reference that is not an image", fr: "Liste vide / référence qui n'est pas une image" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  {
    key: "office-to-pdf", method: "POST", path: "/api/office-to-pdf", json: true, group: "convert",
    title: { en: "Office → PDF", fr: "Office → PDF" },
    icon: "M5 3h9l5 5v13H5zM14 3v5h5M8 13h8M8 17h5",
    card: { en: "Word, Excel, PowerPoint, OpenDocument and CSV, converted by LibreOffice in the sandbox.",
      fr: "Word, Excel, PowerPoint, OpenDocument et CSV, convertis par LibreOffice dans le bac à sable." },
    desc: { en: "LibreOffice headless, with a throwaway profile, macros disabled and no network.\nThe verdict reports the page count and the text coverage of the result: a conversion that produced two blank pages is caught here rather than by the recipient.",
      fr: "LibreOffice headless, avec un profil jetable, macros désactivées et sans réseau.\nLe verdict rapporte le nombre de pages et la couverture texte du résultat : une conversion qui a produit deux pages blanches est attrapée ici plutôt que par le destinataire." },
    params: [
      { name: "file", type: "string", required: true, desc: { en: "<code>asset://as_…</code> of an uploaded document — this direction always starts from an upload.", fr: "<code>asset://as_…</code> d'un document déposé — cette direction part toujours d'un dépôt." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "file", type: "assetpick", pick: "office", label: "file", required: true, value: "" },
      outputBlock(),
    ],
    example: {
      request: { file: "asset://as_9f2c…", output: "asset" },
      response: { asset: { id: "as_51fe…", name: "office.pdf", kind: "pdf", bytes: 240119 }, pages: 6,
        verdict: { status: "ok", score: 94, summary: "6 pages, text kept", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "The asset is not an Office document", fr: "L'asset n'est pas un document Office" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", { en: "Timeout — send it through POST /api/jobs", fr: "Timeout — passez par POST /api/jobs" }]],
  },

  {
    key: "pdf-to-office", method: "POST", path: "/api/pdf-to-office", json: true, group: "convert",
    title: { en: "PDF → Office", fr: "PDF → Office" },
    icon: "M5 3h9l5 5v13H5zM14 3v5h5M9 14l3 3 3-3",
    card: { en: "Back to an editable document — docx, xlsx or pptx, with a verdict on what survived.",
      fr: "Retour à un document éditable — docx, xlsx ou pptx, avec un verdict sur ce qui a survécu." },
    desc: { en: "LibreOffice reads the PDF back through the import filter matching the target format.\nThis direction is lossy by nature. The verdict says how much of the text made it across, which is the only honest way to offer it.",
      fr: "LibreOffice relit le PDF via le filtre d'import correspondant au format cible.\nCette direction est lossy par nature. Le verdict dit quelle part du texte est passée, seule manière honnête de la proposer." },
    params: [
      sourceParam("pdf"),
      { name: "to", type: "enum", required: true, desc: { en: "<code>docx</code>, <code>xlsx</code> or <code>pptx</code>.", fr: "<code>docx</code>, <code>xlsx</code> ou <code>pptx</code>." } },
      ...TOOL_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "to", type: "select", label: "to", options: ["docx", "xlsx", "pptx"], value: "docx" },
      outputBlock(),
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", to: "docx", output: "asset" },
      response: { asset: { id: "as_88cc…", name: "converted.docx", kind: "docx", bytes: 61204 },
        verdict: { status: "warn", score: 74, summary: "text kept, layout approximated", checks: [] } },
    },
    statuses: [["200", ST.toolOk], ["400", { en: "Unknown target format", fr: "Format cible inconnu" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", { en: "Timeout — send it through POST /api/jobs", fr: "Timeout — passez par POST /api/jobs" }]],
  },

  {
    key: "extract", method: "POST", path: "/api/extract", json: true, group: "convert",
    title: { en: "PDF → Markdown", fr: "PDF → Markdown" },
    icon: "M4 5h16v14H4zM7 15l3-6 2 4 2-2 3 4",
    card: { en: "The text back as Markdown, plain text or structured JSON — tables included.",
      fr: "Le texte en Markdown, texte brut ou JSON structuré — tableaux compris." },
    desc: { en: "pdftotext with the layout kept, plus our own column heuristic to rebuild the tables.\nThis is the endpoint an agent wants: a PDF is unreadable to a model, Markdown is not.",
      fr: "pdftotext en conservant la mise en page, plus notre propre heuristique de colonnes pour reconstruire les tableaux.\nC'est l'endpoint que veut un agent : un PDF est illisible pour un modèle, du Markdown non." },
    params: [
      sourceParam("pdf"),
      { name: "format", type: "enum", desc: { en: "<code>markdown</code> (default), <code>text</code> or <code>json</code>.", fr: "<code>markdown</code> (défaut), <code>text</code> ou <code>json</code>." } },
      { name: "pages", type: "string", desc: { en: "<code>\"1\"</code>, <code>\"2-5\"</code> or <code>\"all\"</code>. Absent: the whole document.", fr: "<code>\"1\"</code>, <code>\"2-5\"</code> ou <code>\"all\"</code>. Absent : tout le document." } },
      { name: "layout", type: "boolean", desc: { en: "Keep the column layout, on by default. Turning it off gives reading order instead, at the cost of every table.", fr: "Conserver la disposition en colonnes, actif par défaut. Le couper donne l'ordre de lecture, au prix de tous les tableaux." } },
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "format", type: "select", label: "format", options: ["", "markdown", "text", "json"], value: "" },
        { name: "pages", type: "text", label: "pages", placeholder: "all", value: "" },
        { name: "layout", type: "bool", label: "layout", value: "" },
      ]},
    ],
    example: {
      request: { pdf: "asset://as_9f2c…", format: "markdown", pages: "1-2" },
      response: { format: "markdown", pages: 2, content: "# Contract\n\n| Item | Price |\n|---|---|\n…" },
    },
    statuses: [["200", "ExtractResponse"], ["400", { en: "Unknown format / range outside the document", fr: "Format inconnu / plage hors document" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  // ────────────────────────────────────────── contrat et preuve ───────────

  {
    key: "compose", method: "POST", path: "/api/compose", json: true, group: "contract",
    title: { en: "Document contract", fr: "Contrat de document" },
    icon: "M6 3h9l4 4v14H6zM9 12h7M9 16h5M9 8h4",
    card: { en: "You state constraints, not a render. The service renders, audits, corrects, and answers with the log.",
      fr: "Vous posez des contraintes, pas une demande de rendu. Le service rend, audite, corrige, et répond avec le journal." },
    desc: { en: "At most four pages, no table cut in half, a layout score of at least 90: the service renders, audits, corrects and re-renders until the contract holds or the passes run out.\nEntirely deterministic — no model, no outbound call. A contract that cannot be met still returns the best document produced, and names what is still unmet.",
      fr: "Quatre pages au plus, aucun tableau coupé en deux, un score de mise en page d'au moins 90 : le service rend, audite, corrige et re-rend jusqu'à ce que le contrat tienne ou que les passes soient épuisées.\nEntièrement déterministe — aucun modèle, aucun appel sortant. Un contrat qui ne peut pas être tenu rend quand même le meilleur document produit, et nomme ce qui reste non satisfait." },
    params: [
      { name: "markdown", type: "string", desc: { en: "Source document. <code>html</code>, or <code>template</code> + <code>data</code>, work too — same three modes as the render endpoints.", fr: "Document source. <code>html</code>, ou <code>template</code> + <code>data</code>, marchent aussi — mêmes trois modes que les endpoints de rendu." } },
      CSS_PARAM, OPTIONS_PARAM,
      { name: "constraints", type: "object", desc: {
        en: "<code>max_pages</code>, <code>min_pages</code>, <code>no_split_tables</code>, <code>no_orphan_headings</code>, <code>no_blank_pages</code>, <code>no_overflow</code>, <code>min_layout_score</code> (0–100). Every field is optional; with none, this is a render with a report.",
        fr: "<code>max_pages</code>, <code>min_pages</code>, <code>no_split_tables</code>, <code>no_orphan_headings</code>, <code>no_blank_pages</code>, <code>no_overflow</code>, <code>min_layout_score</code> (0–100). Chaque champ est optionnel ; sans aucun, c'est un rendu avec rapport." } },
      { name: "max_passes", type: "number", desc: { en: "Corrective passes allowed, 1 to 5. Default <code>3</code>. Each pass is a full render.", fr: "Passes correctives autorisées, 1 à 5. Défaut <code>3</code>. Chaque passe est un rendu complet." } },
      { name: "attest", type: "boolean", desc: { en: "Seal the result with a signed attestation, returned in <code>attestation</code>.", fr: "Sceller le résultat par une attestation signée, renvoyée dans <code>attestation</code>." } },
      ...SAVE_PARAMS, OUTPUT_PARAM,
    ],
    fields: [
      { name: "markdown", type: "textarea", rows: 10, label: "markdown", value: SAMPLE_MD },
      cssField,
      { type: "fieldset", legend: { en: "constraints — what the document must satisfy", fr: "constraints — ce que le document doit satisfaire" }, collapsed: false, fields: [
        { type: "row", fields: [
          { name: "constraints.max_pages", type: "number", label: "max_pages", value: "" },
          { name: "constraints.min_pages", type: "number", label: "min_pages", value: "" },
          { name: "constraints.min_layout_score", type: "number", label: "min_layout_score", hint: "0 → 100", value: "" },
        ]},
        { name: "constraints.no_split_tables", type: "checkbox", label: "no_split_tables", value: false },
        { name: "constraints.no_orphan_headings", type: "checkbox", label: "no_orphan_headings", value: false },
        { name: "constraints.no_blank_pages", type: "checkbox", label: "no_blank_pages", value: false },
        { name: "constraints.no_overflow", type: "checkbox", label: "no_overflow", value: false },
      ]},
      { type: "row", fields: [
        { name: "max_passes", type: "number", label: "max_passes", hint: "1 → 5", value: "" },
        { name: "attest", type: "checkbox", label: "attest", value: false },
      ]},
      optionsBlock(), outputBlock(),
    ],
    example: {
      request: () => ({
        markdown: t({ en: "# Quarterly report", fr: "# Rapport trimestriel" }) + "\n\n…",
        options: { theme: "report@1" },
        constraints: { max_pages: 4, no_split_tables: true, min_layout_score: 90 },
        client_id: "demo-client", pdf_name: t({ en: "report-2026", fr: "rapport-2026" }),
      }),
      response: { verdict: "met", score: 94, pages: 4, passes: [{ n: 1, score: 82, pages: 5, applied: ["tighter table"] }, { n: 2, score: 94, pages: 4, applied: [] }], unmet: [] },
    },
    statuses: [["200", { en: "PDF, or JSON { verdict, score, pages, passes, unmet, layout }", fr: "PDF, ou JSON { verdict, score, pages, passes, unmet, layout }" }], ["400", ST.badRequest], ["401", ST.unauthorized], ["500", { en: "Render failed", fr: "Échec du rendu" }], ["504", ST.timeout]],
  },

  {
    key: "attest", method: "POST", path: "/api/attest", json: true, group: "contract",
    title: { en: "Attestation of origin", fr: "Attestation d'origine" },
    icon: "M12 3l7 4v6c0 4-3 7-7 8-4-1-7-4-7-8V7zM9 12l2 2 4-4",
    card: { en: "A signed manifest that says this file came from here, and has not moved since.",
      fr: "Un manifeste signé qui dit que ce fichier vient d'ici, et qu'il n'a pas bougé depuis." },
    desc: { en: "Hash of the document, page count, engine, theme, PDF variant, operations applied, timestamp — sealed into one line, v1.<payload>.<signature>.\nThe record is self-contained: verifying it needs the file and that line, never a lookup in a database we would then have to keep, back up and eventually leak.",
      fr: "Empreinte du document, nombre de pages, moteur, thème, variante PDF, opérations appliquées, horodatage — scellés en une seule ligne, v1.<payload>.<signature>.\nLa fiche est autoportante : la vérifier demande le fichier et cette ligne, jamais une consultation dans une base qu'il faudrait ensuite conserver, sauvegarder, et finir par laisser fuir." },
    params: [
      sourceParam("pdf"),
      { name: "engine", type: "string", desc: { en: "Recorded as claimed by the caller — useful when the PDF was produced by a chain.", fr: "Enregistré tel que déclaré par l'appelant — utile quand le PDF vient d'une chaîne." } },
      { name: "theme", type: "string", desc: { en: "Brand kit as <code>name@version</code>.", fr: "Kit de marque, sous la forme <code>nom@version</code>." } },
      { name: "pdf_variant", type: "string", desc: { en: "e.g. <code>pdf/a-2b</code>.", fr: "par ex. <code>pdf/a-2b</code>." } },
      { name: "source_sha256", type: "string", desc: { en: "SHA-256 of the source document, when you kept it: 64 hexadecimal characters, checked before being sealed.", fr: "SHA-256 du document source, si vous l'avez gardé : 64 caractères hexadécimaux, vérifiés avant d'être scellés." } },
      { name: "operations", type: "string[]", desc: { en: "What was applied, 16 entries at most: <code>compress</code>, <code>ocr</code>, <code>redact</code>…", fr: "Ce qui a été appliqué, 16 entrées au plus : <code>compress</code>, <code>ocr</code>, <code>redact</code>…" } },
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { type: "row", fields: [
        { name: "engine", type: "text", label: "engine", placeholder: "weasyprint", value: "" },
        { name: "theme", type: "text", label: "theme", placeholder: "report@1", value: "" },
        { name: "pdf_variant", type: "text", label: "pdf_variant", placeholder: "pdf/a-2b", value: "" },
      ]},
      { name: "source_sha256", type: "text", label: "source_sha256", hint: { en: "64 hexadecimal characters", fr: "64 caractères hexadécimaux" }, value: "" },
      { name: "operations", type: "list", rows: 2, label: "operations",
        hint: { en: "one per line", fr: "une par ligne" }, value: "" },
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf", theme: "report@1", operations: ["compress", "ocr"] },
      response: { attestation: "v1.eyJ…….", claims: { version: 1, service: "md-to-pdf", issued_at: "2026-08-19T12:04:00Z", output_sha256: "9f2c…", bytes: 184203, pages: 12, theme: "report@1", operations: ["compress", "ocr"] } },
    },
    statuses: [["200", { en: "{ attestation, claims }", fr: "{ attestation, claims }" }], ["400", { en: "Malformed source_sha256", fr: "source_sha256 malformé" }], ["401", ST.unauthorized], ["404", ST.sourceMissing], ["504", ST.timeout]],
  },

  {
    key: "verify", method: "POST", path: "/api/verify", json: true, group: "contract",
    title: { en: "Verify an attestation", fr: "Vérifier une attestation" },
    icon: "M12 3l7 4v6c0 4-3 7-7 8-4-1-7-4-7-8V7zM8 12l3 3 5-6",
    card: { en: "Checks the signature and re-hashes the file: same document, or not.",
      fr: "Vérifie la signature et recalcule l'empreinte du fichier : même document, ou pas." },
    desc: { en: "Takes an attestation and the file it claims to describe, and answers with one of four verdicts.\nThe two failures are told apart deliberately: \"forged\" means the record itself was altered or came from another deployment, \"altered\" means the record is genuine and the document was edited. A single \"invalid\" would hide which incident happened.",
      fr: "Prend une attestation et le fichier qu'elle prétend décrire, et répond par l'un de quatre verdicts.\nLes deux échecs sont distingués volontairement : « forged » signifie que la fiche elle-même a été altérée ou vient d'un autre déploiement, « altered » que la fiche est authentique et que le document a été modifié. Un seul « invalid » masquerait lequel des deux incidents s'est produit." },
    params: [
      sourceParam("pdf"),
      { name: "attestation", type: "string", required: true, desc: { en: "The <code>v1.&lt;payload&gt;.&lt;signature&gt;</code> line returned by <code>/api/attest</code>, or by <code>/api/compose</code> with <code>attest: true</code>.", fr: "La ligne <code>v1.&lt;payload&gt;.&lt;signature&gt;</code> renvoyée par <code>/api/attest</code>, ou par <code>/api/compose</code> avec <code>attest: true</code>." } },
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "attestation", type: "textarea", rows: 3, label: "attestation", required: true, placeholder: "v1.…", value: "" },
    ],
    example: {
      request: { pdf: "/download/demo-client/report.pdf", attestation: "v1.eyJ……." },
      response: { verdict: "valid", detail: "The signature matches and the file still hashes to what was sealed.", attestation: { output_sha256: "9f2c…", pages: 12, issued_at: "2026-08-19T12:04:00Z" } },
    },
    statuses: [["200", { en: "{ verdict: valid | altered | forged | unreadable, detail, attestation }", fr: "{ verdict : valid | altered | forged | unreadable, detail, attestation }" }], ["401", ST.unauthorized], ["404", ST.sourceMissing]],
  },

  // ────────────────────────────────────────── travaux asynchrones ─────────

  {
    key: "jobs", method: "POST", path: "/api/jobs", json: true, group: "jobs",
    title: { en: "Submit a job", fr: "Soumettre un travail" },
    icon: "M12 6v6l4 2M12 3a9 9 0 100 18 9 9 0 000-18z",
    card: { en: "For the tools that outlive a request: OCR, LibreOffice, Ghostscript on a large file.",
      fr: "Pour les outils qui survivent à une requête : OCR, LibreOffice, Ghostscript sur un gros fichier." },
    desc: { en: "Takes the name of an endpoint and the body it would have received, and answers 202 with a job to poll.\nThe result is forced to an asset unless a download destination was named: a job has no socket to stream a file down.\nSupported: compress, ocr, pdfa, office-to-pdf, pdf-to-office, pages, rasterize, repair, images-to-pdf.",
      fr: "Prend le nom d'un endpoint et le corps qu'il aurait reçu, et répond 202 avec un travail à interroger.\nLe résultat est forcé en asset sauf si une destination de téléchargement est nommée : un travail n'a pas de socket pour faire descendre un fichier.\nSupportés : compress, ocr, pdfa, office-to-pdf, pdf-to-office, pages, rasterize, repair, images-to-pdf." },
    params: [
      { name: "endpoint", type: "string", required: true, desc: { en: "<code>ocr</code> or <code>/api/ocr</code>, both work.", fr: "<code>ocr</code> ou <code>/api/ocr</code>, les deux marchent." } },
      { name: "body", type: "object", required: true, desc: { en: "Exactly the body that endpoint takes.", fr: "Exactement le corps que prend cet endpoint." } },
      { name: "callback_url", type: "string", desc: { en: "http(s) URL notified once when the job ends, signed with <code>X-Signature</code>. Sent once, never retried.", fr: "URL http(s) notifiée une fois le travail terminé, signée par <code>X-Signature</code>. Envoyée une fois, jamais réessayée." } },
    ],
    fields: [
      { name: "endpoint", type: "select", label: "endpoint",
        options: ["compress", "ocr", "pdfa", "office-to-pdf", "pdf-to-office", "pages", "rasterize", "repair", "images-to-pdf"], value: "ocr" },
      { name: "body", type: "json", rows: 6, label: "body", required: true,
        value: '{\n  "pdf": "asset://as_…",\n  "mode": "auto"\n}' },
      { name: "callback_url", type: "text", label: "callback_url", placeholder: "https://…", value: "" },
    ],
    example: {
      request: { endpoint: "ocr", body: { pdf: "asset://as_9f2c…", languages: ["fra"] } },
      response: { job_id: "jb_4c1e…", status: "queued", kind: "ocr", poll_url: "/api/jobs/jb_4c1e…" },
    },
    statuses: [["202", { en: "Job accepted", fr: "Travail accepté" }], ["400", { en: "Endpoint not queueable / invalid body", fr: "Endpoint non asynchronisable / corps invalide" }], ["401", ST.unauthorized]],
  },

  {
    key: "job-status", method: "GET", path: "/api/jobs/{id}", group: "jobs",
    title: { en: "Poll a job", fr: "Interroger un travail" },
    icon: "M4 6h16M4 12h10M4 18h7M17 15l3 3-3 3",
    card: { en: "queued, running, done or failed — with the tool's own answer once it is done.",
      fr: "queued, running, done ou failed — avec la réponse de l'outil une fois terminé." },
    desc: { en: "The poll URL is the source of truth, callback or not.\nOnce \"done\", \"result\" holds exactly what the synchronous endpoint would have answered.",
      fr: "L'URL d'interrogation fait foi, avec ou sans callback.\nUne fois « done », « result » contient exactement ce que l'endpoint synchrone aurait répondu." },
    params: [{ name: "id", type: "path", required: true, desc: { en: "Job identifier returned on submission.", fr: "Identifiant de travail renvoyé à la soumission." } }],
    fields: [{ name: "id", type: "text", label: "id", required: true, placeholder: "jb_…", value: "" }],
    buildPath: (v) => `/api/jobs/${encodeURIComponent(v.id || "")}`,
    example: {
      response: { job_id: "jb_4c1e…", status: "done", kind: "ocr", poll_url: "/api/jobs/jb_4c1e…", result: { asset: { id: "as_77aa…", kind: "pdf", pages: 24 }, pages: 24 } },
    },
    statuses: [["200", "JobView"], ["401", ST.unauthorized], ["404", { en: "Unknown or expired job", fr: "Travail inconnu ou expiré" }]],
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
  { id: "contract", title: { en: "Contract and proof", fr: "Contrat et preuve" } },
  { id: "process", title: { en: "Post-processing", fr: "Post-traitement" } },
  { id: "organize", title: { en: "Pages", fr: "Pages" } },
  { id: "optimize", title: { en: "Optimise and repair", fr: "Optimiser et réparer" } },
  { id: "convert", title: { en: "Conversions", fr: "Conversions" } },
  { id: "files", title: { en: "Files", fr: "Fichiers" } },
  { id: "jobs", title: { en: "Asynchronous jobs", fr: "Travaux asynchrones" } },
  { id: "service", title: { en: "Service", fr: "Service" } },
  { id: "legacy", title: { en: "Compatibility", fr: "Compatibilité" } },
];

// [{ id, title, endpoints: [...] }] — les groupes vides sont écartés.
const GROUPED = GROUPS
  .map((g) => ({ ...g, endpoints: ENDPOINTS.filter((e) => e.group === g.id) }))
  .filter((g) => g.endpoints.length);

// Ordre de parcours (précédent / suivant dans la référence).
const ORDERED_KEYS = GROUPED.flatMap((g) => g.endpoints.map((e) => e.key));
