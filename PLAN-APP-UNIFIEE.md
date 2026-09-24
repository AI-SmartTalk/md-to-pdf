# Une seule application — rapport et plan

Écrit après audit de l'application telle qu'elle tourne, mesures à l'appui. Il fait suite à
`PLAN-UX-GRAND-PUBLIC.md`, dont il corrige une omission : j'ai déplacé la console sans la
**réconcilier**. Le résultat se voit, et le constat de départ est juste.

---

## 1. Ce que j'ai mesuré

Les deux surfaces, interrogées dans le même onglet, se décrivent ainsi :

| | `/` — le site public | `/console` — la console |
|---|---|---|
| **`<title>`** | Outils PDF gratuits en ligne… \| AI SmartTalk | md-to-pdf — PDF document engine |
| **Marque affichée** | AI SmartTalk **Documents** | md-to-pdf `v0.2.0` |
| **Thème servi** | `light` | `dark` |
| **Navigation** | Tous les outils · Tarifs · Développeurs | Accueil · Guides · Référence API · Console · Accès |
| **Langue** | un lien `EN` | un sélecteur segmenté `EN / FR` |
| **Page d'accueil** | « Vos documents, et ce qu'on en a fait » | « Un moteur de documents **pour nos produits** » |

Ce ne sont pas deux vues d'un produit. Ce sont **deux produits** qui partagent un domaine.

### Et ils se contredisent

| Le site public dit | La console dit |
|---|---|
| « 21 outils gratuits, **sans compte** » | « **Service interne** AI SmartTalk · Rust · pandoc · WeasyPrint » |
| Une page **Tarifs** avec un plan API à vendre | « L'API est **réservée aux équipes et projets AI SmartTalk** » |
| Bouton **« Accès Pro »** en évidence | « Il **n'est pas ouvert en libre-service** : chaque intégration reçoit son propre token » |

Le bouton « Accès Pro » du site public, et **les quatre appels à l'action de la page
Tarifs**, mènent tous à `/console#/acces` — c'est-à-dire à la page qui explique que ce n'est
pas vendu, avec une adresse e-mail et une promesse de réponse sous un jour ouvré.

> **L'entonnoir se termine sur un mur, et le mur dit le contraire de la vitrine.**

---

## 2. Les sept ruptures, par gravité

| # | Rupture | Ce que ça coûte |
|---|---|---|
| **R1** | **Deux marques, deux navigations, deux accueils.** Cliquer « Développeurs » fait basculer dans une autre application, avec un autre logo et une autre barre. | On ne comprend plus où on est. C'est exactement ce que vous décrivez. |
| **R2** | **Le discours se contredit.** La vitrine vend, la console refuse. | Un visiteur qui suit le parcours jusqu'au bout apprend qu'on ne veut pas de lui. Aucun taux de conversion ne survit à ça. |
| **R3** | **Aucun compte.** Le token se colle **à la main** dans `localStorage`, obtenu par e-mail. | Pas d'inscription, donc pas de rétention, pas de quota lisible, pas de facturation, pas d'historique. Tout le reste en dépend. |
| **R4** | **Trois sources de vérité pour l'API** : 49 routes montées, 41 décrites dans `spec.js`, 37 dans `swagger.yaml`. La console affiche le chiffre de `spec.js`. | La documentation ment, doucement, et personne ne s'en aperçoit avant un ticket. |
| **R5** | **Une préférence partagée pour deux défauts opposés.** Les deux surfaces lisent la même clé `mdpdf.theme` alors que l'une se veut claire et l'autre sombre. | Un passage par la console impose le sombre à la vitrine — c'est très probablement pourquoi votre capture montre l'accueil en sombre. |
| **R6** | **Deux systèmes de traduction.** `static/i18n.js` pour la console, Tera pour le site. | Deux endroits à tenir, deux façons de se tromper, et une langue qui ne suit pas d'une surface à l'autre. |
| **R7** | **Les guides et la référence sont enterrés.** Douze guides existent, écrits, bilingues — accessibles seulement depuis un pied de page. | Le contenu qui installe l'autorité d'un domaine n'est pas indexable et n'est pas lu. |

---

## 3. Le mot d'ordre

> **Une application, trois profondeurs.** Pas deux sites, pas un site plus une console.

| Profondeur | Qui | Ce qu'il voit |
|---|---|---|
| **Anonyme** | Le grand public | Les 21 outils, gratuits, sans compte. Le verdict à chaque fois. |
| **Membre** | Celui qui revient | Son espace : ses documents, son historique de qualité, ses chartes, ses quotas. |
| **Développeur** | L'intégrateur, l'agent | Ses clés en libre-service, la référence, la console d'essai, le MCP — **dans la même coque**, sous la même marque. |

La troisième n'est pas une autre application. C'est une section de celle-ci, comme
`/tarifs` en est une.

---

## 4. Le compte n'est pas une fonctionnalité, c'est la colonne vertébrale

C'est le point que je n'avais pas assez pesé, et vous avez raison d'y revenir. Sans compte :

- l'entonnoir n'a pas de fin — un visiteur satisfait n'a nulle part où aller ;
- le token se distribue à la main, donc l'API ne se vend pas ;
- rien ne se garde, donc rien ne fait revenir ;
- **et surtout, notre différenciateur ne capitalise pas.**

Ce dernier point mérite d'être développé, parce que c'est là que se joue le « les
dépasser ». Un verdict isolé est une jolie carte. **Un historique de verdicts est un
registre de qualité** — « ce contrat a été compressé le 12 mars, texte intact, 96/100,
attesté ». Personne dans cette catégorie ne peut offrir ça, parce que personne d'autre ne
mesure. Il faut un compte pour l'accrocher quelque part.

### Ce qu'un compte débloque chez ceux qu'on veut dépasser

- **iLovePDF** : un récapitulatif des dernières actions et téléchargements, un espace
  d'équipe avec rôles, signatures et restrictions PDF/A, une facturation centralisée, et le
  même compte sur web, bureau et mobile. ([iLovePDF Teams](https://www.ilovepdf.com/blog/new-team-workspace), [profil](https://www.ilovepdf.com/blog/guide-ilovepdf-profile-settings))
- **Smallpdf** : un stockage où tout fichier traité atterrit, accessible depuis n'importe
  quel appareil, conservé indéfiniment en payant ; signature électronique, envoi de gros
  fichiers, application Google Workspace. ([Smallpdf](https://smallpdf.com/starter-guide-to-smallpdf))

Leur compte sert à **garder des fichiers**. C'est le minimum, et c'est là qu'on peut faire
mieux qu'eux sans les imiter.

### Ce que notre compte offrirait qu'ils n'ont pas

| | Eux | Nous |
|---|---|---|
| Historique | La liste des fichiers | La liste des fichiers **avec le verdict de chaque opération** |
| Qualité | — | Un registre : ce qui a été préservé, ce qui a été perdu, page par page |
| Preuve | — | L'**attestation signée** : prouver qu'un document vient de vous, inaltéré |
| Identité visuelle | — | Les **chartes documentaires** : vos documents à votre marque, pas à la nôtre |
| Contrat | — | `/api/compose` : « quatre pages maximum, aucun tableau coupé » — et il s'y tient |
| Agents | — | Le **serveur MCP** : votre assistant utilise vos outils avec votre clé |
| Clés | À la demande | **En libre-service**, nommées, avec leurs quotas et leur consommation |

Rien de tout cela n'est à construire : **tout existe déjà côté moteur**. Il manque
uniquement l'endroit où l'accrocher.

---

## 5. La décision d'architecture qui vous revient

Les comptes demandent de la persistance, et ce service n'a **aucune base de données** — par
conception : il est sans état, il ne garde que des fichiers à durée de vie courte.

Deux voies, et je recommande franchement la première.

### A. L'identité vient d'AI SmartTalk *(recommandé)*

Le core AI SmartTalk a déjà des utilisateurs et des organisations. On s'y adosse par
OAuth/SSO : un compte AI SmartTalk, valable sur tous les produits. Le service ne stocke
qu'un identifiant d'utilisateur, ses clés et ses métadonnées de fichiers.

- **Pour :** une seule identité dans toute la maison, aucun second système de mots de passe
  à sécuriser, les organisations et la facturation existent déjà, et un client AI SmartTalk
  arrive déjà connecté.
- **Contre :** le grand public ne connaît pas la marque AI SmartTalk. Il faut une entrée
  grand public — « continuer avec Google », « continuer par e-mail » — qui crée le compte
  dans le core sans jamais dire « SSO » au visiteur.

### B. Des comptes autonomes dans le service

SQLite embarqué, e-mail plus mot de passe ou lien magique.

- **Pour :** indépendant, déployable seul.
- **Contre :** un deuxième annuaire d'utilisateurs à tenir, à sauvegarder et à sécuriser,
  et deux comptes pour un même client AI SmartTalk. Une nouvelle dépendance dans une image
  auditée à la main.

> **Mon avis :** A, avec une façade grand public. Construire un second système d'identité
> pour vendre des PDF est le genre de dette qu'on paie pendant cinq ans.

---

## 6. Le plan, en six chantiers

### Chantier 1 — Une seule coque *(4-5 j)* — **rien ne se juge avant**

- **Une marque, une barre, un thème, un sélecteur de langue** sur toutes les pages, y
  compris la console. La console garde ses écrans ; elle perd son en-tête, son logo et son
  « Accueil ».
- **La console devient une section** : `/dev`, `/dev/guides`, `/dev/api`, `/dev/console`,
  `/dev/mcp`. Servie par le même gabarit Tera que le reste, avec la même navigation.
- **L'accueil de la console disparaît.** Son rôle — expliquer ce que le service sait faire —
  est déjà tenu, mieux, par `/` et par `/dev`.
- **Une seule clé de préférence par surface** : le thème du site public et celui de la
  console cessent de se contredire (R5).
- **Un seul système de traduction.** Les chaînes de `i18n.js` rejoignent le mécanisme des
  gabarits, ou l'inverse — mais un seul (R6).

### Chantier 2 — Le discours redevient vrai *(1 j)*

- **Supprimer « service interne », « pour nos produits », « réservé aux équipes AI
  SmartTalk », « pas ouvert en libre-service ».** Ces phrases étaient exactes il y a un
  mois ; elles ne le sont plus depuis qu'il y a une page de tarifs.
- **Une seule source de vérité pour l'API** (R4). `swagger.yaml` fait foi, `spec.js` en est
  dérivé au démarrage ou à la construction, et un test échoue si le nombre de routes montées
  ne correspond pas. Le compteur affiché cesse de mentir tout seul.

### Chantier 3 — Les comptes *(8-10 j)* — **le chantier structurant**

- Inscription et connexion grand public : e-mail et Google, sans jargon.
- Session, page de profil, suppression de compte (RGPD : le droit à l'effacement doit être
  un bouton, pas un e-mail).
- **Clés d'API en libre-service** : créer, nommer, révoquer, voir la consommation. Le socle
  existe déjà — `API_KEY` accepte `nom=valeur` et les métriques sont étiquetées par nom. Il
  ne manque que l'écran et la persistance.
- Quotas lisibles : « 47 pages d'OCR sur 2 000 ce mois-ci », pas un 429 sans explication.
- **L'inscription se propose après le premier fichier réussi**, jamais avant — c'est ce que
  mesure l'étude citée dans le plan précédent (2,8 % → 4,9 %).

### Chantier 4 — L'espace de travail *(6-8 j)* — **ce qui nous met devant**

`/app`, visible une fois connecté :

- **Mes documents** : ce qui a été traité, conservé selon le palier (1 h en gratuit, 7 j en
  Pro, 30 j en Équipe), avec la reprise et l'enchaînement en un clic.
- **Le registre de qualité** : chaque opération avec son verdict. « Compressé le 12 mars ·
  texte intact · 96/100 ». C'est notre argument, et c'est ici qu'il devient un actif.
- **Mes chartes** : les brand kits, avec aperçu en direct.
- **Mes chaînes** : rejouer une suite d'opérations qu'on a déjà faite. Leur équivalent est
  « flux de travail », vendu et limité à un seul en gratuit.
- **Mes attestations** : les preuves émises, vérifiables.

### Chantier 5 — Le contenu qui installe l'autorité *(3-4 j)*

Les douze guides existent, sont bilingues, et ne sont accessibles que par un lien de pied de
page. Ils deviennent des pages indexables sous `/guides/<slug>`, reliées aux pages d'outil
qu'elles expliquent. C'est le seul levier de référencement qui reste quand la partie
technique est faite — et il est déjà écrit.

### Chantier 6 — L'équipe et la facturation *(6-8 j)*

Organisations, rôles, facturation Stripe, accord de traitement. À faire **après** avoir vu
si les comptes se remplissent : construire une facturation pour zéro client est le meilleur
moyen de la construire à côté du besoin.

---

## 7. L'architecture cible

```
/                        accueil grand public — le dépôt d'abord
/outils/<slug>           21 pages outil          (/tools/<slug> en anglais)
/guides/<slug>           les douze guides, indexables
/tarifs                                          (/pricing)
/connexion  /inscription
/app                     l'espace de travail (connecté)
   /app/documents        les fichiers et leur registre de qualité
   /app/cles             les clés d'API, en libre-service
   /app/chartes          les brand kits
   /app/equipe           rôles et facturation
/dev                     l'espace développeur — MÊME coque, même marque
   /dev/api  /dev/console  /dev/mcp
```

Une seule barre de navigation :
**Outils · Tarifs · Guides · Développeurs** — puis, à droite, **Se connecter / Mon espace**.

---

## 8. Ce que je ne recommande pas

- **Réécrire la console.** Elle est bonne, elle est testée, et elle sert un public réel. On
  lui retire sa coque, pas ses écrans.
- **Un second annuaire d'utilisateurs.** Voir §5.
- **Rendre les outils payants ou limités pour forcer l'inscription.** Notre gratuit large
  est un avantage concurrentiel ; le compte doit s'obtenir en offrant plus, jamais en
  reprenant.
- **Une application mobile.** Toujours prématuré. Le web rapide couvre le besoin, et
  l'espace de travail lui profite d'abord.
- **Des chiffres de preuve sociale inventés.** Dans un produit dont l'argument est
  l'honnêteté du verdict, ce serait la seule contradiction impardonnable.

---

## 9. Séquencement et effort

```
1. Une seule coque            4-5 j   ← rien ne se juge avant
2. Le discours redevient vrai 1 j     ← à faire dans la foulée, c'est presque gratuit
3. Les comptes                8-10 j  ← le chantier structurant
4. L'espace de travail        6-8 j   ← ce qui nous met devant
5. Les guides indexables      3-4 j
6. Équipe et facturation      6-8 j   ← après avoir vu les comptes se remplir
                              ───────
                              28-36 j
```

**Si une seule chose part cette semaine : les chantiers 1 et 2.** Ils coûtent cinq jours,
ne demandent aucune décision d'architecture, et suppriment à eux seuls la sensation de deux
applications — ainsi que la contradiction qui tue l'entonnoir aujourd'hui.

**La décision à prendre en parallèle**, parce qu'elle conditionne le chantier 3 :
l'identité vient-elle d'AI SmartTalk, ou le service tient-il ses propres comptes ?

---

## 10. Journal de livraison

Écrit après coup, en regardant ce qui tourne. Les chantiers 1 à 5 sont livrés ; le chantier
6 (équipe et facturation) reste volontairement ouvert, comme prévu au §9.

### Ce qui est en place

| Chantier | État | Preuve |
|---|---|---|
| 1. Une seule coque | livré | `/console` → 308 vers `/dev`, qui porte l'en-tête, le pied et le thème du site ; test d'intégration sur la redirection |
| 2. Le discours redevient vrai | livré | plus aucune mention de « service interne » ; les durées affichées sont celles que le service applique (voir ci-dessous) |
| 3. Les comptes | livré | inscription, session, clés en libre-service ; 22 tests d'intégration |
| 4. L'espace de travail | livré | `/app` et `/en/app` : registre de qualité, clés, palier |
| 5. Les guides indexables | livré | 15 guides, sitemap passé de 46 à 78 URL |
| 6. Équipe et facturation | non entrepris | décision de séquencement, §9 |

### Les quatre défauts trouvés à la vérification finale

Aucun n'était visible en lisant le code : il a fallu marcher le parcours complet dans un
navigateur, connecté, avec un vrai fichier.

**Le registre de qualité ne se remplissait jamais pour un membre.** C'est-à-dire : la seule
chose que nos concurrents ne savent pas faire ne fonctionnait pas pour le public qu'elle
vise. Trois causes empilées, corrigées ensemble.

1. `auth.rs` court-circuitait vers « ouvert » quand `API_KEY` n'est pas défini — la
   configuration par défaut — **avant** d'avoir regardé si la clé présentée était celle
   d'un membre. Une clé n'a pas qu'un rôle d'entrée : elle dit aussi qui vous êtes, et seul
   le premier rôle était redondant.
2. Le garde ne reconnaissait que les clés d'API. Or les pages d'outil appellent l'API
   **sans clé**, avec le cookie de session : la grande majorité des membres n'en créera
   jamais. `auth::member_name` lit désormais les deux identifiants. Le cookie est
   `SameSite=Lax`, ce qui est précisément la propriété qui rend cette lecture sûre.
3. Le registre était écrit depuis `helpers::finish_tool`, qui s'exécute **avant** que la
   route n'attache le verdict. Chaque entrée arrivait donc sans verdict — et nommait
   l'outil d'après le fichier produit (`compressed.pdf` au lieu de `compress`). L'écriture
   est passée dans `helpers::deliver_tool`, seul endroit qui voit l'opération complète.

**Le cookie de session n'avait pas de durée.** Le serveur ouvrait une session de trente
jours et le navigateur jetait le cookie à la fermeture : « rester connecté » ne
fonctionnait pas. `max_age` suit maintenant `SESSION_DAYS`.

**Une promesse fausse sur la page d'inscription.** Elle offrait « des fichiers gardés plus
longtemps » en échange d'une adresse ; tout le monde avait deux heures. Plutôt que de
retirer la promesse, on l'a tenue : `ASSET_TTL_MEMBER_SECS` (24 h par défaut), décidé à
partir du propriétaire déjà attaché au travail et jamais de ce qu'envoie l'appelant. Les
pages affichent le chiffre qui s'applique vraiment — servi anonyme pour le référencement,
corrigé par le client quand une session existe, et la réponse indexée de la FAQ nomme les
deux durées pour rester vraie sans JavaScript.

**Deux répertoires d'état sensibles échappaient à `.gitignore`** : `public/sessions/` (des
jetons qui ouvrent un compte) et `public/accounts/` (adresses e-mail et empreintes de mots
de passe). Un commit accidentel n'aurait pas fuité une trace, mais un accès.

### Une correction de robustesse trouvée dans le journal

La connexion passait par `exec::offload`, la file de rendu. Sous charge, se connecter était
refusé avec « le service est saturé » parce que le service compressait un PDF — et une
rafale de connexions pouvait à l'inverse affamer les moteurs de rendu. PBKDF2 ne lance
aucun processus : `exec::offload_cpu` lui donne sa propre file, bornée de la même largeur.

### Une collision qu'il valait mieux ne pas attendre

`owner_name` ne prenait que six caractères hexadécimaux de l'identifiant de compte, soit
24 bits — une collision vers quatre mille comptes. Ce nom étant la **clé** de l'index des
propriétaires, deux comptes qui se télescopent lisent le registre l'un de l'autre. Porté à
douze caractères, et `bind_owner` refuse désormais de déplacer un nom déjà attribué à un
autre compte : une attribution qui se réécrit en silence ne perd pas le travail, elle le
classe chez quelqu'un d'autre.

### Vérification

- `./test_api.sh` : **241 tests, 0 échec** — dont 24 nouveaux sur les comptes, le registre,
  la rétention différenciée et la durée du cookie
- `cargo test` : **518 + 12**, 0 échec
- `cargo clippy --all-targets` : 0 avertissement ; `cargo fmt --check` : propre
- parcours complet marché dans un navigateur : inscription → outil → le registre affiche
  `compressed.pdf · compressé le 20 août · 6 ko · 1 page · Aucun gain · 90/100`, et le
  palier annonce 24 heures

### Dette connue, assumée

- **Chantier 6** non entrepris (équipe, facturation), par séquencement.
- **Réinitialisation du mot de passe** : pas d'envoi d'e-mail, la page le dit franchement
  et propose d'écrire.
- **Coût de PBKDF2 en build debug** : 8 à 12 s pour une connexion. À mesurer en release
  avant de conclure quoi que ce soit — 600 000 tours restent le plancher OWASP.
- **Pluriels anglais** dans la prose des verdicts (`1 pages`), une cinquantaine
  d'occurrences côté Rust.

---

## 11. La revue adverse

Cinq lentilles indépendantes sur `accounts.rs`, `auth.rs`, `routes/auth.rs`, `history.rs` et
les pages associées — la surface que j'avais écrite moi-même et que personne n'avait relue.
Chaque constat a ensuite été soumis à deux sceptiques chargés de le **réfuter**, l'un jouant
l'auditeur, l'autre l'auteur du code, avec pour consigne de réfuter dans le doute.

**17 constats levés, 5 ont survécu.** Les douze écartés incluent plusieurs constats réels que
j'avais corrigés pendant que la revue tournait : les sceptiques les ont trouvés déjà réparés.

### Corrigés pendant la revue, avant qu'elle ne conclue

- **Un mot de passe non borné haché pendant des minutes.** PBKDF2 hache le mot de passe à
  *chaque* itération : un « mot de passe » d'un mégaoctet demandait six cents gigaoctets de
  SHA-256, depuis une requête sans compte ni clé. Borné à 256 caractères, vérifié avant
  `create` **et** avant `authenticate` — refusé en 20 ms au lieu de minutes.
- **Aucune limite de débit sur `/api/auth/*`.** Rien ne séparait une liste de mots de passe
  d'un compte, et quelques clients en boucle rendaient la connexion impossible pour tout le
  monde. `AUTH_ATTEMPTS_PER_MINUTE` (10 par défaut), compté **par adresse cliente** et non
  par compte : compter par compte permettrait de verrouiller quelqu'un hors de son propre
  compte en échouant à sa place.
- **Révoquer une clé nommée `web` détruisait l'attribution des sessions du membre.** Le nom
  est désormais réservé, et deux clés d'un même compte ne peuvent plus le partager — elles
  partageaient leur entrée d'index, donc révoquer l'une désindexait l'autre.

### Confirmés et corrigés après la revue

**Le registre ne nommait jamais le document du visiteur** (sérieux). Toutes les lignes
s'appelaient `compressed.pdf` : le seul nom qui parvenait au registre était celui que
l'outil donne à sa *sortie*. Dix compressions produisaient dix lignes rigoureusement
identiques, sur la page qui porte tout l'argument du produit.
`assets::note_source_name`, posé dans `helpers::resolve_source` — le seul point par lequel
tout outil passe avec le nom de l'appelant encore en main — et oublié au début de chaque job
puisque les threads bloquants sont recyclés. Le registre lit maintenant
`contrat-cadre.pdf`, `facture-2026.pdf`.

**Toute la moitié anglaise du site envoyait vers les pages françaises** (sérieux). Un
anglophone qui cliquait « Sign in » depuis `/en` atterrissait sur « Content de vous revoir »,
au moment précis où on lui demande un mot de passe. `Lang::signin/signup/workspace`, et
l'invitation du client suit la langue de la page.

**`last_used` n'était jamais écrit alors que l'espace l'affiche** (mineur). Toutes les clés
lisaient « jamais utilisée », y compris celle qui porte la production — or c'est exactement
le signal sur lequel un membre s'appuie pour décider laquelle révoquer. Écrit une fois par
jour et par clé : la résolution que la liste affiche, sans une écriture par requête.

**Le secret de clé était accompagné de la même consigne deux fois, dont une en anglais**
(mineur). Sur l'écran le plus sensible de l'espace. Le `notice` de l'API est écrit pour un
intégrateur qui lit du JSON ; la phrase traduite juste au-dessus suffit.

### Confirmé, non corrigé, et pourquoi

**L'énumération des adresses e-mail par `/api/auth/signup`** (sérieux). Une adresse déjà
prise répond 409 en 3 ms ; une adresse neuve répond 201 après le hachage. Le statut comme le
temps disent donc si une adresse a un compte, ce que `authenticate` se donne pourtant
beaucoup de mal à cacher — il hache un `DUMMY` exprès.

Les deux réfutateurs ont confirmé le constat **et** la conclusion : dans les contraintes de
ce service, il n'y a pas de remède propre. Masquer le 409 demande un envoi de courrier de
vérification, donc un expéditeur SMTP, donc une dépendance que l'image auditée à la main
n'accepte pas. Égaliser le temps en hachant *avant* de réserver l'adresse transformerait
chaque sonde en 600 000 tours de PBKDF2 gratuits — le remède serait pire que le mal.

Le seul levier compatible était la limite de débit par adresse, qui est maintenant en place :
elle ne supprime pas la fuite, elle la ramène à dix adresses testables par minute et par
source. C'est un compromis assumé, écrit ici pour qu'il ne soit pas redécouvert comme un
oubli. Le jour où le service aura un expéditeur de courrier, la bonne réponse est de
répondre 202 dans tous les cas.

### Vérification après ces corrections

- `./test_api.sh` : **250 tests, 0 échec** — dont neuf de plus sur le nom du document, la
  réservation du nom `web`, l'unicité des noms de clés, `last_used`, les liens par langue,
  la borne du mot de passe et la limite de débit
- `cargo test` : **518 + 12**, 0 échec ; `clippy --all-targets` : 0 avertissement ;
  `fmt --check` : propre

### Note de méthode

Deux agents de la revue ont effacé le magasin de comptes local (`public/accounts`,
`public/sessions`) en déduisant seuls que tout y était de leur fabrication. Sans conséquence
ici — données de test uniquement — mais c'est la raison pour laquelle ces répertoires sont
désormais dans `.gitignore`, et une raison de plus de ne jamais faire tourner ce genre de
revue contre autre chose qu'un environnement jetable.
