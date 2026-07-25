/* ==========================================================================
   md-to-pdf — documentation et console de test
   La spécification ci-dessous alimente à la fois la doc et le formulaire de la
   console : les deux ne peuvent pas diverger.
   ========================================================================== */
"use strict";

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const escapeHtml = (s) =>
  String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

// ══════════════════════════════════════════════════════════ samples

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

// ══════════════════════════════════════════════════════════ form blocks

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

// Paramètres communs, pour la table de documentation
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

// ══════════════════════════════════════════════════════════ spec

const ENDPOINTS = [
  {
    key: "health", method: "GET", path: "/api/health", auth: false,
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
    key: "convert", method: "POST", path: "/api/convert", json: true,
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
    key: "render", method: "POST", path: "/api/render", json: true,
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
    key: "html-to-pdf", method: "POST", path: "/api/html-to-pdf", json: true,
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
    key: "preview", method: "POST", path: "/api/preview", json: true,
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
    key: "merge", method: "POST", path: "/api/merge", json: true,
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
    key: "watermark", method: "POST", path: "/api/watermark", json: true,
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
    key: "protect", method: "POST", path: "/api/protect", json: true,
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
    key: "download", method: "GET", path: "/download/{client_id}/{pdf_name}", auth: false,
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
    key: "legacy", method: "POST", path: "/", form: true, auth: false,
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

// ══════════════════════════════════════════════════════════ state

const state = {
  baseUrl: localStorage.getItem("mdpdf.baseUrl") || location.origin,
  apiKey: localStorage.getItem("mdpdf.apiKey") || "",
  saved: JSON.parse(localStorage.getItem("mdpdf.saved") || "[]"),
  current: "convert",
  lastCurl: "",
  blobUrl: null,
};

function persist() {
  localStorage.setItem("mdpdf.baseUrl", state.baseUrl);
  localStorage.setItem("mdpdf.apiKey", state.apiKey);
  localStorage.setItem("mdpdf.saved", JSON.stringify(state.saved));
}

// ══════════════════════════════════════════════════════════ highlighting

function highlightJson(text) {
  return escapeHtml(text).replace(
    /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false|null)\b|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
    (m, str, colon, lit, num) => {
      if (str) return colon ? `<span class="hl-key">${str}</span>${colon}` : `<span class="hl-str">${str}</span>`;
      if (lit) return `<span class="hl-lit">${lit}</span>`;
      if (num) return `<span class="hl-num">${num}</span>`;
      return m;
    }
  );
}

function highlightShell(text) {
  return escapeHtml(text)
    .split("\n")
    .map((line) => {
      if (/^\s*#/.test(line)) return `<span class="hl-cmt">${line}</span>`;
      return line
        .replace(/^(\s*)(curl|docker|make|export|cargo)\b/, '$1<span class="hl-cmd">$2</span>')
        .replace(/(\s)(--?[a-zA-Z][\w-]*)/g, '$1<span class="hl-flag">$2</span>')
        .replace(/('(?:[^']|\\')*')/g, '<span class="hl-str">$1</span>');
    })
    .join("\n");
}

function highlightCode(text, lang) {
  if (lang === "json") return highlightJson(text);
  if (lang === "shell") return highlightShell(text);
  return escapeHtml(text);
}

const pretty = (obj) => JSON.stringify(obj, null, 2);

// ══════════════════════════════════════════════════════════ landing

function renderFeatureCards() {
  const wrap = $("#featureCards");
  wrap.innerHTML = ENDPOINTS.map((ep) => `
    <article class="card">
      <h3>
        <svg class="card-icon" viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
          <path d="${ep.icon}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        ${escapeHtml(ep.title)}
      </h3>
      <p>${escapeHtml(ep.card)}</p>
      <a class="card-route" href="#ep-${ep.key}">${ep.method} ${escapeHtml(ep.path)}</a>
    </article>`).join("");
}

const QUICKSTART = {
  curl: () => `# Markdown → PDF, réponse binaire
curl -X POST '${state.baseUrl}/api/convert' \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Bonjour\\n\\nUn **document**.", "options": {"page_numbers": true}}' \\
  --output document.pdf

# Sauvegarde côté serveur : la réponse devient une URL de téléchargement
curl -X POST '${state.baseUrl}/api/convert' \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Bonjour", "client_id": "demo", "pdf_name": "hello"}'
# {"download_url":"/download/demo/hello.pdf"}

# Avec authentification, si API_KEY est configurée côté serveur
curl -X POST '${state.baseUrl}/api/convert' \\
  -H 'X-API-Key: $API_KEY' \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Bonjour"}' --output document.pdf`,

  js: () => `const res = await fetch("${state.baseUrl}/api/convert", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    // "X-API-Key": process.env.API_KEY,   // si API_KEY est configurée
  },
  body: JSON.stringify({
    markdown: "# Rapport\\n\\nContenu en **markdown**.",
    options: { paper_size: "a4", page_numbers: true },
  }),
});

if (!res.ok) {
  const { error, details } = await res.json();
  throw new Error(\`\${error}: \${details}\`);
}

const pdf = await res.arrayBuffer();   // corps binaire application/pdf`,

  python: () => `import requests

res = requests.post(
    "${state.baseUrl}/api/convert",
    json={
        "markdown": "# Rapport\\n\\nContenu en **markdown**.",
        "options": {"paper_size": "a4", "page_numbers": True},
        "client_id": "demo",
        "pdf_name": "rapport",
    },
    # headers={"X-API-Key": os environ API_KEY},
    timeout=90,
)
res.raise_for_status()

print(res.json()["download_url"])   # /download/demo/rapport.pdf`,

  docker: () => `# Lancer le service
docker compose -f docker-compose.prod.yml up -d

# Vérifier
curl -s ${state.baseUrl}/api/health

# Fermer l'API derrière une clé
export API_KEY='une-cle-longue-et-aleatoire'
docker compose -f docker-compose.prod.yml up -d`,
};

function renderQuickstart(lang) {
  const code = QUICKSTART[lang]();
  $("#quickCode").innerHTML = highlightCode(code, lang === "curl" || lang === "docker" ? "shell" : "text");
  $("#quickCode").dataset.raw = code;
}

function renderDeploy() {
  const code = `# docker-compose.prod.yml
services:
  md-to-pdf:
    build: { context: ., dockerfile: Dockerfile }
    ports: ["8000:8000"]
    restart: unless-stopped
    environment:
      API_KEY: "\${API_KEY:-}"
      PDF_PROCESS_TIMEOUT_SECS: "60"
    volumes:
      - pdf-storage:/home/rocket/public/pdf
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8000/api/health"]
      interval: 30s

volumes:
  pdf-storage:`;
  $("#deployCode").innerHTML = highlightCode(code, "shell");
  $("#deployCode").dataset.raw = code;
}

// ══════════════════════════════════════════════════════════ API doc

function renderDoc() {
  $("#docNav").innerHTML = ENDPOINTS.map((ep) => `
    <a href="#ep-${ep.key}" data-key="${ep.key}">
      <span class="m">${ep.method}</span>${escapeHtml(ep.key)}
    </a>`).join("");

  $("#docBody").innerHTML = ENDPOINTS.map((ep) => {
    const params = ep.params.length ? `
      <div class="table-wrap">
        <table class="params">
          <thead><tr><th>Paramètre</th><th>Type</th><th>Description</th></tr></thead>
          <tbody>
            ${ep.params.map((p) => `
              <tr>
                <td>${escapeHtml(p.name)}${p.required ? ' <span class="req">*</span>' : ""}</td>
                <td class="type">${escapeHtml(p.type)}</td>
                <td><p>${p.desc}</p></td>
              </tr>`).join("")}
          </tbody>
        </table>
      </div>` : "";

    const reqBlock = ep.example && ep.example.request ? `
      <div class="ep-col">
        <h4>Requête</h4>
        <div class="code-wrap">
          <button class="copy-btn" data-copy="#req-${ep.key}">copier</button>
          <pre id="req-${ep.key}" data-raw="${escapeHtml(pretty(ep.example.request))}">${highlightJson(pretty(ep.example.request))}</pre>
        </div>
      </div>` : "";

    const resContent = ep.example && ep.example.response
      ? `<pre id="res-${ep.key}" data-raw="${escapeHtml(pretty(ep.example.response))}">${highlightJson(pretty(ep.example.response))}</pre>`
      : `<pre id="res-${ep.key}">${escapeHtml(ep.example && ep.example.responseNote ? ep.example.responseNote : "")}</pre>`;

    const resBlock = `
      <div class="ep-col">
        <h4>Réponse</h4>
        <div class="code-wrap">${resContent}</div>
      </div>`;

    return `
      <article class="endpoint" id="ep-${ep.key}">
        <div class="ep-head">
          <span class="method ${ep.method.toLowerCase()}">${ep.method}</span>
          <code class="path">${escapeHtml(ep.path)}</code>
          <button class="try" data-try="${ep.key}">Tester ↓</button>
        </div>
        <p class="ep-desc">${escapeHtml(ep.desc)}</p>
        <div class="ep-auth">${ep.auth === false ? "🔓 jamais authentifié" : "🔐 X-API-Key si API_KEY est configurée"}${ep.json ? " · application/json" : ep.form ? " · multipart/form-data" : ""}</div>
        ${params}
        <div class="ep-cols">${reqBlock}${resBlock}</div>
        <div class="status-list">
          ${ep.statuses.map(([code, label]) => `<span><b>${code}</b> ${escapeHtml(label)}</span>`).join("")}
        </div>
      </article>`;
  }).join("");

  $$("[data-try]").forEach((btn) => {
    btn.onclick = () => {
      selectEndpoint(btn.dataset.try);
      $("#playground").scrollIntoView({ behavior: "smooth", block: "start" });
    };
  });
}

// ══════════════════════════════════════════════════════════ console form

const fieldId = (name) => "f_" + name.replace(/[^\w]/g, "_");

function renderField(field, container) {
  if (field.type === "fieldset") {
    const fs = document.createElement("fieldset");
    if (field.collapsed) fs.className = "collapsed";
    const legend = document.createElement("legend");
    const setLabel = () => {
      legend.textContent = (fs.classList.contains("collapsed") ? "▸ " : "▾ ") + field.legend;
    };
    legend.onclick = () => { fs.classList.toggle("collapsed"); setLabel(); };
    setLabel();
    fs.appendChild(legend);
    field.fields.forEach((f) => renderField(f, fs));
    container.appendChild(fs);
    return;
  }

  if (field.type === "row") {
    const row = document.createElement("div");
    row.className = "row";
    field.fields.forEach((f) => renderField(f, row));
    container.appendChild(row);
    return;
  }

  const wrap = document.createElement("div");
  wrap.className = "field";
  wrap.dataset.field = field.name;
  if (field.showFor) wrap.dataset.showFor = field.showFor;

  if (field.type === "checkbox") {
    wrap.classList.add("checkbox");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.id = fieldId(field.name);
    input.dataset.name = field.name;
    input.dataset.kind = "checkbox";
    input.checked = !!field.value;
    const label = document.createElement("label");
    label.htmlFor = input.id;
    label.textContent = field.label;
    wrap.append(input, label);
    container.appendChild(wrap);
    return;
  }

  const label = document.createElement("label");
  label.htmlFor = fieldId(field.name);
  label.textContent = field.label + (field.required ? " *" : "");
  if (field.hint) {
    const hint = document.createElement("span");
    hint.className = "hint";
    hint.textContent = " — " + field.hint;
    label.appendChild(hint);
  }
  wrap.appendChild(label);

  let input;
  if (field.type === "textarea" || field.type === "json" || field.type === "pdflist") {
    input = document.createElement("textarea");
    input.rows = field.rows || 4;
    input.spellcheck = false;
    input.value = field.value || "";
    if (field.type === "pdflist") input.placeholder = "/download/demo-client/doc1.pdf";
  } else if (field.type === "select") {
    input = document.createElement("select");
    field.options.forEach((opt) => {
      const o = document.createElement("option");
      o.value = opt;
      o.textContent = opt === "" ? "(défaut)" : opt;
      input.appendChild(o);
    });
    input.value = field.value || "";
  } else {
    input = document.createElement("input");
    input.type = field.type === "number" ? "number" : "text";
    input.spellcheck = false;
    if (field.step) input.step = field.step;
    if (field.placeholder) input.placeholder = field.placeholder;
    input.value = field.value || "";
  }

  input.id = fieldId(field.name);
  input.dataset.name = field.name;
  input.dataset.kind = field.type;

  if (field.type === "pdfpick" || field.type === "pdflist") {
    const picker = document.createElement("div");
    picker.className = "pdf-picker";
    const select = document.createElement("select");
    select.innerHTML = '<option value="">— PDFs générés —</option>' +
      state.saved.map((u) => `<option value="${escapeHtml(u)}">${escapeHtml(u.replace("/download/", ""))}</option>`).join("");
    const add = document.createElement("button");
    add.type = "button";
    add.textContent = field.type === "pdflist" ? "ajouter" : "utiliser";
    add.onclick = () => {
      if (!select.value) return;
      if (field.type === "pdflist") {
        input.value = (input.value.trim() ? input.value.trim() + "\n" : "") + select.value;
      } else {
        input.value = select.value;
      }
    };
    picker.append(select, add);
    wrap.appendChild(picker);
  }

  wrap.appendChild(input);
  container.appendChild(wrap);
}

function selectEndpoint(key) {
  state.current = key;
  const ep = BY_KEY[key];
  const form = $("#form");
  form.innerHTML = "";
  ep.fields.forEach((f) => renderField(f, form));

  $("#reqMethod").textContent = ep.method;
  $("#reqMethod").className = "method " + ep.method.toLowerCase();
  $("#reqPath").textContent = ep.path;
  $("#reqDesc").textContent = ep.desc;

  const mode = form.querySelector('[data-name="__mode"]');
  if (mode) {
    const apply = () => {
      form.querySelectorAll("[data-show-for]").forEach((el) => {
        el.hidden = el.dataset.showFor !== mode.value;
      });
    };
    mode.onchange = apply;
    apply();
  }

  $$("#endpointList button").forEach((b) => b.classList.toggle("active", b.dataset.key === key));
}

function showEmptyResponse() {
  $("#tabPreview").innerHTML = `
    <div class="res-empty">
      <svg viewBox="0 0 24 24" width="34" height="34" aria-hidden="true">
        <path d="M5 4h9l5 5v11H5zM14 4v5h5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/>
      </svg>
      <div>La réponse s'affichera ici — PDF, PNG ou JSON.</div>
      <div><kbd>⌘</kbd> <kbd>⏎</kbd> pour envoyer</div>
    </div>`;
}

// ══════════════════════════════════════════════════════════ request building

function findSpec(fields, name) {
  for (const f of fields) {
    if (f.name === name) return f;
    if (f.fields) {
      const found = findSpec(f.fields, name);
      if (found) return found;
    }
  }
  return null;
}

function collectValues() {
  const values = {};
  $("#form").querySelectorAll("[data-name]").forEach((el) => {
    const wrap = el.closest(".field");
    if (wrap && wrap.hidden) return;
    values[el.dataset.name] = el.type === "checkbox" ? el.checked : el.value;
  });
  return values;
}

function buildPayload(ep, values) {
  const body = {};

  const setDeep = (path, value) => {
    const parts = path.split(".");
    let node = body;
    for (let i = 0; i < parts.length - 1; i++) {
      node[parts[i]] = node[parts[i]] || {};
      node = node[parts[i]];
    }
    node[parts[parts.length - 1]] = value;
  };

  Object.entries(values).forEach(([name, raw]) => {
    if (name.startsWith("__")) return;
    if (raw === "" || raw === false || raw == null) return;

    const spec = findSpec(ep.fields, name);
    const kind = spec ? spec.type : "text";
    let value = raw;

    if (kind === "json") {
      try {
        value = JSON.parse(raw);
      } catch (e) {
        throw new Error(`Champ « ${name} » : JSON invalide — ${e.message}`);
      }
    } else if (kind === "number") {
      value = Number(raw);
      if (Number.isNaN(value)) throw new Error(`Champ « ${name} » : nombre invalide`);
    } else if (kind === "pdflist") {
      value = String(raw).split("\n").map((s) => s.trim()).filter(Boolean);
    }

    setDeep(name, value);
  });

  if (body.options && typeof body.options.toc_depth === "string") {
    body.options.toc_depth = parseInt(body.options.toc_depth, 10);
  }

  return body;
}

function buildHeaders(ep, json) {
  const h = {};
  if (json) h["Content-Type"] = "application/json";
  if (ep.auth !== false && state.apiKey) h["X-API-Key"] = state.apiKey;
  return h;
}

function buildCurl(ep, url, headers, bodyText, formValues) {
  const parts = [`curl -X ${ep.method} '${url}'`];
  Object.entries(headers).forEach(([k, v]) => {
    // la clé n'est jamais recopiée en clair : le presse-papier peut finir n'importe où
    parts.push(`  -H '${k}: ${k === "X-API-Key" ? "$API_KEY" : v}'`);
  });
  if (formValues) {
    Object.entries(formValues).forEach(([k, v]) => {
      if (k.startsWith("__") || v === "" || v === false) return;
      parts.push(`  -F ${JSON.stringify(`${k}=${v}`)}`);
    });
  } else if (bodyText) {
    parts.push(`  -d ${JSON.stringify(bodyText)}`);
  }
  parts.push("  --output response.bin");
  return parts.join(" \\\n");
}

// ══════════════════════════════════════════════════════════ sending

async function send() {
  const ep = BY_KEY[state.current];
  const values = collectValues();
  const base = state.baseUrl.replace(/\/+$/, "");
  const url = base + (ep.buildPath ? ep.buildPath(values) : ep.path);

  let body = null;
  let bodyText = "";
  let headers = {};

  try {
    if (ep.json) {
      const payload = buildPayload(ep, values);
      bodyText = JSON.stringify(payload);
      body = bodyText;
      headers = buildHeaders(ep, true);
    } else if (ep.form) {
      const fd = new FormData();
      Object.entries(values).forEach(([k, v]) => {
        if (k.startsWith("__") || v === "" || v === false) return;
        fd.append(k, v);
      });
      body = fd;
      headers = buildHeaders(ep, false);
    } else {
      headers = buildHeaders(ep, false);
    }
  } catch (e) {
    showError(e.message);
    return;
  }

  state.lastCurl = buildCurl(ep, url, headers, bodyText, ep.form ? values : null);
  $("#tabCurl").innerHTML = highlightShell(state.lastCurl);

  $("#statusBadge").innerHTML = '<span class="spinner"></span>';
  $("#statusBadge").className = "badge idle";
  $("#timing").textContent = "";
  $("#size").textContent = "";
  $("#ctype").textContent = "";
  $("#sendBtn").disabled = true;

  const started = performance.now();
  let res;
  try {
    res = await fetch(url, { method: ep.method, headers, body });
  } catch (e) {
    $("#sendBtn").disabled = false;
    showError("Requête impossible : " + e.message + "\n\nLa base URL est-elle correcte et le service démarré ?");
    return;
  }
  const elapsed = Math.round(performance.now() - started);
  $("#sendBtn").disabled = false;

  const blob = await res.blob();
  const ctype = (res.headers.get("content-type") || "").split(";")[0];

  $("#statusBadge").textContent = res.status + " " + res.statusText;
  $("#statusBadge").className = "badge " + (res.ok ? "ok" : "err");
  $("#timing").textContent = elapsed + " ms";
  $("#size").textContent = formatBytes(blob.size);
  $("#ctype").textContent = ctype;

  await renderResponse(blob, ctype, res.ok);
}

function formatBytes(n) {
  if (n < 1024) return n + " o";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " Ko";
  return (n / (1024 * 1024)).toFixed(1) + " Mo";
}

async function renderResponse(blob, ctype, ok) {
  const preview = $("#tabPreview");
  preview.innerHTML = "";
  if (state.blobUrl) URL.revokeObjectURL(state.blobUrl);
  state.blobUrl = null;

  if (ctype === "application/pdf") {
    state.blobUrl = URL.createObjectURL(blob);
    const frame = document.createElement("iframe");
    frame.className = "preview";
    frame.title = "Aperçu du PDF généré";
    frame.src = state.blobUrl;
    const dl = document.createElement("a");
    dl.className = "preview-link";
    dl.href = state.blobUrl;
    dl.download = "document.pdf";
    dl.textContent = "⤓ télécharger le PDF";
    preview.append(frame, dl);
    $("#tabRaw").textContent = `(corps binaire application/pdf — ${formatBytes(blob.size)})`;
    return;
  }

  if (ctype === "image/png") {
    state.blobUrl = URL.createObjectURL(blob);
    const img = document.createElement("img");
    img.className = "preview";
    img.alt = "Aperçu de la première page";
    img.src = state.blobUrl;
    preview.appendChild(img);
    $("#tabRaw").textContent = `(corps binaire image/png — ${formatBytes(blob.size)})`;
    return;
  }

  const text = await blob.text();
  $("#tabRaw").textContent = text;

  let json = null;
  let body = text;
  try {
    json = JSON.parse(text);
    body = pretty(json);
  } catch (e) { /* texte brut : erreurs de l'endpoint legacy */ }

  const pre = document.createElement("pre");
  pre.innerHTML = json ? highlightJson(body) : escapeHtml(body);
  preview.appendChild(pre);

  if (ok && json && json.download_url) {
    rememberPdf(json.download_url);
    const link = document.createElement("a");
    link.className = "preview-link";
    link.href = state.baseUrl.replace(/\/+$/, "") + json.download_url;
    link.target = "_blank";
    link.rel = "noopener";
    link.textContent = "↗ ouvrir " + json.download_url;
    preview.appendChild(link);
  }
}

function showError(message) {
  $("#statusBadge").textContent = "erreur";
  $("#statusBadge").className = "badge err";
  $("#tabPreview").innerHTML = "";
  const pre = document.createElement("pre");
  pre.textContent = message;
  $("#tabPreview").appendChild(pre);
  $("#tabRaw").textContent = message;
}

// ══════════════════════════════════════════════════════════ saved PDFs

function rememberPdf(url) {
  if (state.saved.includes(url)) return;
  state.saved.unshift(url);
  state.saved = state.saved.slice(0, 20);
  persist();
  renderSaved();
}

function renderSaved() {
  const list = $("#savedList");
  if (!state.saved.length) {
    list.innerHTML = '<li class="empty">aucun pour le moment</li>';
    return;
  }
  list.innerHTML = "";
  state.saved.forEach((url) => {
    const li = document.createElement("li");
    const a = document.createElement("a");
    a.href = state.baseUrl.replace(/\/+$/, "") + url;
    a.target = "_blank";
    a.rel = "noopener";
    a.textContent = url.replace("/download/", "");
    a.title = url;
    const drop = document.createElement("button");
    drop.textContent = "✕";
    drop.title = "retirer de la liste";
    drop.onclick = () => {
      state.saved = state.saved.filter((u) => u !== url);
      persist();
      renderSaved();
      selectEndpoint(state.current);
    };
    li.append(a, drop);
    list.appendChild(li);
  });
}

// ══════════════════════════════════════════════════════════ health

async function ping() {
  const dot = $("#healthDot");
  const text = $("#healthText");
  text.textContent = "connexion…";
  dot.className = "dot";

  try {
    const res = await fetch(state.baseUrl.replace(/\/+$/, "") + "/api/health");
    const json = await res.json();
    dot.className = "dot " + (json.status === "ok" ? "ok" : "err");
    text.textContent = `${json.status} · v${json.version}`;
    $("#statVersion").textContent = json.version;
    $("#statEngines").textContent = json.engines.length;
    $("#statStatus").textContent = json.status;
    $("#healthPill").title = "Moteurs : " + json.engines.join(", ");
  } catch (e) {
    dot.className = "dot err";
    text.textContent = "injoignable";
    $("#statVersion").textContent = "—";
    $("#statEngines").textContent = "—";
    $("#statStatus").textContent = "hors ligne";
  }
}

// ══════════════════════════════════════════════════════════ theme

const MOON = "M12 3a9 9 0 109 9 7 7 0 01-9-9z";
const SUN = "M12 7a5 5 0 100 10 5 5 0 000-10zm0-6v3m0 16v3M4.2 4.2l2.1 2.1m11.4 11.4l2.1 2.1M1 12h3m16 0h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1";

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("mdpdf.theme", theme);
  const icon = $("#themeIcon");
  if (theme === "light") {
    icon.setAttribute("d", SUN);
    icon.setAttribute("fill", "none");
    icon.setAttribute("stroke", "currentColor");
    icon.setAttribute("stroke-width", "1.8");
    icon.setAttribute("stroke-linecap", "round");
  } else {
    icon.setAttribute("d", MOON);
    icon.setAttribute("fill", "currentColor");
    icon.removeAttribute("stroke");
  }
}

function initTheme() {
  const stored = localStorage.getItem("mdpdf.theme");
  const prefersLight = window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches;
  applyTheme(stored || (prefersLight ? "light" : "dark"));
  $("#themeToggle").onclick = () =>
    applyTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light");
}

// ══════════════════════════════════════════════════════════ misc UI

function initCopyButtons() {
  document.addEventListener("click", (e) => {
    const btn = e.target.closest(".copy-btn");
    if (!btn) return;
    const target = $(btn.dataset.copy);
    if (!target) return;
    const text = target.dataset.raw || target.textContent;
    navigator.clipboard.writeText(text).then(() => {
      const old = btn.textContent;
      btn.textContent = "copié ✓";
      setTimeout(() => (btn.textContent = old), 1400);
    });
  });
}

function initScrollSpy() {
  const sections = $$("section[id]");
  const navLinks = new Map($$(".topnav a").map((a) => [a.getAttribute("href").slice(1), a]));
  const docLinks = new Map($$(".doc-nav a").map((a) => [a.getAttribute("href").slice(1), a]));

  if (!("IntersectionObserver" in window)) return;

  const topObserver = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      const link = navLinks.get(entry.target.id);
      if (!link) return;
      navLinks.forEach((l) => l.classList.remove("active"));
      link.classList.add("active");
    });
  }, { rootMargin: "-20% 0px -70% 0px" });
  sections.forEach((s) => topObserver.observe(s));

  const docObserver = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      const link = docLinks.get(entry.target.id);
      if (!link) return;
      docLinks.forEach((l) => l.classList.remove("active"));
      link.classList.add("active");
    });
  }, { rootMargin: "-15% 0px -75% 0px" });
  $$(".endpoint").forEach((e) => docObserver.observe(e));
}

// ══════════════════════════════════════════════════════════ init

function init() {
  initTheme();
  renderFeatureCards();
  renderDoc();
  renderDeploy();

  $("#statEndpoints").textContent = ENDPOINTS.length;
  $("#endpointCount").textContent = ENDPOINTS.length;

  // quickstart tabs
  renderQuickstart("curl");
  $$(".quickstart .tabs button").forEach((btn) => {
    btn.onclick = () => {
      $$(".quickstart .tabs button").forEach((b) => {
        b.classList.remove("active");
        b.setAttribute("aria-selected", "false");
      });
      btn.classList.add("active");
      btn.setAttribute("aria-selected", "true");
      renderQuickstart(btn.dataset.lang);
    };
  });

  // console endpoints
  $("#endpointList").innerHTML = ENDPOINTS.map((ep) =>
    `<button data-key="${ep.key}"><span class="m">${ep.method}</span>${escapeHtml(ep.key)}</button>`).join("");
  $$("#endpointList button").forEach((b) => (b.onclick = () => selectEndpoint(b.dataset.key)));

  // response tabs
  $$(".console-res .tabs button").forEach((btn) => {
    btn.onclick = () => {
      $$(".console-res .tabs button").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      $("#tabPreview").hidden = btn.dataset.tab !== "preview";
      $("#tabRaw").hidden = btn.dataset.tab !== "raw";
      $("#tabCurl").hidden = btn.dataset.tab !== "curl";
    };
  });

  // settings
  $("#baseUrl").value = state.baseUrl;
  $("#apiKey").value = state.apiKey;
  $("#baseUrl").oninput = (e) => { state.baseUrl = e.target.value; persist(); renderQuickstart(currentLang()); };
  $("#apiKey").oninput = (e) => { state.apiKey = e.target.value; persist(); };

  $("#sendBtn").onclick = (e) => { e.preventDefault(); send(); };
  $("#resetBtn").onclick = (e) => { e.preventDefault(); selectEndpoint(state.current); };
  $("#curlBtn").onclick = (e) => {
    e.preventDefault();
    if (!state.lastCurl) { showError("Envoie d'abord une requête pour obtenir la commande équivalente."); return; }
    navigator.clipboard.writeText(state.lastCurl).then(() => {
      $("#curlBtn").textContent = "copié ✓";
      setTimeout(() => ($("#curlBtn").textContent = "Copier le curl"), 1400);
    });
  };
  $("#pingBtn").onclick = (e) => { e.preventDefault(); ping(); };

  $("#form").addEventListener("submit", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { e.preventDefault(); send(); }
  });

  initCopyButtons();
  initScrollSpy();

  selectEndpoint("convert");
  showEmptyResponse();
  renderSaved();
  ping();
}

function currentLang() {
  const active = $(".quickstart .tabs button.active");
  return active ? active.dataset.lang : "curl";
}

init();
