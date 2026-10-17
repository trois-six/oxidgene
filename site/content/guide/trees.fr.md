+++
title = "Les arbres et la page d’accueil"
description = "Retrouver, créer, renommer, dupliquer, configurer et supprimer vos arbres, y importer des données, et désigner la racine SOSA et la personne « Qui suis-je ? » de chacun."
weight = 20
+++

La page d’accueil rassemble tous vos arbres généalogiques. Cette page explique comment retrouver un arbre, en créer un nouveau, le gérer depuis sa carte, et choisir les deux personnes autour desquelles il s’organise : sa racine SOSA et vous-même.

{{ <screenshot name="home" alt="Page d’accueil avec trois cartes d’arbres" lang={lang} /> }}

## La page d’accueil

La page d’accueil, intitulée **Mes Arbres Généalogiques**, affiche une carte par arbre. Chaque carte présente :

- le nom de l’arbre, et sa description s’il en a une ;
- **Personnes modifiées récemment** : les cinq personnes de l’arbre que vous avez modifiées en dernier, de la plus récente à la plus ancienne, avec leur portrait et leurs dates. Un import ne compte pas comme une modification : un arbre que personne n’a retouché depuis son import affiche **Aucune personne modifiée pour l’instant** ;
- la date de sa dernière modification (**Modifié aujourd’hui**, **Modifié il y a 3 jours**…), avec un badge **Récent** s’il a été modifié dans les dernières 24 heures ;
- un lien **Ouvrir**.

Cliquez n’importe où sur une carte pour ouvrir l’arbre. Cliquez sur l’une des personnes modifiées récemment pour ouvrir l’arbre centré sur elle.

L’icône en forme d’engrenage, en haut à droite de la page, ouvre les paramètres de l’application (voir [Paramètres](@/guide/settings.fr.md)).

## Retrouver un arbre

Quand vous avez de nombreux arbres, servez-vous de la barre d’outils au-dessus des cartes :

- Tapez dans **Rechercher un arbre…** pour ne garder que les arbres dont le nom ou la description contient le texte saisi.
- Choisissez un ordre : **Récemment modifiés** (par défaut), **Nom A → Z** ou **Nom Z → A**.
- Passez de **Grille des arbres** à **Liste des arbres** (une carte par ligne) avec les deux boutons d’affichage.

Si rien ne correspond à votre recherche, la page indique **Aucun arbre trouvé**. Si vous n’avez encore aucun arbre, elle affiche **Aucun arbre** et un bouton **Nouvel arbre**.

## Créer un arbre

1. Cliquez sur **Nouvel arbre**, ou sur la carte **Créer un nouvel arbre** à la fin de la grille.
2. Saisissez un **Nom**. Il est obligatoire.
3. Si vous le souhaitez, ajoutez une **Description (facultatif)** : la famille, la région ou la période couverte par l’arbre, par exemple.
4. Cliquez sur **Créer**.

Le nouvel arbre est vide. Vous pouvez y ajouter une première personne depuis la vue arbre (voir [Saisie](@/guide/editing.fr.md)), ou y importer un fichier venant d’un autre logiciel de généalogie (voir [Import et export](@/guide/import-export.fr.md)).

## Gérer un arbre depuis sa carte

Chaque carte porte, en haut à droite, un menu à trois points. Il propose :

| Entrée | Effet |
|---|---|
| **Ouvrir** | Ouvre l’arbre. |
| **Renommer** | Ouvre **Renommer l’arbre**, où vous saisissez le nouveau **Nom** avant de cliquer sur **Enregistrer**. Vous pouvez aussi renommer un arbre depuis ses paramètres. |
| **Dupliquer** | Crée une copie complète de l’arbre, sous son nom suivi de « (copie) ». |
| **Importer** | Ouvre la fenêtre d’import de cet arbre (voir [Import et export](@/guide/import-export.fr.md)). |
| **Paramètres** | Ouvre les paramètres de l’arbre (voir [Paramètres](@/guide/settings.fr.md)). |
| **Supprimer** | Supprime l’arbre après confirmation dans **Supprimer l’arbre**. Cette action est irréversible. |

Dupliquer un arbre permet d’essayer d’abord sur une copie une opération risquée, comme un gros import ou une série de fusions.

### Pendant un import

Pendant qu’un fichier est importé dans un arbre, sa carte est recouverte de la mention **Import en cours** et ne peut être ni ouverte, ni renommée, ni supprimée. Elle redevient disponible d’elle-même une fois l’import terminé, même si vous rechargez la page entre-temps.

## Désigner la racine SOSA

La racine SOSA est la personne à partir de laquelle les ancêtres de l’arbre sont numérotés selon le système de Sosa-Stradonitz : la racine porte le numéro 1, le père de la personne *n* le numéro 2*n* et sa mère le 2*n* + 1. La vue arbre s’ouvre sur cette personne, les ancêtres y affichent leur numéro SOSA, et l’outil de complétude de l’ascendance compte les générations à partir d’elle.

1. Ouvrez les **Paramètres** de l’arbre, depuis le menu de sa carte ou depuis l’icône en forme d’engrenage de la barre latérale gauche de l’arbre.
2. Dans **Arbre et racines**, repérez la carte **Personne racine (SOSA 1)**.
3. Cliquez sur **Modifier**, recherchez la personne et sélectionnez-la.

Le choix est enregistré immédiatement. **Effacer** le retire ; la carte affiche alors **Aucune personne racine sélectionnée**.

## Indiquer qui vous êtes

La carte **Qui suis-je ?**, juste en dessous, désigne la personne qui vous représente dans l’arbre. Elle ne change rien à la généalogie ni à la numérotation SOSA, mais elle :

- signale votre personne par un badge dans la vue arbre, et par un badge **Moi** sur sa fiche ;
- vous propose comme raccourci quand vous cherchez le lien de parenté entre deux personnes (voir [Parenté](@/guide/kinship.fr.md)) ;
- vous désigne comme émetteur des fichiers GEDCOM exportés lorsqu’aucun autre nom n’est indiqué.

Cliquez sur **Modifier** pour choisir la personne, ou sur **Effacer** pour retirer ce choix. Ce réglage appartient à l’arbre : tous ceux qui l’utilisent voient la même personne.

En savoir plus : [spécification de la page d’accueil](@/tech/ui-home.md) (en anglais).
