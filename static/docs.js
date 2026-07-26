/* ==========================================================================
   md-to-pdf — accueil, référence de l'API et déploiement
   La référence affiche un endpoint à la fois : la sidebar sert d'index, le
   panneau de droite scrolle seul. Plus de page unique de plusieurs mètres.
   ========================================================================== */
"use strict";

const Docs = (() => {

  // ───────────────────────────── accueil : cartes ─────────────────────────

  function renderFeatureCards() {
    $("#featureCards").innerHTML = ENDPOINTS.map((ep) => `
      <a class="card" href="#/api/${ep.key}">
        <h3>
          <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
            <path d="${ep.icon}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
          ${escapeHtml(t(ep.title))}
        </h3>
        <p>${escapeHtml(t(ep.card))}</p>
        <span class="route"><span class="method ${ep.method.toLowerCase()}">${ep.method}</span>${escapeHtml(ep.path)}</span>
      </a>`).join("");
  }

  // ───────────────────────────── accueil : quickstart ─────────────────────

  // Le token n'est jamais interpolé ici : les exemples restent copiables et
  // partageables sans fuiter la clé de celui qui les copie.
  const QUICKSTART = {
    curl: () => t({
      en: `# The token is given to you by the team — never hard-coded
export MDTOPDF_KEY='your-token'

# Markdown → PDF, binary response
curl -X POST '${apiBase()}/api/convert' \\
  -H "X-API-Key: $MDTOPDF_KEY" \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Hello\\n\\nA **document**.", "options": {"page_numbers": true}}' \\
  --output document.pdf

# Server-side save: the response becomes a download URL
curl -X POST '${apiBase()}/api/convert' \\
  -H "X-API-Key: $MDTOPDF_KEY" \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Hello", "client_id": "demo", "pdf_name": "hello"}'
# {"download_url":"/download/demo/hello.pdf"}

# Without a token: 401
# {"error":"unauthorized","details":"missing or invalid API key"}`,
      fr: `# Le token vous est fourni par l'équipe — jamais en dur dans le code
export MDTOPDF_KEY='votre-token'

# Markdown → PDF, réponse binaire
curl -X POST '${apiBase()}/api/convert' \\
  -H "X-API-Key: $MDTOPDF_KEY" \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Bonjour\\n\\nUn **document**.", "options": {"page_numbers": true}}' \\
  --output document.pdf

# Sauvegarde côté serveur : la réponse devient une URL de téléchargement
curl -X POST '${apiBase()}/api/convert' \\
  -H "X-API-Key: $MDTOPDF_KEY" \\
  -H 'Content-Type: application/json' \\
  -d '{"markdown": "# Bonjour", "client_id": "demo", "pdf_name": "hello"}'
# {"download_url":"/download/demo/hello.pdf"}

# Sans token : 401
# {"error":"unauthorized","details":"missing or invalid API key"}`,
    }),

    js: () => t({
      en: `const res = await fetch("${apiBase()}/api/convert", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "X-API-Key": process.env.MDTOPDF_KEY,   // server side only
  },
  body: JSON.stringify({
    markdown: "# Report\\n\\nContent in **markdown**.",
    options: { paper_size: "a4", page_numbers: true },
  }),
});

if (res.status === 401) throw new Error("md-to-pdf token missing or refused");
if (!res.ok) {
  const { error, details } = await res.json();
  throw new Error(\`\${error}: \${details}\`);
}

const pdf = await res.arrayBuffer();   // binary application/pdf body`,
      fr: `const res = await fetch("${apiBase()}/api/convert", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "X-API-Key": process.env.MDTOPDF_KEY,   // côté serveur uniquement
  },
  body: JSON.stringify({
    markdown: "# Rapport\\n\\nContenu en **markdown**.",
    options: { paper_size: "a4", page_numbers: true },
  }),
});

if (res.status === 401) throw new Error("token md-to-pdf absent ou refusé");
if (!res.ok) {
  const { error, details } = await res.json();
  throw new Error(\`\${error}: \${details}\`);
}

const pdf = await res.arrayBuffer();   // corps binaire application/pdf`,
    }),

    python: () => t({
      en: `import os
import requests

res = requests.post(
    "${apiBase()}/api/convert",
    json={
        "markdown": "# Report\\n\\nContent in **markdown**.",
        "options": {"paper_size": "a4", "page_numbers": True},
        "client_id": "demo",
        "pdf_name": "report",
    },
    headers={"X-API-Key": os.environ["MDTOPDF_KEY"]},
    timeout=90,
)
res.raise_for_status()

print(res.json()["download_url"])   # /download/demo/report.pdf`,
      fr: `import os
import requests

res = requests.post(
    "${apiBase()}/api/convert",
    json={
        "markdown": "# Rapport\\n\\nContenu en **markdown**.",
        "options": {"paper_size": "a4", "page_numbers": True},
        "client_id": "demo",
        "pdf_name": "rapport",
    },
    headers={"X-API-Key": os.environ["MDTOPDF_KEY"]},
    timeout=90,
)
res.raise_for_status()

print(res.json()["download_url"])   # /download/demo/rapport.pdf`,
    }),

    health: () => t({
      en: `# The service status needs no token
curl -s ${apiBase()}/api/health
# {"status":"ok","version":"0.2.0","engines":["weasyprint","wkhtmltopdf","pdflatex"]}

# Fetching an already-saved PDF needs none either
curl -s ${apiBase()}/download/demo/hello.pdf --output hello.pdf`,
      fr: `# Le statut du service ne demande pas de token
curl -s ${apiBase()}/api/health
# {"status":"ok","version":"0.2.0","engines":["weasyprint","wkhtmltopdf","pdflatex"]}

# Un PDF déjà sauvegardé se récupère sans token non plus
curl -s ${apiBase()}/download/demo/hello.pdf --output hello.pdf`,
    }),
  };

  function currentLang() {
    const active = $("#quickTabs button.active");
    return active ? active.dataset.lang : "curl";
  }

  function renderQuickstart(lang) {
    const code = QUICKSTART[lang]();
    const shell = lang === "curl" || lang === "health";
    $("#quickCode").innerHTML = highlightCode(code, shell ? "shell" : "text");
    $("#quickCode").dataset.raw = code;
  }

  function initQuickstart() {
    renderQuickstart("curl");
    $$("#quickTabs button").forEach((btn) => {
      btn.onclick = () => {
        $$("#quickTabs button").forEach((b) => {
          b.classList.remove("active");
          b.setAttribute("aria-selected", "false");
        });
        btn.classList.add("active");
        btn.setAttribute("aria-selected", "true");
        renderQuickstart(btn.dataset.lang);
      };
    });
  }

  // ───────────────────────────── référence : sidebar ──────────────────────

  let filter = "";

  function matches(ep) {
    if (!filter) return true;
    const q = filter.toLowerCase();
    return (ep.key + " " + ep.path + " " + t(ep.title) + " " + ep.method).toLowerCase().includes(q);
  }

  function renderNav() {
    const groups = GROUPED
      .map((g) => ({ ...g, endpoints: g.endpoints.filter(matches) }))
      .filter((g) => g.endpoints.length);

    if (!groups.length) {
      $("#docNav").innerHTML = `<p class="nav-empty">${escapeHtml(k("api.nomatch"))}</p>`;
      return;
    }

    $("#docNav").innerHTML = groups.map((g) => `
      <div class="nav-group">
        <h4>${escapeHtml(t(g.title))}</h4>
        ${g.endpoints.map((ep) => `
          <button class="nav-item${ep.key === state.docKey ? " active" : ""}" data-key="${ep.key}" title="${escapeHtml(ep.method + " " + ep.path)}">
            <span class="m">${ep.method}</span>
            <span class="label">${escapeHtml(ep.key)}</span>
          </button>`).join("")}
      </div>`).join("");

    $$("#docNav .nav-item").forEach((btn) => {
      btn.onclick = () => { location.hash = "#/api/" + btn.dataset.key; };
    });
  }

  // ───────────────────────────── référence : détail ───────────────────────

  function paramTable(ep) {
    if (!ep.params.length) return `<p class="req-desc">${escapeHtml(k("api.noparams"))}</p>`;
    return `
      <div class="table-wrap">
        <table class="grid">
          <thead><tr><th>${escapeHtml(k("api.param"))}</th><th>${escapeHtml(k("api.type"))}</th><th>${escapeHtml(k("api.desc"))}</th></tr></thead>
          <tbody>
            ${ep.params.map((p) => `
              <tr>
                <td>${escapeHtml(p.name)}${p.required ? ' <span class="req">*</span>' : ""}</td>
                <td class="type">${escapeHtml(p.type)}</td>
                <td class="desc">${t(p.desc)}</td>
              </tr>`).join("")}
          </tbody>
        </table>
      </div>`;
  }

  // Un exemple peut être un objet figé ou une fabrique, quand son contenu
  // dépend de la langue affichée.
  const resolve = (v) => (typeof v === "function" ? v() : v);

  function exampleCols(ep) {
    const req = ep.example && ep.example.request ? `
      <div>
        <h3>${escapeHtml(k("api.request"))}</h3>
        <div class="code-wrap">
          <button class="copy-btn" data-copy="#docReq">copier</button>
          <pre id="docReq" data-raw="${escapeHtml(pretty(resolve(ep.example.request)))}">${highlightJson(pretty(resolve(ep.example.request)))}</pre>
        </div>
      </div>` : "";

    const resBody = ep.example && ep.example.response
      ? `<pre id="docRes" data-raw="${escapeHtml(pretty(resolve(ep.example.response)))}">${highlightJson(pretty(resolve(ep.example.response)))}</pre>`
      : `<pre id="docRes">${escapeHtml(ep.example && ep.example.responseNote ? t(ep.example.responseNote) : "")}</pre>`;

    return `
      <div class="doc-block">
        <div class="doc-cols">
          ${req}
          <div>
            <h3>${escapeHtml(k("api.response"))}</h3>
            <div class="code-wrap">${resBody}</div>
          </div>
        </div>
      </div>`;
  }

  function neighbours(key) {
    const i = ORDERED_KEYS.indexOf(key);
    return {
      prev: i > 0 ? BY_KEY[ORDERED_KEYS[i - 1]] : null,
      next: i >= 0 && i < ORDERED_KEYS.length - 1 ? BY_KEY[ORDERED_KEYS[i + 1]] : null,
    };
  }

  function renderEndpoint(key) {
    const ep = BY_KEY[key];
    if (!ep) return;
    state.docKey = key;

    const auth = ep.auth === false ? k("api.auth.free") : k("api.auth.token");
    const ctype = ep.json ? "application/json" : ep.form ? "multipart/form-data" : null;
    const { prev, next } = neighbours(key);

    $("#docBody").innerHTML = `
      <div class="doc-head">
        <button class="btn sm ghost doc-back" id="docBack">${escapeHtml(k("api.back"))}</button>
        <span class="method ${ep.method.toLowerCase()}">${ep.method}</span>
        <code class="path">${escapeHtml(ep.path)}</code>
        <span class="spacer"></span>
        <button class="btn sm" id="docCopyPath">${escapeHtml(k("api.copypath"))}</button>
        <button class="btn sm primary" id="docTry">${escapeHtml(k("api.try"))}</button>
      </div>

      <h2 class="doc-title">${escapeHtml(t(ep.title))}</h2>
      <p class="doc-desc">${escapeHtml(t(ep.desc))}</p>
      <div class="doc-meta">
        <span class="tag dashed">${auth}</span>
        ${ctype ? `<span class="tag dashed">${ctype}</span>` : ""}
      </div>

      <div class="doc-block">
        <h3>${escapeHtml(k("api.params"))}</h3>
        ${paramTable(ep)}
      </div>

      ${exampleCols(ep)}

      <div class="doc-block">
        <h3>${escapeHtml(k("api.responses"))}</h3>
        <div class="status-list">
          ${ep.statuses.map(([code, label]) => `<span class="tag"><b>${code}</b> ${escapeHtml(t(label))}</span>`).join("")}
        </div>
      </div>

      <div class="doc-prevnext">
        ${prev ? `<button class="btn sm" data-goto="${prev.key}">← ${escapeHtml(prev.key)}</button>` : "<span></span>"}
        ${next ? `<button class="btn sm" data-goto="${next.key}">${escapeHtml(next.key)} →</button>` : "<span></span>"}
      </div>`;

    $("#docPane").scrollTop = 0;

    $("#docTry").onclick = () => { location.hash = "#/console/" + ep.key; };
    $("#docCopyPath").onclick = () => copyText(ep.method + " " + apiBase() + ep.path, k("api.pathcopied"));
    $("#docBack").onclick = () => { $("#apiSplit").dataset.mobile = "list"; };
    $$("#docBody [data-goto]").forEach((b) => {
      b.onclick = () => { location.hash = "#/api/" + b.dataset.goto; };
    });

    $$("#docNav .nav-item").forEach((b) => b.classList.toggle("active", b.dataset.key === key));
  }

  function initSearch() {
    const input = $("#docSearch");
    input.oninput = () => {
      filter = input.value.trim();
      renderNav();
    };
    // ⏎ sur la recherche ouvre le premier résultat : filtrer puis cliquer est
    // le geste le plus fréquent.
    input.onkeydown = (e) => {
      if (e.key !== "Enter") return;
      const first = $("#docNav .nav-item");
      if (first) location.hash = "#/api/" + first.dataset.key;
    };
  }

  function init() {
    renderFeatureCards();
    initQuickstart();
    renderNav();
    initSearch();
  }

  // Retraduction : le détail de l'endpoint est re-rendu par le routeur, tout ce
  // qui vit hors de la vue courante est repris ici.
  function refresh() {
    renderFeatureCards();
    renderQuickstart(currentLang());
    renderNav();
  }

  return { init, refresh, renderEndpoint, renderNav, renderQuickstart, currentLang };
})();
