/* ==========================================================================
   md-to-pdf — console de test
   Trois panneaux à défilement indépendant : index des endpoints, requête,
   réponse. La séparation requête / réponse est redimensionnable et sa
   largeur est mémorisée.
   ========================================================================== */
"use strict";

const Console = (() => {

  // ══════════════════════════════════════════════════════════ formulaire

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

  // `force` reconstruit le formulaire même si l'endpoint est déjà à l'écran.
  // Sans lui, revenir de la référence à la console ne doit rien effacer.
  let rendered = null;

  function select(key, force) {
    const ep = BY_KEY[key];
    if (!ep) return;
    if (rendered === key && !force) { showPane("req"); return; }
    state.current = key;
    rendered = key;

    const form = $("#form");
    form.innerHTML = "";
    ep.fields.forEach((f) => renderField(f, form));

    $("#reqMethod").textContent = ep.method;
    $("#reqMethod").className = "method " + ep.method.toLowerCase();
    $("#reqPath").textContent = ep.path;
    $("#reqDesc").textContent = ep.desc;
    $("#docLink").href = "#/api/" + key;

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

    $$("#endpointList .nav-item").forEach((b) => b.classList.toggle("active", b.dataset.key === key));
    $(".console-req .pane-body").scrollTop = 0;
    showPane("req");
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

  // ══════════════════════════════════════════════════════════ construction

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

  // ══════════════════════════════════════════════════════════ envoi

  async function send() {
    const ep = BY_KEY[state.current];
    const values = collectValues();
    const url = apiBase() + (ep.buildPath ? ep.buildPath(values) : ep.path);

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
    showPane("res");

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
    // Access est déclaré dans app.js, chargé après ce fichier : la garde porte
    // sur la liaison lexicale, pas sur window.
    if (res.status === 401 && typeof Access !== "undefined") Access.flag401();
    $("#timing").textContent = elapsed + " ms";
    $("#size").textContent = formatBytes(blob.size);
    $("#ctype").textContent = ctype;

    showTab("preview");
    await renderResponse(blob, ctype, res.ok);
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
      link.href = apiBase() + json.download_url;
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
    showTab("preview");
    showPane("res");
    toast("La requête n'a pas pu être construite", "err");
  }

  // ══════════════════════════════════════════════════════════ PDFs générés

  function rememberPdf(url) {
    if (state.saved.includes(url)) return;
    state.saved.unshift(url);
    state.saved = state.saved.slice(0, 20);
    persist();
    renderSaved();
  }

  function renderSaved() {
    const list = $("#savedList");
    $("#savedCount").textContent = state.saved.length ? state.saved.length : "";

    if (!state.saved.length) {
      list.innerHTML = '<li class="empty">aucun pour le moment</li>';
      return;
    }

    list.innerHTML = "";
    state.saved.forEach((url) => {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.href = apiBase() + url;
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
        select(state.current, true);
      };
      li.append(a, drop);
      list.appendChild(li);
    });
  }

  // ══════════════════════════════════════════════════════════ panneaux

  // Sous 1080px les panneaux s'alternent au lieu de se serrer : on affiche
  // celui qui vient de changer d'état.
  function showPane(name) {
    const split = $("#consoleSplit");
    if (window.innerWidth > 1080 && name !== "nav") return;
    if (window.innerWidth > 860 && name === "nav") return;
    split.dataset.pane = name;
    $$("#consoleSwitch button").forEach((b) => {
      const on = b.dataset.pane === name;
      b.classList.toggle("active", on);
      b.setAttribute("aria-selected", String(on));
    });
  }

  function showTab(name) {
    $("#tabPreview").hidden = name !== "preview";
    $("#tabRaw").hidden = name !== "raw";
    $("#tabCurl").hidden = name !== "curl";
    $$("#resTabs button").forEach((b) => {
      const on = b.dataset.tab === name;
      b.classList.toggle("active", on);
      b.setAttribute("aria-selected", String(on));
    });
  }

  // Poignée de redimensionnement requête / réponse.
  function initGutter() {
    const split = $("#consoleSplit");
    const gutter = $("#gutter");
    const stored = localStorage.getItem("mdpdf.split");
    if (stored) split.style.setProperty("--split", stored);

    let dragging = false;

    const move = (clientX) => {
      const rect = split.getBoundingClientRect();
      const navWidth = $(".console-nav").getBoundingClientRect().width;
      const usable = rect.width - navWidth;
      const ratio = (clientX - rect.left - navWidth) / usable;
      const clamped = Math.min(0.75, Math.max(0.25, ratio));
      split.style.setProperty("--split", (clamped * 100).toFixed(1) + "%");
    };

    gutter.addEventListener("pointerdown", (e) => {
      dragging = true;
      gutter.classList.add("dragging");
      gutter.setPointerCapture(e.pointerId);
      document.body.style.userSelect = "none";
    });
    gutter.addEventListener("pointermove", (e) => { if (dragging) move(e.clientX); });
    const stop = () => {
      if (!dragging) return;
      dragging = false;
      gutter.classList.remove("dragging");
      document.body.style.userSelect = "";
      localStorage.setItem("mdpdf.split", split.style.getPropertyValue("--split"));
    };
    gutter.addEventListener("pointerup", stop);
    gutter.addEventListener("pointercancel", stop);
    gutter.addEventListener("dblclick", () => {
      split.style.removeProperty("--split");
      localStorage.removeItem("mdpdf.split");
    });
  }

  // ══════════════════════════════════════════════════════════ init

  function init() {
    $("#endpointList").innerHTML = GROUPED.map((g) => `
      <div class="nav-group">
        <h4>${escapeHtml(g.title)}</h4>
        ${g.endpoints.map((ep) => `
          <button class="nav-item" data-key="${ep.key}" title="${escapeHtml(ep.method + " " + ep.path)}">
            <span class="m">${ep.method}</span>
            <span class="label">${escapeHtml(ep.key)}</span>
          </button>`).join("")}
      </div>`).join("");

    $$("#endpointList .nav-item").forEach((b) => {
      b.onclick = () => { location.hash = "#/console/" + b.dataset.key; };
    });

    $$("#resTabs button").forEach((b) => (b.onclick = () => showTab(b.dataset.tab)));
    $$("#consoleSwitch button").forEach((b) => {
      b.onclick = () => {
        $("#consoleSplit").dataset.pane = b.dataset.pane;
        $$("#consoleSwitch button").forEach((x) => {
          const on = x === b;
          x.classList.toggle("active", on);
          x.setAttribute("aria-selected", String(on));
        });
      };
    });

    $("#sendBtn").onclick = (e) => { e.preventDefault(); send(); };
    $("#resetBtn").onclick = (e) => { e.preventDefault(); select(state.current, true); toast("Formulaire réinitialisé"); };
    $("#curlBtn").onclick = (e) => {
      e.preventDefault();
      if (!state.lastCurl) { toast("Envoie d'abord une requête pour obtenir le curl équivalent", "err"); return; }
      copyText(state.lastCurl, "Commande curl copiée");
    };

    $("#form").addEventListener("submit", (e) => e.preventDefault());

    initGutter();
    showEmptyResponse();
    renderSaved();
  }

  return { init, select, send, renderSaved };
})();
