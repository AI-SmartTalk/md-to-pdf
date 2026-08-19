# Métamorphose — du moteur documentaire à la plateforme PDF

Cible assumée : **prendre le marché d'iLovePDF**, sur le grand public *et* sur l'API,
sans casser une seule ligne de l'existant.

Ce document remplace le cadrage produit du `ROADMAP.md` (dont il conserve la thèse
technique) et étend `PLAN-DOCUMENTS-SMARTFLOW.md` (qui reste valable pour l'intégration
AISmartTalk). Il dit aussi, explicitement, **ce que cette ambition invalide** dans les
arbitrages précédents — parce que changer de marché change les coûts, pas seulement les
fonctionnalités.

---

## 1. Ce que nous avons réellement — l'audit, pas la brochure

`v0.2.0`, 15 400 lignes de Rust, 16 routes, 178 tests, déploiement continu sur
`pdf.aismarttalk.tech`. Binaires système : `pandoc`, `weasyprint`, `pdfinfo`, `pdftotext`,
`pdftoppm`, `pdfunite`, `qpdf`.

### Nos forces réelles — et elles sont rares

| Capacité | Où | Ce que ça vaut face au marché |
|---|---|---|
| **Génération** Markdown / HTML / Tera → PDF | `pipeline.rs`, `/api/convert`, `/api/render`, `/api/html-to-pdf` | iLovePDF **ne sait pas produire** un document. Il transforme ce qu'on lui donne. |
| **Layout Doctor** — score /100, issues localisées (bbox), passes correctives | `layout.rs` (1 296 l.) | **Personne ne fait ça.** Ni Adobe, ni iLovePDF, ni Stirling. |
| **Caviardage prouvable** — reconstruction par pixels, entités validées par clé de contrôle | `routes/redact.rs` (1 272 l.) | Le « Censurer PDF » d'iLovePDF pose un rectangle noir. Le nôtre détruit la couche texte. Techniquement supérieur. |
| **Diff pixel avec verdict** | `routes/diff.rs` (786 l.) | Leur « Comparer PDF » est visuel et manuel. Le nôtre rend un verdict machine. |
| **Brand kits versionnés**, immuables, dans la clé de cache | `themes.rs` | Aucun équivalent chez eux. |
| **Blocs natifs** ` ```chart ` / ` ```mermaid ` → SVG | `charts.rs` (2 712 l.), `mermaid.rs` | Aucun équivalent. |
| **CENSOR v2** — régions, niveaux, teaser | `censor.rs` | Aucun équivalent. |
| Cache adressé par contenu, file d'attente, offload, 429 sur saturation | `cache.rs`, `exec.rs` | Socle industriel sain. |
| Anti-SSRF (`urlguard.rs`, 977 l. + `weasyprint-safe.py`) | | Sérieux. Et il va servir. |
| Observabilité log420 + Prometheus + `X-Request-Id` | `obs.rs` | Prêt pour la facturation à l'usage. |

### Le trou structurel, qui n'est pas une fonctionnalité manquante mais un mur

> **Le service ne sait pas recevoir de fichier.**

Aucune route n'accepte d'upload. `Form<ConvertForm>` (legacy `POST /`) est de
l'url-encoded texte. Toutes les routes qui *consomment* un PDF — `/api/merge`,
`/api/watermark`, `/api/protect`, `/api/redact`, `/api/layout`, `/api/diff` — passent par
`helpers::resolve_pdf_path()`, qui n'accepte **qu'un chemin `/download/<client_id>/<nom>`
déjà présent dans `public/pdf`** (`helpers.rs:724`). Autrement dit : elles ne fonctionnent
que sur des PDF **que nous avons nous-mêmes produits**.

Conséquence brutale : sur les ~30 outils d'iLovePDF, nous en couvrons **6**, et **5 des 6
sont inaccessibles à un utilisateur qui arrive avec son fichier**. Pour le marché visé,
notre couverture réelle est de **1 sur 30** (HTML → PDF).

### Les autres manques, par gravité

1. **Pas d'ingestion** (ci-dessus). Bloque 100 % du marché grand public.
2. **Auth mono-clé** (`auth.rs`, 57 lignes, une `API_KEY` statique, comparaison en temps
   constant, c'est tout). Pas de compte, pas d'attribution, pas de quota, pas de
   révocation, pas de facturation. Et `POST /` reste ouvert par conception.
3. **Tout est synchrone** sous `PDF_RENDER_DEADLINE_SECS=120`. Un OCR de 200 pages, une
   conversion LibreOffice ou une compression Ghostscript n'y tiennent pas.
4. **Pas d'OCR.** `/api/redact` sur un scan renvoie `redactions: []`.
5. **Aucune conformité** : ni PDF/A, ni PDF/UA, ni signature, ni horodatage.
6. **Pas de surface agent** (MCP).
7. **Le site public est une console d'intégrateur**, pas un produit. Il dit
   « AI SmartTalk internal service ». Zéro SEO, zéro parcours grand public.

---

## 2. Ce qu'ils ont — iLovePDF, mesuré

**~30 outils**, tous « je dépose un fichier, je récupère un fichier » :

| Famille | Outils |
|---|---|
| Organisation | Fusionner · Diviser · Supprimer des pages · Extraire des pages · Organiser · Scanner vers PDF |
| Optimisation | Compresser · Réparer · **OCR** |
| Vers PDF | JPG · Word · PowerPoint · Excel · HTML |
| Depuis PDF | JPG · Word · PowerPoint · Excel · **PDF/A** |
| Édition | Pivoter · Numéros de page · Filigrane · Rogner · Éditer · **Formulaires** |
| Sécurité | Déverrouiller · Protéger · **Signer** · Censurer · Comparer |
| IA | Résumé · Traduire · **PDF → Markdown** |
| Automatisation | Flux de travail personnalisés (1 gratuit) |

**Distribution** : web + apps desktop macOS/Windows (hors-ligne, traitement par lots) +
mobile iOS/Android + API (iLoveAPI) + intégrations Zapier / Make / WordPress.

**Modèle économique**
- Grand public : freemium. Premium ≈ **4–5 €/mois** en annuel (≈ 50–70 €/an) : taille de
  fichier illimitée, OCR, lots, 10 fichiers simultanés, signatures, flux personnalisés.
  Business ≈ **15 $/utilisateur/mois**.
- API : **2 500 crédits offerts/mois**, puis abonnement ou packs prépayés. Crédits par
  fichier / par page / par tâche : Merge 5, Split 5, Compress 10, OCR **5/page**,
  Résumé **30/page**, Signature **80**.

**Leurs faiblesses, celles qui sont attaquables**

- **Ils ne produisent rien.** Pas de génération depuis des données, pas de charte
  d'entreprise, pas de template. Leur marché s'arrête au fichier existant.
- **Aucun verdict.** On ne sait jamais si le résultat est correct. Compresser peut ruiner
  un scan, on le découvre en l'ouvrant.
- **Aucune preuve.** Rien qui atteste qu'un PDF vient bien d'une source donnée.
- **Tarification opaque.** « 30 crédits par page » pour un résumé est invérifiable a priori.
- **Zéro surface agent.** Pas de MCP. En 2026, c'est un retard, pas un détail.
- **Souveraineté floue.** Argument décisif sur le marché public/santé français et européen.

---

## 3. Le verdict stratégique : deux fossés, pas un

On ne gagne pas ce marché en clonant 30 outils — on arriverait quatrième derrière iLovePDF,
Smallpdf et Stirling PDF, avec dix ans de retard SEO. On ne le gagne pas non plus en
restant sur notre seule différenciation : sans ingestion, **on n'est pas sur le marché du
tout**.

Il faut donc franchir **deux fossés distincts**, et ne pas les confondre :

> **Fossé 1 — le plancher.** Recevoir n'importe quel fichier et faire les ~25 gestes que
> tout le monde fait. C'est un **droit d'entrée**, pas un avantage. Il doit être franchi
> vite, à coût contenu, sans y mettre notre intelligence.
>
> **Fossé 2 — la thèse.** Ce que nous seuls savons faire : **produire** un document,
> le **juger**, et le **prouver**. C'est là que va l'effort, et c'est là que se fait le prix.

La formule tient en une phrase de positionnement :

> **Eux transforment des fichiers. Nous fabriquons des documents — et nous en répondons.**

---

## 4. Le principe de non-régression

Contrainte absolue : **rien ne casse**. Elle se décline en cinq règles tenues sur tout le plan.

1. **`POST /` legacy reste identique**, ouvert, url-encoded, moteur weasyprint. Il sert
   AISmartTalk (`generatePdf.ts`). Il ne bouge pas tant que l'appelant n'a pas migré, et
   il ne se ferme que derrière un flag à défaut permissif.
2. **Les réponses JSON restent additives.** Une requête qui ne demande rien de nouveau
   reçoit exactement ce qu'elle recevait. Les champs nouveaux n'apparaissent que quand ils
   s'appliquent — c'est déjà la discipline en place (`cached`, `layout`, `warnings`).
3. **`resolve_pdf_path()` continue d'accepter `/download/<client_id>/<nom>`.** L'ingestion
   ajoute une *deuxième* forme acceptée, elle n'en remplace aucune.
4. **Aucune route existante ne change de sémantique.** Les nouveaux outils sont de
   nouvelles routes. Les nouveaux paramètres ont des défauts qui reproduisent le
   comportement actuel.
5. **Le cache reste déterministe.** Tout ce qui influence un rendu entre dans la clé ;
   `CACHE_VERSION` est incrémentée à chaque changement de moteur. Aucun LLM dans le chemin
   de rendu par défaut — cette règle du `ROADMAP.md` est confirmée et renforcée.

---

## 5. L'architecture du changement

### 5.1 La notion d'*asset* — la clé de voûte

Tout le plan repose sur une seule abstraction nouvelle :

```
asset := un fichier connu du service, adressé par un id opaque, avec un TTL
```

Un asset naît de trois façons : un **upload** (`POST /api/files`), un **rendu** que l'on
sauvegarde (l'existant), ou la **sortie d'un outil**. Toutes les routes qui consomment un
fichier acceptent alors indifféremment :

- `"/download/<client_id>/<nom>.pdf"` — la forme actuelle, inchangée ;
- `"asset://<id>"` — la forme nouvelle.

`resolve_pdf_path()` devient `resolve_source()` avec les deux branches, l'ancienne
intacte. **Une seule fonction touchée, et par ajout.** C'est ce qui rend les 25 outils du
fossé 1 réalisables en semaines plutôt qu'en trimestres.

### 5.2 Le bac à sable — non négociable

Aujourd'hui, le service ne traite que du contenu qu'il a lui-même généré. Ingérer des
fichiers hostiles est un **modèle de menace entièrement différent** : bombes PDF, zip
bombs, PostScript malveillant (Ghostscript a un historique de CVE d'évasion), macros
LibreOffice, XXE dans les DOCX, polices piégées.

Décision d'architecture : **un conteneur worker distinct**, sans réseau sortant, seccomp,
utilisateur non privilégié, système de fichiers en lecture seule sauf un tmpfs, plafonds
mémoire/CPU/temps par tâche, Ghostscript en `-dSAFER`, LibreOffice sans macros ni réseau,
plafond de taille d'entrée, et bornes sur le nombre de pages. `urlguard.rs` couvre déjà la
moitié du chemin (les URL) ; il faut désormais couvrir les **octets**.

Coût : ~3 j. Non compressible. Un incident ici coûte l'entreprise, pas le sprint.

### 5.3 Rétention et RGPD

Un fichier déposé par un utilisateur est une donnée personnelle. Régime retenu :

- TTL par défaut **2 h** sur les assets uploadés (iLovePDF fait de même), configurable,
  **1 h** en tier gratuit, jusqu'à 30 j en payant sur demande explicite ;
- `DELETE /api/files/{id}` immédiat, et purge par timer (le mécanisme
  `deploy/md-to-pdf-purge.*` existe déjà, il faut l'étendre aux assets) ;
- chiffrement au repos du volume d'assets ;
- pas d'entraînement, pas de lecture humaine, mention explicite. **C'est un argument de
  vente, pas une contrainte** — surtout en France, surtout face à un acteur espagnol dont
  la chaîne de sous-traitance n'est pas lisible.

### 5.4 Travaux asynchrones

`"async": true` sur tout endpoint producteur → `202 {job_id, poll_url}` ;
`GET /api/jobs/{id}` → `queued|running|done|failed` ; `callback_url` optionnel signé HMAC
(passé par `urlguard`). La file et l'offload existent (`exec.rs`) : c'est une couche
d'état, pas une refonte. Sans elle, OCR, LibreOffice et compression sont plafonnés à 120 s.

---

## 6. Les lots

### Lot A — Le socle d'ingestion *(≈ 8-10 j)* — **rien ne part avant**

| # | Contenu |
|---|---|
| A.1 | `POST /api/files` (multipart, plusieurs fichiers) → `{id, type, pages, size, expires_at}`. Détection de type réelle (magic bytes, pas l'extension). Plafonds de taille et de pages par tier. |
| A.2 | `asset://` accepté partout, `resolve_source()` par ajout. `GET/DELETE /api/files/{id}`. |
| A.3 | Le conteneur worker en bac à sable (§5.2). |
| A.4 | Travaux asynchrones + `/api/jobs/{id}` (§5.4). |
| A.5 | Clés multiples : `API_KEY` accepte `nom=valeur,nom2=valeur2` — **une valeur seule reste la clé anonyme d'aujourd'hui**. Le nom entre dans `X-Request-Id`, log420 et `mdtopdf_requests_total{key="…"}`. Quotas par clé, 429 + `Retry-After`. |

**Ce qui devient vrai :** on peut recevoir un fichier, en toute sécurité, et savoir qui l'a
envoyé. Aucun outil de plus, et pourtant c'est le lot le plus important du document.

---

### Lot B — Le plancher : les 25 gestes *(≈ 12-15 j)*

Objectif : **parité fonctionnelle**, atteinte avec le moins d'intelligence possible. La
majorité sont des enveloppes autour d'outils éprouvés. On les livre en série, pas en
soignant chacun.

| Outil | Route | Moyen | Effort |
|---|---|---|---|
| Diviser / Extraire / Supprimer des pages / Organiser | `POST /api/pages` (une route, un verbe `op`) | `qpdf --pages` | 2 j |
| Pivoter | idem | `qpdf --rotate` | ½ j |
| Rogner | `POST /api/crop` | `qpdf` + MediaBox | 1 j |
| **Compresser** | `POST /api/compress` | Ghostscript (`-dSAFER`), 3 niveaux, **avec verdict** (§7.1) | 2 j |
| Réparer | `POST /api/repair` | `qpdf --qdf --replace-input` puis Ghostscript | 1 j |
| Déverrouiller | `POST /api/unlock` | `qpdf --decrypt` (mot de passe fourni **uniquement**) | ½ j |
| Numéros de page sur PDF existant | `POST /api/pages/number` | overlay `qpdf` | 1 j |
| **PDF → JPG/PNG** | `POST /api/rasterize` | `pdftoppm` — **déjà câblé** dans `pdfops.rs:103` | ½ j |
| JPG/PNG → PDF | `POST /api/images-to-pdf` | `img2pdf` | ½ j |
| **Office → PDF** (docx, xlsx, pptx, odt…) | `POST /api/office-to-pdf` | LibreOffice headless, dans le bac à sable | 2 j |
| **PDF → Word/Excel/PowerPoint** | `POST /api/pdf-to-office` | LibreOffice + `pdf2docx` | 2 j |
| **OCR** | `options.ocr: auto\|force\|off` + `POST /api/ocr` | `ocrmypdf` (couche texte **et** PDF/A en un passage) | 2 j |
| **PDF → Markdown** | `POST /api/extract` | `pdftotext -bbox` + notre parseur de tableaux ; OCR en repli | 2 j |
| PDF/A | `options.pdf_variant: pdf/a-3b`, `pdf/ua-1` | WeasyPrint ≥ 62 en natif ; `ocrmypdf`/Ghostscript en conversion | 1 j |
| Filigrane, Protéger, Fusionner, Censurer, Comparer | **existants** | deviennent utilisables sur fichier déposé via `asset://` | 0 j |

> **Décision :** on **ne fait pas** l'éditeur PDF WYSIWYG ni les formulaires interactifs
> dans ce lot. Marché saturé, coût sans fin, hors thèse — l'arbitrage du `ROADMAP.md` tient.
> Les formulaires reviendront éventuellement par le remplissage programmatique
> (`POST /api/forms/fill`, ~2 j), qui est une brique d'API, pas un éditeur.

**Ce qui devient vrai :** on couvre ~26 des 30 outils. On est *dans* le marché.

---

### Lot C — La thèse, rendue visible *(≈ 10-12 j)* — **c'est ici qu'on gagne**

Le lot B nous met à égalité. Le lot C rend l'égalité intenable pour eux.

#### C.1 Le verdict, sur *chaque* outil

Le Layout Doctor ne sert aujourd'hui qu'à nos propres rendus. On l'étend à toute sortie :

- **Compression** → « 4,2 Mo → 780 Ko · texte intact · images à 144 dpi · **lisibilité
  92/100** ». Personne ne dit ça. Tout le monde a déjà ruiné un scan chez un concurrent.
- **OCR** → confiance moyenne, pages douteuses nommées, mots illisibles localisés.
- **Conversion Office** → diff pixel entre la source rendue et le PDF produit : *« 3 pages
  ont bougé, page 7 : un tableau a débordé »*. `routes/diff.rs` existe déjà.
- **Caviardage** → nombre d'occurrences, pages, et la preuve que la couche texte a disparu.

Coût marginal : les briques sont écrites. Valeur : **c'est la seule chose qui fasse
préférer un outil PDF à un autre une fois qu'ils font tous la même chose.**

#### C.2 `POST /api/compose` — le document qui refuse de sortir non conforme

L'appelant pose des **contraintes**, pas une demande de rendu :

```json
{
  "markdown": "...",
  "options": { "theme": "aismarttalk@1" },
  "constraints": { "max_pages": 4, "no_split_tables": true, "min_layout_score": 90 }
}
```

Le service rend, audite, corrige, re-rend, jusqu'à satisfaire le contrat ou épuiser ses
passes — puis répond avec le PDF **et** le journal des passes (`verdict`, `passes`,
`unmet`). Déterministe, zéro LLM, zéro appel sortant.

#### C.3 L'attestation de rendu — le différenciant que personne n'a

Manifeste signé accompagnant chaque PDF : hash de la source, thème + version, moteur +
version, variante PDF, score de layout, caviardages appliqués, horodatage. Retourné en
`X-Document-Attestation`, vérifiable par `POST /api/verify`. **Le hash source est déjà
calculé — c'est la clé de cache.** Coût marginal, promesse unique.

#### C.4 Signature et conformité

`POST /api/sign` : PAdES + horodatage RFC 3161. Avec C.3, on passe de « outil PDF » à
**pièce d'infrastructure de confiance** — le segment où le prix n'est plus de 4 €/mois.

---

### Lot D — La distribution *(≈ 15-20 j)* — sans elle, les lots A-C ne rencontrent personne

C'est le lot le plus étranger à notre culture actuelle, et le plus déterminant sur
l'objectif « manger le marché ».

#### D.1 Le site grand public

Le site actuel dit « AI SmartTalk internal service · Rust · pandoc · WeasyPrint ». Il
s'adresse à des intégrateurs. Il faut un **deuxième site**, pas une refonte du premier :

- **Une page par outil**, adressable, traduite (fr/en/es/de/it/pt/nl). C'est **tout** le
  SEO de ce marché : les 30 pages d'iLovePDF captent l'intégralité de l'intention
  (« compresser pdf », « pdf en word »…). Sans elles, aucun trafic.
- Dépôt par glisser-déposer, traitement, téléchargement. Sans compte pour les outils de base.
- **Le verdict affiché à chaque fois** (C.1) : c'est ce qui différencie visuellement dès
  la première utilisation, et ce qui se raconte.
- L'infrastructure i18n existe (`static/i18n.js`, translate223, guides fr/en).

#### D.2 La surface agent — MCP natif

`GET/POST /mcp` en Model Context Protocol streamable. Outils exposés : `document_render`,
`document_preview` (l'agent **voit** sa page), `document_audit`, `document_compose`,
`document_convert`, `document_ocr`, `document_extract`, `document_redact`, `document_diff`,
`themes_list`. Authentifié par les clés du lot A.5, donc attribué et quota-é.

**Angle mort total du concurrent.** Le jour où quelqu'un tape dans Claude *« convertis ces
12 factures en Excel et dis-moi lesquelles ont mal OCRisé »*, il n'existe aucune réponse
chez iLovePDF. Coût : ~4 j. Ratio différenciation/effort le plus élevé du document.

#### D.3 Comptes, quotas, facturation

Comptes, clés par intégration (le lot A.5 en pose le socle), compteurs d'usage,
Stripe, portail de facturation. ~6 j. Sans cela, il n'y a pas d'offre, seulement un service.

#### D.4 Intégrations

Zapier, Make, n8n, WordPress. Chacune ~1-2 j une fois l'API stable. À faire **après** D.3,
et seulement celles qui remontent du terrain.

> **Hors périmètre pour l'instant : desktop et mobile.** iLovePDF les a, ils coûtent cher,
> et ils ne servent pas la thèse. Le jour où ils manquent vraiment, une app Tauri autour du
> même binaire Rust est un chantier de deux semaines — pas maintenant.

---

### Lot E — L'IA, au seul endroit où elle est légitime *(≈ 8-10 j)*

Règle inchangée : **le LLM propose, le moteur déterministe dispose.**

| Route | Ce que ça fait | Face à eux |
|---|---|---|
| `POST /api/summarize` | Résumé structuré d'un PDF, avec citations de pages | Ils ont l'équivalent, à 30 crédits/page |
| `POST /api/translate` | Traduction **avec re-vérification de mise en page** — l'allemand déborde là où le français tenait, le Layout Doctor rattrape | **Aucun outil de traduction documentaire ne fait ça.** Leur « Traduire PDF » vous rend une page cassée sans le dire. |
| `POST /api/redact/suggest` | Le LLM *propose* les passages sensibles (personnes, montants, clauses, adresses) avec un score ; l'appelant valide ; le caviardage déterministe applique et prouve. **Le LLM ne noircit jamais un pixel.** | Besoin réel et récurrent : RGPD, DPA, appels d'offres, contentieux |
| `POST /api/authoring` | Brief + données JSON → Markdown → PDF, validé par le contrat C.2 | **Hors de leur univers entier.** |
| `POST /api/explain` | Traduit un `LayoutReport` en langage d'action + le CSS prêt à coller | Aucun équivalent |

---

## 7. L'offre

### 7.1 Le principe : un gratuit qui fait mal

iLovePDF monétise la **frustration** — taille de fichier limitée, nombre de tâches limité,
attente entre deux opérations. C'est exactement l'endroit où on les attaque : notre coût
marginal est celui du CPU, pas d'un modèle.

| | **Gratuit** | **Pro** — ~5 €/mois annuel | **Équipe** — ~12 €/utilisateur/mois | **API** |
|---|---|---|---|---|
| Outils du lot B | **Tous, sans limite de tâches** | tous | tous | tous |
| Taille de fichier | 50 Mo | illimitée | illimitée | selon plan |
| Traitement par lots | 3 fichiers | illimité | illimité | illimité |
| **Verdict de qualité** | **oui, toujours** | oui + rapport détaillé | oui | oui |
| Filigrane sur les sorties | **jamais** | jamais | jamais | jamais |
| OCR | 20 pages/mois | 2 000 p./mois | illimité | à la page |
| IA (résumé, traduction) | 10 pages/mois | 500 p./mois | 2 000 p./mois | à la page |
| Brand kits / chartes | — | 1 | illimités | illimités |
| **Attestation + signature** | — | signature | signature + attestation | les deux |
| Rétention | 1 h | 7 j | 30 j | configurable |
| MCP / agents | — | oui | oui | oui |
| Souveraineté (hébergement FR, pas d'entraînement) | oui | oui | oui + DPA | oui + DPA |

**Trois différences de fond avec eux, à marteler :**
1. **Pas de filigrane, jamais**, même en gratuit. C'est la première chose qui se remarque.
2. **Le verdict est gratuit.** C'est notre avantage : le donner renforce la position au
   lieu de la brader — il crée l'attente que les autres ne satisfont pas.
3. **Prix lisible.** Des pages et des fichiers, pas des crédits à 30 unités la page.

### 7.2 Ce qui justifie le prix haut

Pas les outils du lot B — ils sont gratuits partout. Le prix vit dans : **l'attestation**
(C.3), la **signature** (C.4), les **chartes d'entreprise**, le **contrat de document**
(C.2), le **MCP**, la **souveraineté**. Segments où 12 €/utilisateur n'est pas un plafond.

---

## 8. Séquencement

```
Lot A (socle)  ──►  Lot B (plancher)  ──►  Lot C (thèse)  ──►  Lot D (distribution)  ──►  Lot E (IA)
   8-10 j              12-15 j              10-12 j              15-20 j                   8-10 j
```

Total : **≈ 55-70 jours-homme**, soit 3 à 4 mois à une personne, 2 mois à deux.

| Jalon | Après | Ce qui devient vrai |
|---|---|---|
| **J1** | A | Le service reçoit des fichiers, en sécurité, avec attribution et quotas |
| **J2** | A + B | Parité fonctionnelle : ~26 outils sur 30. On existe sur le marché |
| **J3** | + C | Chaque outil rend un verdict. On devient préférable, pas seulement disponible |
| **J4** | + D.1 + D.2 | Le trafic arrive, et les agents aussi |
| **J5** | + D.3 | Il y a une offre, et elle encaisse |
| **J6** | + E | L'IA, mais bordée par le déterminisme |

**Deux non-négociables d'ordonnancement :**
- **A avant tout.** Sans ingestion, le lot B n'a pas d'entrée ; sans bac à sable, le lot B
  est un risque de sécurité ouvert sur Internet.
- **D.1 (les pages SEO) ne peut pas attendre la fin.** Le référencement met 4 à 8 mois à
  mûrir. Publier les pages outil **dès la fin du lot B**, même sobres, fait gagner un
  trimestre sur la seule variable qu'on ne peut pas accélérer avec du code.

**Si un seul chantier devait partir cette semaine :** A.1 + A.2 (ingestion + `asset://`).
Deux jours, et tout le reste devient possible.

---

## 9. Ce que cette ambition invalide dans les plans précédents

Honnêteté sur les arbitrages : viser le grand public **change les prémisses**, et deux
décisions du `ROADMAP.md` ne tiennent plus telles quelles.

| Décision antérieure | Statut | Pourquoi |
|---|---|---|
| « Pas de stockage de documents comme produit » | **Assoupli** | On ne devient pas une GED, mais recevoir des fichiers impose rétention, chiffrement au repos, suppression sur demande, DPA. Ces obligations arrivent **avec le marché**, elles ne sont plus évitables. Le garde-fou devient : *stockage éphémère, jamais une bibliothèque.* |
| « Pas d'éditeur WYSIWYG » | **Maintenu** | Marché saturé, coût sans fin, hors thèse. Ni éditeur PDF, ni éditeur de gabarit. |
| « Pas de LLM dans le chemin de rendu par défaut » | **Renforcé** | Le déterminisme est ce qui rend possibles le cache, le diff **et l'attestation**. L'attestation le rend maintenant contractuel. |
| Lots 3-6 du `ROADMAP.md` | **Absorbés** | 3.1/3.2 → A.4/A.5 · 3.3 → B (OCR) · 4.1 → C.2 · 4.2 → B + C.4 · 4.3 → C.3 · 5.1 → D.2 · 6.x → E. Rien n'est perdu, tout est réordonné autour de l'ingestion. |
| `PLAN-DOCUMENTS-SMARTFLOW.md` | **Toujours valable, en parallèle** | Il sert l'intégration AISmartTalk, qui reste le premier client et le meilleur banc d'essai des brand kits (chantier 1) et du verdict (chantier 4). Ses chantiers 1 et 2 ne coûtent rien au Rust — les mener en parallèle du lot A. |

---

## 10. Risques, nommés

| Risque | Gravité | Traitement |
|---|---|---|
| **Sécurité de l'ingestion** — Ghostscript/LibreOffice sur fichiers hostiles | **critique** | §5.2, non compressible. Bac à sable avant le premier upload public, pas après. |
| **SEO** — 30 pages ne suffisent pas face à 10 ans d'antériorité | élevé | Publier tôt (§8), et différencier le contenu par le verdict, qui donne des pages qu'ils ne peuvent pas écrire |
| **Coût CPU du gratuit** — OCR et Ghostscript sont chers | moyen | Quotas dès le lot A.5, file d'attente déjà en place, dégradation en 429 déjà implémentée |
| **RGPD** — fichiers d'utilisateurs européens | moyen | TTL court, chiffrement au repos, DPA. Transformé en argument de vente (§5.3) |
| **Dispersion** — 30 outils tirent l'équipe loin de la thèse | **élevé** | Le lot B est explicitement livré « sans intelligence ». L'effort va en C et D. Toute demande d'approfondir un outil du plancher se refuse par défaut. |
| **Le legacy `POST /`** — fermé trop tôt, il casse AISmartTalk | moyen | Migrer `generatePdf.ts` vers `/api/convert` + `X-API-Key` **d'abord**, puis flag `PDF_LEGACY_OPEN` à défaut permissif |

---

## 11. La phrase à tenir

> **iLovePDF vous rend un fichier. Nous vous rendons un document, son verdict et sa preuve
> — et nous le fabriquons aussi.**

Tout ce qui ne sert pas cette phrase est du droit d'entrée : à livrer vite, à ne pas aimer.

---
---

# Annexe technique — la spécification d'exécution

Cette annexe est le **contrat d'implémentation**. Elle vaut brief : tout contributeur,
humain ou agent, s'y conforme au mot près. Elle existe parce que la contrainte « ne rien
casser » ne se tient pas à l'intention — elle se tient à la convention.

## A.0 Identité

Le service reste **`md-to-pdf`, le moteur documentaire d'AI SmartTalk**. Le produit public
qui s'appuie dessus s'appelle **AI SmartTalk Documents**. Ni l'un ni l'autre ne se
renomment : le fork est assumé, la filiation avec le projet de Spawnia reste créditée, et
toute page publique porte la marque AI SmartTalk. Aucun nom de concurrent n'apparaît dans
le produit ; la comparaison vit dans ce document, pas dans l'interface.

## A.1 Conventions de code — non négociables

1. **Le code et les commentaires sont en anglais**, comme tout l'existant. Les commentaires
   expliquent **pourquoi**, jamais quoi. Densité de l'existant, pas plus.
2. **Une route = un fichier neuf** sous `src/routes/`. Les fichiers partagés
   (`main.rs`, `types.rs`, `routes/mod.rs`, `Cargo.toml`, `Dockerfile`) sont **câblés en
   intégration**, jamais par le contributeur d'un outil : c'est ce qui permet de travailler
   en parallèle sans conflit.
3. **Aucune nouvelle dépendance Cargo.** L'image de production est auditée ; vingt lignes
   de code valent mieux qu'une crate de plus (cf. `helpers::base64`).
4. Toute route porte le garde `_key: ApiKey`, tout travail bloquant passe par
   `exec::offload`, tout processus externe par `helpers::run_tool` / `run_capture` — jamais
   `Command::output()` en direct, sinon la file d'attente, le budget de temps et les
   métriques ne s'appliquent pas.
5. **Erreurs** : `AppError` uniquement. `BadRequest` pour une entrée invalide, `NotFound`
   pour une référence introuvable, `ProcessFailed` pour un outil qui sort non nul.
6. **Fichiers temporaires** : `tempfile::Builder::new().suffix(".pdf").tempfile()?`, puis
   `.into_temp_path()` pour survivre jusqu'à la livraison.
7. **Tests unitaires obligatoires** dans `#[cfg(test)] mod tests` : parsing, validation,
   cas limites. Pas de test qui exige un binaire externe.
8. **JSON additif** : un corps de requête qui ne mentionne pas un champ nouveau doit se
   comporter exactement comme avant que le champ existe.

## A.2 Le modèle d'asset

```
POST   /api/files          multipart/form-data, champ « file » répétable
       → 201 {"files":[{"id":"as_<32 hex>","name":"contrat.pdf","kind":"pdf",
                        "bytes":184203,"pages":12,"expires_at":"2026-08-19T14:02:11Z"}]}
GET    /api/files/{id}     → les octets, avec le bon Content-Type
DELETE /api/files/{id}     → 204
```

- Stockage : `public/assets/<id>/file.<ext>` + `public/assets/<id>/meta.json`.
- `id` : `as_` suivi de 32 hexa tirés de `/dev/urandom` (pas de crate `uuid`, pas de `rand`).
- **Type détecté aux octets, jamais à l'extension** : `%PDF-`, `\x89PNG`, `\xFF\xD8\xFF`,
  `PK\x03\x04` (OOXML/ODF), `\xD0\xCF\x11\xE0` (OLE), sinon texte.
- TTL par défaut `ASSET_TTL_SECS=7200`, purge paresseuse à chaque accès + balayage.
- Plafonds : `ASSET_MAX_MB` (défaut 100), `ASSET_MAX_PAGES` (défaut 2000).

**La règle qui rend tout le reste possible :**

```rust
// helpers.rs — ajout, jamais un remplacement
pub fn resolve_source(reference: &str) -> Result<PathBuf, AppError>
```

Elle accepte `asset://as_…` **et** `/download/<client_id>/<nom>.pdf`. `resolve_pdf_path()`
reste publique et inchangée : les routes existantes ne bougent pas, elles délèguent.

## A.3 La réponse d'outil

Type nouveau, réservé aux routes nouvelles — les routes existantes gardent
`ConvertResponse` au bit près.

```rust
pub struct ToolResponse {
    download_url: Option<String>,   // quand client_id + pdf_name sont fournis
    asset: Option<AssetRef>,        // quand "output":"asset" — permet le chaînage
    assets: Option<Vec<AssetRef>>,  // split : une entrée par fichier produit
    pages: Option<usize>,
    verdict: Option<Verdict>,       // le différenciant, cf. A.4
    warnings: Option<Vec<String>>,
}
```

Trois formes de sortie, partout les mêmes :
`output` absent → **le binaire** · `client_id` + `pdf_name` → `download_url` ·
`"output":"asset"` → `asset`, pour enchaîner un outil sur le précédent sans repasser
par le réseau.

## A.4 Le verdict — ce que personne d'autre ne rend

```rust
pub struct Verdict { status: String, score: u8, summary: String, checks: Vec<Check> }
pub struct Check { name: String, status: String, detail: String, page: Option<usize> }
```

`status` ∈ `ok | warn | fail`. Chaque outil qui **transforme** un document rend un verdict :

| Outil | Contrôles |
|---|---|
| Compression | pages conservées · texte extractible conservé (ratio de caractères) · dpi image estimé · taux de réduction |
| OCR | confiance moyenne · pages sous seuil · mots illisibles |
| Office → PDF | pages · couverture texte · diff pixel contre le rendu de référence |
| Caviardage | occurrences par page · absence de la chaîne dans la couche texte de sortie |
| Rendu | le `LayoutReport` existant, converti en `Verdict` |

Règle : **un verdict ne bloque jamais une réponse.** Il informe. Seul `/api/compose`
transforme des contraintes en échec explicite.

## A.5 Les routes nouvelles

| Route | Corps | Outil |
|---|---|---|
| `POST /api/pages` | `{pdf, op, pages?, order?, angle?, output?}` — `op` ∈ `split\|extract\|delete\|reorder\|rotate` | `qpdf --pages` / `--rotate` |
| `POST /api/crop` | `{pdf, box:[l,t,r,b] \| "auto", pages?}` | `qpdf` + MediaBox |
| `POST /api/compress` | `{pdf, level: screen\|ebook\|printer\|prepress, verdict?}` | Ghostscript `-dSAFER` |
| `POST /api/repair` | `{pdf}` | `qpdf --qdf` puis Ghostscript |
| `POST /api/unlock` | `{pdf, password}` | `qpdf --decrypt` — **mot de passe fourni uniquement**, aucun cassage |
| `POST /api/pages/number` | `{pdf, format, position, start, from_page}` | overlay WeasyPrint + `qpdf --overlay` |
| `POST /api/rasterize` | `{pdf, pages, dpi, format: png\|jpeg}` | `pdftoppm` (déjà câblé) |
| `POST /api/images-to-pdf` | `{images:[ref], paper_size, fit, margin}` | HTML + WeasyPrint |
| `POST /api/office-to-pdf` | `{file}` | LibreOffice headless |
| `POST /api/pdf-to-office` | `{pdf, to: docx\|xlsx\|pptx}` | LibreOffice |
| `POST /api/ocr` | `{pdf, languages, mode: auto\|force\|off, pdfa?}` | `ocrmypdf` |
| `POST /api/extract` | `{pdf, format: markdown\|text\|json, ocr?}` | `pdftotext -bbox` + parseur maison |
| `POST /api/pdfa` | `{pdf, variant: pdf/a-2b\|pdf/a-3b}` | Ghostscript / `ocrmypdf` |
| `POST /api/compose` | `{markdown, options, constraints}` | cf. §C.2 |
| `POST /api/attest` · `POST /api/verify` | `{pdf}` / `{pdf, attestation}` | cf. §C.3 |
| `GET/POST /mcp` | JSON-RPC 2.0 | cf. §D.2 |
| `GET /api/jobs/{id}` | — | travaux asynchrones |

Et, sans changer une signature : **toutes les routes existantes qui consomment un PDF
acceptent désormais `asset://`**, parce qu'elles passent par `resolve_source`.

## A.6 Le bac à sable

Chaque outil nouveau tourne sur des octets hostiles. Règles appliquées dans le code, pas
seulement dans l'infrastructure :

- Ghostscript **toujours** avec `-dSAFER -dBATCH -dNOPAUSE -dNOOUTERSAVE`, jamais de
  `-dNOSAFER`, et un `-sOutputFile` en fichier temporaire — jamais un chemin d'entrée.
- LibreOffice avec un `-env:UserInstallation` dédié et jetable, `--headless --norestore
  --nolockcheck --nodefault`, macros désactivées, et pas de réseau.
- `ocrmypdf` avec `--jobs 1` et un plafond de pages.
- Toute entrée est bornée en taille **et** en pages avant d'atteindre un outil.
- Le budget de temps global (`helpers::Budget`) s'applique déjà : ne jamais le contourner.

## A.7 Vérification

Boucle courte, dans le conteneur de développement :

```bash
docker compose exec -T rust cargo check --all-targets   # ~20 s à chaud
docker compose exec -T rust cargo test
docker compose exec -T rust cargo clippy -- -D warnings
./test_api.sh http://127.0.0.1:8000                     # suite d'intégration
```

Une route n'est **livrée** que lorsque : elle compile, ses tests passent, `test_api.sh`
la couvre, le README et `static/swagger.yaml` la décrivent, et la console de test l'appelle
pour de vrai.

---
---

# Journal de livraison — ce qui a été construit

Écrit au terme de la session d'implémentation. Il décrit **l'état réel**, pas l'intention.

## Ce qui existe maintenant

| Lot | État | Preuve |
|---|---|---|
| **A — socle d'ingestion** | livré | `POST /api/files` (type lu aux octets), `asset://` accepté partout, `GET/DELETE /api/files/{id}`, travaux asynchrones (`POST /api/jobs` → 202, `GET /api/jobs/{id}`), clés nommées + quotas + attribution dans les logs et les métriques |
| **B — le plancher** | livré | 13 routes : `pages` (5 opérations), `pages/number`, `crop`, `compress`, `repair`, `unlock`, `rasterize`, `images-to-pdf`, `office-to-pdf`, `pdf-to-office`, `ocr`, `extract`, `pdfa` |
| **C — contrat et preuve** | livré | `POST /api/compose` (contraintes → verdict `met`/`unmet` + journal des passes), `POST /api/attest` / `POST /api/verify` (`valid`/`altered`/`forged`/`unreadable`), verdict sur chaque outil transformant |
| **D — distribution** | livré | `GET/POST /mcp` (12 outils, `document_preview` rend de vraies images), 42 pages publiques bilingues rendues par Tera, `sitemap.xml`, `robots.txt`, palier gratuit `PUBLIC_TOOLS` |
| **E — l'IA** | **non fait** | Aucune ligne. C'était le dernier lot du plan et il le reste. |

**Non-régression tenue.** `pipeline.rs` et `cache.rs` : `git diff` vide. Les routes
antérieures n'ont changé que par ajout (`resolve_pdf_source`, garde `PublicOrKey`, champ
`output` optionnel). Vérifié en exécution : `POST /` rend le même PDF et le même
`{"download_url": …}`, `/api/watermark` rend le même JSON au bit près.

## Ce qui a été vérifié, et comment

Rien de ce tableau n'est une affirmation de principe : chaque ligne a été exécutée.

| Vérification | Résultat |
|---|---|
| `cargo check --all-targets` · `cargo clippy` (`-D warnings`) · `cargo fmt --check` | propres, **0 avertissement** |
| `cargo test` | **490 tests**, 0 échec (178 au début de la session) |
| `./test_api.sh` sur l'instance de développement, déploiement fermé | **181 tests d'intégration**, 0 échec |
| `./test_api.sh` sur l'**image de production**, palier gratuit allumé, conteneur durci | **181 tests**, 0 échec |
| Image de production reconstruite depuis le `Dockerfile` | construite, lancée, `capabilities` complètes (`compress`, `pdfa`, `ocr`, `office`, `images-to-pdf`, `pages`) |
| SSRF LibreOffice (B1) | rejoué : `OPTIONS /leak` **émis** sans la garde, **aucune requête** avec — et la conversion réussit toujours |
| Réparation d'un PDF chiffré (B2) | avant : page blanche en `verdict: ok`. Après : **400** qui nomme la cause et renvoie vers `/api/unlock` |
| Déverrouillage d'un PDF non chiffré (S1) | avant : « Encryption removed from 3 pages ». Après : `warn — this document was not encrypted` |
| Propriété des fichiers | `alpha` lit les siens (200), `beta` obtient **404** en lecture comme en suppression, sur les fichiers **déposés** comme sur ceux **produits** |
| Balayeur d'assets (B4) | observé : dépôt à T, `Asset store: 1 expired uploads swept` au tick suivant, répertoire disparu du disque |
| Non-régression | `POST /` rend le même PDF ; `/api/watermark`, `/api/merge`, `/api/protect` rendent le même JSON au bit près malgré leur nouveau champ |
| 42 pages publiques (21 fr + 21 en) | toutes en 200, rendues et regardées dans un navigateur |
| MCP | `initialize`, `tools/list` (12 outils), `tools/call` — `document_preview` renvoie une vraie image, méthode inconnue → `-32601` |

## Ce que la revue adverse a trouvé

`REVUE-LOT-B.md` a audité les treize routes en **rejouant** les scénarios, pas en lisant.
Elle a trouvé quatre constats bloquants, dix sérieux, neuf mineurs — dont un **SSRF prouvé**
via LibreOffice et deux verdicts qui mentaient. Ils ont été corrigés ; le rapport est
conservé au dépôt parce qu'il documente ce qui a été vérifié, et comment.

Ce document est le seul endroit du dépôt où l'on trouve la preuve qu'un `/api/repair` peut
rendre une page blanche en criant victoire. Le supprimer une fois le bug corrigé serait
perdre la raison pour laquelle le test qui l'attrape existe.

## Les trois décisions à prendre avant d'ouvrir au public

Elles ne sont pas techniques, elles sont vôtres :

1. **`PUBLIC_TOOLS=true` ouvre l'ingestion à Internet.** Le durcissement en place
   (capacités retirées, `no-new-privileges`, `/tmp` en tmpfs, environnement des enfants
   assaini, LibreOffice sans réseau) est un plancher — **pas** le conteneur worker du §5.2.
   Le plan disait « non compressible » ; il l'est resté.
2. **La racine du service reste la console d'intégrateur.** Le site public vit sous
   `/outils` et `/tools`. Pour capter le trafic, c'est la racine qu'il faudra lui donner —
   décision de produit, pas de code.
3. **Le lot E (résumé, traduction, caviardage suggéré, rédaction) n'est pas commencé.**
   C'est le seul endroit du plan où un LLM est légitime, et il reste borné par la même règle :
   le LLM propose, le moteur déterministe dispose.
