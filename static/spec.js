/* ==========================================================================
   md-to-pdf — spécification de l'API
   Source unique : elle alimente la référence, la console et les exemples de
   code. La doc ne peut donc pas diverger de ce que la console envoie.
   ========================================================================== */
"use strict";

// ══════════════════════════════════════════════════════════ exemples

const SAMPLE_MD = `# Rapport d'analyse

Un paragraphe en **markdown** avec une liste :

- premier point
- deuxième point

## Section premium

{{CENSOR}}

## Conclusion

Contenu de nouveau visible.`;

const SAMPLE_HTML = `<h1>Facture #123</h1>
<p>Client : <strong>ACME</strong></p>
<table border="1" cellpadding="6" cellspacing="0">
  <tr><th>Article</th><th>Prix</th></tr>
  <tr><td>Abonnement</td><td>49,00 €</td></tr>
</table>`;

const SAMPLE_TEMPLATE = `<h1>{{ title }}</h1>
<p>Bonjour {{ name }},</p>
<ul>
  {% for line in lines %}<li>{{ line }}</li>{% endfor %}
</ul>`;

const SAMPLE_DATA = `{
  "title": "Facture #123",
  "name": "Jean Dupont",
  "lines": ["Abonnement annuel", "Support premium"]
}`;

// ══════════════════════════════════════════════════════════ blocs de champs

const cssField = {
  name: "css", type: "textarea", rows: 3, label: "css",
  hint: "concaténé après templates/default.css", value: "",
};

const engineField = {
  name: "engine", type: "select", label: "engine",
  options: ["", "weasyprint", "wkhtmltopdf", "pdflatex"], value: "",
};

const optionsBlock = () => ({
  type: "fieldset", legend: "options (mise en page)", collapsed: true, fields: [
    { type: "row", fields: [
      { name: "options.paper_size", type: "select", label: "paper_size", options: ["", "a4", "a3", "letter"], value: "" },
      { name: "options.orientation", type: "select", label: "orientation", options: ["", "portrait", "landscape"], value: "" },
    ]},
    { type: "row", fields: [
      { name: "options.margins.top", type: "text", label: "marge haut", placeholder: "2cm", value: "" },
      { name: "options.margins.right", type: "text", label: "droite", placeholder: "2cm", value: "" },
      { name: "options.margins.bottom", type: "text", label: "bas", placeholder: "2cm", value: "" },
      { name: "options.margins.left", type: "text", label: "gauche", placeholder: "2cm", value: "" },
    ]},
    { name: "options.page_numbers", type: "checkbox", label: "page_numbers", value: false },
    { name: "options.page_number_format", type: "text", label: "page_number_format",
      hint: 'valeur CSS content, ex. counter(page) " / " counter(pages)', value: "" },
    { type: "row", fields: [
      { name: "options.toc", type: "checkbox", label: "toc", value: false },
      { name: "options.toc_depth", type: "number", label: "toc_depth", value: "" },
    ]},
    { name: "options.watermark", type: "text", label: "watermark (filigrane CSS)", value: "" },
  ],
});

const saveBlock = () => ({
  type: "fieldset", legend: "sauvegarde serveur — sinon PDF binaire en réponse", collapsed: false, fields: [
    { type: "row", fields: [
      { name: "client_id", type: "text", label: "client_id", placeholder: "demo-client", value: "" },
      { name: "pdf_name", type: "text", label: "pdf_name", placeholder: "mon-document", value: "" },
    ]},
  ],
});

const headerFooterBlock = () => ({
  type: "fieldset", legend: "en-tête / pied de page", collapsed: true, fields: [
    { name: "header_html", type: "textarea", rows: 2, label: "header_html", hint: "prioritaire sur header_template", value: "" },
    { name: "footer_html", type: "textarea", rows: 2, label: "footer_html", value: "" },
    { type: "row", fields: [
      { name: "header_template", type: "text", label: "header_template", placeholder: "header.html", value: "" },
      { name: "footer_template", type: "text", label: "footer_template", placeholder: "footer.html", value: "" },
    ]},
  ],
});

// Paramètres communs, pour les tables de la référence
const OPTIONS_PARAM = {
  name: "options", type: "object", desc:
    "Mise en page : <code>paper_size</code> (a4, a3, letter), <code>orientation</code>, " +
    "<code>margins</code> {top, right, bottom, left}, <code>page_numbers</code>, " +
    "<code>page_number_format</code>, <code>toc</code>, <code>toc_depth</code>, <code>watermark</code>.",
};

const SAVE_PARAMS = [
  { name: "client_id", type: "string", desc: "Dossier de destination. Nom simple : <code>[A-Za-z0-9._-]</code>, 128 caractères max, ne commence pas par un point." },
  { name: "pdf_name", type: "string", desc: "Nom du fichier (le suffixe <code>.pdf</code> est ajouté s'il manque). Mêmes contraintes que <code>client_id</code>." },
];

const CSS_PARAM = { name: "css", type: "string", desc: "Feuille de style additionnelle, appliquée après <code>templates/default.css</code>." };

// ══════════════════════════════════════════════════════════ endpoints

const ENDPOINTS = [
  {
    key: "health", method: "GET", path: "/api/health", auth: false, group: "service",
    title: "Santé du service",
    icon: "M12 21s-7-4.4-7-10a7 7 0 0114 0c0 5.6-7 10-7 10z",
    card: "Statut, version et moteurs PDF réellement installés dans l'image. Jamais authentifié : c'est la sonde du healthcheck.",
    desc: "Retourne le statut du service, sa version et la liste des moteurs PDF réellement présents dans l'image.\n« status » vaut « degraded » si pandoc ou WeasyPrint manquent.",
    params: [],
    fields: [],
    example: { response: { status: "ok", version: "0.2.0", engines: ["weasyprint", "wkhtmltopdf", "pdflatex"] } },
    statuses: [["200", "Service joignable"]],
  },

  {
    key: "convert", method: "POST", path: "/api/convert", json: true, group: "generate",
    title: "Markdown → PDF",
    icon: "M4 4h11l5 5v11H4z",
    card: "Conversion Markdown via pandoc, avec CSS, en-têtes, sommaire et remplacement des blocs CENSOR.",
    desc: "Convertit du Markdown en PDF via pandoc.\nLes tags CENSOR ({{CENSOR}}, <CENSOR>, {{ CENSOR }}…) sont remplacés par le bloc d'image floutée « contenu premium ».",
    params: [
      { name: "markdown", type: "string", required: true, desc: "Le document source." },
      CSS_PARAM,
      { name: "engine", type: "enum", desc: "<code>weasyprint</code> (défaut), <code>wkhtmltopdf</code> ou <code>pdflatex</code>." },
      OPTIONS_PARAM,
      { name: "header_html", type: "string", desc: "HTML injecté dans l'en-tête. Prioritaire sur <code>header_template</code>." },
      { name: "footer_html", type: "string", desc: "HTML injecté après le corps du document." },
      { name: "header_template", type: "string", desc: "Nom d'un fichier du dossier <code>templates/</code>, par ex. <code>header.html</code>." },
      { name: "footer_template", type: "string", desc: "Idem pour le pied de page." },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "markdown", type: "textarea", rows: 12, label: "markdown", required: true, value: SAMPLE_MD },
      cssField, engineField, optionsBlock(), headerFooterBlock(), saveBlock(),
    ],
    example: {
      request: {
        markdown: "# Rapport\n\nContenu en **markdown**.",
        options: { paper_size: "a4", page_numbers: true },
        client_id: "demo-client", pdf_name: "rapport-2026",
      },
      response: { download_url: "/download/demo-client/rapport-2026.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "Requête invalide"], ["401", "Clé d'API"], ["404", "Template introuvable"], ["500", "Échec pandoc"], ["504", "Timeout"]],
  },

  {
    key: "render", method: "POST", path: "/api/render", json: true, group: "generate",
    title: "Template Tera → PDF",
    icon: "M4 6h16M4 12h10M4 18h7",
    card: "Rendu d'un template Tera (syntaxe Jinja2) avec vos données JSON, puis conversion via WeasyPrint.",
    desc: "Rend un template HTML au format Tera avec les données fournies, puis convertit le résultat en PDF via WeasyPrint.\nLe champ « data » doit être un objet JSON.",
    params: [
      { name: "template", type: "string", required: true, desc: "Template HTML, syntaxe Tera : <code>{{ variable }}</code>, <code>{% for %}</code>, filtres." },
      { name: "data", type: "object", required: true, desc: "Contexte de rendu. Doit être un objet (un tableau ou une chaîne renvoie 400)." },
      CSS_PARAM, OPTIONS_PARAM, ...SAVE_PARAMS,
    ],
    fields: [
      { name: "template", type: "textarea", rows: 8, label: "template", required: true, value: SAMPLE_TEMPLATE },
      { name: "data", type: "json", rows: 6, label: "data", required: true, value: SAMPLE_DATA },
      cssField, optionsBlock(), saveBlock(),
    ],
    example: {
      request: { template: "<h1>{{ title }}</h1>", data: { title: "Facture #123" } },
      response: { download_url: "/download/demo-client/facture-123.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "Template ou data invalide"], ["401", "Clé d'API"], ["500", "Échec WeasyPrint"], ["504", "Timeout"]],
  },

  {
    key: "html-to-pdf", method: "POST", path: "/api/html-to-pdf", json: true, group: "generate",
    title: "HTML → PDF",
    icon: "M8 6l-4 6 4 6M16 6l4 6-4 6",
    card: "Pour du HTML déjà construit côté client. Les chemins relatifs sont résolus depuis le répertoire du service.",
    desc: "Convertit du HTML brut en PDF via WeasyPrint, sans passer par pandoc.\nLes chemins relatifs (images, static/…) sont résolus depuis le répertoire de travail du serveur.",
    params: [
      { name: "html", type: "string", required: true, desc: "Document HTML complet ou fragment." },
      CSS_PARAM, OPTIONS_PARAM, ...SAVE_PARAMS,
    ],
    fields: [
      { name: "html", type: "textarea", rows: 12, label: "html", required: true, value: SAMPLE_HTML },
      cssField, optionsBlock(), saveBlock(),
    ],
    example: {
      request: { html: "<h1>Facture</h1>", options: { paper_size: "a4" } },
      response: { download_url: "/download/demo-client/facture.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "Requête invalide"], ["401", "Clé d'API"], ["500", "Échec WeasyPrint"], ["504", "Timeout"]],
  },

  {
    key: "preview", method: "POST", path: "/api/preview", json: true, group: "generate",
    title: "Aperçu PNG",
    icon: "M4 5h16v14H4zM8 13l3-3 3 3 2-2 2 2",
    card: "La première page en PNG 150 dpi, pour une vignette ou un aperçu avant génération.",
    desc: "Rend la première page en PNG (150 dpi).\nModes exclusifs, évalués dans cet ordre : markdown, template + data, html.",
    params: [
      { name: "markdown", type: "string", desc: "Mode markdown (prioritaire)." },
      { name: "template", type: "string", desc: "Mode template, à combiner avec <code>data</code>." },
      { name: "data", type: "object", desc: "Contexte du template. Obligatoire si <code>template</code> est fourni." },
      { name: "html", type: "string", desc: "Mode HTML brut." },
      CSS_PARAM,
      { name: "engine", type: "enum", desc: "Moteur utilisé en mode markdown." },
      OPTIONS_PARAM,
    ],
    fields: [
      { name: "__mode", type: "select", label: "mode", options: ["markdown", "template", "html"], value: "markdown" },
      { name: "markdown", type: "textarea", rows: 8, label: "markdown", value: SAMPLE_MD, showFor: "markdown" },
      { name: "template", type: "textarea", rows: 6, label: "template", value: SAMPLE_TEMPLATE, showFor: "template" },
      { name: "data", type: "json", rows: 5, label: "data", value: SAMPLE_DATA, showFor: "template" },
      { name: "html", type: "textarea", rows: 8, label: "html", value: SAMPLE_HTML, showFor: "html" },
      cssField, engineField, optionsBlock(),
    ],
    example: {
      request: { markdown: "# Aperçu" },
      responseNote: "Corps binaire image/png",
    },
    statuses: [["200", "image/png"], ["400", "Aucun mode fourni"], ["401", "Clé d'API"], ["500", "Échec pdftoppm"], ["504", "Timeout"]],
  },

  {
    key: "merge", method: "POST", path: "/api/merge", json: true, group: "process",
    title: "Fusion de PDFs",
    icon: "M7 4h7l4 4v12H7zM3 8h3v12h9",
    card: "Concatène des PDFs déjà sauvegardés, dans l'ordre fourni (pdfunite).",
    desc: "Fusionne au moins deux PDFs précédemment sauvegardés, dans l'ordre du tableau.\nLes chemins sont ceux renvoyés par les autres endpoints : /download/<client_id>/<pdf_name>.",
    params: [
      { name: "pdfs", type: "string[]", required: true, desc: "Au moins 2 chemins <code>/download/…</code>. Tout chemin sortant de <code>public/pdf</code> est rejeté." },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdfs", type: "pdflist", label: "pdfs", required: true,
        hint: "un chemin par ligne", value: "" },
      saveBlock(),
    ],
    example: {
      request: { pdfs: ["/download/demo-client/a.pdf", "/download/demo-client/b.pdf"], client_id: "demo-client", pdf_name: "dossier-complet" },
      response: { download_url: "/download/demo-client/dossier-complet.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "Moins de 2 PDFs / chemin invalide"], ["401", "Clé d'API"], ["404", "PDF introuvable"], ["500", "Échec pdfunite"], ["504", "Timeout"]],
  },

  {
    key: "watermark", method: "POST", path: "/api/watermark", json: true, group: "process",
    title: "Filigrane",
    icon: "M12 3l7 4v6c0 4-3 7-7 8-4-1-7-4-7-8V7z",
    card: "Superpose un texte en diagonale sur toutes les pages d'un PDF existant (overlay qpdf).",
    desc: "Génère un calque contenant le texte puis le superpose au PDF source avec qpdf.\nLe texte est échappé : aucun risque d'injection dans le calque HTML.",
    params: [
      { name: "pdf", type: "string", required: true, desc: "Chemin <code>/download/…</code> du PDF source." },
      { name: "text", type: "string", required: true, desc: "Texte du filigrane." },
      { name: "opacity", type: "number", desc: "Entre 0 et 1. Défaut <code>0.06</code>." },
      { name: "angle", type: "number", desc: "Entre -360 et 360 degrés. Défaut <code>-45</code>." },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "text", type: "text", label: "text", required: true, value: "BROUILLON" },
      { type: "row", fields: [
        { name: "opacity", type: "number", step: "0.01", label: "opacity", hint: "0 → 1", value: "0.06" },
        { name: "angle", type: "number", step: "1", label: "angle", hint: "-360 → 360", value: "-45" },
      ]},
      saveBlock(),
    ],
    example: {
      request: { pdf: "/download/demo-client/rapport.pdf", text: "CONFIDENTIEL", opacity: 0.08 },
      response: { download_url: "/download/demo-client/rapport-filigrane.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "opacity / angle hors bornes"], ["401", "Clé d'API"], ["404", "PDF introuvable"], ["500", "Échec qpdf"], ["504", "Timeout"]],
  },

  {
    key: "protect", method: "POST", path: "/api/protect", json: true, group: "process",
    title: "Protection par mot de passe",
    icon: "M7 11V8a5 5 0 0110 0v3M5 11h14v9H5z",
    card: "Chiffrement AES-256 via qpdf. Le mot de passe transite par un fichier d'arguments, jamais par la ligne de commande.",
    desc: "Chiffre un PDF sauvegardé en AES-256 (mot de passe utilisateur = mot de passe propriétaire).",
    params: [
      { name: "pdf", type: "string", required: true, desc: "Chemin <code>/download/…</code> du PDF source." },
      { name: "password", type: "string", required: true, desc: "Non vide, sans retour à la ligne." },
      ...SAVE_PARAMS,
    ],
    fields: [
      { name: "pdf", type: "pdfpick", label: "pdf", required: true, value: "" },
      { name: "password", type: "text", label: "password", required: true, value: "secret123" },
      saveBlock(),
    ],
    example: {
      request: { pdf: "/download/demo-client/rapport.pdf", password: "s3cr3t" },
      response: { download_url: "/download/demo-client/rapport-protege.pdf" },
    },
    statuses: [["200", "PDF ou download_url"], ["400", "Mot de passe vide"], ["401", "Clé d'API"], ["404", "PDF introuvable"], ["500", "Échec qpdf"], ["504", "Timeout"]],
  },

  {
    key: "download", method: "GET", path: "/download/{client_id}/{pdf_name}", auth: false, group: "files",
    title: "Téléchargement",
    icon: "M12 4v10m0 0l-4-4m4 4l4-4M5 20h14",
    card: "Récupère un PDF sauvegardé, servi en pièce jointe avec le bon Content-Disposition.",
    desc: "Sert un PDF précédemment sauvegardé. Les deux segments sont validés : impossible de sortir de public/pdf.",
    params: [
      { name: "client_id", type: "path", required: true, desc: "Segment d'URL." },
      { name: "pdf_name", type: "path", required: true, desc: "Segment d'URL, avec l'extension." },
    ],
    fields: [
      { type: "row", fields: [
        { name: "client_id", type: "text", label: "client_id", required: true, value: "demo-client" },
        { name: "pdf_name", type: "text", label: "pdf_name", required: true, value: "mon-document.pdf" },
      ]},
    ],
    buildPath: (v) => `/download/${encodeURIComponent(v.client_id || "")}/${encodeURIComponent(v.pdf_name || "")}`,
    example: { responseNote: "Corps binaire application/pdf" },
    statuses: [["200", "application/pdf"], ["404", "Fichier inconnu"]],
  },

  {
    key: "legacy", method: "POST", path: "/", form: true, auth: false, group: "legacy",
    title: "Endpoint historique (FormData)",
    icon: "M4 7h16M4 12h16M4 17h10",
    card: "L'API d'origine, préservée à l'identique : FormData en entrée, erreurs en texte brut.",
    desc: "Endpoint d'origine conservé pour la rétro-compatibilité. Corps en multipart/form-data, erreurs renvoyées en texte brut.\nJamais soumis à API_KEY.",
    params: [
      { name: "markdown", type: "field", required: true, desc: "Document source." },
      { name: "css", type: "field", desc: "CSS additionnel." },
      { name: "engine", type: "field", desc: "weasyprint, wkhtmltopdf ou pdflatex." },
      { name: "header_template", type: "field", desc: "Fichier du dossier <code>templates/</code>." },
      { name: "footer_template", type: "field", desc: "Idem pied de page." },
      { name: "client_id", type: "field", desc: "Sauvegarde serveur." },
      { name: "pdf_name", type: "field", desc: "Sauvegarde serveur." },
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
    example: { responseNote: "PDF binaire, ou {\"download_url\": \"…\"} si client_id et pdf_name sont fournis" },
    statuses: [["200", "PDF ou download_url"], ["400", "Erreur pandoc (texte brut)"], ["500", "Erreur interne"]],
  },
];

const BY_KEY = Object.fromEntries(ENDPOINTS.map((e) => [e.key, e]));

// Regroupement des endpoints dans les navigations. L'ordre des groupes est
// celui de la liste ci-dessous ; un endpoint sans groupe connu finit en fin.
const GROUPS = [
  { id: "generate", title: "Génération" },
  { id: "process", title: "Post-traitement" },
  { id: "files", title: "Fichiers" },
  { id: "service", title: "Service" },
  { id: "legacy", title: "Compatibilité" },
];

// [{ id, title, endpoints: [...] }] — les groupes vides sont écartés.
const GROUPED = GROUPS
  .map((g) => ({ ...g, endpoints: ENDPOINTS.filter((e) => e.group === g.id) }))
  .filter((g) => g.endpoints.length);

// Ordre de parcours (précédent / suivant dans la référence).
const ORDERED_KEYS = GROUPED.flatMap((g) => g.endpoints.map((e) => e.key));
