/* ==========================================================================
   md-to-pdf — shell applicatif
   Routage par hash entre quatre vues, palette de recherche ⌘K, réglages de
   connexion et sonde de santé. Chaque vue occupe la fenêtre : le shell ne
   défile pas, ses panneaux si.
   ========================================================================== */
"use strict";

// ══════════════════════════════════════════════════════════ routage

const VIEWS = ["home", "api", "console", "acces"];

// Anciens ancrages de la page unique — les liens partagés continuent de tomber
// sur la bonne vue. #deploy n'existe plus : la configuration serveur ne concerne
// que le dépôt, pas les intégrateurs.
const LEGACY = {
  "": "#/", "#top": "#/", "#features": "#/", "#quickstart": "#/",
  "#doc": "#/api", "#playground": "#/console", "#deploy": "#/", "#/deploy": "#/",
};

const TITLES = {
  home: "md-to-pdf — Moteur de documents PDF",
  api: "Référence API — md-to-pdf",
  console: "Console — md-to-pdf",
  acces: "Accès — md-to-pdf",
};

function parseHash() {
  const raw = location.hash;

  if (raw.startsWith("#ep-")) return { view: "api", key: raw.slice(4) };
  if (LEGACY[raw] !== undefined) return { view: LEGACY[raw].replace("#/", "") || "home", key: null };

  const parts = raw.replace(/^#\/?/, "").split("/").filter(Boolean);
  const view = VIEWS.includes(parts[0]) ? parts[0] : "home";
  return { view, key: parts[1] && BY_KEY[parts[1]] ? parts[1] : null };
}

function route() {
  const { view, key } = parseHash();

  VIEWS.forEach((v) => $("#view-" + v).classList.toggle("active", v === view));
  $$("#topnav a").forEach((a) => {
    if (a.dataset.route === view) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  });
  document.title = TITLES[view];

  if (view === "api") {
    Docs.renderEndpoint(key || state.docKey || ORDERED_KEYS[0]);
    // En dessous de 860px la sidebar et le détail se partagent l'écran :
    // arriver sur /api sans endpoint doit montrer l'index, pas une fiche.
    $("#apiSplit").dataset.mobile = key ? "detail" : "list";
  } else if (view === "console") {
    Console.select(key || state.current);
  }
}

// ══════════════════════════════════════════════════════════ accès / token

// Le service est réservé aux intégrations AI SmartTalk : sans token, tout
// /api/* répond 401. L'état de la clé est donc visible en permanence — pastille
// dans la barre, bandeau dans la console — et saisissable depuis deux endroits.
const Access = (() => {
  const MAIL = "contact+mdtopdf@aismarttalk.tech";

  function setStatus(kind, message) {
    ["#keyState", "#acquireState"].forEach((sel) => {
      const el = $(sel);
      el.className = "key-state" + (kind ? " " + kind : "");
      el.textContent = message;
    });
  }

  function refresh() {
    const has = !!state.apiKey.trim();
    $("#keyAlert").hidden = has;
    $("#keyBanner").hidden = has;
    $("#apiKey").value = state.apiKey;
    $("#acquireKey").value = state.apiKey;
    if (!has) setStatus("", "aucun token enregistré");
    else if (!$("#keyState").classList.contains("ok")) setStatus("", "token enregistré — non vérifié");
  }

  // Un token collé arrive souvent avec une espace ou un retour à la ligne. Il
  // est nettoyé ici plutôt qu'à chaque usage : la vérification nettoyait sa
  // copie, les requêtes de la console non — la clé passait le test puis
  // échouait en 401 sur les vrais appels.
  function setKey(value, quiet) {
    state.apiKey = String(value).trim();
    persist();
    refresh();
    if (!quiet && state.apiKey) toast("Token enregistré dans ce navigateur");
  }

  // Aucun endpoint « ping authentifié » n'existe : on génère le plus petit PDF
  // possible et on lit le statut. 401 = clé refusée, 200 = clé acceptée.
  async function verify() {
    const key = state.apiKey;
    if (!key) { setStatus("err", "renseignez d'abord un token"); return; }

    setStatus("", "vérification sur " + apiBase() + "…");
    $("#verifyKeyBtn").disabled = true;
    try {
      const res = await fetch(apiBase() + "/api/convert", {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-API-Key": key },
        body: JSON.stringify({ markdown: "# ping" }),
      });
      if (res.status === 401) setStatus("err", "401 — token refusé par " + apiBase());
      else if (res.ok) setStatus("ok", "✓ token valide sur " + apiBase());
      else setStatus("err", res.status + " — erreur renvoyée par " + apiBase());
    } catch (e) {
      setStatus("err", "service injoignable : " + e.message);
    } finally {
      $("#verifyKeyBtn").disabled = false;
    }
  }

  // Appelé par la console quand une requête revient en 401.
  function flag401() {
    setStatus("err", "401 — token absent ou refusé par " + apiBase());
    $("#keyAlert").hidden = false;
    toast("401 sur " + apiBase() + " — token absent ou refusé.", "err");
  }

  function initPopover() {
    const panel = $("#settingsPanel");
    const toggle = $("#settingsToggle");

    const close = () => { panel.hidden = true; toggle.setAttribute("aria-expanded", "false"); };
    const open = () => {
      panel.hidden = false;
      toggle.setAttribute("aria-expanded", "true");
      $("#apiKey").focus();
    };

    toggle.onclick = (e) => { e.stopPropagation(); panel.hidden ? open() : close(); };
    panel.onclick = (e) => e.stopPropagation();
    document.addEventListener("click", () => { if (!panel.hidden) close(); });

    return { open, close };
  }

  function init() {
    const popover = initPopover();

    $("#baseUrl").value = state.baseUrl;
    $("#baseUrl").oninput = (e) => {
      state.baseUrl = e.target.value;
      persist();
      Docs.renderQuickstart(Docs.currentLang());
      Console.renderSaved();
    };
    $("#pingBtn").onclick = (e) => { e.preventDefault(); ping(); };

    $("#apiKey").oninput = (e) => setKey(e.target.value, true);
    $("#acquireKey").onkeydown = (e) => { if (e.key === "Enter") $("#acquireSave").click(); };
    $("#acquireSave").onclick = () => {
      setKey($("#acquireKey").value.trim());
      if (state.apiKey) verify();
    };

    $("#pasteKeyBtn").onclick = async () => {
      try {
        setKey((await navigator.clipboard.readText()).trim());
      } catch (e) {
        setStatus("err", "presse-papier inaccessible — collez à la main");
      }
    };
    $("#verifyKeyBtn").onclick = (e) => { e.preventDefault(); verify(); };
    $("#clearKeyBtn").onclick = () => { setKey("", true); setStatus("", "token effacé"); };

    $("#bannerKeyBtn").onclick = (e) => { e.stopPropagation(); popover.open(); };
    $("#copyMailBtn").onclick = () => copyText(MAIL, "Adresse copiée");

    refresh();
    return popover;
  }

  return { init, refresh, verify, flag401 };
})();

// ══════════════════════════════════════════════════════════ santé

async function ping() {
  const dot = $("#healthDot");
  const text = $("#healthText");
  text.textContent = "connexion…";
  dot.className = "dot";
  $("#settingsStatus").textContent = "";

  try {
    const res = await fetch(apiBase() + "/api/health");
    const json = await res.json();
    dot.className = "dot " + (json.status === "ok" ? "ok" : "err");
    text.textContent = `${json.status} · v${json.version}`;
    $("#brandVersion").textContent = "v" + json.version;
    $("#statVersion").textContent = json.version;
    $("#statEngines").textContent = json.engines.length;
    $("#statStatus").textContent = json.status;
    $("#healthPill").title = "Moteurs : " + json.engines.join(", ");
    $("#settingsStatus").textContent = json.status + " · " + json.engines.join(", ");
  } catch (e) {
    dot.className = "dot err";
    text.textContent = "injoignable";
    $("#brandVersion").textContent = "—";
    $("#statVersion").textContent = "—";
    $("#statEngines").textContent = "—";
    $("#statStatus").textContent = "hors ligne";
    $("#settingsStatus").textContent = "injoignable";
  }
}

// ══════════════════════════════════════════════════════════ palette ⌘K

const Palette = (() => {
  const COMMANDS = [
    { name: "Accueil", hint: "vue", hash: "#/" },
    { name: "Référence API", hint: "vue", hash: "#/api" },
    { name: "Console de test", hint: "vue", hash: "#/console" },
    { name: "Demander un accès", hint: "vue", hash: "#/acces" },
    { name: "Éditeur markdown", hint: "page", href: "/static/editor.html" },
    { name: "swagger.yaml", hint: "fichier", href: "/static/swagger.yaml" },
  ];

  let items = [];
  let index = 0;

  const overlay = () => $("#paletteOverlay");

  function build(query) {
    const q = query.trim().toLowerCase();
    const eps = ENDPOINTS
      .filter((ep) => !q || (ep.key + " " + ep.path + " " + ep.title).toLowerCase().includes(q))
      .map((ep) => ({
        method: ep.method,
        name: ep.title,
        hint: ep.path,
        hash: "#/api/" + ep.key,
        consoleHash: "#/console/" + ep.key,
      }));

    const cmds = COMMANDS.filter((c) => !q || c.name.toLowerCase().includes(q));
    items = eps.concat(cmds);
    index = 0;
    render();
  }

  function render() {
    const list = $("#paletteList");
    if (!items.length) {
      list.innerHTML = '<p class="nav-empty">Aucun résultat.</p>';
      return;
    }
    list.innerHTML = items.map((it, i) => `
      <button class="palette-item" role="option" aria-selected="${i === index}" data-i="${i}">
        <span class="m">${it.method || ""}</span>
        <span class="name">${escapeHtml(it.name)}</span>
        <span class="desc">${escapeHtml(it.hint || "")}</span>
      </button>`).join("");

    $$("#paletteList .palette-item").forEach((btn) => {
      btn.onmousemove = () => {
        index = Number(btn.dataset.i);
        $$("#paletteList .palette-item").forEach((b, i) => b.setAttribute("aria-selected", String(i === index)));
      };
      btn.onclick = (e) => run(items[Number(btn.dataset.i)], e.shiftKey);
    });

    const active = $('#paletteList [aria-selected="true"]');
    if (active) active.scrollIntoView({ block: "nearest" });
  }

  function run(item, inConsole) {
    if (!item) return;
    close();
    if (item.href) { window.open(item.href, "_blank", "noopener"); return; }
    location.hash = inConsole && item.consoleHash ? item.consoleHash : item.hash;
  }

  function open() {
    overlay().hidden = false;
    const input = $("#paletteInput");
    input.value = "";
    build("");
    input.focus();
  }

  function close() { overlay().hidden = true; }
  const isOpen = () => !overlay().hidden;

  function init() {
    $("#cmdkBtn").onclick = open;
    overlay().onclick = (e) => { if (e.target === overlay()) close(); };
    $("#paletteInput").oninput = (e) => build(e.target.value);
    $("#paletteInput").onkeydown = (e) => {
      if (e.key === "ArrowDown") { e.preventDefault(); index = Math.min(items.length - 1, index + 1); render(); }
      else if (e.key === "ArrowUp") { e.preventDefault(); index = Math.max(0, index - 1); render(); }
      else if (e.key === "Enter") { e.preventDefault(); run(items[index], e.shiftKey); }
    };
  }

  return { init, open, close, isOpen };
})();

// ══════════════════════════════════════════════════════════ init

function init() {
  initTheme();
  initCopyButtons();
  Docs.init();
  Console.init();

  const settings = Access.init();
  Palette.init();

  $("#statEndpoints").textContent = ENDPOINTS.length;
  $("#endpointCount").textContent = ENDPOINTS.length;
  $("#healthPill").onclick = () => ping();

  // Le lien d'évitement ne doit pas écrire dans le hash : celui-ci est la route.
  $(".skip-link").onclick = (e) => { e.preventDefault(); $("#app").focus(); };

  window.addEventListener("hashchange", route);
  route();

  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      Palette.isOpen() ? Palette.close() : Palette.open();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      if (parseHash().view !== "console") location.hash = "#/console";
      Console.send();
      return;
    }
    if (e.key === "Escape") {
      Palette.close();
      settings.close();
    }
  });

  ping();
}

init();
