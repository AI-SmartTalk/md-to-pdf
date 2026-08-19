/* ==========================================================================
   AI SmartTalk Documents — client des pages d'outil
   Aucune dépendance externe : le service doit rester utilisable hors ligne.

   Une seule implémentation sert toutes les pages. Ce qu'un outil fait est
   entièrement déclaré dans le HTML, par des attributs `data-*` :

     <section data-tool
              data-endpoint="/api/compress"
              data-accept="application/pdf,.pdf"
              data-file-param="pdf">
       <label class="dropzone" data-role="drop">
         <input type="file" data-role="input">
       </label>
       <select data-param="level" data-type="string"> … </select>
       <button type="button" data-role="run">Compresser</button>
     </section>

   Le reste — état, erreurs, verdict, téléchargement, enchaînement — est
   construit ici. Voir README.md du répertoire pour le contrat complet.
   ========================================================================== */
"use strict";

(function () {
  // ═══════════════════════════════════════════════════════ constantes

  // La console d'intégrateur range sa clé sous ce nom : la vitrine lit la même,
  // pour qu'un intégrateur connecté n'ait pas à la ressaisir ici.
  const KEY_STORAGE = "mdpdf.apiKey";
  const THEME_STORAGE = "mdpdf.theme";

  // Vide en production : le site est servi par le service lui-même. Une balise
  // <meta name="mdpdf-api-base" content="https://…"> permet de viser ailleurs.
  const META_BASE = document.querySelector('meta[name="mdpdf-api-base"]');
  const API_BASE = ((META_BASE && META_BASE.content) || "").replace(/\/+$/, "");

  const DEFAULT_MAX_MB = 100; // ASSET_MAX_MB côté service
  const DEFAULT_RETENTION_HOURS = 2; // ASSET_TTL_SECS = 7200

  const POLL_FIRST_MS = 700;
  const POLL_MAX_MS = 3000;
  const POLL_TIMEOUT_MS = 10 * 60 * 1000;

  // ═══════════════════════════════════════════════════════ langue

  /* Les pages sont écrites en français et en anglais ; seules les chaînes
     produites par ce script vivent ici. `<html lang>` fait foi. */
  const STRINGS = {
    fr: {
      "drop.replace": "Remplacer le fichier",
      "files.remove": "Retirer",
      "files.ready": "déjà en ligne",
      "files.tooMany": "Cet outil accepte au plus {max} fichier(s).",
      "files.tooFew": "Cet outil demande au moins {min} fichier(s).",
      "files.none": "Déposez d'abord un fichier.",
      "files.tooLarge": "« {name} » pèse {size} : la limite est de {max}.",
      "files.rejected": "« {name} » n'est pas d'un type accepté par cet outil.",
      "status.uploading": "Envoi du fichier… {percent} %",
      "status.working": "Traitement en cours…",
      "status.queued": "En file d'attente…",
      "status.running": "Traitement en cours…",
      "status.done": "Terminé.",
      "status.deleting": "Suppression…",
      "status.deleted": "Fichiers supprimés de nos serveurs.",
      "error.title": "L'opération n'a pas abouti",
      "error.network": "Le service est injoignable.",
      "error.timeout": "Le traitement dépasse le temps d'attente prévu. Le travail continue côté serveur : rechargez la page plus tard avec le lien de suivi.",
      "error.badJson": "Réponse inattendue du service.",
      "result.title": "Résultat",
      "result.download": "Télécharger",
      "result.downloadAll": "Tout télécharger",
      "result.pages": "{count} page(s)",
      "result.open": "Ouvrir le fichier produit",
      "result.delete": "Supprimer maintenant",
      "result.retention": "Vos fichiers et ce résultat sont effacés de nos serveurs au bout de {hours} h. Vous pouvez aussi les supprimer tout de suite.",
      "chain.title": "Enchaîner sans redéposer le fichier",
      "verdict.title": "Verdict",
      "verdict.unit": "sur 100",
      "verdict.ok": "Conforme",
      "verdict.warn": "À vérifier",
      "verdict.fail": "Problème détecté",
      "verdict.page": "page {page}",
      "verdict.foot": "Verdict rendu par le moteur AI SmartTalk, sur le fichier produit — pas sur une estimation.",
      "warnings.title": "Remarques",
      "preload.loading": "Reprise du fichier précédent…",
      "run.working": "Traitement…",
      "theme.toLight": "Passer au thème clair",
      "theme.toDark": "Passer au thème sombre",
      "bytes.b": "o", "bytes.kb": "ko", "bytes.mb": "Mo",
    },
    en: {
      "drop.replace": "Replace the file",
      "files.remove": "Remove",
      "files.ready": "already uploaded",
      "files.tooMany": "This tool accepts at most {max} file(s).",
      "files.tooFew": "This tool needs at least {min} file(s).",
      "files.none": "Add a file first.",
      "files.tooLarge": "“{name}” weighs {size}: the limit is {max}.",
      "files.rejected": "“{name}” is not a type this tool accepts.",
      "status.uploading": "Uploading… {percent}%",
      "status.working": "Working…",
      "status.queued": "Queued…",
      "status.running": "Working…",
      "status.done": "Done.",
      "status.deleting": "Deleting…",
      "status.deleted": "Files deleted from our servers.",
      "error.title": "The operation did not complete",
      "error.network": "The service cannot be reached.",
      "error.timeout": "This is taking longer than we wait for. The job continues on the server: come back later with the tracking link.",
      "error.badJson": "Unexpected response from the service.",
      "result.title": "Result",
      "result.download": "Download",
      "result.downloadAll": "Download all",
      "result.pages": "{count} page(s)",
      "result.open": "Open the produced file",
      "result.delete": "Delete now",
      "result.retention": "Your files and this result are erased from our servers after {hours} h. You can also delete them right now.",
      "chain.title": "Chain another tool without uploading again",
      "verdict.title": "Verdict",
      "verdict.unit": "out of 100",
      "verdict.ok": "Clean",
      "verdict.warn": "Worth a look",
      "verdict.fail": "Problem found",
      "verdict.page": "page {page}",
      "verdict.foot": "Verdict produced by the AI SmartTalk engine, on the delivered file — not on an estimate.",
      "warnings.title": "Notes",
      "preload.loading": "Picking up the previous file…",
      "run.working": "Working…",
      "theme.toLight": "Switch to the light theme",
      "theme.toDark": "Switch to the dark theme",
      "bytes.b": "B", "bytes.kb": "kB", "bytes.mb": "MB",
    },
  };

  const LANG = (document.documentElement.lang || "fr").toLowerCase().startsWith("en") ? "en" : "fr";

  function t(key, vars) {
    const raw = (STRINGS[LANG] && STRINGS[LANG][key]) || STRINGS.fr[key] || key;
    if (!vars) return raw;
    return raw.replace(/\{(\w+)\}/g, (m, name) => (vars[name] !== undefined ? String(vars[name]) : m));
  }

  /* ─── contrôles du verdict ─────────────────────────────────────────────────
     L'API rend sa prose en anglais, comme ses erreurs et ses journaux. Ce qui
     se traduit, c'est le `name` du contrôle : un identifiant stable, en
     kebab-case, que deux outils partagent dès qu'ils vérifient la même chose.
     Le tableau ci-dessous est l'inventaire complet de ces noms, et il fait foi.

       page-count            partout — le nombre de pages a-t-il bougé
       text-preserved        /api/compress, /api/pdf-to-office — couche texte conservée
       text-layer            /api/office-to-pdf — le PDF produit porte-t-il du texte
       size                  /api/compress — poids avant/après
       no-gain               /api/compress — le fichier n'a pas rétréci
       ocr-coverage          /api/ocr — pages effectivement océrisées
       unreadable-page       /api/ocr — une page dont rien n'a pu être lu
       tool-warning          /api/ocr, /api/unlock — avertissement toléré du binaire
       recovery-method       /api/repair — quelle passe a produit le fichier
       encryption            /api/unlock — le chiffrement a bien été retiré
       reconstruction-checked /api/pdf-to-office — la sortie a-t-elle pu être relue
       pages-cropped         /api/crop — pages effectivement rognées
       content-detected      /api/crop — pages sans contenu mesurable
       overlay-alignment     /api/pages/number — une page de calque par page source
       page-geometry         /api/pages/number — toutes les pages au même format
       numbered-pages        /api/pages/number — étendue de la numérotation
       pdfa-marker           /api/pdfa — la variante déclarée dans le XMP
       output-intent         /api/pdfa — profil ICC présent
       pdfa-conformance      /api/pdfa — ajustements signalés par le moteur

     Un nom absent de ce tableau s'affiche tel quel : le `detail` de l'API et le
     `summary` du verdict restent lisibles, et rien ne disparaît de l'écran. */
  const CHECK_LABELS = {
    "page-count": { fr: "Nombre de pages", en: "Page count" },
    "text-preserved": { fr: "Texte conservé", en: "Text preserved" },
    "text-layer": { fr: "Couche texte", en: "Text layer" },
    "size": { fr: "Poids du fichier", en: "File size" },
    "no-gain": { fr: "Aucun gain", en: "No gain" },
    "ocr-coverage": { fr: "Couverture de l'OCR", en: "OCR coverage" },
    "unreadable-page": { fr: "Page illisible", en: "Unreadable page" },
    "tool-warning": { fr: "Avertissement du moteur", en: "Engine warning" },
    "recovery-method": { fr: "Méthode de récupération", en: "Recovery method" },
    "encryption": { fr: "Chiffrement", en: "Encryption" },
    "reconstruction-checked": { fr: "Fidélité vérifiée", en: "Fidelity checked" },
    "pages-cropped": { fr: "Pages rognées", en: "Pages cropped" },
    "content-detected": { fr: "Contenu détecté", en: "Content detected" },
    "overlay-alignment": { fr: "Alignement du calque", en: "Overlay alignment" },
    "page-geometry": { fr: "Format des pages", en: "Page geometry" },
    "numbered-pages": { fr: "Pages numérotées", en: "Numbered pages" },
    "pdfa-marker": { fr: "Marquage PDF/A", en: "PDF/A marker" },
    "output-intent": { fr: "Profil de sortie", en: "Output intent" },
    "pdfa-conformance": { fr: "Conformité PDF/A", en: "PDF/A conformance" },
  };

  /* Un nom inconnu — outil plus récent que cette page — se lit encore : le
     kebab-case redevient une phrase plutôt que de disparaître. */
  function checkLabel(name) {
    const known = CHECK_LABELS[name];
    if (known) return known[LANG] || known.en;
    if (!name) return "";
    return name.replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase());
  }

  // ═══════════════════════════════════════════════════════ utilitaires

  const $ = (sel, root) => (root || document).querySelector(sel);
  const $$ = (sel, root) => Array.from((root || document).querySelectorAll(sel));

  /* Fabrique un nœud. `text` passe par textContent : tout ce qui vient de
     l'API — nom de fichier, détail d'un contrôle, message d'erreur — est
     affiché tel quel, jamais interprété comme du HTML. */
  function el(tag, options, children) {
    const node = document.createElement(tag);
    const opts = options || {};
    if (opts.class) node.className = opts.class;
    if (opts.text != null) node.textContent = opts.text;
    Object.keys(opts.attrs || {}).forEach((name) => {
      const value = opts.attrs[name];
      if (value != null && value !== false) node.setAttribute(name, value === true ? "" : String(value));
    });
    (children || []).forEach((child) => child && node.appendChild(child));
    return node;
  }

  function formatBytes(n) {
    if (n == null) return "";
    if (n < 1024) return n + " " + t("bytes.b");
    if (n < 1024 * 1024) return (n / 1024).toFixed(1).replace(".0", "") + " " + t("bytes.kb");
    return (n / (1024 * 1024)).toFixed(1) + " " + t("bytes.mb");
  }

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  // ═══════════════════════════════════════════════════════ thème

  function applyTheme(theme) {
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem(THEME_STORAGE, theme); } catch (e) { /* navigation privée */ }
    $$('[data-role="theme-toggle"]').forEach((btn) => {
      btn.setAttribute("aria-pressed", String(theme === "light"));
      btn.setAttribute("aria-label", theme === "light" ? t("theme.toDark") : t("theme.toLight"));
      btn.setAttribute("title", btn.getAttribute("aria-label"));
    });
  }

  function initTheme() {
    let stored = null;
    try { stored = localStorage.getItem(THEME_STORAGE); } catch (e) { /* idem */ }
    const prefersLight = window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches;
    applyTheme(stored || (prefersLight ? "light" : "dark"));

    $$('[data-role="theme-toggle"]').forEach((btn) => {
      btn.addEventListener("click", () => {
        applyTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light");
      });
    });
  }

  // ═══════════════════════════════════════════════════════ clé d'API

  function apiKey() {
    try { return (localStorage.getItem(KEY_STORAGE) || "").trim(); } catch (e) { return ""; }
  }

  function setApiKey(value) {
    try { localStorage.setItem(KEY_STORAGE, value.trim()); } catch (e) { /* rien à faire */ }
  }

  /* La clé est facultative : un déploiement sans `API_KEY` laisse tout passer,
     et les outils publics n'en demandent pas. Le champ existe pour les
     intégrateurs qui testent depuis la vitrine. */
  function initApiKeyFields() {
    $$('[data-role="api-key"]').forEach((input) => {
      input.value = apiKey();
      input.addEventListener("input", () => setApiKey(input.value));
    });
  }

  function authHeaders() {
    const key = apiKey();
    return key ? { "X-API-Key": key } : {};
  }

  // ═══════════════════════════════════════════════════════ appels HTTP

  /* L'API rend ses erreurs en `{error, details}`. On les transporte telles
     quelles jusqu'à l'affichage : reformuler l'erreur d'un service, c'est
     empêcher l'utilisateur d'en parler à quelqu'un qui saurait la lire. */
  function ApiError(error, details, status) {
    this.name = "ApiError";
    this.error = error || t("error.title");
    this.details = details || "";
    this.status = status || 0;
    this.message = this.error;
  }
  ApiError.prototype = Object.create(Error.prototype);

  function parseJson(text) {
    try { return JSON.parse(text); } catch (e) { return null; }
  }

  function errorFrom(status, text) {
    const body = parseJson(text);
    if (body && (body.error || body.details)) return new ApiError(body.error, body.details, status);
    return new ApiError(t("error.badJson"), (text || "").slice(0, 400), status);
  }

  async function readError(response) {
    const text = await response.text().catch(() => "");
    return errorFrom(response.status, text);
  }

  async function apiJson(path, options) {
    const opts = options || {};
    let response;
    try {
      response = await fetch(API_BASE + path, {
        method: opts.method || "GET",
        headers: Object.assign({ Accept: "application/json" }, authHeaders(), opts.headers || {}),
        body: opts.body,
      });
    } catch (e) {
      throw new ApiError(t("error.network"), String(e && e.message ? e.message : e), 0);
    }
    if (!response.ok && response.status !== 202) throw await readError(response);
    return response;
  }

  /* Envoi multipart. XHR plutôt que fetch pour une seule raison : la
     progression de l'upload, que fetch ne rapporte pas. Le champ s'appelle
     « file » et se répète — c'est ce qu'attend POST /api/files. */
  function uploadFiles(files, onProgress) {
    return new Promise((resolve, reject) => {
      const form = new FormData();
      files.forEach((file) => form.append("file", file, file.name));

      const xhr = new XMLHttpRequest();
      xhr.open("POST", API_BASE + "/api/files");
      xhr.setRequestHeader("Accept", "application/json");
      const headers = authHeaders();
      Object.keys(headers).forEach((name) => xhr.setRequestHeader(name, headers[name]));

      xhr.upload.onprogress = (event) => {
        if (event.lengthComputable && onProgress) onProgress(event.loaded / event.total);
      };
      xhr.onerror = () => reject(new ApiError(t("error.network"), "", 0));
      xhr.onload = () => {
        if (xhr.status < 200 || xhr.status >= 300) return reject(errorFrom(xhr.status, xhr.responseText));
        const body = parseJson(xhr.responseText);
        if (!body || !Array.isArray(body.files)) return reject(new ApiError(t("error.badJson"), xhr.responseText.slice(0, 400), xhr.status));
        resolve(body.files);
      };
      xhr.send(form);
    });
  }

  // ═══════════════════════════════════════════════════════ paramètres

  /* `data-param="options.theme"` écrit dans un objet imbriqué : les corps de
     requête de l'API en ont, et la page ne devrait pas avoir à le savoir. */
  function assignPath(target, path, value) {
    const parts = path.split(".");
    let node = target;
    for (let i = 0; i < parts.length - 1; i += 1) {
      if (typeof node[parts[i]] !== "object" || node[parts[i]] === null) node[parts[i]] = {};
      node = node[parts[i]];
    }
    node[parts[parts.length - 1]] = value;
  }

  function fieldValue(input) {
    const type = (input.dataset.type || "string").toLowerCase();

    if (input.type === "checkbox") return input.checked;

    const raw = (input.value != null ? String(input.value) : "").trim();
    // Un champ vide laisse le défaut du service s'appliquer : c'est la règle
    // « JSON additif » vue depuis le client.
    if (raw === "" && !input.hasAttribute("data-required")) return undefined;

    switch (type) {
      case "number": {
        const n = Number(raw);
        return Number.isFinite(n) ? n : undefined;
      }
      case "boolean":
        return raw === "true" || raw === "1" || raw === "on";
      case "numbers":
        return raw.split(",").map((part) => Number(part.trim())).filter((n) => Number.isFinite(n));
      case "strings":
        return raw.split(",").map((part) => part.trim()).filter(Boolean);
      case "json": {
        const parsed = parseJson(raw);
        return parsed === null ? raw : parsed;
      }
      default:
        return raw;
    }
  }

  function collectParams(root) {
    const body = {};
    $$("[data-param]", root).forEach((input) => {
      // Un groupe de boutons radio ne compte que par celui qui est coché.
      if (input.type === "radio" && !input.checked) return;
      const value = fieldValue(input);
      if (value === undefined) return;
      assignPath(body, input.dataset.param, value);
    });
    return body;
  }

  // ═══════════════════════════════════════════════════════ un outil

  function Tool(root) {
    this.root = root;
    this.endpoint = root.dataset.endpoint || "";
    this.method = (root.dataset.method || "POST").toUpperCase();
    this.fileParam = root.dataset.fileParam || "pdf";
    this.multiple = root.hasAttribute("data-multiple") && root.dataset.multiple !== "false";
    this.minFiles = Number(root.dataset.minFiles || (this.multiple ? 2 : 1));
    this.maxFiles = Number(root.dataset.maxFiles || (this.multiple ? 20 : 1));
    this.maxBytes = Number(root.dataset.maxMb || DEFAULT_MAX_MB) * 1024 * 1024;
    this.retentionHours = Number(root.dataset.retentionHours || DEFAULT_RETENTION_HOURS);
    this.output = root.dataset.output || "asset";
    this.outputName = root.dataset.outputName || "";

    this.entries = []; // { file?, asset? } — un asset suffit, le fichier est déjà en ligne
    this.busy = false;
    this.produced = []; // assets rendus par le dernier appel, pour la suppression immédiate

    this.dom = {
      drop: $('[data-role="drop"]', root),
      input: $('[data-role="input"]', root),
      files: $('[data-role="files"]', root),
      run: $('[data-role="run"]', root),
      status: $('[data-role="status"]', root),
      error: $('[data-role="error"]', root),
      result: $('[data-role="result"]', root),
      verdict: $('[data-role="verdict"]', root),
      chain: $('[data-role="chain"]', root),
    };

    this.ensureContainers();
    this.bind();
    this.preloadFromHash();
  }

  /* Une page peut ne déclarer que le strict nécessaire : les zones d'état, de
     résultat et d'erreur sont créées si elles manquent. */
  Tool.prototype.ensureContainers = function () {
    const host = $('[data-role="panel"]', this.root) || this.root;

    if (!this.dom.status) {
      this.dom.status = el("p", { class: "status", attrs: { "data-role": "status", hidden: true, "aria-live": "polite" } });
      host.appendChild(this.dom.status);
    }
    if (!this.dom.error) {
      this.dom.error = el("div", { class: "alert", attrs: { "data-role": "error", hidden: true, role: "alert" } });
      host.appendChild(this.dom.error);
    }
    if (!this.dom.result) {
      this.dom.result = el("div", { class: "result", attrs: { "data-role": "result", hidden: true } });
      host.appendChild(this.dom.result);
    }
    // Le résultat reçoit le focus après un traitement : il lui faut être
    // focalisable sans entrer dans l'ordre de tabulation.
    if (!this.dom.result.hasAttribute("tabindex")) this.dom.result.setAttribute("tabindex", "-1");
    if (!this.dom.files && this.dom.drop) {
      this.dom.files = el("ul", { class: "file-list", attrs: { "data-role": "files" } });
      this.dom.drop.insertAdjacentElement("afterend", this.dom.files);
    }
  };

  Tool.prototype.bind = function () {
    const self = this;

    if (this.dom.input) {
      if (this.multiple) this.dom.input.setAttribute("multiple", "");
      if (this.root.dataset.accept && !this.dom.input.getAttribute("accept")) {
        this.dom.input.setAttribute("accept", this.root.dataset.accept);
      }
      this.dom.input.addEventListener("change", () => {
        self.addFiles(Array.from(self.dom.input.files || []));
        // Remettre le champ à zéro permet de re-choisir le même fichier après
        // l'avoir retiré : sans cela, « change » ne se déclenche pas.
        self.dom.input.value = "";
      });
    }

    if (this.dom.drop) {
      ["dragenter", "dragover"].forEach((type) => {
        self.dom.drop.addEventListener(type, (event) => {
          event.preventDefault();
          self.dom.drop.classList.add("is-dragging");
        });
      });
      ["dragleave", "dragend", "drop"].forEach((type) => {
        self.dom.drop.addEventListener(type, () => self.dom.drop.classList.remove("is-dragging"));
      });
      self.dom.drop.addEventListener("drop", (event) => {
        event.preventDefault();
        const files = event.dataTransfer ? Array.from(event.dataTransfer.files || []) : [];
        if (files.length) self.addFiles(files);
      });
    }

    if (this.dom.run) {
      this.dom.run.addEventListener("click", (event) => {
        event.preventDefault();
        self.run();
      });
    }

    // Un formulaire soumis à la touche Entrée doit lancer l'outil, pas recharger.
    const form = this.root.closest("form") || $("form", this.root);
    if (form) form.addEventListener("submit", (event) => { event.preventDefault(); self.run(); });
  };

  // ─────────────────────────────────────────── fichiers

  Tool.prototype.accepts = function (file) {
    const accept = (this.root.dataset.accept || "").trim();
    if (!accept) return true;
    const name = file.name.toLowerCase();
    const type = (file.type || "").toLowerCase();
    return accept.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean).some((rule) => {
      if (rule.startsWith(".")) return name.endsWith(rule);
      if (rule.endsWith("/*")) return type.startsWith(rule.slice(0, -1));
      return type === rule;
    });
  };

  Tool.prototype.addFiles = function (files) {
    this.clearError();

    for (const file of files) {
      if (!this.accepts(file)) {
        this.showError(new ApiError(t("files.rejected", { name: file.name }), ""));
        continue;
      }
      if (file.size > this.maxBytes) {
        this.showError(new ApiError(t("files.tooLarge", {
          name: file.name,
          size: formatBytes(file.size),
          max: formatBytes(this.maxBytes),
        }), ""));
        continue;
      }
      if (!this.multiple) this.entries = [];
      if (this.entries.length >= this.maxFiles) {
        this.showError(new ApiError(t("files.tooMany", { max: this.maxFiles }), ""));
        break;
      }
      this.entries.push({ file: file, asset: null });
    }

    this.renderFiles();
  };

  Tool.prototype.renderFiles = function () {
    if (!this.dom.files) return;
    const self = this;
    this.dom.files.textContent = "";

    this.entries.forEach((entry, index) => {
      const name = entry.file ? entry.file.name : entry.asset.name;
      const size = entry.file ? entry.file.size : entry.asset.bytes;

      const item = el("li", { class: "file-item" + (entry.asset ? " is-ready" : "") }, [
        el("span", { class: "file-name", text: name }),
        el("span", { class: "file-meta", text: formatBytes(size) + (entry.asset && !entry.file ? " · " + t("files.ready") : "") }),
        el("button", {
          class: "btn ghost sm file-drop",
          text: t("files.remove"),
          attrs: { type: "button", "aria-label": t("files.remove") + " — " + name },
        }),
      ]);

      $(".file-drop", item).addEventListener("click", () => {
        self.entries.splice(index, 1);
        self.renderFiles();
      });

      self.dom.files.appendChild(item);
    });

    if (this.dom.drop) this.dom.drop.classList.toggle("has-files", this.entries.length > 0);
  };

  /* Reprise d'un asset produit par un autre outil : `#asset=as_…` dans l'URL.
     C'est ce qui rend l'enchaînement gratuit — le fichier est déjà chez nous. */
  Tool.prototype.preloadFromHash = function () {
    const params = new URLSearchParams(location.hash.replace(/^#/, ""));
    const id = params.get("asset") || new URLSearchParams(location.search).get("asset");
    if (!id) return;

    const self = this;
    this.setStatus(t("preload.loading"), true);
    apiJson("/api/files/" + encodeURIComponent(id) + "/meta")
      .then((response) => response.json())
      .then((meta) => {
        self.entries = [{ file: null, asset: meta }];
        self.renderFiles();
        self.setStatus("", false);
      })
      .catch((err) => {
        self.setStatus("", false);
        self.showError(err);
      });
  };

  // ─────────────────────────────────────────── exécution

  Tool.prototype.run = async function () {
    if (this.busy) return;

    this.clearError();
    this.hideResult();

    const needsFiles = !!this.dom.input || !!this.dom.drop;
    if (needsFiles) {
      if (!this.entries.length) return this.showError(new ApiError(t("files.none"), ""));
      if (this.entries.length < this.minFiles) return this.showError(new ApiError(t("files.tooFew", { min: this.minFiles }), ""));
    }

    this.setBusy(true);

    try {
      const refs = await this.ensureAssets();
      const body = collectParams(this.root);

      if (refs.length) {
        assignPath(body, this.fileParam, this.multiple ? refs : refs[0]);
      }
      if (this.output) body.output = this.output;

      this.setStatus(t("status.working"), true);

      const response = await apiJson(this.endpoint, {
        method: this.method,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });

      let data;
      if (response.status === 202) {
        const accepted = await response.json();
        data = await this.poll(accepted.poll_url || ("/api/jobs/" + accepted.job_id));
      } else {
        const type = response.headers.get("Content-Type") || "";
        if (type.indexOf("application/json") === -1) {
          // Un outil configuré en sortie binaire rend les octets directement.
          const blob = await response.blob();
          this.setStatus(t("status.done"), false);
          return this.renderBlob(blob);
        }
        data = await response.json();
      }

      this.setStatus(t("status.done"), false);
      this.renderResult(data || {});
    } catch (err) {
      this.setStatus("", false);
      this.showError(err);
    } finally {
      this.setBusy(false);
    }
  };

  /* Met en ligne ce qui ne l'est pas encore, en une seule requête, et rend les
     références `asset://…` dans l'ordre des fichiers déposés. */
  Tool.prototype.ensureAssets = async function () {
    const pending = this.entries.filter((entry) => !entry.asset && entry.file);

    if (pending.length) {
      const self = this;
      this.setStatus(t("status.uploading", { percent: 0 }), true, 0);

      const metas = await uploadFiles(pending.map((entry) => entry.file), (ratio) => {
        self.setStatus(t("status.uploading", { percent: Math.round(ratio * 100) }), true, ratio);
      });

      pending.forEach((entry, index) => { entry.asset = metas[index] || null; });
      this.renderFiles();
    }

    return this.entries.filter((entry) => entry.asset).map((entry) => "asset://" + entry.asset.id);
  };

  /* Travail asynchrone : la réponse 202 donne une URL de suivi, on l'interroge
     jusqu'à `done` ou `failed`. L'attente s'allonge progressivement — inutile
     de marteler le service pour un OCR de deux cents pages. */
  Tool.prototype.poll = async function (pollUrl) {
    const started = Date.now();
    let wait = POLL_FIRST_MS;

    for (;;) {
      if (Date.now() - started > POLL_TIMEOUT_MS) throw new ApiError(t("error.timeout"), pollUrl, 0);

      await sleep(wait);
      wait = Math.min(Math.round(wait * 1.35), POLL_MAX_MS);

      const job = await (await apiJson(pollUrl)).json();

      if (job.status === "done") return job.result || {};
      if (job.status === "failed") throw new ApiError(job.error || t("error.title"), job.job_id || "", 0);

      this.setStatus(job.status === "queued" ? t("status.queued") : t("status.running"), true);
    }
  };

  // ─────────────────────────────────────────── état visible

  Tool.prototype.setBusy = function (busy) {
    this.busy = busy;
    if (this.dom.run) {
      this.dom.run.disabled = busy;
      if (busy) {
        this.dom.run.dataset.label = this.dom.run.dataset.label || this.dom.run.textContent;
        this.dom.run.textContent = t("run.working");
      } else if (this.dom.run.dataset.label) {
        this.dom.run.textContent = this.dom.run.dataset.label;
      }
    }
  };

  Tool.prototype.setStatus = function (message, spinning, ratio) {
    const node = this.dom.status;
    if (!node) return;

    node.textContent = "";
    if (!message) { node.hidden = true; return; }

    node.hidden = false;
    if (spinning) node.appendChild(el("span", { class: "spinner", attrs: { "aria-hidden": "true" } }));
    node.appendChild(el("span", { text: message }));

    if (ratio != null) {
      const bar = el("span", { class: "progress" }, [el("span")]);
      bar.firstChild.style.width = Math.round(ratio * 100) + "%";
      node.appendChild(bar);
    }
  };

  Tool.prototype.clearError = function () {
    if (this.dom.error) { this.dom.error.hidden = true; this.dom.error.textContent = ""; }
  };

  Tool.prototype.showError = function (err) {
    const node = this.dom.error;
    if (!node) return;

    node.textContent = "";
    node.hidden = false;
    node.appendChild(el("p", { class: "alert-title", text: err && err.error ? err.error : t("error.title") }));
    if (err && err.details) node.appendChild(el("p", { class: "alert-details", text: err.details }));
    if (err && err.status) node.appendChild(el("p", { class: "alert-details", text: "HTTP " + err.status }));
  };

  Tool.prototype.hideResult = function () {
    if (this.dom.result) { this.dom.result.hidden = true; this.dom.result.textContent = ""; }
    if (this.dom.verdict && this.dom.verdict !== this.dom.result) { this.dom.verdict.hidden = true; this.dom.verdict.textContent = ""; }
    if (this.dom.chain) this.dom.chain.hidden = true;
  };

  // ─────────────────────────────────────────── résultat

  Tool.prototype.renderResult = function (data) {
    const host = this.dom.result;
    if (!host) return;

    host.textContent = "";
    host.hidden = false;

    const assets = [];
    if (data.asset) assets.push(data.asset);
    if (Array.isArray(data.assets)) data.assets.forEach((a) => assets.push(a));
    this.produced = assets;

    const head = el("div", { class: "result-head" }, [
      el("h2", { text: t("result.title") }),
    ]);
    if (data.pages != null) head.appendChild(el("span", { class: "tag", text: t("result.pages", { count: data.pages }) }));
    host.appendChild(head);

    if (assets.length) host.appendChild(this.renderFileList(assets));

    if (data.download_url) {
      host.appendChild(el("p", {}, [
        el("a", { class: "btn", text: t("result.open"), attrs: { href: API_BASE + data.download_url, rel: "noopener" } }),
      ]));
    }

    // Le verdict est la raison d'être de ce produit : il vient avant les
    // remarques, et juste après le fichier.
    if (data.verdict) {
      const card = renderVerdict(data.verdict);
      if (this.dom.verdict && this.dom.verdict !== host) {
        this.dom.verdict.textContent = "";
        this.dom.verdict.hidden = false;
        this.dom.verdict.appendChild(card);
      } else {
        host.appendChild(card);
      }
    }

    if (Array.isArray(data.warnings) && data.warnings.length) {
      host.appendChild(el("h3", { text: t("warnings.title") }));
      host.appendChild(el("ul", { class: "warnings" }, data.warnings.map((w) => el("li", { text: String(w) }))));
    }

    this.renderChain(assets[0]);
    host.appendChild(this.renderRetention());

    // Le focus part sur le résultat : au clavier comme au lecteur d'écran, la
    // réponse ne doit pas être à chercher.
    host.focus({ preventScroll: true });
    host.scrollIntoView({ behavior: "smooth", block: "nearest" });
  };

  Tool.prototype.renderFileList = function (assets) {
    const self = this;
    return el("ul", { class: "result-files" }, assets.map((asset) => {
      const button = el("button", {
        class: "btn primary sm",
        text: t("result.download"),
        attrs: { type: "button" },
      });
      button.addEventListener("click", () => self.download(asset, button));

      return el("li", { class: "result-file" }, [
        el("span", { class: "file-name", text: asset.name || asset.id }),
        el("span", { class: "file-meta", text: formatBytes(asset.bytes) + (asset.pages ? " · " + t("result.pages", { count: asset.pages }) : "") }),
        button,
      ]);
    }));
  };

  /* GET /api/files/{id} peut demander la clé d'API : un simple <a href> ne la
     porterait pas. On récupère les octets, puis on déclenche l'enregistrement. */
  Tool.prototype.download = async function (asset, button) {
    const label = button ? button.textContent : "";
    if (button) { button.disabled = true; button.textContent = t("run.working"); }
    try {
      const response = await apiJson("/api/files/" + encodeURIComponent(asset.id));
      const blob = await response.blob();
      saveBlob(blob, asset.name || (asset.id + "." + (asset.kind || "bin")));
    } catch (err) {
      this.showError(err);
    } finally {
      if (button) { button.disabled = false; button.textContent = label; }
    }
  };

  Tool.prototype.renderBlob = function (blob) {
    const host = this.dom.result;
    if (!host) return;
    host.textContent = "";
    host.hidden = false;
    // À défaut de `data-output-name`, on reprend le nom déposé : « contrat.pdf »
    // vaut mieux que « document.pdf » dans le dossier des téléchargements.
    const source = this.entries[0] && (this.entries[0].file || this.entries[0].asset);
    const name = this.outputName || (source && source.name) || "document.pdf";
    const button = el("button", { class: "btn primary", text: t("result.download"), attrs: { type: "button" } });
    button.addEventListener("click", () => saveBlob(blob, name));
    host.appendChild(el("div", { class: "result-head" }, [el("h2", { text: t("result.title") })]));
    host.appendChild(button);
    host.appendChild(this.renderRetention());
  };

  /* Enchaînement : les liens déclarés `data-chain` repartent avec l'asset
     produit, donc sans nouveau dépôt ni nouvel envoi réseau. */
  Tool.prototype.renderChain = function (asset) {
    if (!this.dom.chain) return;
    const links = $$("a[data-chain]", this.dom.chain);
    if (!asset || !links.length) { this.dom.chain.hidden = true; return; }

    links.forEach((link) => {
      const base = link.dataset.chainHref || (link.getAttribute("href") || "").split("#")[0];
      link.dataset.chainHref = base;
      link.setAttribute("href", base + "#asset=" + asset.id);
    });
    this.dom.chain.hidden = false;
  };

  /* La rétention se rappelle après chaque traitement, avec le moyen d'y couper
     court tout de suite. C'est un argument, pas une petite ligne. */
  Tool.prototype.renderRetention = function () {
    const self = this;
    const note = el("p", { class: "notice", text: t("result.retention", { hours: this.retentionHours }) });

    const ids = this.produced.map((a) => a.id)
      .concat(this.entries.filter((e) => e.asset).map((e) => e.asset.id));

    if (ids.length) {
      const button = el("button", { class: "btn ghost sm", text: t("result.delete"), attrs: { type: "button" } });
      button.addEventListener("click", async () => {
        button.disabled = true;
        self.setStatus(t("status.deleting"), true);
        await Promise.all(ids.map((id) => apiJson("/api/files/" + encodeURIComponent(id), { method: "DELETE" }).catch(() => null)));
        self.entries = [];
        self.produced = [];
        self.renderFiles();
        self.hideResult();
        self.setStatus(t("status.deleted"), false);
      });
      note.appendChild(button);
    }

    return note;
  };

  // ═══════════════════════════════════════════════════════ verdict

  /* La carte de verdict. Structure fixe, contenu entièrement issu de l'API :
     un statut, un score, une phrase, et des contrôles nommés — avec la page
     concernée quand l'API la connaît.

     Le nom du contrôle est traduit ici (CHECK_LABELS) ; le `detail` rendu par
     l'API, en anglais, reste affiché juste en dessous. */
  function renderVerdict(verdict) {
    const status = ["ok", "warn", "fail"].indexOf(verdict.status) >= 0 ? verdict.status : "ok";
    const score = Math.max(0, Math.min(100, Number(verdict.score) || 0));

    const ring = el("div", { class: "verdict-score", attrs: { role: "img", "aria-label": score + " / 100" } }, [
      el("strong", { class: "score-value", text: String(score) }),
      el("span", { class: "score-unit", text: t("verdict.unit") }),
    ]);

    const card = el("section", { class: "verdict", attrs: { "data-status": status, "aria-label": t("verdict.title") } }, [
      el("div", { class: "verdict-head" }, [
        ring,
        el("div", { class: "verdict-title" }, [
          el("span", { class: "verdict-status", text: t("verdict." + status) }),
          el("p", { class: "verdict-summary", text: verdict.summary || "" }),
        ]),
      ]),
    ]);

    if (Array.isArray(verdict.checks) && verdict.checks.length) {
      card.appendChild(el("ul", { class: "verdict-checks" }, verdict.checks.map((check) => {
        const checkStatus = ["ok", "warn", "fail"].indexOf(check.status) >= 0 ? check.status : "ok";
        const known = Object.prototype.hasOwnProperty.call(CHECK_LABELS, check.name);
        // `title` garde le nom brut sous la main : c'est lui qui fait contrat
        // avec l'API, pas le libellé traduit.
        const name = el("div", { class: "check-name" }, [
          el("span", { text: checkLabel(check.name), attrs: { title: check.name || null } }),
        ]);
        if (check.page != null) name.appendChild(el("span", { class: "check-page", text: t("verdict.page", { page: check.page }) }));

        // Contrôle inconnu et sans détail : le résumé du verdict prend le relais,
        // pour qu'une ligne ne reste jamais vide de sens.
        const detail = check.detail || (known ? "" : verdict.summary || "");

        return el("li", { class: "verdict-check", attrs: { "data-status": checkStatus, "data-check": check.name || null } }, [
          name,
          el("p", { class: "check-detail", text: detail }),
        ]);
      })));
    }

    card.appendChild(el("p", { class: "verdict-foot", text: t("verdict.foot") }));

    // L'anneau part de zéro et rejoint le score une fois la carte posée : la
    // transition ne joue que si la valeur change après le premier calcul.
    ring.style.setProperty("--score", "0");
    requestAnimationFrame(() => requestAnimationFrame(() => ring.style.setProperty("--score", String(score))));

    return card;
  }

  // ═══════════════════════════════════════════════════════ divers

  function saveBlob(blob, name) {
    const url = URL.createObjectURL(blob);
    const link = el("a", { attrs: { href: url, download: name } });
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
  }

  // Sans cela, un fichier lâché à côté de la zone de dépôt remplacerait la page.
  function guardWindowDrops() {
    ["dragover", "drop"].forEach((type) => {
      window.addEventListener(type, (event) => {
        if (event.target.closest && event.target.closest('[data-role="drop"]')) return;
        event.preventDefault();
      });
    });
  }

  function init() {
    initTheme();
    initApiKeyFields();
    guardWindowDrops();
    $$("[data-tool]").forEach((root) => {
      if (root.dataset.endpoint) new Tool(root);
    });
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();

  // Exposé pour les pages qui auraient besoin d'appeler l'API elles-mêmes.
  window.Documents = { t: t, checkLabel: checkLabel, api: apiJson, upload: uploadFiles, verdict: renderVerdict, apiKey: apiKey, setApiKey: setApiKey };
})();
