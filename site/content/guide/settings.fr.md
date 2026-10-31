+++
title = "Paramètres"
description = "Paramètres de l’arbre (racines, confidentialité, seuils des anomalies, historique, export) et de l’application (thèmes, langue, pedigree, noms, affichage des dates, options de saisie, API et assistant IA)."
weight = 150
+++

OxidGene distingue deux sortes de réglages. Les **paramètres de l’arbre** appartiennent à un arbre et valent pour tous ceux qui l’utilisent. Les **paramètres de l’application** sont vos préférences sur cet appareil : l’apparence de l’application, sa langue, le tracé des pedigrees, le classement des noms, l’écriture des dates et l’aide à la saisie. Cette page passe les deux en revue.

Chaque réglage est enregistré dès que vous le modifiez ; il n’y a pas de bouton d’enregistrement à ne pas oublier, sauf pour le nom de l’arbre, l’émetteur et les seuils des anomalies.

## Ouvrir les paramètres

- **Paramètres de l’arbre** : sur la page d’accueil, ouvrez le menu à trois points de la carte de l’arbre et choisissez **Paramètres**, ou cliquez sur l’icône en forme d’engrenage de la barre latérale gauche, sur n’importe quelle page de l’arbre.
- **Paramètres de l’application** : cliquez sur l’icône en forme d’engrenage en haut à droite de la page d’accueil.

{{ <screenshot name="tree-settings" alt="Page des paramètres d’un arbre, avec son menu de gauche" lang={lang} /> }}

Le menu de gauche des paramètres de l’arbre compte quatre groupes : **Paramètres** (**Arbre et racines**, **Confidentialité**, **Seuils des anomalies**), **Outils** (**Historique**), **Exporter** (**Exporter l’arbre**) et **Préférences globales**, qui reprend les paramètres de l’application pour que vous y accédiez depuis un arbre.

## Arbre et racines

- **Nom de l’arbre** : saisissez le nouveau nom et cliquez sur l’icône de disquette à côté.
- **Personne racine (SOSA 1)** et **Qui suis-je ?** : la personne à partir de laquelle l’arbre est numéroté, et celle qui vous représente. Voir [Les arbres](@/guide/trees.fr.md) pour le rôle de chacune.
- **Confidentialité par défaut** : le même réglage que dans la section Confidentialité ci-dessous.

## Confidentialité

La **Confidentialité par défaut** dit ce que signifie « suit l’arbre » pour chaque personne, couple et document de l’arbre : **Privé** ou **Public**. Chaque personne, couple ou document peut toujours en décider autrement dans son propre formulaire ; l’arbre ne tranche que pour ceux qui n’ont rien choisi.

Un nouvel arbre est Privé : une généalogie contient des personnes vivantes, et la publication doit être un choix délibéré.

La confidentialité est enregistrée dès aujourd’hui, mais ne masque encore rien. Comme l’indique la section : **Enregistré dès maintenant. Rien n’est encore masqué — cela prendra effet avec les comptes et la publication.**

## Seuils des anomalies

{{ <screenshot name="tree-settings-anomalies" alt="Section Seuils des anomalies des paramètres de l’arbre, ses seuils en quatre cartes : dates, filiation, unions et lieux" lang={lang} /> }}

**Seuils des anomalies** fixe les limites qu’appliquent à cet arbre les [règles d’anomalies](@/guide/tools.fr.md#reperer-les-anomalies) de la page Outils : un âge, un écart ou une distance n’est signalé qu’au-delà. Les valeurs par défaut conviennent à la plupart des arbres ; adaptez-les à une population ou à une époque, par exemple un arbre d’ancêtres très âgés ou de mariages très précoces.

Les seuils sont rangés en quatre cartes, comme les anomalies : **Dates** (vie la plus longue, vie la plus longue avant une année donnée, cette année, âge maximal sans décès, tolérance sur l’âge noté, marge des dates approximatives), **Filiation** (parent le plus jeune, père et mère les plus âgés, naissance après le décès du père, jumeaux, enfants trop rapprochés ou trop éloignés), **Unions** (conjoints le plus jeune et le plus âgé à l’union, écart de naissance des conjoints) et **Lieux** (lieux les plus éloignés d’un même jour, en km).

1. Saisissez les nouvelles valeurs. Une valeur hors de sa plage le signale sous son champ (« Entre 80 et 150. »), et deux valeurs qui doivent s’accorder le signalent au-dessus des boutons ; **Enregistrer** reste désactivé tant qu’elles ne sont pas corrigées.
2. Cliquez sur **Enregistrer**. La page Outils applique alors les nouveaux seuils, et ses règles les citent dans leurs titres et leurs phrases.

**Rétablir les valeurs par défaut** remplit chaque champ de sa valeur par défaut ; cliquez sur **Enregistrer** pour les garder. Chaque modification est consignée dans l’[Historique](#historique) de l’arbre, seuil par seuil.

## Historique

L’**Historique**, dans le groupe **Outils**, est le journal de l’arbre : chaque modification, de la plus récente à la plus ancienne, y compris les réglages, les médias, les imports, les exports et les restaurations. Filtrez-le avec **Tout**, **Données**, **Paramètres**, **Médias**, **Imports**, **Exports** et **Restaurations**, et cliquez sur **Afficher plus** pour remonter plus loin. **Voir la modification** compare une fiche avant et après, et **Restaurer la version précédente** annule cette modification pour cette fiche. Voir [Historique](@/guide/history.fr.md).

Les autres outils d’un arbre (anomalies, lieux non localisés, doublons, convertisseur de dates…) se trouvent sur la page Outils, accessible par l’icône en forme de clé de la barre latérale de l’arbre. Voir [Outils](@/guide/tools.fr.md).

## Exporter l’arbre

Téléchargez l’arbre en GEDCOM ou en GEDZIP, choisissez le logiciel de destination et les options, et renseignez l’émetteur. Voir [Import et export](@/guide/import-export.fr.md).

## Paramètres de l’application

Les paramètres de l’application comptent sept sections : **Apparence**, **Langue**, **Pedigree**, **Noms**, **Affichage des dates**, **Options de saisie** et **API**. Toutes sauf API figurent aussi sous **Préférences globales** dans les paramètres de chaque arbre ; c’est la même préférence aux deux endroits, et non un réglage de l’arbre, et elle vaut pour tous les arbres que vous ouvrez sur cet appareil.

Ces préférences sont conservées sur cet appareil. Sur un autre ordinateur, ou dans un autre navigateur, vous les choisissez de nouveau.

{{ <screenshot name="app-settings-dark" alt="Sélecteur de thème des paramètres de l’application, dans le thème sombre" lang={lang} /> }}

### Apparence

La carte **Thème** présente chaque thème sous forme de petite vignette d’aperçu : ses couleurs, ses polices, ses angles et ses espacements. **Clair** et **Sombre** en font partie. Cliquez sur une vignette et toute l’application change aussitôt.

OxidGene ne bascule pas de lui-même entre clair et sombre : il garde le thème que vous avez choisi.

#### Vos propres thèmes (application de bureau)

Dans l’application de bureau, vous pouvez ajouter vos propres thèmes sous forme de fichiers JSON. La section Apparence indique le dossier où les placer (`~/.config/oxidgene/themes/` sous Linux). Déposez-y un fichier et rouvrez la section : le thème apparaît avec l’étiquette **Personnalisé**. Un fichier illisible est signalé sous le sélecteur, avec la raison.

Un thème n’a besoin d’indiquer que ce qui diffère d’un thème intégré :

```json
{
  "id": "sepia",
  "name": "Sépia",
  "base": "light",
  "colors": { "orange": "#8a5a2b", "bg-deep": "#f6f0e4" },
  "style": { "heading_font": "serif", "corners": "round" }
}
```

- `id` : lettres minuscules, chiffres et `-`, différent des thèmes intégrés.
- `name` : le nom affiché sous la vignette.
- `base` : le thème intégré dont on part.
- `colors` : les couleurs à changer, uniquement en valeurs hexadécimales.
- `style` : des préréglages facultatifs pour les polices (`body_font`, `heading_font`), les angles (`corners`), les ombres (`elevation`) et la densité (`density`).

La liste complète des noms de couleurs et des préréglages figure dans la [spécification de l’interface commune](@/tech/ui-common.md) (en anglais).

### Langue

Cliquez sur une langue pour changer aussitôt celle de l’interface : English, Français, Deutsch, Español, Italiano, Nederlands, Polski ou Português. Votre généalogie elle-même (noms, notes, sources) n’est jamais traduite.

#### Vos propres langues (application de bureau)

Dans l’application de bureau, vous pouvez ajouter une langue sous forme de fichier JSON portant son code, par exemple `ca.json`. La section Langue indique le dossier où le placer (`~/.config/oxidgene/languages/` sous Linux). Déposez-y le fichier et rouvrez la section pour le voir dans la liste. Partez de l’un des fichiers de langue de l’application, dans son dépôt de sources (`assets/i18n/`) : un texte absent de votre fichier s’affiche en anglais, et un fichier illisible est signalé par **Fichier de langue invalide.**

### Pedigree

**Type de pedigree** choisit la façon dont la vue arbre dessine le pedigree sur cet appareil :

- **Graphique** : **Arbre**, **Roue des ancêtres**, **Éventail**, **Roue des descendants**, **Éventail des descendants**, **Lignée**, **Lignée des descendants**, **Sablier** ou **Papillon**. Chaque vignette décrit le graphique en une ligne.
- **Thème du pedigree** : **Classique** (cartes sobres et connecteurs courbes) ou **Médiéval** (blasons et traits à l’équerre).

**Profondeur du pedigree** fixe le nombre de générations affichées à la première ouverture d’un arbre : **Générations ascendantes** (4 par défaut) et **Générations descendantes** (3 par défaut), chacune de 0 à 10. Un arbre dont vous avez déjà ajusté la vue garde ses propres profondeurs.

Voir [Le pedigree](@/guide/pedigree.fr.md) pour ce que montre chaque graphique.

### Noms

**Tri des patronymes** décide où se classe un nom de famille à particule dans les listes et le dictionnaire :

- **Particule incluse** (par défaut) : « de la Cruz » est classé à la lettre D.
- **Particule ignorée** : « de la Cruz » est classé à la lettre C.

Le nom est toujours affiché en entier ; seule sa place dans l’ordre alphabétique change.

### Affichage des dates

Comment les pages écrivent les dates, dans tous les arbres que vous ouvrez sur cet appareil. Vos dates elles-mêmes ne sont jamais modifiées, seule leur présentation change. Un **Aperçu**, sous les réglages, montre quelques dates telles que les pages les écriront.

- **Format des dates** : la date complète avec le mois abrégé (par défaut), la forme numérique (`12/03/1842`), le mois et l’année, ou l’année seule. Chaque option de la liste est écrite sous la forme qu’elle produit. La précision d’une date, comme « avant » ou « vers », est toujours écrite, quel que soit le format.
- **Symboles des événements** : avec **Oui**, les dates de vie s’écrivent `* 1842 + 1907` au lieu de `1842-1907`.
- **« Circa » pour les dates approximatives** : avec **Oui**, une date approximative s’écrit *v. 1842* plutôt que *vers 1842*.
- **Calendrier d’affichage** : **Grégorien** (par défaut), **Julien**, **Républicain** ou **Hébreu**. Une date enregistrée dans un autre calendrier est suivie de son équivalent dans celui-ci, entre parenthèses. Les années seules, comme sur les fiches du pedigree, restent grégoriennes.

### Options de saisie

Comment les formulaires vous aident pendant la saisie, dans tous les arbres que vous ouvrez sur cet appareil.

- **Suggestions de saisie** (Oui par défaut) : les champs de lieu, de nom, de prénom, de profession et de source proposent, pendant la saisie, ce que l’arbre contient déjà et les dictionnaires intégrés. Vous pouvez toujours taper autre chose.
- **Noms de famille en majuscules** (Oui par défaut) : les noms de famille s’écrivent en capitales au fil de la frappe. Avec **Non**, un nom est enregistré tel que tapé.
- **Proposer les personnes existantes** (Oui par défaut) : quand vous ajoutez un parent, un conjoint, un enfant, un frère ou une sœur depuis la vue arbre, les personnes déjà présentes sont proposées avant d’en créer une. Avec **Non**, seule une nouvelle personne est proposée.
- **Format de saisie des dates** : l’ordre des champs jour, mois et année de chaque champ de date : jour/mois/année (par défaut), jour-mois-année, année-mois-jour (ISO 8601), ou jour, mois choisi par son nom, année.
- **Calendrier de saisie par défaut** : le calendrier dans lequel commence un champ de date vide. Chaque champ peut toujours passer à un autre.

Les suggestions de lieux viennent d’un dictionnaire des lieux intégré à l’application, qui couvre douze pays. Il ne demande ni téléchargement ni accès au réseau.

### API

Cette section sert à relier d’autres programmes à OxidGene ; vous n’en avez pas besoin pour votre généalogie au quotidien.

- **Spécification OpenAPI** ouvre la description de l’API REST.
- **Connecter un client** donne l’adresse de l’API et un exemple de commande à copier.
- Dans l’application de bureau, la section affiche aussi un **Jeton d’accès**. Quiconque le détient peut lire et modifier tous les arbres tant que l’application est ouverte : ne le confiez qu’à des outils de confiance. L’adresse et le jeton changent à chaque démarrage de l’application.
- Dans la version web, l’adresse GraphQL et un exemple sont également indiqués.

La carte **Assistant IA (MCP)**, dans l’application de bureau, donne la commande qui permet à un assistant IA de lire vos arbres, et indique comment l’autoriser aussi à les modifier. Voir [Assistant IA](@/guide/assistant.fr.md).

En savoir plus : [spécification des paramètres de l’arbre](@/tech/ui-settings.md), [spécification des paramètres de l’application](@/tech/ui-app-settings.md) (en anglais).
