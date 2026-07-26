/* ==========================================================================
   md-to-pdf — shell applicatif
   Routage par hash entre quatre vues, palette de recherche ⌘K, réglages de
   connexion et sonde de santé. Chaque vue occupe la fenêtre : le shell ne
   défile pas, ses panneaux si.
   ========================================================================== */
"use strict";

// ══════════════════════════════════════════════════════════ routage

const VIEWS = ["home", "guides", "api", "console", "acces"];

// Anciens ancrages de la page unique — les liens partagés continuent de tomber
// sur la bonne vue. #deploy n'existe plus : la configuration serveur ne concerne
// que le dépôt, pas les intégrateurs.
const LEGACY = {
  "": "#/", "#top": "#/", "#features": "#/", "#quickstart": "#/",
  "#doc": "#/api", "#playground": "#/console", "#deploy": "#/", "#/deploy": "#/",
};

// Une clé de route appartient soit aux endpoints, soit aux guides : la vue
// décide dans quel index la chercher, sinon `#/guides/themes` tomberait sur
// l'endpoint `themes`.
function validKey(view, key) {
  if (!key) return null;
  if (view === "guides") return Guides.byKey[key] ? key : null;
  return BY_KEY[key] ? key : null;
}

function parseHash() {
  const raw = location.hash;

  if (raw.startsWith("#ep-")) return { view: "api", key: raw.slice(4) };
  if (LEGACY[raw] !== undefined) return { view: LEGACY[raw].replace("#/", "") || "home", key: null };

  const parts = raw.replace(/^#\/?/, "").split("/").filter(Boolean);
  const view = VIEWS.includes(parts[0]) ? parts[0] : "home";
  return { view, key: validKey(view, parts[1]) };
}

function route() {
  const { view, key } = parseHash();

  VIEWS.forEach((v) => $("#view-" + v).classList.toggle("active", v === view));
  $$("#topnav a").forEach((a) => {
    if (a.dataset.route === view) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  });
  document.title = k("title." + view);

  if (view === "api") {
    Docs.renderEndpoint(key || state.docKey || ORDERED_KEYS[0]);
    // En dessous de 860px la sidebar et le détail se partagent l'écran :
    // arriver sur /api sans endpoint doit montrer l'index, pas une fiche.
    $("#apiSplit").dataset.mobile = key ? "detail" : "list";
  } else if (view === "guides") {
    Guides.render(key || Guides.keys()[0]);
    $("#guideSplit").dataset.mobile = key ? "detail" : "list";
  } else if (view === "console") {
    Console.select(key || state.current);
  }
}

// ══════════════════════════════════════════════════════════ langue

// Le catalogue réécrit `innerHTML` : tout ce qu'une autre fonction avait injecté
// dans un nœud traduit — les compteurs de l'accueil — est réécrit après coup.
function initLang() {
  const paint = () => {
    $$("#langSwitch button").forEach((b) => {
      const on = b.dataset.lang === I18n.current();
      b.classList.toggle("active", on);
      b.setAttribute("aria-pressed", String(on));
    });
  };

  $$("#langSwitch button").forEach((b) => {
    b.onclick = () => I18n.set(b.dataset.lang);
  });

  I18n.onChange(() => {
    paint();
    $("#statEndpoints").textContent = ENDPOINTS.length;
    $("#endpointCount").textContent = ENDPOINTS.length;
    Docs.refresh();
    Console.refresh();
    route();
    ping();
  });

  I18n.apply();
  paint();
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
    if (!has) setStatus("", k("key.none"));
    else if (!$("#keyState").classList.contains("ok")) setStatus("", k("key.unverified"));
  }

  // Un token collé arrive souvent avec une espace ou un retour à la ligne. Il
  // est nettoyé ici plutôt qu'à chaque usage : la vérification nettoyait sa
  // copie, les requêtes de la console non — la clé passait le test puis
  // échouait en 401 sur les vrais appels.
  function setKey(value, quiet) {
    state.apiKey = String(value).trim();
    persist();
    refresh();
    if (!quiet && state.apiKey) toast(k("key.saved"));
  }

  // Aucun endpoint « ping authentifié » n'existe : on génère le plus petit PDF
  // possible et on lit le statut. 401 = clé refusée, 200 = clé acceptée.
  async function verify() {
    const key = state.apiKey;
    if (!key) { setStatus("err", k("key.needed")); return; }

    setStatus("", k("key.checking") + apiBase() + "…");
    $("#verifyKeyBtn").disabled = true;
    try {
      const res = await fetch(apiBase() + "/api/convert", {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-API-Key": key },
        body: JSON.stringify({ markdown: "# ping" }),
      });
      if (res.status === 401) setStatus("err", "401" + k("key.refused") + apiBase());
      else if (res.ok) setStatus("ok", k("key.valid") + apiBase());
      else setStatus("err", res.status + k("key.error") + apiBase());
    } catch (e) {
      setStatus("err", k("key.unreachable") + e.message);
    } finally {
      $("#verifyKeyBtn").disabled = false;
    }
  }

  // Appelé par la console quand une requête revient en 401.
  function flag401() {
    setStatus("err", k("key.401") + apiBase());
    $("#keyAlert").hidden = false;
    toast("401 · " + apiBase() + k("key.401.toast"), "err");
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
        setStatus("err", k("key.clipboard"));
      }
    };
    $("#verifyKeyBtn").onclick = (e) => { e.preventDefault(); verify(); };
    $("#clearKeyBtn").onclick = () => { setKey("", true); setStatus("", k("key.cleared")); };

    $("#bannerKeyBtn").onclick = (e) => { e.stopPropagation(); popover.open(); };
    $("#copyMailBtn").onclick = () => copyText(MAIL, k("key.mail.copied"));

    refresh();
    return popover;
  }

  return { init, refresh, verify, flag401 };
})();

// ══════════════════════════════════════════════════════════ santé

async function ping() {
  const dot = $("#healthDot");
  const text = $("#healthText");
  text.textContent = k("chrome.health.connecting");
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
    $("#healthPill").title = k("chrome.health.engines") + json.engines.join(", ");
    $("#settingsStatus").textContent = json.status + " · " + json.engines.join(", ");
  } catch (e) {
    dot.className = "dot err";
    text.textContent = k("chrome.health.offline");
    $("#brandVersion").textContent = "—";
    $("#statVersion").textContent = "—";
    $("#statEngines").textContent = "—";
    $("#statStatus").textContent = k("chrome.health.offline");
    $("#settingsStatus").textContent = k("chrome.health.offline");
  }
}

// ══════════════════════════════════════════════════════════ palette ⌘K

const Palette = (() => {
  const commands = () => [
    { name: k("palette.cmd.home"), hint: k("palette.hint.view"), hash: "#/" },
    { name: k("palette.cmd.guides"), hint: k("palette.hint.view"), hash: "#/guides" },
    { name: k("palette.cmd.api"), hint: k("palette.hint.view"), hash: "#/api" },
    { name: k("palette.cmd.console"), hint: k("palette.hint.view"), hash: "#/console" },
    { name: k("palette.cmd.acces"), hint: k("palette.hint.view"), hash: "#/acces" },
    { name: k("palette.cmd.editor"), hint: k("palette.hint.page"), href: "/static/editor.html" },
    { name: "swagger.yaml", hint: k("palette.hint.file"), href: "/static/swagger.yaml" },
  ];

  let items = [];
  let index = 0;

  const overlay = () => $("#paletteOverlay");

  function build(query) {
    const q = query.trim().toLowerCase();

    const eps = ENDPOINTS
      .filter((ep) => !q || (ep.key + " " + ep.path + " " + t(ep.title)).toLowerCase().includes(q))
      .map((ep) => ({
        method: ep.method,
        name: t(ep.title),
        hint: ep.path,
        hash: "#/api/" + ep.key,
        consoleHash: "#/console/" + ep.key,
      }));

    // Les guides passent avant les endpoints : c'est la réponse à « comment
    // fait-on X », et la palette est le premier endroit où on la cherche.
    const guides = Guides.all()
      .filter((g) => !q || (g.key + " " + t(g.title) + " " + t(g.lede)).toLowerCase().includes(q))
      .map((g) => ({ name: t(g.title), hint: k("palette.hint.guide"), hash: "#/guides/" + g.key }));

    const cmds = commands().filter((c) => !q || c.name.toLowerCase().includes(q));
    items = guides.concat(eps, cmds);
    index = 0;
    render();
  }

  function render() {
    const list = $("#paletteList");
    if (!items.length) {
      list.innerHTML = `<p class="nav-empty">${escapeHtml(k("palette.empty"))}</p>`;
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
  initLang();
  initTheme();
  initCopyButtons();
  Guides.init();
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
