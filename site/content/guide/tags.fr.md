+++
title = "Étiquettes"
description = "Classer les personnes, les couples et les documents avec vos propres étiquettes, les colorer, les retrouver dans la recherche et les gérer dans le Dictionnaire."
weight = 75
+++

Une étiquette est un court libellé que vous posez sur des fiches pour les retrouver : *à vérifier*, *acte introuvable*, *Soldats 1914–1918*. Les étiquettes sont votre propre vocabulaire. OxidGene n'en fournit aucune et ne traduit jamais celles que vous écrivez. Un arbre n'a qu'un seul vocabulaire : une étiquette créée sur une personne est la même étiquette sur toutes les autres fiches.

Pour l'instant, les étiquettes se posent sur les **personnes**, les **couples** et les **documents** (photos, numérisations et PDF). Les événements, sources, citations, lieux, dépôts et notes pourront en recevoir plus tard.

## Ajouter ou retirer une étiquette

Sur la fiche d’une personne, cliquez sur le bouton **Étiquettes…** de l’en-tête. Sur la fiche d’un couple, il se trouve dans la rangée d’actions. Pour un document, utilisez **Étiquettes…** dans le menu de la visionneuse ou dans le menu de la vignette.

Le sélecteur liste les étiquettes de l’arbre, celles déjà posées sur la fiche en premier :

- Saisissez dans le champ pour réduire la liste. Les accents et les majuscules ne comptent pas.
- **Cochez** une étiquette pour la poser sur la fiche, et **décochez-la** pour la retirer. Chaque changement est enregistré aussitôt : il n’y a pas de bouton **Enregistrer**.
- Quand aucune étiquette ne correspond à ce que vous avez saisi, choisissez **Créer** pour en créer une et la poser sur la fiche. Une nouvelle étiquette reçoit une couleur que les autres étiquettes de l’arbre n’utilisent pas encore. Vous pourrez la changer plus tard dans le Dictionnaire.
- Au clavier, les flèches déplacent la sélection, Espace ou Entrée coche une étiquette, et Échap ferme le sélecteur.

Deux écritures qui ne diffèrent que par la casse ou les accents sont une seule étiquette : saisir *a verifier* retrouve celle créée sous le nom *À vérifier*.

## Où les étiquettes apparaissent

Les étiquettes ne prennent aucune place sur une fiche qui n’en a pas.

- Sur la fiche d’une personne ou d’un couple, leurs étiquettes apparaissent sous l’en-tête, sous forme de pastilles avec leur couleur.
- Dans les résultats de recherche, les étiquettes d’une personne apparaissent sous forme de petits points après le nom. Survolez un point pour lire son étiquette.
- Cliquez sur une pastille ou un point pour ouvrir l’onglet **Étiquettes** du Dictionnaire sur cette étiquette.

## L’onglet Étiquettes

Ouvrez le **Dictionnaire** depuis la barre latérale de gauche et choisissez l’onglet **Étiquettes**. Chaque ligne est une étiquette, avec sa couleur et le nombre de fiches qui la portent, par type : personnes, couples, documents, etc. Cliquez sur la flèche d’une ligne pour lister ces fiches, regroupées par type, et ouvrir celle que vous voulez.

- **Modifier** (le crayon) change le libellé et la couleur. Si le nouveau libellé est celui d’une autre étiquette, OxidGene propose de **fusionner** les deux : chaque fiche portant l’une ou l’autre porte ensuite l’étiquette fusionnée, une seule fois.
- **Supprimer** (la croix) retire l’étiquette de toutes les fiches qui la portent. La confirmation indique combien elles sont. L’opération est définitive.
- **Nouvelle étiquette** crée une étiquette que vous poserez ensuite sur des fiches.

L’onglet **Médias** garde son nuage d’étiquettes, qui liste les étiquettes portées par les documents.

## Retrouver des personnes par étiquette

Dans la **Recherche**, ouvrez les filtres et choisissez **Étiquettes**. Saisissez le nom d’une étiquette et choisissez-la : chaque étiquette choisie devient une pastille, et les résultats ne gardent que les personnes qui portent **toutes** les étiquettes choisies. Voir [Recherche](@/guide/search.fr.md).

## Étiquettes et fichiers

Un export GEDCOM ou GEDZIP conserve vos étiquettes, et leur import dans OxidGene les restitue, avec leurs couleurs. Les autres logiciels de généalogie les ignorent. Un fichier GeneWeb `.gw` n’a pas de place pour elles : il n’en contient aucune.

En savoir plus : [spécification des étiquettes](@/tech/tags.md) et [modèle de données](@/tech/data-model.md).
