# AI SmartTalk Documents — le site public

Ce répertoire est la vitrine grand public du moteur `md-to-pdf` : une page d'accueil, une
page par outil, en français et en anglais. Il ne partage rien avec la console
d'intégrateur (`static/index.html`, `static/app.css`, `static/app.js`), qui reste intacte.

- `brand.css` — le système visuel. Il recopie les jetons de `static/app.css` (couleurs,
  rayons, ombres, `--sans`) : les deux surfaces doivent visiblement venir de la même maison.
  Thème sombre par défaut, thème clair sur `:root[data-theme="light"]`.
- `app.js` — le client, générique : une seule implémentation sert toutes les pages d'outil.
  Il envoie les fichiers à `POST /api/files`, appelle l'endpoint déclaré, suit les travaux
  asynchrones (202 → `poll_url` → `GET /api/jobs/{id}`), affiche le verdict, propose le
  téléchargement et l'enchaînement, et rappelle la rétention de 2 h.

## Ce qu'une page d'outil déclare

Sur le conteneur `[data-tool]` : `data-endpoint` (obligatoire), `data-accept`
(types acceptés), `data-file-param` (champ JSON qui reçoit la référence, défaut `pdf`),
`data-multiple` + `data-min-files`/`data-max-files`, `data-output` (`asset` par défaut,
`binary` pour les routes antérieures au socle d'assets), `data-output-name`, `data-max-mb`,
`data-retention-hours`.

Sur chaque réglage : `data-param="nom"` — ou `data-param="options.theme"` pour un champ
imbriqué — et `data-type` parmi `string`, `number`, `boolean`, `numbers`, `strings`, `json`.
Un champ vide est omis du corps, sauf s'il porte `data-required` : le défaut du service
s'applique alors, conformément à la règle du JSON additif.

Les points d'accroche du client sont des `data-role` : `drop` (le `<label>` de dépôt, qui
enveloppe l'`<input type="file" data-role="input">`), `files`, `run`, `status`, `error`,
`result`, `verdict`, `chain` (contenant des `<a data-chain href="…">`), `api-key`,
`theme-toggle`. Ceux qui manquent sont créés ; `drop`, `input` et `run` sont à écrire.

## Ajouter une page

Copier une page existante, changer les attributs ci-dessus, le contenu éditorial et la FAQ,
ajouter la carte dans la grille de l'accueil et le lien dans les deux langues. Aucun
JavaScript à écrire : si un outil demande autre chose, l'ajouter à `app.js` pour tous
plutôt qu'un script de page. Aucune dépendance externe, jamais : le site doit rester
utilisable hors ligne, et aucun nom de concurrent n'apparaît nulle part.
