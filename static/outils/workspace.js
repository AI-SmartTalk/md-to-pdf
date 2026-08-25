/* ==========================================================================
   AI SmartTalk Documents — l'espace de travail (/app, /en/app)
   Aucune dépendance externe.

   La page servie par Tera n'est qu'une structure vide : tout ce qui s'affiche
   ici appartient à quelqu'un, et rien de ce qui appartient à quelqu'un ne
   traverse un gabarit rendu sans session. Le cookie `mdpdf_session` est
   HttpOnly — ce script ne le lit jamais, il se contente de le laisser partir
   avec `credentials: "same-origin"`.

   Trois lectures, dans l'ordre où elles comptent :
     GET /api/history   le registre de qualité — le cœur de la page
     GET /api/keys      les clés, créées et révoquées ici
     GET /api/usage     le palier et ses limites
   ========================================================================== */
"use strict";

(function () {
  // ═══════════════════════════════════════════════════════ langue

  var LANG = (document.documentElement.lang || "fr").toLowerCase().indexOf("en") === 0 ? "en" : "fr";

  var STRINGS = {
    fr: {
      "signedInAs": "connecté en tant que {email}",
      "error.network": "Le service est injoignable.",
      "error.generic": "L'opération n'a pas abouti.",

      "history.empty.title": "Rien n'est encore inscrit ici",
      "history.empty.body": "Dès que vous traitez un document en étant connecté, l'opération s'inscrit ici avec son verdict : ce qui a été préservé, ce qui a été perdu, et sur combien de pages. Les fichiers, eux, restent effacés au bout de quelques heures — c'est la mesure qu'on garde, pas le document.",
      "history.empty.cta": "Choisir un outil",
      "history.cleared": "Historique effacé.",
      "history.count": "{count} opération",
      "history.countMany": "{count} opérations",
      "history.noVerdict": "Aucun verdict n'a été enregistré pour cette opération.",
      "history.score": "{score}/100",
      "history.pages": "{count} page",
      "history.pagesMany": "{count} pages",
      "history.on": "le {date}",

      "keys.empty": "Aucune clé pour l'instant. Créez-en une par intégration : révoquer l'une n'arrête pas les autres.",
      "keys.created": "créée le {date}",
      "keys.lastUsed": "dernier appel le {date}",
      "keys.neverUsed": "jamais utilisée",
      "keys.revoke": "Révoquer",
      "keys.revokeAsk": "Confirmer ?",
      "keys.revokeYes": "Oui, révoquer",
      "keys.revokeNo": "Annuler",
      "keys.nameRequired": "Donnez un nom à la clé : c'est ce qui permettra de savoir laquelle révoquer.",
      "keys.secretTitle": "Voici la clé « {name} »",
      "keys.copied": "Copié.",
      "keys.copyFailed": "Copie impossible : sélectionnez la clé et copiez-la à la main.",
      "keys.creating": "Création…",

      "usage.plan": "Palier",
      "usage.keys": "Clés actives",
      "usage.maxFile": "Fichier le plus lourd",
      "usage.retention": "Conservation",
      "usage.rate": "Requêtes par minute",
      "usage.hours": "heures",
      "usage.unlimited": "sans limite",
      "usage.plan.free": "Gratuit",
      "usage.plan.pro": "Pro",
      "usage.plan.team": "Équipe",
    },
    en: {
      "signedInAs": "signed in as {email}",
      "error.network": "The service cannot be reached.",
      "error.generic": "The operation did not complete.",

      "history.empty.title": "Nothing has been recorded here yet",
      "history.empty.body": "As soon as you process a document while signed in, the operation lands here with its verdict: what survived, what did not, and over how many pages. The files themselves are still erased after a few hours — it is the measurement we keep, not the document.",
      "history.empty.cta": "Pick a tool",
      "history.cleared": "History erased.",
      "history.count": "{count} operation",
      "history.countMany": "{count} operations",
      "history.noVerdict": "No verdict was recorded for this operation.",
      "history.score": "{score}/100",
      "history.pages": "{count} page",
      "history.pagesMany": "{count} pages",
      "history.on": "on {date}",

      "keys.empty": "No key yet. Create one per integration: revoking one does not stop the others.",
      "keys.created": "created {date}",
      "keys.lastUsed": "last call {date}",
      "keys.neverUsed": "never used",
      "keys.revoke": "Revoke",
      "keys.revokeAsk": "Confirm?",
      "keys.revokeYes": "Yes, revoke",
      "keys.revokeNo": "Cancel",
      "keys.nameRequired": "Give the key a name: it is what tells you which one to revoke later.",
      "keys.secretTitle": "Here is the “{name}” key",
      "keys.copied": "Copied.",
      "keys.copyFailed": "Copying failed: select the key and copy it by hand.",
      "keys.creating": "Creating…",

      "usage.plan": "Plan",
      "usage.keys": "Active keys",
      "usage.maxFile": "Largest file",
      "usage.retention": "Retention",
      "usage.rate": "Requests per minute",
      "usage.hours": "hours",
      "usage.unlimited": "unlimited",
      "usage.plan.free": "Free",
      "usage.plan.pro": "Pro",
      "usage.plan.team": "Team",
    },
  };

  function t(key, vars) {
    var raw = (STRINGS[LANG] && STRINGS[LANG][key]) || STRINGS.fr[key] || key;
    if (!vars) return raw;
    return raw.replace(/\{(\w+)\}/g, function (m, name) {
      return vars[name] !== undefined ? String(vars[name]) : m;
    });
  }

  /* ─── ce qu'une opération a fait ───────────────────────────────────────
     Le service enregistre l'opération sous le nom du fichier qu'elle produit
     (`compressed.pdf`) ; il pourra un jour l'enregistrer sous le nom de
     l'outil (`compress`). Les deux formes se lisent ici, et une troisième,
     inconnue, se lit encore — en clair plutôt qu'en kebab-case.

     Le libellé est un participe passé, parce que la ligne se lit
     « contrat.pdf · compressé le 12 mars ». */
  var TOOL_VERBS = {
    "compress": { fr: "compressé", en: "compressed" },
    "compressed": { fr: "compressé", en: "compressed" },
    "convert": { fr: "converti", en: "converted" },
    "converted": { fr: "converti", en: "converted" },
    "crop": { fr: "rogné", en: "cropped" },
    "cropped": { fr: "rogné", en: "cropped" },
    "document": { fr: "produit", en: "produced" },
    "images": { fr: "assemblé depuis des images", en: "built from images" },
    "images-to-pdf": { fr: "assemblé depuis des images", en: "built from images" },
    "merge": { fr: "fusionné", en: "merged" },
    "merged": { fr: "fusionné", en: "merged" },
    "number": { fr: "numéroté", en: "numbered" },
    "numbered": { fr: "numéroté", en: "numbered" },
    "ocr": { fr: "océrisé", en: "OCR-ed" },
    "pages": { fr: "pages extraites", en: "pages extracted" },
    "pdfa": { fr: "converti en PDF/A", en: "converted to PDF/A" },
    "protect": { fr: "protégé", en: "protected" },
    "protected": { fr: "protégé", en: "protected" },
    "rasterize": { fr: "rastérisé", en: "rasterised" },
    "rasterized": { fr: "rastérisé", en: "rasterised" },
    "redact": { fr: "caviardé", en: "redacted" },
    "repair": { fr: "réparé", en: "repaired" },
    "repaired": { fr: "réparé", en: "repaired" },
    "unlock": { fr: "déverrouillé", en: "unlocked" },
    "unlocked": { fr: "déverrouillé", en: "unlocked" },
    "watermark": { fr: "filigrané", en: "watermarked" },
    "watermarked": { fr: "filigrané", en: "watermarked" },
    "office-to-pdf": { fr: "converti depuis Office", en: "converted from Office" },
    "pdf-to-office": { fr: "converti vers Office", en: "converted to Office" },
    "extract": { fr: "contenu extrait", en: "content extracted" },
    "compose": { fr: "composé", en: "composed" },
    "attest": { fr: "attesté", en: "attested" },
    "diff": { fr: "comparé", en: "compared" },
  };

  function toolVerb(tool) {
    var name = String(tool || "").toLowerCase().replace(/\.[a-z0-9]+$/, "");
    var known = TOOL_VERBS[name];
    if (known) return known[LANG] || known.en;
    if (!name) return "";
    return name.replace(/[-_]/g, " ");
  }

  /* La table des contrôles vit dans app.js et fait foi : en écrire une
     seconde ici serait s'assurer qu'elles divergent. Le repli ne sert que si
     app.js n'a pas été servi. */
  function checkLabel(name) {
    if (window.Documents && window.Documents.checkLabel) return window.Documents.checkLabel(name);
    if (!name) return "";
    return String(name).replace(/-/g, " ").replace(/^./, function (c) { return c.toUpperCase(); });
  }

  // ═══════════════════════════════════════════════════════ utilitaires

  var $ = function (sel, root) { return (root || document).querySelector(sel); };
  var role = function (name, root) { return (root || document).querySelector('[data-role="' + name + '"]'); };

  /* `text` passe par textContent : un nom de fichier ou un détail de contrôle
     vient de l'API, et rien de ce qui vient de l'API n'est interprété. */
  function el(tag, options, children) {
    var node = document.createElement(tag);
    var opts = options || {};
    if (opts.class) node.className = opts.class;
    if (opts.text != null) node.textContent = opts.text;
    Object.keys(opts.attrs || {}).forEach(function (name) {
      var value = opts.attrs[name];
      if (value != null && value !== false) node.setAttribute(name, value === true ? "" : String(value));
    });
    (children || []).forEach(function (child) { if (child) node.appendChild(child); });
    return node;
  }

  function clear(node) { while (node && node.firstChild) node.removeChild(node.firstChild); }

  function plural(count, oneKey, manyKey) {
    return t(Math.abs(Number(count)) <= 1 ? oneKey : manyKey, { count: count });
  }

  /* Les mêmes unités que les pages d'outil : « 780 ko », « 4,2 Mo ». */
  function formatBytes(bytes) {
    var value = Number(bytes);
    if (!isFinite(value) || value < 0) return "";
    if (value < 1024) return value + " " + (LANG === "fr" ? "o" : "B");
    if (value < 1024 * 1024) return Math.round(value / 1024) + " " + (LANG === "fr" ? "ko" : "kB");
    var mb = value / (1024 * 1024);
    return (mb < 10 ? mb.toFixed(1) : Math.round(mb)) + " " + (LANG === "fr" ? "Mo" : "MB");
  }

  /* « 12 mars » quand c'est cette année, « 12 mars 2025 » sinon : l'année de
     l'année en cours est du bruit dans une liste qu'on parcourt. */
  function formatDate(iso) {
    if (!iso) return "";
    var date = new Date(iso);
    if (isNaN(date.getTime())) return String(iso);
    var options = { day: "numeric", month: "long" };
    if (date.getFullYear() !== new Date().getFullYear()) options.year = "numeric";
    try {
      return new Intl.DateTimeFormat(LANG === "fr" ? "fr-FR" : "en-GB", options).format(date);
    } catch (e) {
      return date.toISOString().slice(0, 10);
    }
  }

  function statusOf(value) {
    return ["ok", "warn", "fail"].indexOf(value) >= 0 ? value : null;
  }

  var STATUS_MARK = { ok: "✓", warn: "!", fail: "✕" };

  // ═══════════════════════════════════════════════════════ appels

  var ROOT = $(".ws");
  var SIGNIN = (ROOT && ROOT.dataset.signin) || "/connexion";
  var TOOLS_ROOT = (ROOT && ROOT.dataset.tools) || "/outils";

  /* Une session non ouverte n'est pas une erreur à afficher : c'est une page
     qu'on n'aurait pas dû atteindre. On repart vers la connexion sans laisser
     l'espace vide derrière soi dans l'historique du navigateur. */
  function signedOut() {
    window.location.replace(SIGNIN);
  }

  function api(path, options) {
    var opts = options || {};
    var init = {
      method: opts.method || "GET",
      // Le cookie de session est HttpOnly : on ne le lit pas, on le laisse partir.
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    };
    if (opts.body !== undefined) {
      init.headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(opts.body);
    }

    return fetch(path, init).then(function (response) {
      if (response.status === 401) {
        var unauthorized = new Error("unauthorized");
        unauthorized.unauthorized = true;
        throw unauthorized;
      }
      if (response.status === 204) return null;

      return response.text().then(function (raw) {
        var payload = null;
        if (raw) { try { payload = JSON.parse(raw); } catch (e) { payload = null; } }
        if (!response.ok) {
          var message = (payload && (payload.message || payload.error)) || t("error.generic");
          throw new Error(message);
        }
        return payload;
      });
    }, function () {
      throw new Error(t("error.network"));
    });
  }

  function flash(node, message, tone) {
    if (!node) return;
    node.textContent = message || "";
    node.setAttribute("data-tone", tone || "err");
    node.hidden = !message;
  }

  // ═══════════════════════════════════════════════════════ 1. le registre

  /* Ce que la ligne dit en un coup d'œil, à partir du verdict : le contrôle
     qui mérite d'être vu. Un échec passe devant un avertissement, qui passe
     devant un contrôle réussi — parce que c'est l'ordre dans lequel on veut
     l'apprendre. */
  function leadCheck(verdict) {
    if (!verdict || !Array.isArray(verdict.checks) || !verdict.checks.length) return null;
    var byStatus = function (wanted) {
      for (var i = 0; i < verdict.checks.length; i += 1) {
        if (verdict.checks[i].status === wanted) return verdict.checks[i];
      }
      return null;
    };
    return byStatus("fail") || byStatus("warn") || verdict.checks[0];
  }

  function operationRow(entry) {
    var verdict = entry.verdict || null;
    var status = verdict ? statusOf(verdict.status) : null;

    var summary = el("summary", {}, [
      entry.file ? el("span", { class: "op-file", text: entry.file }) : null,
    ]);

    var bits = [];
    var verb = toolVerb(entry.tool);
    var date = formatDate(entry.at);
    if (verb || date) {
      bits.push(el("span", { text: (verb ? verb + " " : "") + (date ? t("history.on", { date: date }) : "") }));
    }
    if (entry.bytes != null) bits.push(el("span", { class: "op-size", text: formatBytes(entry.bytes) }));
    if (entry.pages != null) {
      bits.push(el("span", { class: "op-pages", text: plural(entry.pages, "history.pages", "history.pagesMany") }));
    }

    var lead = leadCheck(verdict);
    if (lead) {
      var leadStatus = statusOf(lead.status) || "ok";
      bits.push(el("span", {
        class: "op-check",
        text: STATUS_MARK[leadStatus] + " " + checkLabel(lead.name),
        attrs: { title: lead.detail || null },
      }));
    }
    if (verdict && verdict.score != null) {
      bits.push(el("span", { class: "op-score", text: t("history.score", { score: verdict.score }) }));
    }

    bits.forEach(function (bit, index) {
      if (index || entry.file) summary.appendChild(el("span", { class: "op-sep", text: "·", attrs: { "aria-hidden": "true" } }));
      summary.appendChild(bit);
    });

    var detail = el("div", { class: "op-detail" });
    if (verdict && verdict.summary) {
      detail.appendChild(el("p", { class: "op-summary", text: verdict.summary }));
    }
    if (verdict && Array.isArray(verdict.checks) && verdict.checks.length) {
      // Les classes de la carte de verdict : même structure, même pastille,
      // même sens. Une opération repliée et une opération dépliée doivent se
      // reconnaître d'un écran à l'autre.
      detail.appendChild(el("ul", { class: "verdict-checks" }, verdict.checks.map(function (check) {
        var checkStatus = statusOf(check.status) || "ok";
        var name = el("div", { class: "check-name" }, [
          el("span", { text: checkLabel(check.name), attrs: { title: check.name || null } }),
        ]);
        if (check.page != null) {
          name.appendChild(el("span", { class: "check-page", text: "page " + check.page }));
        }
        return el("li", { class: "verdict-check", attrs: { "data-status": checkStatus } }, [
          name,
          check.detail ? el("p", { class: "check-detail", text: check.detail }) : null,
        ]);
      })));
    } else {
      detail.appendChild(el("p", { class: "op-none", text: t("history.noVerdict") }));
    }

    return el("details", {
      class: "ws-op",
      attrs: { "data-status": status || "none" },
    }, [summary, detail]);
  }

  function renderHistory(entries) {
    var pane = role("ws-history");
    var ask = role("ws-clear-ask");
    if (!pane) return;
    clear(pane);

    if (!entries || !entries.length) {
      if (ask) ask.hidden = true;
      pane.appendChild(el("div", { class: "ws-empty" }, [
        el("strong", { text: t("history.empty.title") }),
        el("p", { text: t("history.empty.body") }),
        el("a", { class: "btn primary sm", text: t("history.empty.cta"), attrs: { href: TOOLS_ROOT } }),
      ]));
      return;
    }

    if (ask) ask.hidden = false;
    var list = el("div", { class: "ws-ops" });
    entries.forEach(function (entry) { list.appendChild(operationRow(entry)); });
    pane.appendChild(list);
  }

  function loadHistory() {
    return api("/api/history").then(function (payload) {
      renderHistory((payload && payload.entries) || []);
    });
  }

  function initHistoryControls() {
    var ask = role("ws-clear-ask");
    var confirmBox = role("ws-clear-confirm");
    var yes = role("ws-clear-yes");
    var no = role("ws-clear-no");
    var note = role("ws-history-flash");
    if (!ask || !confirmBox || !yes || !no) return;

    // Deux temps plutôt qu'une boîte de dialogue du navigateur : effacer son
    // registre est une demande qu'on fait sans vouloir partir, et la confirmation
    // doit rester dans la page, à l'endroit où elle a été demandée.
    ask.addEventListener("click", function () {
      ask.hidden = true;
      confirmBox.hidden = false;
      yes.focus();
    });
    no.addEventListener("click", function () {
      confirmBox.hidden = true;
      ask.hidden = false;
      ask.focus();
    });
    yes.addEventListener("click", function () {
      yes.disabled = true;
      api("/api/history", { method: "DELETE" }).then(function () {
        confirmBox.hidden = true;
        renderHistory([]);
        flash(note, t("history.cleared"), "ok");
      }).catch(function (error) {
        if (error && error.unauthorized) return signedOut();
        flash(note, error.message);
      }).then(function () {
        yes.disabled = false;
      });
    });
  }

  // ═══════════════════════════════════════════════════════ 2. les clés

  function keyRow(key) {
    var meta = [];
    if (key.created_at) meta.push(t("keys.created", { date: formatDate(key.created_at) }));
    meta.push(key.last_used ? t("keys.lastUsed", { date: formatDate(key.last_used) }) : t("keys.neverUsed"));

    var actions = el("span", { class: "key-actions" });
    var revoke = el("button", { class: "btn ghost sm", text: t("keys.revoke"), attrs: { type: "button" } });
    var confirmWrap = el("span", { class: "ws-confirm", attrs: { hidden: true } });
    var yes = el("button", { class: "btn sm", text: t("keys.revokeYes"), attrs: { type: "button" } });
    var no = el("button", { class: "btn ghost sm", text: t("keys.revokeNo"), attrs: { type: "button" } });
    confirmWrap.appendChild(el("span", { text: t("keys.revokeAsk") }));
    confirmWrap.appendChild(yes);
    confirmWrap.appendChild(no);
    actions.appendChild(revoke);
    actions.appendChild(confirmWrap);

    var row = el("li", { class: "ws-key" }, [
      el("span", { class: "key-name", text: key.name || key.id }),
      key.hint ? el("span", { class: "key-hint", text: key.hint }) : null,
      el("span", { class: "key-meta", text: meta.join(" · ") }),
      actions,
    ]);

    revoke.addEventListener("click", function () {
      revoke.hidden = true;
      confirmWrap.hidden = false;
    });
    no.addEventListener("click", function () {
      confirmWrap.hidden = true;
      revoke.hidden = false;
    });
    yes.addEventListener("click", function () {
      yes.disabled = true;
      api("/api/keys/" + encodeURIComponent(key.id), { method: "DELETE" }).then(function () {
        // Retirée sur place : recharger la page pour voir disparaître une ligne
        // qu'on vient soi-même de supprimer est une confirmation qu'on n'a pas
        // demandée.
        var list = row.parentNode;
        row.remove();
        if (list && !list.children.length) renderKeys([]);
        adjustKeyCount(-1);
      }).catch(function (error) {
        if (error && error.unauthorized) return signedOut();
        yes.disabled = false;
        flash(role("ws-keys-flash"), error.message);
      });
    });

    return row;
  }

  function renderKeys(keys) {
    var pane = role("ws-keys");
    if (!pane) return;
    clear(pane);

    if (!keys || !keys.length) {
      pane.appendChild(el("p", { class: "ws-empty", text: t("keys.empty") }));
      return;
    }

    var list = el("ul", { class: "ws-keys" });
    keys.forEach(function (key) { list.appendChild(keyRow(key)); });
    pane.appendChild(list);
  }

  function loadKeys() {
    return api("/api/keys").then(function (payload) {
      renderKeys((payload && payload.keys) || []);
    });
  }

  /* Le compteur du palier vient de /api/usage ; le recharger pour une clé de
     plus ou de moins coûte un aller-retour pour un chiffre qu'on connaît. */
  function adjustKeyCount(delta) {
    var node = role("ws-key-count");
    if (!node) return;
    var value = parseInt(node.textContent, 10);
    if (isFinite(value)) node.textContent = String(Math.max(0, value + delta));
  }

  function showSecret(created) {
    var panel = role("ws-secret");
    var title = role("ws-secret-title");
    var value = role("ws-secret-value");
    if (!panel || !value) return;

    if (title) title.textContent = t("keys.secretTitle", { name: (created.key && created.key.name) || "" });
    value.textContent = created.secret || "";
    panel.hidden = false;
    panel.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  function initKeyControls() {
    var form = role("ws-key-form");
    var input = role("ws-key-name");
    var note = role("ws-keys-flash");
    var copy = role("ws-secret-copy");

    if (form && input) {
      form.addEventListener("submit", function (event) {
        event.preventDefault();
        var name = input.value.trim();
        if (!name) { flash(note, t("keys.nameRequired")); input.focus(); return; }

        var button = form.querySelector('button[type="submit"]');
        var label = button ? button.textContent : "";
        if (button) { button.disabled = true; button.textContent = t("keys.creating"); }
        flash(note, "");

        api("/api/keys", { method: "POST", body: { name: name } }).then(function (created) {
          input.value = "";
          showSecret(created || {});
          adjustKeyCount(1);
          return loadKeys();
        }).catch(function (error) {
          if (error && error.unauthorized) return signedOut();
          flash(note, error.message);
        }).then(function () {
          if (button) { button.disabled = false; button.textContent = label; }
        });
      });
    }

    if (copy) {
      copy.addEventListener("click", function () {
        var value = role("ws-secret-value");
        var secret = value ? value.textContent : "";
        var done = function () { flash(note, t("keys.copied"), "ok"); };
        var failed = function () {
          // Le presse-papiers est refusé hors HTTPS et hors geste utilisateur :
          // la clé reste sélectionnable, et on le dit plutôt que de faire semblant.
          flash(note, t("keys.copyFailed"));
          if (value) {
            var range = document.createRange();
            range.selectNodeContents(value);
            var selection = window.getSelection();
            selection.removeAllRanges();
            selection.addRange(range);
          }
        };
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(secret).then(done, failed);
        } else {
          failed();
        }
      });
    }
  }

  // ═══════════════════════════════════════════════════════ 3. le palier

  function planLabel(plan) {
    var key = "usage.plan." + String(plan || "").toLowerCase();
    var label = t(key);
    return label === key ? String(plan || "") : label;
  }

  function limitItem(label, value, unit, extraRole) {
    var strong = el("strong", { class: "limit-value", text: String(value) });
    if (extraRole) strong.setAttribute("data-role", extraRole);
    var item = el("li", {}, [el("span", { class: "limit-label", text: label }), strong]);
    if (unit) strong.appendChild(el("small", { text: " " + unit }));
    return item;
  }

  function renderUsage(usage) {
    var pane = role("ws-usage");
    if (!pane || !usage) return;
    clear(pane);

    var limits = usage.limits || {};
    var rate = limits.requests_per_minute;
    if (rate === "unlimited" || rate == null || rate === "") rate = t("usage.unlimited");

    pane.appendChild(el("ul", { class: "ws-limits" }, [
      limitItem(t("usage.plan"), planLabel(usage.plan)),
      // Le compteur porte un rôle : créer ou révoquer une clé le corrige sur place.
      limitItem(t("usage.keys"), usage.keys != null ? usage.keys : 0, null, "ws-key-count"),
      limitItem(t("usage.maxFile"), limits.max_file_mb != null ? limits.max_file_mb : "—", LANG === "fr" ? "Mo" : "MB"),
      limitItem(t("usage.retention"), limits.retention_hours != null ? limits.retention_hours : "—", t("usage.hours")),
      limitItem(t("usage.rate"), rate),
    ]));
  }

  function loadUsage() {
    return api("/api/usage").then(renderUsage);
  }

  // ═══════════════════════════════════════════════════════ démarrage

  function initSignOut() {
    var button = role("ws-signout");
    if (!button) return;
    button.addEventListener("click", function () {
      button.disabled = true;
      api("/api/auth/logout", { method: "POST" }).catch(function () {
        // Une déconnexion qui échoue côté serveur ne doit pas coincer le
        // visiteur sur une page qu'il veut quitter.
        return null;
      }).then(function () {
        window.location.href = ROOT && ROOT.dataset.tools === "/tools" ? "/en" : "/";
      });
    });
  }

  function boot() {
    if (!ROOT) return;

    var loading = role("ws-loading");
    var body = role("ws-body");

    api("/api/auth/me").then(function (session) {
      var account = (session && session.account) || {};
      var identity = role("ws-identity");
      if (identity) identity.textContent = t("signedInAs", { email: account.email || "" });

      if (loading) loading.hidden = true;
      if (body) body.hidden = false;

      initHistoryControls();
      initKeyControls();
      initSignOut();

      // Le registre d'abord : c'est ce qu'on vient voir, et les deux autres
      // sections n'ont pas à le faire attendre.
      loadHistory().catch(function (error) { flash(role("ws-history-flash"), error.message); });
      loadKeys().catch(function (error) { flash(role("ws-keys-flash"), error.message); });
      loadUsage().catch(function () { /* le palier est informatif : son absence ne dit rien d'utile */ });
    }).catch(function (error) {
      if (error && error.unauthorized) return signedOut();
      if (loading) loading.textContent = error.message || t("error.generic");
    });
  }

  // `window.Documents` est posé par app.js, plus bas dans le document : attendre
  // DOMContentLoaded garantit que tous les scripts différés ont joué.
  if (window.Documents) boot();
  else document.addEventListener("DOMContentLoaded", boot);
})();
