/* ==========================================================================
   md-to-pdf — internationalisation
   Two languages, English first. The markup ships in English so the page reads
   correctly before this file runs and keeps reading correctly if it never does;
   French is applied on top at boot. A stored choice always wins over the
   browser, and an unsupported browser language falls back to English rather
   than to whichever locale happens to be first.
   ========================================================================== */
"use strict";

const I18n = (() => {
  const SUPPORTED = ["en", "fr"];
  const FALLBACK = "en";
  const STORAGE = "mdpdf.lang";

  function detect() {
    const stored = localStorage.getItem(STORAGE);
    if (SUPPORTED.includes(stored)) return stored;

    const tags = navigator.languages && navigator.languages.length
      ? navigator.languages
      : [navigator.language || FALLBACK];

    for (const tag of tags) {
      const base = String(tag).toLowerCase().split("-")[0];
      if (SUPPORTED.includes(base)) return base;
    }
    return FALLBACK;
  }

  let lang = detect();
  const listeners = [];

  /* Translate a value. A plain string is already translated — that is what lets
     a spec field be migrated one entry at a time without breaking the rest. */
  function t(value) {
    if (value == null) return "";
    if (typeof value === "string") return value;
    if (typeof value === "object") return value[lang] != null ? value[lang] : (value[FALLBACK] || "");
    return String(value);
  }

  /* Look a key up in the interface catalogue. An unknown key returns the key
     itself: a missing translation must be visible, never an empty box. */
  function k(key) {
    const entry = UI[key];
    return entry === undefined ? key : t(entry);
  }

  /* Walk the static markup. Content is authored in this repository, so
     `innerHTML` is the right assignment: several strings carry <code> and links. */
  function apply(root) {
    const scope = root || document;

    scope.querySelectorAll("[data-i18n]").forEach((el) => {
      el.innerHTML = k(el.dataset.i18n);
    });
    scope.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
      el.placeholder = k(el.dataset.i18nPlaceholder);
    });
    scope.querySelectorAll("[data-i18n-title]").forEach((el) => {
      el.title = k(el.dataset.i18nTitle);
    });
    scope.querySelectorAll("[data-i18n-aria]").forEach((el) => {
      el.setAttribute("aria-label", k(el.dataset.i18nAria));
    });

    document.documentElement.lang = lang;
  }

  function set(next) {
    if (!SUPPORTED.includes(next) || next === lang) return;
    lang = next;
    localStorage.setItem(STORAGE, lang);
    apply();
    listeners.forEach((fn) => fn(lang));
  }

  function onChange(fn) {
    listeners.push(fn);
  }

  return {
    t,
    k,
    apply,
    set,
    onChange,
    supported: SUPPORTED,
    current: () => lang,
  };
})();

/* Shorthands used everywhere else: `t()` for {en, fr} pairs carried by the API
   spec and the guides, `k()` for the interface catalogue below. */
const t = I18n.t;
const k = I18n.k;

// ══════════════════════════════════════════════════════════ catalogue

const UI = {
  // ─────────────────────────── chrome ───────────────────────────
  "nav.home": { en: "Home", fr: "Accueil" },
  "nav.guides": { en: "Guides", fr: "Guides" },
  "nav.api": { en: "API reference", fr: "Référence API" },
  "nav.console": { en: "Console", fr: "Console" },
  "nav.acces": { en: "Access", fr: "Accès" },

  "chrome.skip": { en: "Skip to content", fr: "Aller au contenu" },
  "chrome.search": { en: "Search", fr: "Rechercher" },
  "chrome.search.aria": { en: "Search an endpoint or a guide", fr: "Rechercher un endpoint ou un guide" },
  "chrome.health.title": { en: "Service status — click to test again", fr: "Statut du service — cliquer pour retester" },
  "chrome.health.connecting": { en: "connecting…", fr: "connexion…" },
  "chrome.health.offline": { en: "unreachable", fr: "injoignable" },
  "chrome.health.engines": { en: "Engines: ", fr: "Moteurs : " },
  "chrome.key.aria": { en: "API key and base URL", fr: "Clé d'API et base URL" },
  "chrome.theme.aria": { en: "Toggle light / dark theme", fr: "Basculer le thème clair / sombre" },
  "chrome.lang.aria": { en: "Language", fr: "Langue" },

  // ─────────────────────────── settings popover ───────────────────────────
  "settings.title": { en: "API key", fr: "Clé d'API" },
  "settings.token": { en: "token", fr: "token" },
  "settings.token.ph": { en: "paste your token", fr: "collez votre token" },
  "settings.paste": { en: "Paste", fr: "Coller" },
  "settings.paste.title": { en: "Paste from the clipboard", fr: "Coller depuis le presse-papier" },
  "settings.verify": { en: "Verify the key", fr: "Vérifier la clé" },
  "settings.clear": { en: "Clear", fr: "Effacer" },
  "settings.baseurl": { en: "base URL", fr: "base URL" },
  "settings.ping": { en: "Test the service", fr: "Tester le service" },
  "settings.note": {
    en: 'Token and base URL stay in this browser (<code>localStorage</code>) and are never copied into the code examples. No token? <a href="#/acces">Ask for one</a>.',
    fr: 'Token et base URL restent dans ce navigateur (<code>localStorage</code>) et ne sont jamais recopiés dans les exemples de code. Pas de token ? <a href="#/acces">Demandez-en un</a>.',
  },

  // ─────────────────────────── access / token runtime ───────────────────────────
  "key.none": { en: "no token stored", fr: "aucun token enregistré" },
  "key.unverified": { en: "token stored — not verified", fr: "token enregistré — non vérifié" },
  "key.saved": { en: "Token stored in this browser", fr: "Token enregistré dans ce navigateur" },
  "key.cleared": { en: "token cleared", fr: "token effacé" },
  "key.needed": { en: "enter a token first", fr: "renseignez d'abord un token" },
  "key.checking": { en: "checking on ", fr: "vérification sur " },
  "key.refused": { en: " — token refused by ", fr: " — token refusé par " },
  "key.valid": { en: "✓ token valid on ", fr: "✓ token valide sur " },
  "key.error": { en: " — error returned by ", fr: " — erreur renvoyée par " },
  "key.unreachable": { en: "service unreachable: ", fr: "service injoignable : " },
  "key.401": { en: "401 — token missing or refused by ", fr: "401 — token absent ou refusé par " },
  "key.401.toast": { en: " — token missing or refused.", fr: " — token absent ou refusé." },
  "key.clipboard": { en: "clipboard unavailable — paste by hand", fr: "presse-papier inaccessible — collez à la main" },
  "key.mail.copied": { en: "Address copied", fr: "Adresse copiée" },

  // ─────────────────────────── home ───────────────────────────
  "home.eyebrow": { en: "AI SmartTalk internal service · Rust · pandoc · WeasyPrint", fr: "Service interne AI SmartTalk · Rust · pandoc · WeasyPrint" },
  "home.title": { en: "A document engine<br><span class=\"grad\">for our products</span>", fr: "Un moteur de documents<br><span class=\"grad\">pour nos produits</span>" },
  "home.lede": {
    en: "Markdown, HTML or Tera templates in. Paginated PDF, PNG preview, merge, watermark and AES-256 encryption out. One JSON API, one container.",
    fr: "Markdown, HTML ou templates Tera en entrée. PDF paginé, preview PNG, fusion, filigrane et chiffrement AES-256 en sortie. Une seule API JSON, un seul conteneur.",
  },
  "home.lede.small": {
    en: 'The API serves AI SmartTalk teams and products: every call needs a token. <a href="#/acces">Request access</a>.',
    fr: 'L\'API est réservée aux équipes et projets AI SmartTalk : chaque appel demande un token. <a href="#/acces">Demander un accès</a>.',
  },
  "home.cta.console": { en: "Open the console", fr: "Ouvrir la console" },
  "home.cta.guides": { en: "Read the guides", fr: "Lire les guides" },
  "home.stat.version": { en: "version", fr: "version" },
  "home.stat.endpoints": { en: "endpoints", fr: "endpoints" },
  "home.stat.engines": { en: "PDF engines", fr: "moteurs PDF" },
  "home.stat.status": { en: "status", fr: "statut" },
  "home.pipe.in": { en: "input", fr: "entrée" },
  "home.pipe.out": { en: "output", fr: "sortie" },
  "home.pipe.premium": { en: "premium content", fr: "contenu premium" },
  "home.features.title": { en: "What the service can do", fr: "Ce que le service sait faire" },
  "home.features.lede": {
    en: '<span id="endpointCount">—</span> endpoints, one contract: binary PDF by default, a download URL when you ask for the file to be saved.',
    fr: '<span id="endpointCount">—</span> endpoints, un contrat unique : PDF binaire par défaut, URL de téléchargement si vous demandez la sauvegarde.',
  },
  "home.quick.title": { en: "Thirty seconds to the first PDF", fr: "Démarrage en trente secondes" },
  "home.quick.lede": {
    en: "No SDK: HTTP and JSON, plus your token in the <code>X-API-Key</code> header.",
    fr: "Aucun SDK : de l'HTTP et du JSON, plus votre token dans l'en-tête <code>X-API-Key</code>.",
  },
  "home.quick.tabs.aria": { en: "Code examples", fr: "Exemples de code" },
  "home.quick.notoken": { en: "No token", fr: "Sans token" },
  "home.quick.n1.title": { en: "Binary response by default", fr: "Réponse binaire par défaut" },
  "home.quick.n1.body": {
    en: "Without <code>client_id</code>/<code>pdf_name</code>, the response body is the PDF itself.",
    fr: "Sans <code>client_id</code>/<code>pdf_name</code>, le corps de la réponse est le PDF lui-même.",
  },
  "home.quick.n2.title": { en: "Server-side save", fr: "Sauvegarde serveur" },
  "home.quick.n2.body": {
    en: 'With both fields, the response becomes <code>{"download_url": …}</code> and the file stays served by the service.',
    fr: 'Avec les deux champs, la réponse devient <code>{"download_url": …}</code> et le fichier reste servi par le service.',
  },
  "home.quick.n3.title": { en: "Token required", fr: "Token obligatoire" },
  "home.quick.n3.body": {
    en: 'Every <code>/api/*</code> needs <code>X-API-Key</code> (or <code>Authorization: Bearer</code>). Only <code>/api/health</code> and <code>/download</code> stay open. <a href="#/acces">Get a token</a>.',
    fr: 'Tous les <code>/api/*</code> exigent <code>X-API-Key</code> (ou <code>Authorization: Bearer</code>). Seuls <code>/api/health</code> et <code>/download</code> restent ouverts. <a href="#/acces">Obtenir un token</a>.',
  },
  "home.foot": {
    en: '<strong>md-to-pdf</strong> — fork maintained by <a href="https://aismarttalk.tech" target="_blank" rel="noopener">AI SmartTalk</a>, after the original project by <a href="https://github.com/Spawnia/md-to-pdf" target="_blank" rel="noopener">Spawnia</a>.',
    fr: '<strong>md-to-pdf</strong> — fork maintenu par <a href="https://aismarttalk.tech" target="_blank" rel="noopener">AI SmartTalk</a>, d\'après le projet original de <a href="https://github.com/Spawnia/md-to-pdf" target="_blank" rel="noopener">Spawnia</a>.',
  },
  "home.foot.acces": { en: "request access", fr: "demander un accès" },
  "home.foot.editor": { en: "markdown editor", fr: "éditeur markdown" },

  // ─────────────────────────── API reference ───────────────────────────
  "api.aria": { en: "API reference", fr: "Référence de l'API" },
  "api.list.aria": { en: "Endpoint list", fr: "Liste des endpoints" },
  "api.filter": { en: "Filter endpoints…", fr: "Filtrer les endpoints…" },
  "api.back": { en: "← Endpoints", fr: "← Endpoints" },
  "api.copypath": { en: "Copy the path", fr: "Copier le chemin" },
  "api.try": { en: "Try in the console", fr: "Tester dans la console" },
  "api.params": { en: "Parameters", fr: "Paramètres" },
  "api.param": { en: "Parameter", fr: "Paramètre" },
  "api.type": { en: "Type", fr: "Type" },
  "api.desc": { en: "Description", fr: "Description" },
  "api.noparams": { en: "No parameter.", fr: "Aucun paramètre." },
  "api.request": { en: "Request", fr: "Requête" },
  "api.response": { en: "Response", fr: "Réponse" },
  "api.responses": { en: "Responses", fr: "Réponses" },
  "api.auth.free": { en: "🔓 no token", fr: "🔓 sans token" },
  "api.auth.token": { en: "🔐 token required · X-API-Key", fr: "🔐 token requis · X-API-Key" },
  "api.nomatch": { en: "No endpoint matches.", fr: "Aucun endpoint ne correspond." },
  "api.pathcopied": { en: "Path copied", fr: "Chemin copié" },

  // ─────────────────────────── console ───────────────────────────
  "console.aria": { en: "Test console", fr: "Console de test" },
  "console.sub": { en: "real requests from the browser", fr: "requêtes réelles depuis le navigateur" },
  "console.panes.aria": { en: "Displayed panel", fr: "Panneau affiché" },
  "console.endpoints": { en: "Endpoints", fr: "Endpoints" },
  "console.saved": { en: "Generated PDFs", fr: "PDFs générés" },
  "console.saved.empty": { en: "none yet", fr: "aucun pour le moment" },
  "console.saved.drop": { en: "remove from the list", fr: "retirer de la liste" },
  "console.reset": { en: "Reset the form", fr: "Réinitialiser le formulaire" },
  "console.reset.done": { en: "Form reset", fr: "Formulaire réinitialisé" },
  "console.banner": {
    en: "<strong>No token stored.</strong> The <code>/api/*</code> endpoints will answer 401.",
    fr: "<strong>Aucun token enregistré.</strong> Les endpoints <code>/api/*</code> répondront 401.",
  },
  "console.banner.enter": { en: "Enter the token", fr: "Saisir le token" },
  "console.banner.ask": { en: "Ask for one", fr: "En demander un" },
  "console.send": { en: "Send", fr: "Envoyer" },
  "console.curl": { en: "Copy the curl", fr: "Copier le curl" },
  "console.curl.copied": { en: "curl command copied", fr: "Commande curl copiée" },
  "console.curl.none": { en: "Send a request first to get the equivalent curl", fr: "Envoie d'abord une requête pour obtenir le curl équivalent" },
  "console.doclink": { en: "See the docs ↗", fr: "Voir la doc ↗" },
  "console.idle": { en: "idle", fr: "en attente" },
  "console.tab.preview": { en: "Preview", fr: "Aperçu" },
  "console.tab.raw": { en: "Raw", fr: "Brut" },
  "console.empty": { en: "The response shows up here — PDF, PNG or JSON.", fr: "La réponse s'affichera ici — PDF, PNG ou JSON." },
  "console.empty.send": { en: "to send", fr: "pour envoyer" },
  "console.download": { en: "⤓ download the PDF", fr: "⤓ télécharger le PDF" },
  "console.pdf.aria": { en: "Preview of the generated PDF", fr: "Aperçu du PDF généré" },
  "console.png.aria": { en: "Preview of the first page", fr: "Aperçu de la première page" },
  "console.binary": { en: "(binary body ", fr: "(corps binaire " },
  "console.open": { en: "↗ open ", fr: "↗ ouvrir " },
  "console.error": { en: "error", fr: "erreur" },
  "console.error.build": { en: "The request could not be built", fr: "La requête n'a pas pu être construite" },
  "console.error.network": {
    en: "Request failed: ",
    fr: "Requête impossible : ",
  },
  "console.error.network.hint": {
    en: "\n\nIs the base URL right and the service running?",
    fr: "\n\nLa base URL est-elle correcte et le service démarré ?",
  },
  "console.field.json": { en: "invalid JSON", fr: "JSON invalide" },
  "console.field.number": { en: "invalid number", fr: "nombre invalide" },
  "console.field": { en: "Field", fr: "Champ" },
  "console.default": { en: "(default)", fr: "(défaut)" },
  "console.pdfpicker": { en: "— generated PDFs —", fr: "— PDFs générés —" },
  "console.add": { en: "add", fr: "ajouter" },
  "console.use": { en: "use", fr: "utiliser" },

  // ─────────────────────────── access view ───────────────────────────
  "acces.eyebrow": { en: "Internal service", fr: "Service interne" },
  "acces.title": { en: "Getting access", fr: "Obtenir un accès" },
  "acces.lede": {
    en: "md-to-pdf runs for AI SmartTalk products and teams. It is not open self-service: every integration gets its own token.",
    fr: "md-to-pdf tourne pour les produits et les équipes AI SmartTalk. Il n'est pas ouvert en libre-service : chaque intégration reçoit son propre token.",
  },
  "acces.s1.title": { en: "Ask for a token", fr: "Demandez un token" },
  "acces.s1.body": { en: "One mail with the project and the expected volume. Answer within one working day.", fr: "Un mail avec le projet concerné et le volume attendu. Réponse sous un jour ouvré." },
  "acces.s1.copy": { en: "Copy the address", fr: "Copier l'adresse" },
  "acces.s2.title": { en: "Store it here", fr: "Enregistrez-le ici" },
  "acces.s2.body": { en: "The token stays in this browser and is used by every console request.", fr: "Le token reste dans ce navigateur et sert à toutes les requêtes de la console." },
  "acces.s2.save": { en: "Save", fr: "Enregistrer" },
  "acces.s3.title": { en: "Test, then integrate", fr: "Testez, puis intégrez" },
  "acces.s3.body": {
    en: "The console sends real requests with your token and hands you back the equivalent <code>curl</code>.",
    fr: "La console envoie de vraies requêtes avec votre token et vous rend le <code>curl</code> équivalent.",
  },
  "acces.s3.ref": { en: "See the reference", fr: "Voir la référence" },
  "acces.note1": {
    en: "<strong>Where the token goes.</strong> <code>X-API-Key</code> header or <code>Authorization: Bearer</code> on every <code>/api/*</code>. Only <code>/api/health</code> and <code>/download/&lt;client_id&gt;/&lt;pdf_name&gt;</code> answer without authentication.",
    fr: "<strong>Où passe le token.</strong> En-tête <code>X-API-Key</code> ou <code>Authorization: Bearer</code> sur tous les <code>/api/*</code>. Seuls <code>/api/health</code> et <code>/download/&lt;client_id&gt;/&lt;pdf_name&gt;</code> répondent sans authentification.",
  },
  "acces.note2": {
    en: "<strong>One token per integration.</strong> Do not commit it and do not put it in front-end code: it grants full access to the engine. If it leaks, write to the same address and we replace it.",
    fr: "<strong>Un token par intégration.</strong> Ne le committez pas et ne le mettez pas dans du code front : il donne un accès complet au moteur. En cas de fuite, écrivez à la même adresse, on le remplace.",
  },

  // ─────────────────────────── palette ───────────────────────────
  "palette.aria": { en: "Quick search", fr: "Recherche rapide" },
  "palette.ph": { en: "Go to an endpoint, a guide or a view…", fr: "Aller à un endpoint, un guide ou une vue…" },
  "palette.results": { en: "Results", fr: "Résultats" },
  "palette.navigate": { en: "navigate", fr: "naviguer" },
  "palette.open": { en: "open", fr: "ouvrir" },
  "palette.try": { en: "try in the console", fr: "tester dans la console" },
  "palette.close": { en: "close", fr: "fermer" },
  "palette.empty": { en: "No result.", fr: "Aucun résultat." },
  "palette.hint.view": { en: "view", fr: "vue" },
  "palette.hint.guide": { en: "guide", fr: "guide" },
  "palette.hint.page": { en: "page", fr: "page" },
  "palette.hint.file": { en: "file", fr: "fichier" },
  "palette.cmd.home": { en: "Home", fr: "Accueil" },
  "palette.cmd.guides": { en: "Guides", fr: "Guides" },
  "palette.cmd.api": { en: "API reference", fr: "Référence API" },
  "palette.cmd.console": { en: "Test console", fr: "Console de test" },
  "palette.cmd.acces": { en: "Request access", fr: "Demander un accès" },
  "palette.cmd.editor": { en: "Markdown editor", fr: "Éditeur markdown" },

  // ─────────────────────────── guides shell ───────────────────────────
  "guides.aria": { en: "Guides", fr: "Guides" },
  "guides.list.aria": { en: "Guide list", fr: "Liste des guides" },
  "guides.filter": { en: "Filter guides…", fr: "Filtrer les guides…" },
  "guides.back": { en: "← Guides", fr: "← Guides" },
  "guides.nomatch": { en: "No guide matches.", fr: "Aucun guide ne correspond." },
  "guides.copy": { en: "copy", fr: "copier" },
  "guides.copied": { en: "copied ✓", fr: "copié ✓" },

  // ─────────────────────────── misc ───────────────────────────
  "copy.ok": { en: "Copied to the clipboard", fr: "Copié dans le presse-papier" },
  "copy.fail": { en: "Copy failed — the clipboard is blocked", fr: "Copie impossible — le presse-papier est bloqué" },
  "bytes.b": { en: "B", fr: "o" },
  "bytes.kb": { en: "KB", fr: "Ko" },
  "bytes.mb": { en: "MB", fr: "Mo" },

  "title.home": { en: "md-to-pdf — PDF document engine", fr: "md-to-pdf — Moteur de documents PDF" },
  "title.guides": { en: "Guides — md-to-pdf", fr: "Guides — md-to-pdf" },
  "title.api": { en: "API reference — md-to-pdf", fr: "Référence API — md-to-pdf" },
  "title.console": { en: "Console — md-to-pdf", fr: "Console — md-to-pdf" },
  "title.acces": { en: "Access — md-to-pdf", fr: "Accès — md-to-pdf" },
};
