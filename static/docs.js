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
          ${escapeHtml(ep.title)}
        </h3>
        <p>${escapeHtml(ep.card)}</p>
        <span class="route"><span class="method ${ep.method.toLowerCase()}">${ep.method}</span>${escapeHtml(ep.path)}</span>
      </a>`).join("");
  }

  // ───────────────────────────── accueil : quickstart ─────────────────────

  // Le token n'est jamais interpolé ici : les exemples restent copiables et
  // partageables sans fuiter la clé de celui qui les copie.
  const QUICKSTART = {
    curl: () => `# Le token vous est fourni par l'équipe — jamais en dur dans le code
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

    js: () => `const res = await fetch("${apiBase()}/api/convert", {
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

    python: () => `import os
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

    health: () => `# Le statut du service ne demande pas de token
curl -s ${apiBase()}/api/health
# {"status":"ok","version":"0.2.0","engines":["weasyprint","wkhtmltopdf","pdflatex"]}

# Un PDF déjà sauvegardé se récupère sans token non plus
curl -s ${apiBase()}/download/demo/hello.pdf --output hello.pdf`,
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
    return (ep.key + " " + ep.path + " " + ep.title + " " + ep.method).toLowerCase().includes(q);
  }

  function renderNav() {
    const groups = GROUPED
      .map((g) => ({ ...g, endpoints: g.endpoints.filter(matches) }))
      .filter((g) => g.endpoints.length);

    if (!groups.length) {
      $("#docNav").innerHTML = '<p class="nav-empty">Aucun endpoint ne correspond.</p>';
      return;
    }

    $("#docNav").innerHTML = groups.map((g) => `
      <div class="nav-group">
        <h4>${escapeHtml(g.title)}</h4>
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
    if (!ep.params.length) return '<p class="req-desc">Aucun paramètre.</p>';
    return `
      <div class="table-wrap">
        <table class="grid">
          <thead><tr><th>Paramètre</th><th>Type</th><th>Description</th></tr></thead>
          <tbody>
            ${ep.params.map((p) => `
              <tr>
                <td>${escapeHtml(p.name)}${p.required ? ' <span class="req">*</span>' : ""}</td>
                <td class="type">${escapeHtml(p.type)}</td>
                <td class="desc">${p.desc}</td>
              </tr>`).join("")}
          </tbody>
        </table>
      </div>`;
  }

  function exampleCols(ep) {
    const req = ep.example && ep.example.request ? `
      <div>
        <h3>Requête</h3>
        <div class="code-wrap">
          <button class="copy-btn" data-copy="#docReq">copier</button>
          <pre id="docReq" data-raw="${escapeHtml(pretty(ep.example.request))}">${highlightJson(pretty(ep.example.request))}</pre>
        </div>
      </div>` : "";

    const resBody = ep.example && ep.example.response
      ? `<pre id="docRes" data-raw="${escapeHtml(pretty(ep.example.response))}">${highlightJson(pretty(ep.example.response))}</pre>`
      : `<pre id="docRes">${escapeHtml(ep.example && ep.example.responseNote ? ep.example.responseNote : "")}</pre>`;

    return `
      <div class="doc-block">
        <div class="doc-cols">
          ${req}
          <div>
            <h3>Réponse</h3>
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

    const auth = ep.auth === false ? "🔓 sans token" : "🔐 token requis · X-API-Key";
    const ctype = ep.json ? "application/json" : ep.form ? "multipart/form-data" : null;
    const { prev, next } = neighbours(key);

    $("#docBody").innerHTML = `
      <div class="doc-head">
        <button class="btn sm ghost doc-back" id="docBack">← Endpoints</button>
        <span class="method ${ep.method.toLowerCase()}">${ep.method}</span>
        <code class="path">${escapeHtml(ep.path)}</code>
        <span class="spacer"></span>
        <button class="btn sm" id="docCopyPath">Copier le chemin</button>
        <button class="btn sm primary" id="docTry">Tester dans la console</button>
      </div>

      <h2 class="doc-title">${escapeHtml(ep.title)}</h2>
      <p class="doc-desc">${escapeHtml(ep.desc)}</p>
      <div class="doc-meta">
        <span class="tag dashed">${auth}</span>
        ${ctype ? `<span class="tag dashed">${ctype}</span>` : ""}
      </div>

      <div class="doc-block">
        <h3>Paramètres</h3>
        ${paramTable(ep)}
      </div>

      ${exampleCols(ep)}

      <div class="doc-block">
        <h3>Réponses</h3>
        <div class="status-list">
          ${ep.statuses.map(([code, label]) => `<span class="tag"><b>${code}</b> ${escapeHtml(label)}</span>`).join("")}
        </div>
      </div>

      <div class="doc-prevnext">
        ${prev ? `<button class="btn sm" data-goto="${prev.key}">← ${escapeHtml(prev.key)}</button>` : "<span></span>"}
        ${next ? `<button class="btn sm" data-goto="${next.key}">${escapeHtml(next.key)} →</button>` : "<span></span>"}
      </div>`;

    $("#docPane").scrollTop = 0;

    $("#docTry").onclick = () => { location.hash = "#/console/" + ep.key; };
    $("#docCopyPath").onclick = () => copyText(ep.method + " " + apiBase() + ep.path, "Chemin copié");
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

  return { init, renderEndpoint, renderNav, renderQuickstart, currentLang };
})();
