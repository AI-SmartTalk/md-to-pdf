# Roadmap — du convertisseur au moteur documentaire

## Où nous en sommes

`v0.2.0`, branche `feature/document-engine-lot1-lot2`. 15 400 lignes de Rust, 178 tests
unitaires, déployé en continu sur `master`.

**Acquis solides**

| Brique | État |
|---|---|
| Pipeline de rendu unique (`src/pipeline.rs`) | Un seul chemin, donc pas de dérive entre endpoints |
| Cache adressé par contenu + file d'attente + offload | `CACHE_VERSION`, LRU, TTL, 429 sur saturation |
| Moteurs | weasyprint (défaut, gardé), wkhtmltopdf, pdflatex |
| Brand kits versionnés | `themes/<nom>/v<n>/`, immuables, dans la clé de cache |
| Blocs natifs | ` ```chart `, ` ```mermaid ` → SVG inline |
| CENSOR v2 | régions, niveaux, teaser |
| Caviardage prouvable | `/api/redact`, reconstruction par pixels, entités validées par clé de contrôle |
| Layout Doctor | score 0-100, issues localisées (bbox), passes correctives |
| Aperçu / diff | multi-pages, contact sheet, diff pixel avec verdict |
| Sécurité | anti-SSRF (`urlguard` + `weasyprint-safe.py`), politique d'hôtes, garde-fous fetch |
| Observabilité | log420, `X-Request-Id`, Prometheus |

**Ce qui manque, par ordre de gravité**

1. **Auth mono-clé.** Une seule `API_KEY` statique, pas d'attribution, pas de quota, pas de
   révocation. L'UI promet un token par intégration : écart connu. Et `POST /` reste ouvert.
   → bloque toute ouverture à des tiers et toute facturation.
2. **Tout est synchrone**, sous `PDF_RENDER_DEADLINE_SECS=120`. Aucune place pour l'OCR, un
   LLM, ou un batch de 200 pages. → plafond de verre sur tout ce qui suit.
3. **Pas d'OCR.** `/api/redact` sur un scan renvoie `redactions: []`. Promesse cassée,
   documentée mais cassée.
4. **Aucune conformité.** Ni PDF/A, ni PDF/UA, ni signature, ni horodatage. → exclut
   l'archivage légal, le secteur public (European Accessibility Act) et le contractuel.
5. **Aucune surface agent.** Un LLM qui veut produire un document doit écrire de la glue HTTP.

---

## La thèse

Ce service sait déjà faire une chose que presque aucun concurrent ne fait : **il s'auto-inspecte**.
Layout Doctor rend un score et des issues localisées, `/api/preview` rend des pixels, `/api/diff`
rend un verdict. C'est exactement la boucle de rétroaction qui manque à un LLM : il écrit du
Markdown à l'aveugle et ne sait pas que son tableau est coupé en deux.

Le positionnement à tenir n'est donc pas « encore une API PDF », mais :

> **Le service qui rend un document, son verdict et sa preuve.**

Trois promesses, trois lots.

---

## Lot 3 — Le socle qui débloque tout (≈ 5-7 j)

Rien de spectaculaire, mais tout le reste en dépend.

### 3.1 Clés multiples, attribution, quotas
- `API_KEY` accepte `nom=valeur,nom2=valeur2`. Rétro-compatible : une valeur seule reste
  la clé anonyme d'aujourd'hui.
- Le nom de la clé entre dans `X-Request-Id`, les logs log420 et le label des métriques
  (`mdtopdf_requests_total{key="…"}`) → on sait enfin **qui** consomme quoi.
- Quota par clé (rendus/minute, Mo/jour), 429 avec `Retry-After`.
- Migrer `generatePdf.ts` d'AISmartTalk vers `POST /api/convert` + `X-API-Key`, **puis**
  fermer `POST /` derrière un flag `PDF_LEGACY_OPEN=false` (défaut `true` une version, puis bascule).

### 3.2 Travaux asynchrones
- Tout endpoint producteur accepte `"async": true` → `202 {job_id, poll_url}`.
- `GET /api/jobs/{id}` → `queued|running|done|failed` + résultat.
- `"callback_url"` optionnel : POST signé HMAC à la fin (l'URL passe par `urlguard`).
- La file et l'offload existent déjà — c'est une couche de state, pas une refonte.

### 3.3 OCR
- `tesseract-ocr` (+ `fra`, `eng`) dans l'image, exposé via `"ocr": "auto" | "force" | "off"`.
- Débloque `/api/redact` sur les scans, et ouvre `/api/extract` (texte + tableaux + entités
  d'un PDF entrant) — la brique dont un agent a besoin pour *lire* avant d'écrire.

---

## Lot 4 — Le contrat de document (≈ 6-8 j) — *l'effet whaou n°1*

### 4.1 `POST /api/compose` — le document qui refuse de sortir non conforme

L'appelant ne demande plus un rendu, il pose des **contraintes** :

```json
{
  "markdown": "...",
  "options": { "theme": "aismarttalk@1" },
  "constraints": {
    "max_pages": 4,
    "no_split_tables": true,
    "no_orphan_headings": true,
    "min_layout_score": 90
  }
}
```

Le service rend, audite avec le Layout Doctor, applique ses correctifs, re-rend — jusqu'à
satisfaire le contrat ou épuiser ses passes. Il répond avec le PDF **et** le journal :

```json
{
  "download_url": "...",
  "verdict": "met",
  "passes": [
    {"n": 1, "score": 71, "applied": ["table-scale:0.92 p.3"]},
    {"n": 2, "score": 94, "applied": ["break-before:h2 p.2"]}
  ],
  "unmet": []
}
```

Entièrement déterministe, zéro LLM, zéro appel sortant. C'est `autolayout` élevé au rang de
contrat — et c'est ce qui transforme « j'espère que c'est joli » en garantie vérifiable.

### 4.2 Conformité
- `options.pdf_variant`: `pdf/a-3b` (archivage légal) et `pdf/ua-1` (accessibilité).
  WeasyPrint ≥ 62 les produit nativement : coût faible, valeur contractuelle forte.
- `POST /api/sign` : signature PAdES + horodatage RFC 3161 sur un PDF sauvegardé.

### 4.3 Attestation de rendu — *le différenciant que personne n'a*

Chaque PDF produit peut être accompagné d'un manifeste : hash de la source, thème + version,
moteur + version, variante PDF, score de layout, caviardages appliqués, horodatage. Signé,
retourné en `X-Document-Attestation` et vérifiable par `POST /api/verify`.

Le hash de source est **déjà calculé** — c'est la clé de cache. Le coût est marginal, et la
promesse est unique : *« prouvez-moi que ce contrat PDF vient bien de cette source, avec cette
charte, sans altération. »* C'est le seul angle qui fait passer un service PDF de commodité à
pièce d'infrastructure de confiance.

---

## Lot 5 — La surface agent (≈ 6-8 j) — *l'effet whaou n°2*

### 5.1 Serveur MCP natif — `GET/POST /mcp`

Le service s'expose en Model Context Protocol (HTTP streamable). Claude Code, Claude Desktop,
n'importe quel agent le découvre et l'utilise **sans une ligne de glue**. Outils exposés :

`document_render` · `document_preview` (renvoie les PNG → l'agent *voit* sa page) ·
`document_audit` · `document_compose` · `document_redact` · `document_diff` · `document_extract`
· `themes_list`

Authentifié par la clé du lot 3.1, donc attribué et quota-é. C'est le vecteur d'adoption : le
jour où quelqu'un tape dans Claude *« fais-moi le rapport mensuel en PDF à la charte
AISmartTalk »* et que ça sort relié, paginé, avec sommaire et couverture — la démo est faite.

### 5.2 `POST /api/agent/session` — la boucle voir/corriger

Une session tient un document vivant : l'agent envoie une révision, reçoit le rendu, les pages
en PNG et le rapport de layout, corrige, renvoie. Le diff entre révisions est gratuit
(`/api/diff` existe). Ce qui coûte cher aujourd'hui — re-poster 40 Ko de Markdown à chaque
itération — devient un delta.

### 5.3 `POST /api/explain`

Traduit un `LayoutReport` en langage d'action : `orphan-heading p.3` → *« le titre "Résultats"
est seul en bas de page ; ajoutez `break-before: page` »* + le CSS prêt à coller. Une seule
route, un LLM court, immédiatement utile aux humains comme aux agents.

---

## Lot 6 — L'intelligence, au seul endroit où elle est légitime (≈ 5-7 j)

Règle de conception : **le LLM propose, le moteur déterministe dispose.** Jamais l'inverse.

### 6.1 `POST /api/redact/suggest`
Le caviardage actuel exige de connaître les chaînes littérales — c'est sa limite pratique. Le
LLM lit le texte extrait et *propose* les passages sensibles (clauses de confidentialité, noms
de personnes, montants, adresses) avec un score de confiance. L'appelant valide, puis le
caviardage déterministe existant applique et prouve. Le LLM ne noircit jamais un pixel.

Besoin réel et récurrent : RGPD, DPA, réponses à appels d'offres, pièces de contentieux.

### 6.2 `POST /api/authoring`
Brief + données JSON → Markdown structuré → PDF, validé par le contrat du lot 4.1. Un jeu de
données brut ressort en rapport de douze pages, avec couverture, sommaire, graphiques natifs et
pagination propre. Le LLM écrit, le moteur garantit la forme.

### 6.3 `POST /api/translate`
Traduction d'un document en préservant la structure — et **en re-vérifiant la mise en page** :
l'allemand déborde là où le français tenait. Le Layout Doctor rattrape, ce qu'aucun outil de
traduction documentaire ne fait.

---

## Ordre recommandé

```
Lot 3 (socle)  ──►  Lot 4 (contrat + preuve)  ──►  Lot 5 (MCP + agent)  ──►  Lot 6 (LLM)
   5-7 j                    6-8 j                        6-8 j                   5-7 j
```

Ne pas inverser 3 et le reste : sans clés attribuées ni jobs asynchrones, tout ce qui suit est
soit infacturable, soit condamné au 504.

**Si un seul lot doit partir en premier : 3.1 + 3.2, puis 4.1.** Le contrat de document est ce
qui rend le service défendable ; le MCP est ce qui le rend adoptable.

## Ce que je ne recommande pas

- **Un éditeur WYSIWYG.** Marché saturé, coût sans fin, et hors de la thèse.
- **Le stockage de documents comme produit.** `/download` + purge suffit ; devenir une GED
  attire des obligations (rétention, RGPD, chiffrement au repos) sans revenu associé.
- **Un LLM dans le chemin de rendu par défaut.** Il détruirait le déterminisme dont dépendent
  le cache, le diff et l'attestation. Il reste sur des routes distinctes, explicitement appelées.
