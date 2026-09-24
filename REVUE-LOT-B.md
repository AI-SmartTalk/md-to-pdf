# Revue adverse du lot B — les douze outils du plancher

Revue conduite le 19 août 2026 sur la copie de travail (`master`, arbre non commité).
Aucun fichier n'a été modifié par cette revue.

**Méthode.** Lecture des treize routes nouvelles (`pages`, `pages/number`, `crop`,
`compress`, `repair`, `unlock`, `rasterize`, `images-to-pdf`, `office-to-pdf`,
`pdf-to-office`, `ocr`, `extract`, `pdfa`), du socle (`assets.rs`, `jobs.rs`,
`helpers.rs`, `auth.rs`) et du `git diff` des routes préexistantes ; puis **exécution
réelle** des chaînes d'outils dans le conteneur `pandoc` (qpdf 11.3.0, Ghostscript 10.00.0,
poppler, img2pdf 0.4.4, LibreOffice 7.4.7 installé pour l'occasion). Chaque constat porte
la mention **vérifié** (rejoué sur un vrai fichier), **lu** (établi par lecture du code
seul) ou **non vérifié**.

**État de l'arbre au moment de la revue.** `cargo check --all-targets` propre,
`cargo test` : **438 tests verts**. L'arbre a bougé pendant la revue (d'autres agents
écrivaient `merge.rs`, `protect.rs`, `watermark.rs`, `types.rs`) ; les numéros de ligne
correspondent à l'état final relu.

---

## 1. Constats, par gravité

### Bloquant

| # | Fichier:ligne | Ce qui est faux | Scénario concret qui le déclenche | Correction en une phrase |
|---|---|---|---|---|
| **B1** | `src/routes/office.rs:222-256` (et le commentaire du `Dockerfile:32`) | **SSRF prouvé.** Le commentaire du Dockerfile dit « LibreOffice with a throwaway profile and **no network** » et le module dit « macros désactivées, et pas de réseau ». Rien dans le code, l'image ou le compose ne coupe le réseau de `soffice`. — **vérifié** | Un `.fodt`/`.odt`/`.docx` contenant `<draw:image xlink:href="http://…"/>` est déposé sur `POST /api/files`, puis `POST /api/office-to-pdf`. LibreOffice émet un `OPTIONS` puis un `GET` vers l'URL choisie par l'attaquant (`User-Agent: LibreOffice 7.4.7.2`), et **les octets récupérés sont embarqués dans le PDF rendu** (4 objets `/Image` dans le fichier de sortie de mon test). Cible naturelle : `http://169.254.169.254/latest/meta-data/`, un service interne du réseau `ai-toolkit-network`, ou un scan de ports en aveugle. | Lancer `soffice` sans réseau (namespace réseau dédié / conteneur worker de §5.2), ou à défaut immédiat forcer `-env:UserInstallation` avec un `registrymodifications.xcu` qui met `Inet/Settings` en refus et supprimer `xlink:href` distants avant conversion. |
| **B2** | `src/routes/repair.rs:113-151` | **`/api/repair` rend une page blanche et l'appelle une réussite.** `attempt()` juge une passe sur « pdfinfo lit ≥ 1 page », jamais sur le code de retour. Or Ghostscript, sur un PDF qu'il ne peut pas ouvrir, écrit `Couldn't initialise file` **et sort en 0**, en produisant un PDF **d'une page vide**. — **vérifié** | Un PDF chiffré (mot de passe utilisateur) — ou tout fichier dont l'en-tête est illisible — est envoyé à `POST /api/repair`. Passe 1 : `qpdf` sort en 2 (« invalid password »), aucun fichier. Passe 2 : `gs` sort en **0** et écrit un A4 blanc d'une page. `page_count` = 1 > 0 → succès. Le verdict rendu est `recovery-method: warn` + `page-count: **ok** — 1 pages recovered, the damaged file reported no page count to compare with`, résumé « 1 pages recovered by ghostscript ». L'appelant archive une page blanche en croyant son document réparé. | Refuser la passe Ghostscript quand la sortie ne contient aucun texte **et** aucune image alors que la source en avait, et transformer `Couldn't initialise file` / `No pages will be processed` sur stdout+stderr en échec explicite. |
| **B3** | `docker-compose.prod.yml` (aucune section), `src/routes/files.rs:35` | **Le bac à sable du plan (§5.2 / lot A.3) n'existe pas.** Pas de conteneur worker, pas de `read_only`, pas de `cap_drop`, pas de seccomp, pas d'isolation réseau : Ghostscript, LibreOffice, ocrmypdf et img2pdf tournent dans le conteneur de service, avec le réseau, un système de fichiers inscriptible et l'environnement complet du processus (`API_KEY`, `ATTESTATION_SECRET`, jeton log420 hérités par tout `Command`). Seuls `mem_limit: 1g`, `cpus: 2` et `pids_limit: 256` sont posés. — **lu** | `PUBLIC_TOOLS=true` (indispensable pour que le site `/outils` fonctionne : sa page appelle `POST /api/files` sans clé, cf. `auth.rs:329`) suffit à ouvrir l'ingestion à Internet. Le jour où on allume le site, n'importe qui pousse 100 Mo d'octets hostiles dans ces cinq parseurs. B1 est la première conséquence, pas la seule. | Ne pas mettre `PUBLIC_TOOLS=true` en production avant le conteneur worker du §5.2 ; le plan dit « non compressible », il l'est resté. |
| **B4** | `src/assets.rs:444-488` vs `README.md:378` et `static/outils/catalog.fr.json:6,10,80` | **Rien ne balaie les assets expirés.** `purge_expired()` n'est appelée qu'au démarrage (`assets.rs:484`). L'expiration à la lecture (`meta()`) ne supprime que l'asset qu'on relit. Le README affirme « Expiry is enforced on read **as well as by the sweeper** », le commentaire `assets.rs:398` parle d'« a file the sweeper has not got to yet », et le site public promet « Vos fichiers sont effacés au bout de deux heures ». Il n'y a pas de sweeper. `deploy/pdf-purge.sh` ne connaît que `public/pdf`. — **lu** | Un visiteur dépose un contrat, ferme l'onglet. Le conteneur tourne trente jours sans redémarrer : le fichier est toujours sur le volume `pdf-assets` trente jours plus tard, alors que l'API répond « expiré ». Amplification : un `split` de 500 pages crée 500 répertoires d'assets en un appel (`pages.rs:142`). C'est un manquement RGPD **et** un remplissage de disque. | Un `rocket::tokio::spawn` d'une boucle `interval(asset_ttl / 4)` appelant `assets::purge_expired()`, ou étendre `deploy/md-to-pdf-purge.timer` à `public/assets` — et, tant que ce n'est pas fait, corriger les trois textes qui promettent l'inverse. |

### Sérieux

| # | Fichier:ligne | Ce qui est faux | Scénario concret | Correction |
|---|---|---|---|---|
| **S1** | `src/routes/repair.rs:350-358` | **`/api/unlock` affirme un contrôle qu'il ne fait pas.** Le check `encryption` est posé en `ok` avec « the N pages are no longer password protected » sans que rien n'ait vérifié que la source était chiffrée. L'aveu de l'agent (« un fichier non chiffré fait sortir qpdf en avertissement, l'avertissement remonte dans `warnings` ») est **faux** : `qpdf --password=X --decrypt` sur un PDF non chiffré sort en **0, silencieusement**. — **vérifié** | J'envoie un PDF ordinaire et un mot de passe quelconque : réponse 200, `verdict.status = ok`, score 100, « Encryption removed from 3 pages », aucun `warnings`. Le service certifie avoir retiré une protection qui n'a jamais existé. | Lire `qpdf --show-encryption` (ou `pdfinfo`) sur la source avant la passe, et rendre un check `encryption: warn — ce document n'était pas chiffré` dans ce cas. |
| **S2** | `src/routes/numbering.rs:546-557` | **Le check `page-geometry` décrit un comportement de qpdf qui n'existe qu'à moitié.** Il dit « the numbering layer follows page 1 … **and is scaled to fit on the others** ». qpdf `--overlay` **centre toujours** et **ne réduit que** si le calque est plus grand que la page de destination ; il n'agrandit jamais. — **vérifié** (calque A6 sur A4 : facteur 1.0, simple centrage ; calque A4 sur A5 : facteur 0.7047) | Document dont la page 1 est A5 et les suivantes A4. Le calque A5 est centré sans mise à l'échelle sur les pages A4 : le numéro, demandé à 1 cm du bas, atterrit à **151 pt du bas** (5,3 cm), en plein milieu du texte. Le verdict ne dit que « warn … scaled to fit », donc l'appelant croit à un simple redimensionnement. | Soit construire un calque par géométrie distincte (WeasyPrint sait faire des `@page` nommées), soit dire la vérité dans le `detail` : « centré, réduit si nécessaire, jamais agrandi — le numéro peut tomber hors marge sur les pages plus grandes ». |
| **S3** | `src/assets.rs:189` | **Un PDF légèrement abîmé ne peut pas être déposé comme PDF.** `detect_kind` exige `%PDF-` à l'offset 0. Un PDF précédé de quelques octets parasites — le cas de dégradation le plus banal — devient `text`, et `resolve_pdf_source` (`helpers.rs`) le refuse par « Asset … is a text file, this endpoint needs a PDF ». — **vérifié** (qpdf répare ce même fichier sans broncher : exit 0, 3 pages) | Un utilisateur arrive avec exactement le fichier pour lequel `/api/repair` existe. Il obtient un 400 à l'`upload`… non : l'upload passe, c'est `/api/repair` qui refuse. L'outil de réparation ne peut pas réparer la panne qu'il annonce. | Chercher `%PDF-` dans les 1024 premiers octets (comme le font qpdf et poppler) au lieu de l'exiger à l'offset 0. |
| **S4** | `src/routes/pages.rs:133`, `src/routes/crop.rs:119`, `src/routes/images_to_pdf.rs:99` | **`page_count()` est appelé après `finish_tool()`, qui déplace le fichier.** `assets::store` fait un `fs::rename` (`assets.rs:325`) ; avec `"output":"asset"` le fichier temporaire n'existe plus quand on le mesure. Quatre autres routes (`compress`, `ocr`, `office`, `pdfa`) commentent explicitement « Measured before `finish_tool`, which moves the file away » — la règle est connue, elle n'est pas tenue partout. `merge.rs:66` fait la même inversion mais avec `.ok()`, donc il rendrait `pages: null` au lieu d'un 500. — **lu** | Aujourd'hui masqué : `/tmp` (overlay) et `public/assets` (bind mount en dev, volume nommé en prod) sont sur des systèmes de fichiers différents, donc `rename` échoue et le `fs::copy` de repli laisse la source en place. Le jour où `TMPDIR` pointe dans le même volume — ou dans un tmpfs partagé —, `POST /api/pages` avec `"output":"asset"` répond 500 après avoir pourtant stocké l'asset. | Déplacer les trois `page_count` avant l'appel à `finish_tool`, comme les routes qui le font déjà. |
| **S5** | `deploy/nginx-md-to-pdf.conf:31`, `deploy/apache-md-to-pdf.conf:40` | **La limite documentée est inatteignable.** `ASSET_MAX_MB=100`, `Rocket.toml` monte `data-form` à 110 MiB et `file` à 100 MiB — mais le proxy coupe à **12 Mo** (`client_max_body_size 12m`, `LimitRequestBody 12582912`). — **lu** | Un dépôt de 30 Mo reçoit un **413 du proxy**, sans message du service, alors que le README et le site annoncent 100 Mo (et l'offre « 50 Mo en gratuit »). | Monter les deux limites de proxy à ≥ `ASSET_MAX_MB` + marge, dans le même commit que `Rocket.toml`. |
| **S6** | `src/jobs.rs:100-139` → `src/exec.rs:73` | **Les travaux asynchrones n'allongent aucun budget.** `jobs::submit` passe par `exec::offload`, qui pose `Budget::start(config().render_deadline)` — 120 s par défaut. L'en-tête de `jobs.rs` ouvre pourtant sur « un OCR de 200 pages … ne tient pas sous le deadline, et aucun relèvement ne l'y fait tenir ». La couche d'état a été livrée, pas la levée de la contrainte. — **lu** | `POST /api/jobs {"endpoint":"ocr","body":{…}}` sur un scan de 150 pages : le job passe `running` puis `failed: Timeout` au bout de 120 s. `/api/ocr` accepte pourtant jusqu'à **300 pages** (`ocr.rs:31`) : le plafond d'entrée et le budget d'exécution ne se parlent pas. | Donner aux jobs leur propre deadline (`JOB_DEADLINE_SECS`, par ex. 900) dans `jobs::submit`, ou aligner `MAX_PAGES` de l'OCR sur ce qui tient en 120 s. |
| **S7** | `src/routes/files.rs:73-95`, `src/jobs.rs:142` | **Aucune notion de propriétaire sur les assets ni sur les jobs.** Avec les clés multiples du lot A.5, la clé `client-b` peut `GET`/`DELETE /api/files/{id}` d'un asset déposé par `client-a`, et poller `/api/jobs/{id}` de `client-a` (dont la réponse contient les `AssetMeta` produits). L'isolation repose entièrement sur le secret des identifiants. — **lu** | Un id d'asset qui fuite dans un log applicatif, un ticket de support ou un `Referer` donne à un tiers la lecture **et la suppression** du document. | Écrire le nom de clé dans `meta.json` et dans `JobView`, et refuser l'accès quand `ApiKey` ne correspond pas (le tier `public` restant un propriétaire à part entière). |
| **S8** | `src/jobs.rs:100-116` | **La file de jobs n'est bornée par rien.** Aucun plafond sur le nombre de jobs suivis, aucun quota par clé ; chaque `submit` insère une `JobView` gardée `JOB_TTL_SECS` (1 h) et lance un `tokio::spawn`. — **lu** | Une boucle qui poste 100 000 `POST /api/jobs` remplit la `HashMap` du registre (et 100 000 tâches tokio en attente du sémaphore) : les jobs échouent en 429 après `queue_timeout`, mais les enregistrements, eux, restent une heure en mémoire. | Un plafond (`MAX_TRACKED_JOBS`) et un compteur par clé, refusés en 429 comme le fait déjà `exec::offload`. |
| **S9** | `test_api.sh` | **Trois routes sans aucune couverture d'intégration** : `/api/images-to-pdf`, `/api/office-to-pdf`, `/api/pdf-to-office` (0 occurrence dans `test_api.sh`, alors que les dix autres en ont 2 à 11). Ce sont exactement les trois qui dépendent de binaires **absents de l'image de dev au moment où elles ont été écrites** (`img2pdf` a été installé à la main par un agent, `soffice` et `ocrmypdf` n'y étaient pas du tout). — **vérifié** | Une régression sur la ligne de commande `soffice` ou `img2pdf` ne sera vue qu'en production. `A.7` dit qu'une route n'est livrée que lorsque `test_api.sh` la couvre. | Trois cas dans `test_api.sh`, après reconstruction de l'image. |
| **S10** | `src/routes/office.rs:232`, et tous les `helpers::run_tool` | **Les processus enfants héritent de l'environnement complet du service** — `API_KEY`, `ATTESTATION_SECRET`, `LOG420_TOKEN`, `PDF_ALLOWED_URL_HOSTS`. Pour un parseur qui traite des octets hostiles, c'est une surface qui ne coûte rien à retirer. — **lu** | Toute exécution de code dans LibreOffice ou Ghostscript (macro, CVE d'évasion — Ghostscript en a l'historique) donne les secrets du service en plus du système de fichiers. | `command.env_clear()` puis réinjection explicite de `PATH`, `HOME`, `LANG` dans `run_tool`/`run_capture`. |

### Mineur

| # | Fichier:ligne | Ce qui est faux | Scénario | Correction |
|---|---|---|---|---|
| **M1** | `src/assets.rs:245-248` | `text_kind` fait `std::str::from_utf8(head)` sur les **1024 premiers octets** : un caractère multi-octets à cheval sur la frontière rend l'erreur, et le fichier devient `Unknown`. — **lu** | Un `.md` ou `.txt` français de plus d'1 Ko dont un « é » commence à l'octet 1023 est stocké en `kind: unknown`, extension `.bin`, refusé par tous les outils. Environ 1 à 3 % des fichiers texte accentués. | Utiliser `from_utf8` en tolérant une erreur dont `error_len()` est `None` (troncature en fin de tampon). |
| **M2** | `src/routes/numbering.rs:292-320` | `validate_margin` accepte une **unité vide** : `"margin":"10"` passe la validation et atterrit tel quel dans `bottom: 10;`, déclaration CSS invalide. — **lu** | Le numéro perd son positionnement et se place en haut à gauche du calque, sans un mot dans le verdict. | Refuser l'unité vide sauf pour la valeur `0`. |
| **M3** | `src/routes/compress.rs:105-131` | Quand la compression a grossi le fichier, `delivered = &before` : les checks `page-count` et `text-preserved` comparent l'original **à lui-même** et sortent forcément `ok`. Seul `no-gain` mentionne la tentative jetée. — **lu** | L'appelant lit « page-count ok, text-preserved ok » et croit que la compression a été auditée ; elle a été annulée. | Nommer la tentative dans les deux `detail` (« l'original est rendu : ces mesures portent sur lui »). |
| **M4** | `src/routes/pages.rs:496`, vs `repair.rs:101`, `numbering.rs:94` | **Trois conventions pour la même question qpdf.** `pages` ajoute `--warning-exit-0` ; `repair`/`unlock` ignorent le code de retour et jugent sur la sortie ; `numbering` laisse `run_tool` échouer sur un simple avertissement. Un même PDF légèrement bancal réussit sur une route et échoue sur l'autre. — **lu** | `POST /api/pages` accepte un fichier ; `POST /api/pages/number` sur le même fichier rend un 500. | Choisir `--warning-exit-0` partout, et remonter l'avertissement dans `warnings`. |
| **M5** | `src/routes/compress.rs:67`, `rasterize.rs:69`, `pdfa.rs:71`, `ocr.rs:102` | Un PDF chiffré fait échouer `pdfinfo`/`pdftotext` en `ProcessFailed` → **500**, alors que c'est une erreur d'appelant. — **vérifié** (`pdfinfo` exit 1, `pdftotext` exit 1 sur un PDF à mot de passe utilisateur) | « Compressez ce PDF protégé » → 500 sans explication, au lieu d'un 400 « ce document est chiffré, passez-le d'abord par `/api/unlock` ». | Détecter « Incorrect password » dans le stderr de `pdfinfo` et rendre un 400 qui nomme `/api/unlock`. |
| **M6** | `src/jobs.rs:182` | Le client HTTP du callback emprunte `config().mermaid_timeout` — un réglage qui n'a rien à voir. — **lu** | Baisser le timeout Mermaid raccourcit silencieusement les webhooks. | Un `JOB_CALLBACK_TIMEOUT_SECS` propre, ou `process_timeout`. |
| **M7** | `README.md:256` | `POST /api/jobs` est absent du tableau des routes (seul `GET /api/jobs/{id}` y figure). — **lu** | Un intégrateur ne trouve pas comment soumettre un travail. | Une ligne dans le tableau. |
| **M8** | `src/routes/crop.rs:436-464` | Le CropBox est réécrit, la **MediaBox est laissée intacte** (approximation déclarée par l'agent), mais le verdict ne le dit nulle part. — **lu** | Une chaîne de traitement qui lit la MediaBox (impression, imposition) voit la page d'origine et croit le rognage sans effet. | Un check `mediabox: warn — le CropBox est réécrit, la MediaBox reste celle de la source`. |
| **M9** | `src/routes/jobs.rs:65-135` | `crop`, `pages/number`, `unlock` et `extract` exposent un `run()` mais ne sont pas dans la table de dispatch asynchrone, sans que rien ne l'explique. — **lu** | `POST /api/jobs {"endpoint":"crop"}` répond 400 « cannot be run asynchronously » alors que la route existe. | Les ajouter (une ligne chacune) ou dire dans le commentaire pourquoi elles restent synchrones. |

---

## 2. Le contrat de non-régression

Le brief demandait de confirmer que **le seul changement** sur les routes préexistantes est
`resolve_pdf_path` → `resolve_pdf_source`. **Ce n'est pas le cas** — mais aucune des
différences trouvées n'est une régression au sens du §4 du plan. État exact :

| Fichier | Changement | Verdict |
|---|---|---|
| `pipeline.rs`, `cache.rs` | **aucun** (`git diff` vide) | conforme |
| `redact.rs`, `layout.rs`, `diff.rs`, `preview.rs`, `convert.rs` | `resolve_pdf_source` + garde `ApiKey` → `PublicOrKey` | additif |
| `merge.rs`, `watermark.rs`, `protect.rs` | `resolve_pdf_source`, garde `PublicOrKey`, champ `output` optionnel, **type de réponse `ConvertResponse` → `ToolResponse`**, ajout de `pages` | **compatible** : `download_url` reste sérialisé quand il existe, l'absence de destination continue de renvoyer le binaire, `pages` est un champ nouveau. Vérifié champ par champ (`types.rs:202-212` contre `types.rs:240-254`). Ces trois routes ne posaient jamais `cached`, `layout` ni `warnings`, seuls champs de `ConvertResponse` perdus au passage. |
| `legacy.rs:23,50` | **nouveau plafond de 10 Mo sur `markdown`** | changement de comportement assumé et commenté : `data-form` est passé de 10 MiB à 110 MiB pour les dépôts, et `POST /` est ouvert. Le plafond ferme la brèche à l'endroit exact où elle s'ouvrait. Correct, mais ce n'est pas « rien n'a changé ». |
| `render.rs`, `html_to_pdf.rs` | inchangés, gardés en `ApiKey` | conforme, et le commentaire d'`auth.rs:334` dit pourquoi |

**Le point qui mérite une décision, pas une correction** : `PublicOrKey` rend
`/api/protect`, `/api/redact`, `/api/merge`, `/api/watermark`, `/api/preview`,
`/api/convert`, `/api/layout`, `/api/diff` et **`POST /api/files`** accessibles sans clé
dès que `PUBLIC_TOOLS=true`. Le défaut est `false` (`.env.example:30`,
`docker-compose.prod.yml:44`) — mais le site public `/outils` ne fonctionne pas sans, donc
allumer le site **et** ouvrir l'ingestion sont le même geste. C'est ce geste, et pas un
autre, qui exige B3.

---

## 3. Les aveux des agents, vérifiés un par un

| Aveu | Verdict |
|---|---|
| `crop` : « l'approche du compteur `BeginPage` est fausse, l'opérande est stable » | **Confirmé et excellent.** Rejoué : boîtes par page correctes, pages non ciblées intactes, MediaBox préservée. |
| `crop` : « `auto` ne voit que la couche texte ; un scan garde sa boîte » | Exact, et le check `content-detected` le dit. |
| `crop` : « une boîte hors page est rognée avec un avertissement plutôt que refusée » | Exact, et défendable. |
| `numbering` : « qpdf **scale**/centre le calque sur les pages de format différent » | **Infirmé à moitié → S2.** qpdf réduit mais n'agrandit jamais. |
| `numbering` : « la géométrie et la chaîne WeasyPrint/qpdf ont été vérifiées à la main, y compris une page pivotée » | **Confirmé.** Rejoué : sur une page `/Rotate 90`, le calque à géométrie inversée retombe très exactement à 20 pt du bas dans la vue pivotée. C'est subtil et c'est juste. |
| `repair` : « un fichier non chiffré fait sortir qpdf en avertissement » | **Infirmé → S1.** Sortie 0, silence complet. |
| `repair` : « une passe est jugée sur sa sortie et non sur le code de retour » | Aveu exact, **conséquence sous-estimée → B2**. |
| `office` : « `libreoffice-draw` n'est pas dans le Dockerfile, `writer_pdf_import` risque d'être absent » | **Infirmé.** `libpdfimportlo.so`, `libpdfiumlo.so` et `xpdfimport` sont dans **`libreoffice-core`**, tiré par `libreoffice-writer`. J'ai rejoué la ligne de commande exacte d'`office.rs:232` : `convert /tmp/rev/t3.pdf -> out1/t3.docx using filter : MS Word 2007 XML`, 5 363 octets. La route fonctionne. |
| `office` : « soffice sort en 0 après un refus, seule la présence du fichier fait foi » | **Confirmé** (`Error: source file could not be loaded`, exit 0, répertoire de sortie vide). La garde est nécessaire et elle est là. |
| `office` : « le round-trip double le coût » | Exact, et c'est le prix du verdict. Acceptable, mais à mettre dans la doc de tarification. |
| `office` : « `save_pdf` force `.pdf` sur un docx » | Exact, signalé par un `warnings` — le moins mauvais choix vu l'interdiction de toucher `helpers.rs`. |
| `images_to_pdf` : contrat CLI d'`img2pdf` validé empiriquement | **Confirmé.** J'ai rejoué `--from-file <NUL-list> --output … --border 12.00pt --pagesize A4 --fit into` : 2 pages A4, ordre respecté. |
| `images_to_pdf` : « `img2pdf` n'est pas dans l'image lancée » | Vrai au moment de son rapport, **faux aujourd'hui** : `img2pdf 0.4.4` est présent. `ocrmypdf` et `soffice`, eux, **manquent toujours** dans le conteneur `pandoc` de dev. |
| `ocr` : « le format du sidecar est supposé, pas observé » | **Non vérifiable** : `ocrmypdf` absent de l'image de dev. C'est le seul outil dont aucune ligne n'a jamais été exécutée. À rejouer avant livraison. |
| `ocr` : « les codes de sortie tolérés sont détectés par le texte de stderr » | Aveu exact et bien commenté ; fragile à une reformulation d'ocrmypdf, mais dégrade vers l'erreur, pas vers le faux succès. Bon arbitrage. |
| `rasterize` : « `/download` étiquette tout en `application/pdf` » | Exact, et le `warnings` pointe `"output":"asset"`. |
| `pages` : « `download_url` ne pointe que sur la première part » | Exact, déclaré, et compensé par `assets`. |
| `pdfa` : « `-dPDFACompatibilityPolicy=1` retire l'élément fautif, il ne fait pas échouer » | Aveu exact et honnête — le brief se trompait, le code a raison. **Non rejoué** de mon côté. |
| `compress` : « le verdict décrit le fichier livré, donc l'original quand on le garde » | Exact → M3. |
| `extract` : « la route n'a jamais été exercée sur un vrai PDF » | Toujours vrai. |

---

## 4. Ce qui est solide

Ce lot n'est pas une série d'enveloppes bâclées. Ce qui suit a été vérifié, pas supposé.

- **La discipline des arguments de processus est réelle.** Les cinq appels Ghostscript
  (`crop`, `compress`, `repair`, `rasterize`, `pdfa`) portent tous
  `-dSAFER -dBATCH -dNOPAUSE -dNOOUTERSAVE`, aucun `-dNOSAFER`, `-sOutputFile` toujours sur
  un temporaire. Aucune valeur libre venue du corps de la requête n'atteint un argument :
  langues OCR (liste blanche `ocr.rs:25`), niveaux de compression (`compress.rs:175`),
  variantes PDF/A (`pdfa.rs:160`), formats, orientations, formats de papier, positions —
  tous des `match` vers des `&'static str`. Les nombres sont bornés **et** testés
  `is_finite()` là où ils sont flottants.
- **Le mot de passe de `/api/unlock` ne passe jamais par la ligne de commande** : `@argfile`
  temporaire, un argument par ligne, saut de ligne refusé (`repair.rs:303`), mot de passe
  vide refusé avec une justification produit écrite dans le code. Mauvais mot de passe →
  qpdf exit 2 « invalid password » → **400**, pas 500. Rejoué.
- **Aucun chemin n'est construit à partir d'une entrée.** `validate_id` n'accepte que
  `as_` + 32 hexa minuscules, `display_name` ne sert qu'à l'affichage, le fichier stocké
  s'appelle toujours `file.<ext>`, et `assets::path` re-canonicalise et vérifie le
  confinement dans la racine — la même défense que `resolve_pdf_path`.
- **La détection de type aux octets est faite sérieusement** : magic bytes d'abord, nom de
  fichier seulement pour départager deux conteneurs zip, et un test qui vérifie qu'un zip
  renommé `.pdf` n'est pas un PDF.
- **La validation synchrone avant le créneau de rendu est systématique** : les treize routes
  valident dans le handler *puis* revalident dans `run()`, avec un commentaire qui dit
  pourquoi (le dispatcher de jobs est un second point d'entrée). C'est rare et c'est bien
  fait.
- **Le vocabulaire des verdicts est cohérent de bout en bout.** Les vingt noms de checks
  émis par le Rust correspondent **exactement** — pas un orphelin d'un côté ni de l'autre —
  à l'inventaire de `static/outils/app.js:172-191`, avec un repli lisible pour un nom
  inconnu. `page-count` veut dire la même chose dans six routes.
- **Les cas d'échec silencieux ont été cherchés, souvent trouvés, parfois bien traités** :
  la vérification d'existence *et* de taille du fichier LibreOffice (parce que `soffice`
  sort en 0 après un refus — vérifié), le refus de rendre un PDF vide dans `repair`, le
  `no-gain` de `compress`, le `truncated:` de `rasterize` et d'`extract`, l'avertissement
  qui nomme `/api/ocr` quand `extract` ne trouve pas de couche texte.
- **`/api/health` dit la vérité sur les binaires réellement installés** (`health.rs:8`) —
  c'est ce qui permet de constater depuis l'extérieur que `ocrmypdf` et `soffice` manquent
  dans une image donnée.
- **Le budget de temps et la file d'attente s'appliquent partout** : tout passe par
  `exec::offload`, donc `Budget::start`, le sémaphore et le 429 sur saturation. Aucun
  `Command::output()` en direct dans les treize routes.
- **Le socle compile et tient** : `cargo check --all-targets` propre, **438 tests verts**,
  et les tests unitaires des nouvelles routes portent bien sur les cas limites (parsing de
  plages, injections d'arguments, bornes) sans exiger un binaire externe.

---

## 5. Ordre de correction suggéré

1. **B3 puis B1** — ne pas allumer `PUBLIC_TOOLS` avant le bac à sable ; couper le réseau de
   LibreOffice est le geste le plus urgent et le moins cher des deux.
2. **B2 et S1** — deux verdicts qui mentent. C'est le péché capital de ce service ; il vaut
   mieux les corriger avant que la première page publique ne les affiche.
3. **B4** — quatre lignes de sweeper, et trois textes qui redeviennent vrais.
4. **S5** — une ligne de nginx, sans quoi la promesse « 100 Mo » est fausse dès le premier
   visiteur.
5. **S2, S3, S4** — trois corrections courtes, chacune dans un seul fichier.
6. Le reste au fil de l'eau.

---

## Clôture de B1 et B3 — le bac à sable existe

*Ajouté après coup ; ce qui précède décrit l'état du jour où la revue a été faite.*

**B3 est levé.** `docker-compose.prod.yml` déclare un second service, `md-to-pdf-worker` :
même image, même binaire, `MDPDF_ROLE=worker`, et **`network_mode: none`**. Ghostscript,
LibreOffice, Tesseract, pandoc, WeasyPrint, poppler, qpdf et img2pdf s'y exécutent. L'API et
lui se parlent par un spool de fichiers sur un volume partagé (`src/sandbox.rs`), et le
worker refuse tout programme hors de sa liste blanche de treize noms — une API compromise
n'obtient pas un interpréteur de commandes dans le conteneur isolé.

Ce qui a rendu la chose faisable sans toucher aux routes : tous les enfants de ce service
passaient déjà par un seul point, `helpers::run_command` et `run_pandoc`, réunis en
`helpers::spawn_and_wait`.

**B1 est levé par construction, et c'est mieux qu'un correctif par vecteur.** Le durcissement
du profil LibreOffice reste en place, mais il n'est plus la ligne de défense : le conteneur
qui exécute `soffice` n'a pas d'interface réseau, donc aucun `xlink:href` distant n'a de
chemin, quel que soit le format ou l'astuce.

### Constaté sur la pile de production reconstruite

```
interfaces du worker : lo + les tunnels factices du noyau — pas d'eth0
interfaces de l'API  : lo + eth0
curl depuis le worker : échoue (impossible de résoudre l'hôte)
curl depuis l'API     : 200
worker arrêté  → /api/health = degraded, sandbox = unreachable, conversion refusée en 10 s
worker démarré → /api/health = ok, sandbox = ok, conversion en 200
./test_api.sh   266 tests, 0 échec
```

L'arrêt du worker est la preuve que l'isolement n'est pas décoratif : sans lui, plus aucune
conversion n'aboutit — les convertisseurs sont bien là-bas.

### Ce que le worker ne monte pas

`public/accounts` et `public/sessions`. Il n'a aucune raison de voir une empreinte de mot de
passe ni un jeton de session.

### Deux défauts de production trouvés en faisant ce chantier

- **L'image ne créait que `public/pdf` et `public/cache`.** Docker crée en `root:root` tout
  point de montage absent de l'image, donc les volumes `assets`, `accounts` et `sessions`
  naissaient inaccessibles au service : `POST /api/files` échouait en « Permission denied »
  sur tout déploiement neuf. Les sept répertoires d'état sont désormais créés et possédés
  par `rocket` dans le `Dockerfile`.
- **`public/accounts` et `public/sessions` n'étaient sur aucun volume.** Chaque
  redéploiement aurait effacé tous les comptes, clés et registres de qualité, et déconnecté
  tout le monde. Deux volumes ajoutés.

### Ce qui reste ouvert

`read_only: true` sur le worker a été essayé et retiré, pas oublié : le service désigne ses
fichiers par des chemins relatifs, les convertisseurs doivent donc tourner depuis
`/home/rocket`, et pandoc y écrit un temporaire quel que soit `TMPDIR`. L'activer demanderait
de rendre ces chemins absolus dans tout le service — et n'ajouterait rien à ce qui compte
ici : ce conteneur n'a pas de réseau. Le raisonnement est écrit dans le compose, à côté de la
ligne absente.
