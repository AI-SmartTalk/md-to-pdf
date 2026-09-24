/* ==========================================================================
   md-to-pdf — console de test
   Trois panneaux à défilement indépendant : index des endpoints, requête,
   réponse. La séparation requête / réponse est redimensionnable et sa
   largeur est mémorisée.
   ========================================================================== */
"use strict";

const Console = (() => {

  // ══════════════════════════════════════════════════════════ assets

  // Les fichiers déposés par POST /api/files vivent à côté des PDFs générés :
  // même durée de vie qu'une session de travail, même stockage local. `state`
  // vient de ui.js ; on lui ajoute ici ce que seule la console utilise.
  state.assets = JSON.parse(localStorage.getItem("mdpdf.assets") || "[]");

  const persistAssets = () => localStorage.setItem("mdpdf.assets", JSON.stringify(state.assets));

  const IMAGE_KINDS = ["png", "jpeg", "gif", "webp", "tiff"];
  const OFFICE_KINDS = ["docx", "xlsx", "pptx", "odt", "ods", "odp", "ole", "csv"];

  // Ce qu'un sélecteur de source propose. Un outil qui n'accepte pas de PDF ne
  // doit pas en proposer : le 400 arriverait après l'envoi, pas avant.
  function sourceOptions(pick) {
    const out = [];
    if (pick === "pdf") {
      state.saved.forEach((url) => out.push({ value: url, label: url.replace("/download/", "") }));
    }
    state.assets.forEach((asset) => {
      if (pick === "pdf" && asset.kind !== "pdf") return;
      if (pick === "image" && !IMAGE_KINDS.includes(asset.kind)) return;
      if (pick === "office" && !OFFICE_KINDS.includes(asset.kind)) return;
      out.push({ value: "asset://" + asset.id, label: asset.name + " · " + asset.kind });
    });
    return out;
  }

  function fillPicker(select) {
    const pick = select.dataset.pick;
    const chosen = select.value;
    select.innerHTML =
      `<option value="">${escapeHtml(k(pick === "pdf" ? "console.pdfpicker" : "console.assetpicker"))}</option>` +
      sourceOptions(pick)
        .map((o) => `<option value="${escapeHtml(o.value)}">${escapeHtml(o.label)}</option>`)
        .join("");
    select.value = chosen;
  }

  // Un fichier déposé doit être utilisable dans la foulée : les sélecteurs déjà
  // à l'écran sont regarnis sans reconstruire le formulaire, qui effacerait ce
  // que l'utilisateur vient de saisir.
  function refreshPickers() {
    $$("#form .pdf-picker select").forEach(fillPicker);
  }

  function rememberAssets(list) {
    if (!Array.isArray(list)) return 0;
    let added = 0;
    list.forEach((asset) => {
      if (!asset || !asset.id || state.assets.some((a) => a.id === asset.id)) return;
      state.assets.unshift(asset);
      added++;
    });
    if (!added) return 0;
    state.assets = state.assets.slice(0, 20);
    persistAssets();
    renderAssets();
    refreshPickers();
    return added;
  }

  // Tout ce qu'une réponse peut contenir d'asset : le dépôt lui-même, la sortie
  // d'un outil, le résultat d'un travail asynchrone.
  function harvestAssets(json) {
    if (!json || typeof json !== "object") return 0;
    let added = rememberAssets(json.files) + rememberAssets(json.assets);
    if (json.asset) added += rememberAssets([json.asset]);
    if (json.result) added += harvestAssets(json.result);
    return added;
  }

  function renderAssets() {
    const list = $("#assetList");
    if (!list) return;
    $("#assetCount").textContent = state.assets.length ? state.assets.length : "";

    if (!state.assets.length) {
      list.innerHTML = `<li class="empty">${escapeHtml(k("console.assets.empty"))}</li>`;
      return;
    }

    list.innerHTML = "";
    state.assets.forEach((asset) => {
      const li = document.createElement("li");
      const use = document.createElement("a");
      use.href = "#";
      use.textContent = asset.name + " · " + asset.kind + (asset.pages ? " · " + asset.pages + " p." : "");
      use.title = "asset://" + asset.id;
      use.onclick = (e) => {
        e.preventDefault();
        copyText("asset://" + asset.id, k("console.assets.copied"));
      };
      const drop = document.createElement("button");
      drop.textContent = "✕";
      drop.title = k("console.assets.drop");
      drop.onclick = () => {
        state.assets = state.assets.filter((a) => a.id !== asset.id);
        persistAssets();
        renderAssets();
        refreshPickers();
      };
      li.append(use, drop);
      list.appendChild(li);
    });
  }

  // ══════════════════════════════════════════════════════════ formulaire

  const fieldId = (name) => "f_" + name.replace(/[^\w]/g, "_");

  // La valeur par défaut d'un champ peut être une chaîne, une paire {en, fr}
  // ou une fabrique (les documents d'exemple, qui dépendent de la langue).
  const fieldValue = (field) =>
    typeof field.value === "function" ? field.value() : t(field.value);

  function renderField(field, container) {
    if (field.type === "fieldset") {
      const fs = document.createElement("fieldset");
      if (field.collapsed) fs.className = "collapsed";
      const legend = document.createElement("legend");
      const setLabel = () => {
        legend.textContent = (fs.classList.contains("collapsed") ? "▸ " : "▾ ") + t(field.legend);
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
      label.textContent = t(field.label);
      wrap.append(input, label);
      container.appendChild(wrap);
      return;
    }

    const label = document.createElement("label");
    label.htmlFor = fieldId(field.name);
    label.textContent = t(field.label) + (field.required ? " *" : "");
    if (field.hint) {
      const hint = document.createElement("span");
      hint.className = "hint";
      hint.textContent = " — " + t(field.hint);
      label.appendChild(hint);
    }
    wrap.appendChild(label);

    let input;
    if (field.type === "textarea" || field.type === "json" || field.type === "pdflist"
        || field.type === "assetlist" || field.type === "list") {
      input = document.createElement("textarea");
      input.rows = field.rows || 4;
      input.spellcheck = false;
      input.value = fieldValue(field);
      if (field.type === "pdflist") input.placeholder = "/download/demo-client/doc1.pdf";
      if (field.type === "assetlist") input.placeholder = "asset://as_…";
    } else if (field.type === "file") {
      // Le seul endpoint multipart de l'API : sans ce champ, aucun des outils
      // qui consomment un asset n'est essayable depuis la console.
      input = document.createElement("input");
      input.type = "file";
      if (field.multiple) input.multiple = true;
    } else if (field.type === "select" || field.type === "bool") {
      input = document.createElement("select");
      (field.options || ["", "true", "false"]).forEach((opt) => {
        const o = document.createElement("option");
        o.value = opt;
        o.textContent = opt === "" ? k("console.default") : opt;
        input.appendChild(o);
      });
      input.value = fieldValue(field);
    } else {
      input = document.createElement("input");
      input.type = field.type === "number" ? "number" : "text";
      input.spellcheck = false;
      if (field.step) input.step = field.step;
      if (field.placeholder) input.placeholder = field.placeholder;
      input.value = fieldValue(field);
    }

    input.id = fieldId(field.name);
    input.dataset.name = field.name;
    input.dataset.kind = field.type;

    const picks = { pdfpick: "pdf", pdflist: "pdf", assetpick: "any", assetlist: "any" };
    if (picks[field.type]) {
      const multi = field.type === "pdflist" || field.type === "assetlist";
      const picker = document.createElement("div");
      picker.className = "pdf-picker";
      const select = document.createElement("select");
      select.dataset.pick = field.pick || picks[field.type];
      fillPicker(select);
      const add = document.createElement("button");
      add.type = "button";
      add.textContent = multi ? k("console.add") : k("console.use");
      add.onclick = () => {
        if (!select.value) return;
        if (multi) {
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
    $("#reqDesc").textContent = t(ep.desc);
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
        <div>${escapeHtml(k("console.empty"))}</div>
        <div><kbd>⌘</kbd> <kbd>⏎</kbd> ${escapeHtml(k("console.empty.send"))}</div>
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
      if (el.type === "file") {
        // `el.value` d'un champ fichier ne vaut qu'un chemin factice : ce qui
        // s'envoie, ce sont les objets File eux-mêmes.
        values[el.dataset.name] = el.files;
        return;
      }
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
      if (typeof FileList !== "undefined" && raw instanceof FileList) return;

      const spec = findSpec(ep.fields, name);
      const kind = spec ? spec.type : "text";
      let value = raw;

      if (kind === "json") {
        try {
          value = JSON.parse(raw);
        } catch (e) {
          throw new Error(`${k("console.field")} « ${name} » : ${k("console.field.json")} — ${e.message}`);
        }
      } else if (kind === "number") {
        value = Number(raw);
        if (Number.isNaN(value)) throw new Error(`${k("console.field")} « ${name} » : ${k("console.field.number")}`);
      } else if (kind === "pdflist" || kind === "assetlist" || kind === "list") {
        value = String(raw).split("\n").map((s) => s.trim()).filter(Boolean);
        if (!value.length) return;
      } else if (kind === "bool") {
        // Une case à cocher ne sait pas envoyer `false` : buildPayload ignore les
        // valeurs vides. Un drapeau qui n'a de sens qu'à false a donc besoin de trois
        // états — défaut, true, false.
        value = raw === "true";
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

  function buildCurl(ep, url, headers, bodyText, formValues, fileNames) {
    const parts = [`curl -X ${ep.method} '${url}'`];
    Object.entries(headers).forEach(([k, v]) => {
      // la clé n'est jamais recopiée en clair : le presse-papier peut finir n'importe où
      parts.push(`  -H '${k}: ${k === "X-API-Key" ? "$API_KEY" : v}'`);
    });
    if (fileNames && fileNames.length) {
      // Le champ « file » est répétable : c'est ainsi qu'un dépôt multiple se
      // transcrit en curl, et c'est la forme que tout client HTTP sait produire.
      fileNames.forEach((name) => parts.push(`  -F ${JSON.stringify("file=@" + name)}`));
    } else if (formValues) {
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
    let fileNames = null;

    try {
      if (ep.multipart) {
        const fd = new FormData();
        fileNames = [];
        Object.entries(values).forEach(([name, value]) => {
          if (name.startsWith("__")) return;
          if (typeof FileList !== "undefined" && value instanceof FileList) {
            Array.from(value).forEach((file) => {
              fd.append(name, file);
              fileNames.push(file.name);
            });
            return;
          }
          if (value === "" || value === false || value == null) return;
          fd.append(name, value);
        });
        if (!fileNames.length) throw new Error(k("console.file.none"));
        body = fd;
        // Content-Type est laissé au navigateur : la frontière multipart en fait
        // partie, et une valeur écrite à la main la casserait.
        headers = buildHeaders(ep, false);
      } else if (ep.json) {
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

    state.lastCurl = buildCurl(ep, url, headers, bodyText, ep.form && !ep.multipart ? values : null, fileNames);
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
      showError(k("console.error.network") + e.message + k("console.error.network.hint"));
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

  // Juste assez pour que le fichier téléchargé porte un nom qu'un système
  // d'exploitation sache ouvrir.
  const BINARY_EXTENSIONS = {
    "application/pdf": "pdf",
    "application/zip": "zip",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": "docx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": "xlsx",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": "pptx",
    "application/vnd.oasis.opendocument.text": "odt",
    "application/vnd.oasis.opendocument.spreadsheet": "ods",
    "application/vnd.oasis.opendocument.presentation": "odp",
  };

  async function renderResponse(blob, ctype, ok) {
    const preview = $("#tabPreview");
    preview.innerHTML = "";
    if (state.blobUrl) URL.revokeObjectURL(state.blobUrl);
    state.blobUrl = null;

    if (ctype === "application/pdf") {
      state.blobUrl = URL.createObjectURL(blob);
      const frame = document.createElement("iframe");
      frame.className = "preview";
      frame.title = k("console.pdf.aria");
      frame.src = state.blobUrl;
      const dl = document.createElement("a");
      dl.className = "preview-link";
      dl.href = state.blobUrl;
      dl.download = "document.pdf";
      dl.textContent = k("console.download");
      preview.append(frame, dl);
      $("#tabRaw").textContent = `${k("console.binary")}application/pdf — ${formatBytes(blob.size)})`;
      return;
    }

    if (ctype.startsWith("image/")) {
      state.blobUrl = URL.createObjectURL(blob);
      const img = document.createElement("img");
      img.className = "preview";
      img.alt = k("console.png.aria");
      img.src = state.blobUrl;
      preview.appendChild(img);
      $("#tabRaw").textContent = `${k("console.binary")}${ctype} — ${formatBytes(blob.size)})`;
      return;
    }

    // Un docx, un zip ou un octet-stream n'ont pas de visionneuse ici : le lien
    // de téléchargement vaut mieux qu'un aperçu de leurs octets en texte.
    if (ctype && blob.size && !ctype.startsWith("text/") && ctype !== "application/json") {
      state.blobUrl = URL.createObjectURL(blob);
      const dl = document.createElement("a");
      dl.className = "preview-link";
      dl.href = state.blobUrl;
      dl.download = "response." + (BINARY_EXTENSIONS[ctype] || "bin");
      dl.textContent = k("console.download.file") + ctype;
      preview.appendChild(dl);
      $("#tabRaw").textContent = `${k("console.binary")}${ctype} — ${formatBytes(blob.size)})`;
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

    // Un asset déposé ou produit rejoint la liste latérale et les sélecteurs :
    // c'est ce qui rend la chaîne « je dépose, puis j'enchaîne » réellement
    // praticable depuis la console.
    if (ok && json) {
      const added = harvestAssets(json);
      if (added) toast(added + " " + k("console.assets.added"));
    }

    if (ok && json && json.download_url) {
      rememberPdf(json.download_url);
      const link = document.createElement("a");
      link.className = "preview-link";
      link.href = apiBase() + json.download_url;
      link.target = "_blank";
      link.rel = "noopener";
      link.textContent = k("console.open") + json.download_url;
      preview.appendChild(link);
    }
  }

  function showError(message) {
    $("#statusBadge").textContent = k("console.error");
    $("#statusBadge").className = "badge err";
    $("#tabPreview").innerHTML = "";
    const pre = document.createElement("pre");
    pre.textContent = message;
    $("#tabPreview").appendChild(pre);
    $("#tabRaw").textContent = message;
    showTab("preview");
    showPane("res");
    toast(k("console.error.build"), "err");
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
      list.innerHTML = `<li class="empty">${escapeHtml(k("console.saved.empty"))}</li>`;
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
      drop.title = k("console.saved.drop");
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

  function renderGroups() {
    $("#endpointList").innerHTML = GROUPED.map((g) => `
      <div class="nav-group">
        <h4>${escapeHtml(t(g.title))}</h4>
        ${g.endpoints.map((ep) => `
          <button class="nav-item" data-key="${ep.key}" title="${escapeHtml(ep.method + " " + ep.path)}">
            <span class="m">${ep.method}</span>
            <span class="label">${escapeHtml(ep.key)}</span>
          </button>`).join("")}
      </div>`).join("");

    $$("#endpointList .nav-item").forEach((b) => {
      b.onclick = () => { location.hash = "#/console/" + b.dataset.key; };
    });
  }

  function init() {
    renderGroups();

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
    $("#resetBtn").onclick = (e) => { e.preventDefault(); select(state.current, true); toast(k("console.reset.done")); };
    $("#curlBtn").onclick = (e) => {
      e.preventDefault();
      if (!state.lastCurl) { toast(k("console.curl.none"), "err"); return; }
      copyText(state.lastCurl, k("console.curl.copied"));
    };

    $("#form").addEventListener("submit", (e) => e.preventDefault());

    initGutter();
    showEmptyResponse();
    renderSaved();
    renderAssets();
  }

  // Retraduction : la liste des endpoints et le formulaire courant portent des
  // libellés issus de la spec, reconstruits ici plutôt que rechargés.
  function refresh() {
    renderGroups();
    renderSaved();
    renderAssets();
    showEmptyResponse();
    select(state.current, true);
  }

  return { init, refresh, select, send, renderSaved, renderAssets };
})();
