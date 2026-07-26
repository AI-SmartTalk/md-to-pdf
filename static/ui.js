/* ==========================================================================
   md-to-pdf — socle d'interface
   Sélecteurs, échappement, coloration syntaxique maison, état persistant,
   thème, toasts et boutons « copier ». Aucune dépendance externe.
   ========================================================================== */
"use strict";

const $ = (sel, root) => (root || document).querySelector(sel);
const $$ = (sel, root) => Array.from((root || document).querySelectorAll(sel));

const escapeHtml = (s) =>
  String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

const pretty = (obj) => JSON.stringify(obj, null, 2);

function formatBytes(n) {
  if (n < 1024) return n + " o";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " Ko";
  return (n / (1024 * 1024)).toFixed(1) + " Mo";
}

// ══════════════════════════════════════════════════════════ état persistant

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

const apiBase = () => state.baseUrl.replace(/\/+$/, "");

// ══════════════════════════════════════════════════════════ coloration

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

// ══════════════════════════════════════════════════════════ toasts

function toast(message, kind) {
  const host = $("#toasts");
  if (!host) return;
  const el = document.createElement("div");
  el.className = "toast" + (kind ? " " + kind : "");
  el.textContent = message;
  host.appendChild(el);
  setTimeout(() => {
    el.classList.add("leaving");
    setTimeout(() => el.remove(), 220);
  }, 2400);
}

function copyText(text, okMessage) {
  return navigator.clipboard.writeText(text).then(
    () => toast(okMessage || "Copié dans le presse-papier"),
    () => toast("Copie impossible — le presse-papier est bloqué", "err")
  );
}

// ══════════════════════════════════════════════════════════ thème

const MOON = "M12 3a9 9 0 109 9 7 7 0 01-9-9z";
const SUN = "M12 7a5 5 0 100 10 5 5 0 000-10zm0-6v3m0 16v3M4.2 4.2l2.1 2.1m11.4 11.4l2.1 2.1M1 12h3m16 0h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1";

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("mdpdf.theme", theme);
  const icon = $("#themeIcon");
  if (!icon) return;
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

// ══════════════════════════════════════════════════════════ copier

// Délégation : les blocs de code sont rendus après coup, un écouteur global
// évite d'avoir à recâbler chaque bouton à chaque rendu.
function initCopyButtons() {
  document.addEventListener("click", (e) => {
    const btn = e.target.closest(".copy-btn");
    if (!btn) return;
    const target = $(btn.dataset.copy);
    if (!target) return;
    copyText(target.dataset.raw || target.textContent).then(() => {
      const old = btn.textContent;
      btn.textContent = "copié ✓";
      setTimeout(() => (btn.textContent = old), 1400);
    });
  });
}
