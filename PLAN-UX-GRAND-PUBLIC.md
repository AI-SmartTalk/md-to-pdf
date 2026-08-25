# Du site d'ingénieurs au produit grand public

Plan UX/UI pour **AI SmartTalk Documents**. Écrit après navigation réelle du site et
analyse des leaders. Il ne remet pas en cause le moteur ni la thèse : il change à qui on
parle, et dans quel ordre.

---

## 1. Le diagnostic, en une phrase

> Le design est bon. La **posture** est celle d'un service interne : on explique avant de
> laisser faire, on nomme les choses comme un ingénieur, et la première action utile est à
> deux clics.

Un particulier qui arrive ne veut pas comprendre notre thèse. Il veut **déposer un fichier
et récupérer un fichier**. Tout ce qui se met entre les deux est une perte.

---

## 2. Ce que j'ai vu, en naviguant

| # | Constat | Pourquoi c'est bloquant |
|---|---|---|
| **1** | **La racine est la console d'intégrateur.** « SERVICE INTERNE AI SMARTTALK · RUST · PANDOC · WEASYPRINT », « 38 endpoints », `POST /api/convert` | Le visiteur qui tape le domaine tombe sur une page qui lui dit qu'il n'est pas le bienvenu. C'est le défaut n°1 et il annule tous les autres efforts. |
| **2** | **Aucune icône, nulle part.** 21 cartes de texte gris | On ne *scanne* pas 21 blocs de prose. Chez les leaders, chaque outil a une icône colorée : on trouve « compresser » en 300 ms, sans lire. |
| **3** | **Thème sombre par défaut** | iLovePDF, Smallpdf, Adobe : tous en clair. Le sombre signale « outil de développeur ». Le blanc signale « simple et sûr » — sur des documents personnels, ça compte. |
| **4** | **Le héros ne permet pas d'agir.** Un titre, cinq cartes de texte, aucune zone de dépôt | Il faut choisir un outil *puis* déposer. Deux étapes avant la première valeur, là où les leaders en ont une. |
| **5** | **Vocabulaire d'ingénieur** : « verdict », « la couche texte n'existe plus », « AES 256 bits », « PDF/A-1b, 2b ou 3b », « hébergement souverain », « **Océriser** » | « Océriser » n'est pas un mot connu. « Couche texte » non plus. On demande au visiteur d'apprendre notre vocabulaire pour utiliser un outil gratuit. |
| **6** | **Le badge « avec verdict »** sur chaque carte | C'est du jargon interne exposé au public. Personne ne sait ce que c'est **avant** d'avoir lu le paragraphe qui l'explique. |
| **7** | **Le panneau « LE VERDICT » de la page outil fait huit lignes de prose** | Notre meilleur argument est présenté sous la forme la moins lisible qui soit : un pavé. |
| **8** | **Le bouton d'action est petit et pâle** | Chez iLovePDF, le CTA est un rectangle rouge qu'on ne peut pas manquer. Le nôtre se confond avec la page. |
| **9** | **Zéro preuve sociale.** Pas de note, pas de compteur, pas de certification, pas d'avis | Déposer un document personnel sur un site inconnu demande de la confiance. On n'en offre aucune. |
| **10** | **Pas de « Se connecter » ni de tarifs** | Aucun entonnoir. Un visiteur satisfait n'a nulle part où aller. |
| **11** | **Trois entrées de navigation pour 21 outils** | Il faut un menu par catégorie. Les catégories existent déjà dans le catalogue, elles ne sont pas exposées. |
| **12** | **Titres de cartes verbeux** : « Réparer un PDF endommagé », « Océriser un PDF scanné » | Les leaders disent « Réparer PDF », « OCR PDF ». Court = scannable. |

---

## 3. Ce que font les leaders, avec les chiffres

- **iLovePDF : 188 M de visites/mois, 71 % d'organique.** Il domine « pdf to word »
  (7,3 M de recherches/mois) et « jpg to pdf » (5,1 M). ([SEMrush](https://www.semrush.com/website/ilovepdf.com/overview/), [TechList](https://techlist.ai/ilovepdf.com))
  → **Le point d'entrée n'est pas l'accueil, c'est la page d'outil.** C'est elle qu'il faut
  soigner en premier.
- **Smallpdf** ouvre sur *« We make PDF easy. »* et affiche : plus d'un milliard
  d'utilisateurs, ISO/IEC 27001, RGPD, notes Capterra/G2/Trustpilot, support 24/7.
  ([Smallpdf](https://smallpdf.com/)) Son UI est jugée « la plus soignée du marché », celle
  d'iLovePDF « datée ». ([comparatif 2026](https://www.pdftechno.com/blogs/ilovepdf-vs-smallpdf-vs-pdftechno-which-one-makes-the-most-sense))
  → **Notre design peut être meilleur que les deux.** C'est notre seul avantage déjà acquis.
- **Le mobile dépasse la moitié du trafic web mondial**, et les deux leaders ont des apps.
  → Le mobile n'est pas un cas secondaire, c'est le cas principal.
- **Activation d'abord :** une méta-analyse 2026 sur 190 entreprises PLG mesure le passage
  de **2,8 % à 4,9 % de conversion** quand la page fait **accomplir l'action cœur — déposer
  un fichier — avant de proposer l'abonnement**, avec **−41 % d'abandon précoce**.
  ([Reforge/MKT1, via amraandelma](https://www.amraandelma.com/free-trial-conversion-statistics/))
  → C'est la justification chiffrée du chantier 1.
- Repère de conversion freemium : **3-5 %**, premier quartile **8-10 %**.
  ([First Page Sage](https://firstpagesage.com/seo-blog/saas-freemium-conversion-rates/))

---

## 4. Le principe directeur

> **Un fichier avant une phrase.**
> Puis : montrer, ne pas expliquer. Puis seulement : nommer.

Décliné en trois règles qui tranchent tous les arbitrages qui suivent :

1. **Rien ne se met entre l'arrivée et le dépôt.** Ni titre à lire, ni choix à faire.
2. **Notre différence se prouve après coup, pas avant.** Le verdict ne se promet pas dans le
   héros : il s'affiche quand le fichier revient, et il émerveille à ce moment-là.
3. **Le vocabulaire technique n'est pas supprimé, il est déplacé** — vers l'API, la console
   et les plans chers, où il est un signe de sérieux et non une barrière.

---

## 5. Le plan, en cinq chantiers

### Chantier 1 — Rendre la racine au grand public *(3-4 j)* — **rien d'autre ne compte avant**

- **`/` devient AI SmartTalk Documents.** La console d'intégrateur passe sous `/console`,
  **intacte**, et gagne une place d'honneur dans le pied de page et un lien « Développeurs »
  dans la nav. Elle n'est pas cachée : elle est adressée à qui de droit.
- **Thème clair par défaut**, bascule sombre conservée. Le thème clair existe déjà dans
  `brand.css` : c'est un changement d'attribut, pas une refonte.
- **Zone de dépôt dans le héros**, immédiatement, avant tout texte.

**Et l'idée qui nous met devant :** le service détecte déjà le type de fichier **aux
octets**. Donc on peut faire ce qu'aucun concurrent ne fait —

> **Déposez d'abord, on vous propose quoi faire ensuite.**
> Un PDF de 40 Mo → « Compresser » proposé en premier. Un scan sans texte → « OCR ».
> Un `.docx` → « Convertir en PDF ». Trois PDF d'un coup → « Fusionner ».

iLovePDF et Smallpdf obligent à choisir l'outil **avant** de déposer. On supprime cette
étape, et on transforme notre détection de type en argument visible.

Le héros devient : un grand rectangle de dépôt, un bouton franc, une ligne de réassurance
(« Supprimés au bout de 2 h · aucun filigrane · gratuit »), et rien d'autre au-dessus de la
ligne de flottaison.

### Chantier 2 — Rendre les 21 outils scannables *(2 j)*

- **Une icône par outil**, en SVG inline (aucune dépendance externe, le service reste
  utilisable hors ligne), **colorée par catégorie** : Optimiser, Organiser, Convertir,
  Éditer, Sécuriser. Les catégories sont déjà dans le catalogue.
- **Libellés courts sur les cartes** : « Compresser PDF », « Réparer PDF », « OCR PDF »,
  « PDF en Word ». Le `h1` de la page garde la formule longue que les gens recherchent —
  le SEO et la lisibilité ne demandent pas la même chaîne, et le catalogue peut porter les
  deux (`label` court, `h1` long).
- **« Océriser » disparaît** de la surface publique au profit de « OCR » et « rendre un scan
  cherchable ».
- **Le badge « avec verdict » disparaît**, remplacé par ce qu'il veut dire :
  **« ✓ vérifié »**, ou rien.
- **Méga-menu** dans la nav, par catégorie, comme le fait iLovePDF — pour 21 outils, une
  liste plate ne suffit pas.

### Chantier 3 — La page d'outil, qui est la vraie porte d'entrée *(2-3 j)*

71 % du trafic arrive ici, pas sur l'accueil. C'est donc la page la plus rentable à soigner.

- **La zone de dépôt monte tout en haut**, au-dessus du texte explicatif. Aujourd'hui le
  visiteur lit deux paragraphes avant de pouvoir agir.
- **Le CTA devient franc** : pleine largeur du panneau, couleur d'accent pleine, libellé à
  l'impératif (« Compresser mon PDF »).
- **Le panneau « LE VERDICT » sort de la barre latérale.** Avant traitement, il devient une
  ligne : *« On vérifie que rien n'a été perdu. »* Après traitement, il devient la carte de
  résultat — et c'est là qu'il impressionne.
- **La carte de résultat est le moment produit.** Grand, lisible, sans jargon :

  > **✓ Terminé** — 4,2 Mo → **780 Ko** *(−81 %)*
  > ✓ Texte conservé  ✓ 12 pages  ⚠ Images réduites à 150 dpi
  > **[ Télécharger ]**  · ou enchaîner : Fusionner · Protéger · Signer

  Les noms de contrôles (`text-preserved`, `page-count`) sont **déjà traduisibles** — la
  table de traduction existe dans `app.js`. Il n'y a que la mise en forme à faire.
- **Barre de progression réelle** pendant le traitement. Le silence fait douter.

### Chantier 4 — La confiance et l'entonnoir *(3-4 j)*

Rien de ceci n'existe aujourd'hui, et c'est ce qui transforme un visiteur en compte.

- **Bandeau de réassurance** sous la zone de dépôt, avec picto : *Supprimés au bout de 2 h ·
  Aucun filigrane, jamais · Hébergé en France · Aucun entraînement de modèle*. On tient déjà
  ces promesses ; elles sont aujourd'hui écrites en gris clair au milieu d'un paragraphe.
- **Preuve sociale** : compteur de documents traités, note et avis. À défaut d'historique,
  commencer par ce qui est vrai et vérifiable (« hébergé en France », « code auditable »)
  plutôt que d'inventer un chiffre.
- **« Se connecter » et « Créer un compte »** dans la nav, à droite, visibles.
- **Page Tarifs** — Gratuit / Pro / Équipe / API, avec la grille déjà arbitrée dans
  `PLAN-METAMORPHOSE.md` §7. Sans page de tarifs, il n'y a pas d'offre, seulement un service.
- **Le moment de la proposition d'abonnement : après le premier fichier réussi**, jamais
  avant. C'est exactement ce que mesure l'étude citée : 2,8 % → 4,9 %.

### Chantier 5 — Le mobile, la vitesse, les langues *(2 j)*

- **Vérifier et corriger le parcours mobile de bout en bout** : plus de la moitié du trafic.
  La zone de dépôt doit ouvrir la galerie et l'appareil photo, pas seulement un explorateur.
- **Budget de performance** sur la page d'outil : c'est une page de conversion, elle doit
  s'afficher en moins d'une seconde. Aucune dépendance externe — l'avantage est déjà pris,
  il faut le mesurer et le tenir.
- **Deux langues à parité**, et l'`hreflang` par outil (aujourd'hui il pointe vers l'index,
  pas vers la page équivalente). Détail invisible, effet direct sur le référencement.

---

## 6. Ce qui ne bouge pas — et pourquoi c'est important

**La thèse ne disparaît pas, elle change d'étage.** Ce serait une erreur de la diluer : c'est
la seule chose qui nous distingue une fois que tout le monde sait compresser un PDF.

| Public | Ce qu'il voit du verdict |
|---|---|
| **Grand public** | Une carte de résultat visuelle, trois lignes, zéro jargon. Le mot « verdict » n'apparaît jamais. |
| **Pro / Équipe** | Le rapport détaillé, l'historique, l'attestation signée |
| **API / Développeurs** | `verdict.checks[]`, `/api/compose`, `/api/attest`, MCP — la console d'aujourd'hui, **intacte**, valorisée comme surface avancée |

La console d'intégrateur n'est pas un problème à corriger : c'est un **produit pour un autre
public**. Elle est très bien telle qu'elle est. Elle n'est simplement pas à la bonne adresse.

---

## 7. Ce que je ne recommande pas

- **Copier l'esthétique d'iLovePDF.** Elle est jugée datée, et c'est notre seul avantage
  déjà acquis. On prend leurs *patterns* (icônes, dépôt immédiat, méga-menu, CTA franc), pas
  leur apparence.
- **Cacher la console.** Elle est excellente et elle sert les plans chers. On la déplace, on
  ne l'enterre pas.
- **Une application mobile.** Coûteuse, et une page web rapide couvre 90 % du besoin. À
  reconsidérer quand le trafic la réclamera.
- **Inventer des chiffres de preuve sociale.** Dans un produit dont l'argument est
  l'honnêteté du verdict, un compteur inventé est une contradiction qui se paiera.

---

## 8. Ordre et effort

```
1. Racine au grand public   3-4 j   ← rien d'autre ne compte avant
2. Outils scannables        2 j
3. Page d'outil             2-3 j   ← 71 % du trafic arrive ici
4. Confiance et entonnoir   3-4 j
5. Mobile, vitesse, langues 2 j
                            ─────
                            12-15 j
```

**Si une seule chose devait être faite cette semaine :** le chantier 1. Tant que la racine
affiche « SERVICE INTERNE · RUST · PANDOC », les onze autres constats sont sans objet.


---
---

# Journal de livraison

Écrit au terme de l'implémentation. Il décrit **l'état réel**, pas l'intention.

## Les cinq chantiers

| Chantier | État | Ce qui existe maintenant |
|---|---|---|
| **1 — La racine au grand public** | livré | `/` et `/en` servent le site public ; la console est à `/console`, **intacte** ; `/outils` et `/tools` répondent 308 plutôt que de servir un double ; thème **clair par défaut**, décidé par le serveur, la préférence de l'utilisateur gardant le dernier mot |
| **2 — Les outils scannables** | livré | 22 icônes SVG en ligne, colorées par catégorie ; libellés courts sur les cartes (« Compresser PDF ») et formule longue conservée en `h1` pour la recherche ; méga-menu par catégorie, au clavier et sans JavaScript ; « Océriser » a disparu de la surface publique |
| **3 — La page d'outil** | livré | Zone de dépôt **au-dessus** du texte, bouton pleine largeur, réglages repliés, carte de résultat, barre de réassurance, outils voisins en bas |
| **4 — Confiance et entonnoir** | livré | Bandeau de réassurance sous le dépôt, page **Tarifs** (Gratuit / Pro / Équipe / API), entrées « Tarifs » et « Développeurs » dans la navigation, bouton « Accès Pro » |
| **5 — Mobile, vitesse, langues** | livré | Zéro débordement horizontal mesuré à 360, 390, 414 et 768 px sur cinq pages ; cibles tactiles portées à 40 px ; `hreflang` pointant vers **la page équivalente** et non vers l'index |

## L'idée qui nous met devant

**Déposez d'abord, on vous propose quoi faire.** Le service lisait déjà le type d'un fichier
aux octets ; il sait maintenant aussi si un PDF porte une couche texte (`has_text`, deux
pages de `pdftotext`, quelques millisecondes). La page d'accueil s'en sert pour proposer
l'OCR à qui dépose un scan et la compression à qui dépose un fichier lourd.

Les concurrents imposent de choisir l'outil **avant** de déposer. On supprime cette étape.
Et le fichier n'est envoyé qu'une fois : la proposition mène à `…/<outil>#asset=<id>`, où la
page d'outil le retrouve déjà en ligne.

## Le SEO mis en place

- Une page par intention, avec l'expression cherchée en `<title>` et en `h1`. Les titres
  sont construits pour tenir sous la troncature de Google (62 caractères) en gardant le nom
  de marque, avec les deux qualificatifs qui décident du clic : « en ligne », « gratuit ».
- Canoniques absolues, `hreflang` vers **le même outil** dans l'autre langue.
- JSON-LD : `WebApplication`, `SoftwareApplication`, `BreadcrumbList`, `FAQPage`,
  `Organization`.
- `sitemap.xml` : 46 URLs, alternates, `lastmod` pris sur le catalogue et non sur l'heure du
  déploiement, priorités.
- `robots.txt` qui tient `/download/` hors de tout index — les documents produits pour des
  appelants identifiés ne sont pas des pages publiques.
- Une page 404 qui propose les vingt autres outils à un visiteur, et reste du JSON pour une
  intégration.
- `og:image` en 1200×630, **rendu par le moteur du service lui-même** : la carte sociale ne
  peut pas diverger de la marque.

## Ce qui a été vérifié

| Vérification | Résultat |
|---|---|
| `cargo test` | **493 tests**, 0 échec |
| `cargo clippy -D warnings` · `cargo fmt --check` | 0 avertissement |
| `./test_api.sh` | **205 tests d'intégration**, 0 échec, dont 34 nouveaux sur le site public, le SEO et les deux formes de 404 |
| Parcours complet dans un navigateur | dépôt → détection (`has_text`) → suggestions → page d'outil avec le fichier **déjà en ligne** → compression → carte de résultat |
| Débordement horizontal | 0 px à 360 / 390 / 414 / 768 px sur `/`, `/tarifs`, une page d'outil, `/en` et la 404 |
| Non-régression de la console | toutes ses vues répondent à `/console` |

## Ce qui reste

- **La prose des verdicts est en anglais côté API** (« 1 pages » y compris : le pluriel n'y
  est pas fait). Le site public ne la montre plus — il compose sa propre accroche à partir
  des chiffres qu'il connaît — mais un intégrateur anglophone la lit encore. Une cinquantaine
  d'occurrences à reprendre, sans risque mais sans intérêt à faire à l'aveugle.
- **Aucune preuve sociale** : pas de compteur, pas d'avis. Volontaire — dans un produit dont
  l'argument est l'honnêteté du verdict, un chiffre inventé se paierait.
- **Pas d'authentification** : les paliers payants renvoient vers la page d'accès de la
  console, et la page de tarifs le dit franchement plutôt que d'afficher un bouton qui ne
  mène nulle part.
