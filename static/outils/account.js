/* ==========================================================================
   AI SmartTalk Documents — inscription et connexion
   Aucune dépendance externe : le service doit rester utilisable hors ligne.

   Deux pages, un seul script. Ce que le formulaire fait est déclaré dans le
   HTML :

     <form data-account="signup|signin" data-next="/app">

   Le cookie de session s'appelle `mdpdf_session`, il est `HttpOnly` : ce
   script ne peut pas le lire et n'a pas à le faire. Il suffit d'appeler les
   endpoints avec `credentials: "same-origin"` — le navigateur pose et renvoie
   le cookie tout seul.
   ========================================================================== */
"use strict";

(function () {
  const form = document.querySelector("form[data-account]");
  if (!form) return;

  const MODE = form.dataset.account === "signup" ? "signup" : "signin";
  const LANG = document.documentElement.lang === "fr" ? "fr" : "en";

  // ═══════════════════════════════════════════════════════ chaînes

  const STRINGS = {
    fr: {
      "title": "La connexion n'a pas abouti",
      "title.signup": "Le compte n'a pas été créé",
      "working": "Vérification…",
      "working.signup": "Création du compte…",
      "email.missing": "Indiquez votre adresse e-mail.",
      "email.invalid": "Cette adresse e-mail ne semble pas valide.",
      "password.missing": "Indiquez votre mot de passe.",
      "password.short": "Le mot de passe doit faire au moins {min} caractères.",
      "network": "Le service est injoignable. Vérifiez votre connexion, puis réessayez.",
      "unexpected": "Le service a répondu de façon inattendue.",
      "throttled": "Trop de tentatives d'affilée. Patientez une minute, puis réessayez.",
    },
    en: {
      "title": "Sign-in did not go through",
      "title.signup": "The account was not created",
      "working": "Checking…",
      "working.signup": "Creating your account…",
      "email.missing": "Enter your e-mail address.",
      "email.invalid": "This does not look like an e-mail address.",
      "password.missing": "Enter your password.",
      "password.short": "The password must be at least {min} characters.",
      "network": "The service is unreachable. Check your connection, then try again.",
      "unexpected": "The service answered in an unexpected way.",
      "throttled": "Too many attempts in a row. Wait a minute, then try again.",
    },
  };

  function t(key, vars) {
    let value = STRINGS[LANG][key] || STRINGS.en[key] || key;
    if (vars) {
      Object.keys(vars).forEach(function (name) {
        value = value.split("{" + name + "}").join(vars[name]);
      });
    }
    return value;
  }

  /* Le serveur répond en anglais, en `{error, details}` : `details` est la phrase
     utile. On la traduit quand on la reconnaît, et on la montre telle quelle
     sinon — une phrase anglaise exacte vaut mieux qu'un « une erreur est
     survenue » français. */
  const KNOWN = [
    {
      match: /already exists for this address/i,
      fr: "Un compte existe déjà pour cette adresse.",
      en: "An account already exists for this address.",
      offer: true, // proposer l'autre formulaire
    },
    {
      match: /does not look like an e-?mail/i,
      fr: "Cette adresse e-mail ne semble pas valide.",
      en: "This does not look like an e-mail address.",
    },
    {
      match: /password must be at least (\d+)/i,
      fr: "Le mot de passe doit faire au moins $1 caractères.",
      en: "The password must be at least $1 characters.",
    },
    {
      match: /wrong e-?mail address or password/i,
      fr: "Adresse e-mail ou mot de passe incorrect.",
      en: "Wrong e-mail address or password.",
      offer: true,
    },
    {
      match: /accounts are not enabled|storage/i,
      fr: "Les comptes ne sont pas disponibles sur ce serveur.",
      en: "Accounts are not available on this server.",
    },
  ];

  function translate(details) {
    if (!details) return null;
    for (let i = 0; i < KNOWN.length; i++) {
      const found = details.match(KNOWN[i].match);
      if (!found) continue;
      const phrase = KNOWN[i][LANG].replace(/\$(\d)/g, function (_, n) {
        return found[Number(n)] || "";
      });
      return { text: phrase, offer: Boolean(KNOWN[i].offer) };
    }
    return { text: details, offer: false };
  }

  // ═══════════════════════════════════════════════════════ destination

  /* `?suite=/outils/compresser-pdf` ramène d'où l'on venait — c'est ce qui rend
     supportable de proposer l'inscription après un premier fichier réussi.
     Un paramètre d'URL est écrit par n'importe qui : seul un chemin de ce site
     est accepté, jamais « //ailleurs.test » ni une URL absolue. */
  function safePath(raw, fallback) {
    if (!raw) return fallback;
    if (raw.charAt(0) !== "/") return fallback;
    if (raw.charAt(1) === "/" || raw.charAt(1) === "\\") return fallback;
    // Espaces, antislashs et caractères de contrôle : aucun n'a sa place dans
    // un chemin honnête, et chacun sert à en déguiser un autre.
    if (/[\s\\]|[\u0000-\u001f\u007f]/.test(raw)) return fallback;
    return raw;
  }

  const SUITE = safePath(
    new URLSearchParams(window.location.search).get("suite"),
    null
  );
  const NEXT = SUITE || form.dataset.next || "/";

  // Le lien vers l'autre formulaire garde la destination : passer de
  // l'inscription à la connexion ne doit pas faire oublier d'où l'on venait.
  if (SUITE) {
    document.querySelectorAll("a[data-carry-suite]").forEach(function (link) {
      link.setAttribute(
        "href",
        link.getAttribute("href") + "?suite=" + encodeURIComponent(SUITE)
      );
    });
  }

  // ═══════════════════════════════════════════════════════ éléments

  const emailField = form.querySelector('input[name="email"]');
  const passwordField = form.querySelector('input[name="password"]');
  const reveal = form.querySelector('[data-role="reveal"]');
  const hint = form.querySelector('[data-role="password-hint"]');
  const alertBox = form.querySelector('[data-role="error"]');
  const alertTitle = form.querySelector('[data-role="error-title"]');
  const alertDetails = form.querySelector('[data-role="error-details"]');
  const alertLink = form.querySelector('[data-role="error-link"]');
  const submit = form.querySelector('[data-role="submit"]');
  const status = form.querySelector('[data-role="status"]');
  const statusText = form.querySelector('[data-role="status-text"]');

  const MIN = Number(passwordField.getAttribute("minlength")) || 10;

  // ═══════════════════════════════════════════════════════ déjà connecté ?

  /* Quelqu'un qui a déjà une session et qui clique « Se connecter » veut son
     espace, pas un formulaire. `replace` plutôt que `assign` : revenir en
     arrière doit ramener à la page précédente, pas rejouer la redirection. */
  fetch("/api/auth/me", {
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  })
    .then(function (res) {
      if (res.ok) window.location.replace(NEXT);
    })
    .catch(function () {
      /* hors ligne, ou comptes non installés : le formulaire reste la réponse */
    });

  // ═══════════════════════════════════════════════════════ le mot de passe

  if (reveal) {
    reveal.addEventListener("change", function () {
      passwordField.type = reveal.checked ? "text" : "password";
    });
  }

  /* Le compteur dit la règle pendant la frappe. Il n'est pas dans une région
     vivante : annoncer un changement à chaque touche noie le lecteur d'écran,
     et `aria-describedby` suffit à faire lire la consigne à la prise de focus. */
  if (hint) {
    passwordField.addEventListener("input", function () {
      // `Array.from` compte les caractères, pas les unités UTF-16 : c'est ce
      // que compte le serveur, et un emoji ne doit pas valoir deux.
      const typed = Array.from(passwordField.value).length;
      if (!typed) {
        hint.textContent = hint.dataset.min;
        return;
      }
      if (typed >= MIN) {
        hint.textContent = hint.dataset.ok;
        return;
      }
      const left = MIN - typed;
      hint.textContent = hint.dataset.left
        .split("{n}")
        .join(String(left))
        .split("{s}")
        .join(left > 1 ? "s" : "");
    });
  }

  // ═══════════════════════════════════════════════════════ messages

  function clearError() {
    alertBox.hidden = true;
    alertDetails.textContent = "";
    if (alertLink) alertLink.hidden = true;
  }

  function showError(message, offer) {
    alertTitle.textContent = t(MODE === "signup" ? "title.signup" : "title");
    alertDetails.textContent = message;
    if (alertLink) alertLink.hidden = !offer;
    alertBox.hidden = false;
    // Le message est dans une région `role="alert"` : il est annoncé seul. Le
    // focus reste dans le formulaire, là où la correction doit se faire.
  }

  function busy(on) {
    submit.disabled = on;
    submit.setAttribute("aria-busy", on ? "true" : "false");
    status.hidden = !on;
    statusText.textContent = on
      ? t(MODE === "signup" ? "working.signup" : "working")
      : "";
  }

  // ═══════════════════════════════════════════════════════ soumission

  form.addEventListener("submit", function (event) {
    event.preventDefault();
    clearError();

    const email = emailField.value.trim();
    const password = passwordField.value;

    if (!email) {
      showError(t("email.missing"), false);
      emailField.focus();
      return;
    }
    // Une adresse se valide en la contactant, pas par une expression régulière :
    // ce test n'attrape que l'oubli manifeste, ce que le serveur refuserait de
    // toute façon — mais un aller-retour plus tôt.
    if (!/^[^\s@]+@[^\s@.]+\.[^\s@]+$/.test(email)) {
      showError(t("email.invalid"), false);
      emailField.focus();
      return;
    }
    if (!password) {
      showError(t("password.missing"), false);
      passwordField.focus();
      return;
    }
    // La longueur n'est exigée qu'à l'inscription : refuser ici le mot de passe
    // court de quelqu'un qui en a déjà un l'empêcherait d'entrer chez lui.
    if (MODE === "signup" && Array.from(password).length < MIN) {
      showError(t("password.short", { min: MIN }), false);
      passwordField.focus();
      return;
    }

    send(email, password);
  });

  function send(email, password) {
    busy(true);

    fetch(MODE === "signup" ? "/api/auth/signup" : "/api/auth/login", {
      method: "POST",
      // Le cookie de session est posé par cette réponse : sans cette ligne il
      // ne serait ni envoyé ni accepté.
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ email: email, password: password }),
    })
      .then(function (res) {
        if (res.ok) {
          window.location.assign(NEXT);
          return null;
        }
        return res
          .json()
          .catch(function () {
            return null;
          })
          .then(function (payload) {
            fail(res.status, payload);
          });
      })
      .catch(function () {
        busy(false);
        showError(t("network"), false);
      });
  }

  function fail(status, payload) {
    busy(false);

    if (status === 429) {
      showError(t("throttled"), false);
      return;
    }

    const known = translate(payload && payload.details);
    if (known) {
      showError(known.text, known.offer);
      return;
    }
    showError(t("unexpected"), false);
  }
})();
