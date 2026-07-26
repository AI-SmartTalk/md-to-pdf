# Rapport d'activité — quatrième trimestre

Document de démonstration : il n'existe que pour montrer ce qu'un thème fait de
chaque élément d'un document. Les chiffres sont fictifs.

## Synthèse

L'activité progresse de **18 %** sur le trimestre, portée par les conversions
*assistées* et par la mise en service de la nouvelle passerelle documentaire. Le
détail des mesures est publié sur [aismarttalk.tech](https://aismarttalk.tech)
et dans l'[annexe technique](https://pdf.aismarttalk.tech/api/health)[^methode].

[^methode]: Les volumes sont comptés à la requête aboutie ; les reprises après
erreur ne sont comptées qu'une fois.

| Indicateur              | T3      | T4      |  Écart |
|-------------------------|---------|---------|-------:|
| Documents convertis     | 128 400 | 151 900 | +18,3 % |
| Temps de rendu médian   | 1,9 s   | 1,2 s   | −36,8 % |
| Requêtes servies du cache | 0 %   | 41 %    |      — |
| Incidents               | 3       | 0       |     −3 |

::: success
**Objectif tenu.** Le temps de rendu médian repasse sous les deux secondes pour
la première fois depuis la mise en production.
:::

> Un rapport se lit une fois et se cite pendant des années : ce qui compte, c'est
> qu'il reste lisible imprimé, en noir et blanc, deux ans plus tard.

```json
{
  "markdown": "# Rapport",
  "options": { "theme": "aismarttalk@1", "page_numbers": true }
}
```

## Éléments de mise en page

### Hiérarchie typographique

#### Titre de quatrième niveau

##### Titre de cinquième niveau

###### Titre de sixième niveau

Corps de texte courant, suivi d'une `valeur littérale` insérée dans la phrase et
d'une note de bas de page[^cache]. Les paragraphes doivent tenir sur la ligne de
base du thème sans veuve ni orpheline en bas de page.

[^cache]: Le cache est adressé par le contenu : deux requêtes identiques ne
rendent qu'une fois.

### Listes imbriquées

1. Préparer la source
   - Nettoyer le Markdown
   - Résoudre les images
     1. Vérifier le domaine
     2. Mesurer le poids
2. Rendre le document
   - Appliquer le thème
   - Numéroter les pages
3. Archiver le résultat

### Figures

![Aperçu du composant de conversation, flouté pour l'exemple.](static/blured.png)

### Tableau large

::: wide
| Référence | Client        | Gabarit    | Pages | Moteur     | Rendu   | Cache | État     |
|-----------|---------------|------------|------:|------------|--------:|-------|----------|
| DOC-4102  | Atelier Nord  | contrat    |    12 | weasyprint |  1,10 s | froid | publié   |
| DOC-4103  | Atelier Nord  | avenant    |     3 | weasyprint |  0,42 s | chaud | publié   |
| DOC-4104  | Groupe Vernet | rapport    |    48 | weasyprint |  3,80 s | froid | publié   |
| DOC-4105  | Groupe Vernet | annexe     |     9 | weasyprint |  0,90 s | chaud | brouillon |
| DOC-4106  | Maison Perrin | facture    |     1 | weasyprint |  0,21 s | chaud | publié   |
:::

### Encadrés

::: warning
**À surveiller.** Le quota de rendu simultané est atteint deux fois par semaine
aux heures de pointe.
:::

::: danger
**Bloquant.** Un document dont les images pointent vers un hôte non autorisé est
refusé avant tout rendu.
:::

### Tableau long

| Semaine | Documents | Pages   | Cache | Incidents |
|--------:|----------:|--------:|------:|----------:|
|      40 |     9 120 |  38 400 |  22 % |         0 |
|      41 |     9 980 |  41 100 |  28 % |         0 |
|      42 |    10 340 |  43 900 |  31 % |         1 |
|      43 |    11 010 |  46 200 |  35 % |         0 |
|      44 |    11 560 |  48 800 |  38 % |         0 |
|      45 |    12 200 |  51 300 |  40 % |         0 |
|      46 |    12 480 |  52 700 |  41 % |         0 |
|      47 |    13 100 |  55 000 |  43 % |         0 |
|      48 |    13 640 |  57 200 |  44 % |         0 |
|      49 |    14 020 |  58 900 |  45 % |         0 |
|      50 |    14 380 |  60 400 |  46 % |         0 |
|      51 |    14 900 |  62 100 |  47 % |         0 |
|      52 |     9 300 |  38 200 |  49 % |         0 |

### Code

```rust
pub fn resolve(spec: Option<&str>) -> Result<Option<&'static Theme>, AppError> {
    let spec = match spec.map(str::trim) {
        Some(spec) if !spec.is_empty() => spec,
        _ => return Ok(None),
    };

    let (name, version) = parse_spec(spec)?;
    find(&name, version).map(Some)
}
```

---

Fin du document de démonstration.
