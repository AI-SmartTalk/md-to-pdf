# Générer des documents depuis les SmartFlows — plan de mise en œuvre

Portée : `md-to-pdf` (moteur) · `aismarttalk` (core, SmartFlows) · `aismarttalk-chat` (portail).

---

## 1. Le constat

Le moteur est très en avance sur ce qu'on en montre. Sept écarts, du plus rentable au plus lourd.

| # | Écart | Où | Gravité |
|---|---|---|---|
| A | Le formulaire du builder expose **3 champs** (contenu, nom, en-tête) là où le moteur en accepte **16** | `GenerationForms.tsx:18-90` vs `generatePdf/params.ts` | ⚠️ le plus rentable |
| B | Le PDF arrive comme **une URL dans du texte**, recopiée par le LLM | `generatePdf/index.ts:39-50` | ⚠️ tue l'effet |
| C | **Aucune charte client.** 3 thèmes sur le disque du moteur, identiques pour tous | `Organization` (aucun champ marque) · `themes/` | ⚠️ bloque le « custom » |
| D | Le **layout score est calculé puis jeté** — la preuve de qualité est produite et perdue | `DocumentEngine.render()` → `layout` jamais lu | moyenne |
| E | Le PDF est un **cul-de-sac** : ni reprise, ni ajustement, ni version | — | moyenne |
| F | **Le portail ignore les SmartFlows** (zéro occurrence dans `aismarttalk-chat`) : aucun retour d'avancement | — | moyenne |
| G | Tout est **synchrone sous 120 s** de silence total | `RENDER_TIMEOUT_MS` · `PDF_RENDER_DEADLINE_SECS` | faible à ce périmètre |

**La bonne nouvelle, décisive pour le chiffrage :** le backend de A est déjà écrit (`readPageSetup` lit
les 16 options, personne ne les saisit), et C se livre **sans toucher une ligne de Rust** — la cascade
du pipeline est `défaut → thème → options → CSS client`, le CSS client a le dernier mot
(`pipeline.rs:247`) **et entre dans la clé de cache** (`pipeline.rs:210-218`). Le core peut donc
compiler une charte en custom properties et l'envoyer dans `css`.

---

## 2. L'expérience cible

Le récit à tenir, du point de vue du salarié :

1. Il demande *« fais-moi le rapport trimestriel »*.
2. Une **carte document apparaît tout de suite** dans le fil : « rédaction… » puis « mise en page… ».
3. La **première page se dessine en vignette** dès qu'elle existe (`/api/preview`, ~1 s), **à la charte
   de son entreprise** — son logo, ses couleurs, sa couverture.
4. Il l'ouvre dans le panneau latéral : les pages défilent.
5. Un bandeau sobre : *« 6 pages · mise en page vérifiée 96/100 »*. Si le score baisse :
   *« un tableau déborde page 3 »* + un bouton **Corriger**.
6. Il tape *« en paysage, sans sommaire »* → nouvelle version, ce qui a bougé lui est montré.
7. Télécharger, ou verser au corpus.

**80 % des briques existent déjà** : `Canvas` (avec `CanvasFileType.PDF` dans l'enum !), `CanvasPanel`,
`MessageAttachments`, `/api/preview`, `/api/diff`, Layout Doctor. Ce qui manque, c'est le câblage.

---

## 3. Les quatre chantiers

### Chantier 1 — Le brand kit d'entreprise *(le « custom ce qu'on veut »)*

Le cœur de la promesse. Chaque entreprise obtient sa charte documentaire ; le SmartFlow dit
simplement « à notre charte » et n'a plus jamais à parler de CSS.

- **Modèle `BrandKit`** rattaché à `Organization` : logo, couleurs (primaire / accent / texte / fond),
  familles typographiques, pied de page, mentions légales, thème de base (`report`, `minimal`, …).
- **`brandKitToCss()`** — un compilateur de ~40 lignes : tokens → custom properties, posées après le
  thème. Rien d'autre à écrire, la cascade du moteur fait le reste.
- **`DocumentEngine.render()`** accepte un `brandKit` et remplit `css` + `cover.logo`.
- **Écran « Charte documentaire »** avec **aperçu en direct** : `POST /api/preview` sur un document
  d'exemple avec le CSS compilé. La charte se voit *avant* d'être enregistrée — c'est un effet whaou
  à lui seul, et ça coûte un appel HTTP.
- **Auto-charte à l'onboarding** *(le vrai moment whaou)* : le core a déjà `analyze-website.ts`.
  On saisit l'URL du site, on extrait logo et couleurs dominantes, on **propose** la charte en 10 s.
  L'utilisateur ajuste au lieu de partir d'une page blanche.

> Zéro modification du moteur Rust. **≈ 2-3 j.**

### Chantier 2 — Le formulaire rattrape le moteur

Passer de 3 à 16 options **sans faire un mur de champs** :

- En tête, un sélecteur **« Modèle de document »** : Rapport · Note · Facture · Contrat ·
  Compte-rendu. Chaque preset pose d'un coup `theme` + `cover` + `toc` + `pageNumbers` + `margins` +
  `autolayout`. Un clic, un document qui a l'air fini.
- Dessous, un dépliant **« Ajuster »** pour le reste (papier, orientation, marges, filigrane,
  numérotation, profondeur du sommaire, graphiques, libellé de caviardage, moteur, pied de page).
- Un bouton **Aperçu** (`DocumentEngine.preview()` à ajouter côté core, le endpoint moteur existe) :
  l'admin voit sa page avec des variables d'exemple. **Aujourd'hui il configure à l'aveugle** et doit
  lancer une vraie conversation pour découvrir le résultat.

> `readPageSetup` lit déjà tout : c'est de l'UI, pas de la logique. **≈ 2 j.**

### Chantier 3 — Le document arrive comme un document

- `generatePdf` crée un **`Canvas`** (`fileType: PDF`, `messageId`, `purpose: ANALYSIS`) au lieu de
  laisser une URL au LLM. Le portail affiche déjà les `Canvas` d'un message via `MessageAttachments` :
  **une fois l'attachment créé, l'affichage est gratuit.**
- Ajouter la **vignette page 1** (`/api/preview` en `png`, mise en cache), le bouton **Télécharger**,
  et l'ouverture dans `CanvasPanel` avec les pages en images.
- **Point à trancher explicitement :** `Canvas.sourceUrl` n'est plus alimentée depuis
  `:fire: plus d'original à télécharger`. Un PDF **que l'assistant fabrique** ne pose pas le problème
  de rétention d'un fichier **que l'utilisateur dépose** — il faut donc rouvrir ce champ pour les
  documents produits, et seulement pour eux.
- **États visibles** pendant le rendu (« rédaction… » / « mise en page… ») : c'est ce qui donne 80 % du
  bénéfice perçu de l'asynchrone pour 10 % de son coût, et ça repousse le besoin du point G.

> **≈ 3-4 j** (core + portail).

### Chantier 4 — Le verdict et la reprise

- Afficher le `LayoutReport` : *« 6 pages · 96/100 »*. Score bas → nommer les défauts en clair
  (« un tableau déborde page 3 »), pas en `kind: overflow`.
- Bouton **Reprendre** : relance avec les ajustements demandés, `/api/diff` montre ce qui a bougé.
- C'est **le différenciant que personne n'a** : un document qui se juge lui-même et le dit.

> **≈ 3 j**, dépend du chantier 3.

---

## 4. Séquencement

| Sprint | Contenu | Ce qui devient vrai |
|---|---|---|
| **1** (≈ 1 sem.) | Chantier 2 + BrandKit (modèle, compilateur, écran, aperçu) | L'admin peut enfin personnaliser — et le voit |
| **2** (≈ 1 sem.) | Chantier 3 | L'utilisateur final voit la différence |
| **3** (≈ 1 sem.) | Chantier 4 + auto-charte depuis le site | Le document se juge, et l'onboarding épate |

**Ne pas commencer par le socle du `ROADMAP.md`.** Les lots 3.1 (clés multiples) et 3.2 (asynchrone)
sont justes, mais à ce périmètre le core est le seul appelant et un rendu courant tient en quelques
secondes. Ils redeviennent nécessaires quand arrivent les rapports longs et les tiers — pas avant.

---

## 5. Landings

### Portail chat — `aismarttalk-chat/src/components/landing/`

Nouvelle section **« Vos documents, à votre charte »**, placée **après `ExperienceSection`, avant
`SovereigntySection`**. Raison : `ExperienceSection` raconte le ressenti, la souveraineté rassure ; une
capacité concrète doit tomber pendant qu'on parle encore de ce que ça *fait*, pas après la gouvernance.

Trois preuves, pas plus :
- **À votre charte, pas à la nôtre** — logo, couleurs, couverture de l'entreprise.
- **Mise en page vérifiée, score à l'appui** — le document se relit lui-même.
- **Le rapport que personne n'a le temps d'écrire** — il sort relié, paginé, avec sommaire.

Visuel : le fil → la carte document → la page rendue. Ajouter `#documents` à `navLinks`
(`LandingPage.tsx:57-64`).

> Toutes les chaînes via `useT()`, clés ajoutées dans translate223 (skill `aismarttalk-i18n`).
> Aucune chaîne en dur, aucun fallback en 3ᵉ argument.

### Service — `md-to-pdf/static/index.html`

Audience distincte : intégrateurs. Tenir la thèse du `ROADMAP.md` — *« le service qui rend un document,
**son verdict et sa preuve** »*. Mettre le **layout score** et l'**aperçu** en héro : c'est précisément
ce qu'aucun concurrent ne montre, et ça se démontre dans la console de test déjà en place.

---

## 6. Ce que je ne recommande pas

- **Un éditeur WYSIWYG de gabarit PDF.** Marché saturé, coût sans fin, hors thèse (déjà tranché au
  `ROADMAP.md` — je le confirme).
- **Laisser le LLM choisir la charte.** Le brand kit est un réglage d'entreprise, pas une décision de
  conversation. Le SmartFlow choisit un *modèle* ; la marque, jamais.
- **Attendre MCP / agent (lots 5-6) pour livrer.** Ces lots sont excellents et invisibles pour
  l'utilisateur final. Ils viennent après, pas avant.
- **Stocker les PDF produits durablement.** `/download` + purge suffit ; devenir une GED attire des
  obligations sans revenu.
