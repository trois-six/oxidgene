+++
title = "Recherche"
description = "Retrouver une personne par son nom ou son numéro SOSA, puis affiner les résultats avec des filtres."
weight = 90
+++

Cette page explique comment retrouver des personnes dans un arbre : la recherche rapide de la barre supérieure, qui propose des noms et des personnes au fil de la saisie, et la page des résultats, avec ses filtres, ses tris et ses deux présentations.

## Recherche rapide

La barre supérieure de l'arbre, de la fiche d'une personne et de la page des résultats contient deux champs, **Nom / # SOSA** et **Prénom(s)**, et un bouton loupe. Utilisez l'un ou l'autre seul, ou les deux ensemble. Appuyez sur `/` en dehors de tout champ de saisie pour placer le curseur dans le champ du nom.

Pendant la saisie, un panneau s'ouvre sous les champs :

- d'abord, jusqu'à cinq noms de l'arbre qui complètent le champ en cours — noms de famille dans le premier, prénoms dans le second —, chacun avec le nombre de personnes qui le portent. Ces nombres tiennent compte de l'autre champ : si un nom de famille est saisi, un prénom ne compte que les personnes portant ce nom. Choisir un nom complète le champ et laisse le panneau ouvert, pour resserrer la liste des personnes ;
- ensuite, jusqu'à six personnes correspondantes, chacune avec son portrait, son nom, ses années et une ligne qui dit qui elle est (« marié avec … », « fils de … »), de quoi distinguer deux homonymes ;
- lorsqu'il y a plus de résultats que le panneau n'en affiche, un lien **Voir les … résultats** en bas mène à la page complète des résultats.

Les flèches **Bas** et **Haut** parcourent les noms puis les personnes, **Entrée** choisit l'élément en surbrillance et **Échap** ferme le panneau. Choisir une personne ouvre l'arbre centré sur elle ; depuis la fiche d'une personne ou d'un couple, c'est la fiche de la personne qui s'ouvre.

Aucune recherche n'est lancée tant que les deux champs réunis comptent moins de deux caractères. Si les suggestions de saisie de l'arbre sont désactivées dans les [paramètres de l'arbre](@/guide/settings.fr.md), le panneau ne liste que des personnes, sans les noms.

## Aller à un numéro SOSA

Saisissez un nombre seul dans **Nom / # SOSA** pour trouver une personne par son numéro SOSA, compté à partir de la racine SOSA de l'arbre. Le panneau affiche la personne qui porte ce numéro ; appuyez sur **Entrée** pour vous y rendre directement. Si l'arbre n'a pas de racine SOSA ou si personne ne porte ce numéro, Entrée lance une recherche ordinaire.

## Ouvrir la page des résultats

Appuyez sur **Entrée** sans élément en surbrillance dans le panneau, ou cliquez sur la loupe, pour ouvrir la page des résultats. Sa barre supérieure reprend votre recherche dans les deux champs : modifiez-les puis appuyez de nouveau sur **Entrée** pour mettre à jour les résultats sur place.

Ouverte sans aucun critère, la page affiche **Saisissez au moins un critère de recherche.**

{{ <screenshot name="search" alt="Résultats de recherche pour le nom LANDREVEL, filtrés sur les personnes ayant un média" lang={lang} /> }}

La ligne au-dessus des résultats donne le nombre exact de personnes trouvées. Chaque résultat affiche :

- le portrait, ou une silhouette selon le sexe, et une bordure colorée selon le sexe ;
- le nom de famille en capitales et les prénoms ;
- les années de naissance et de décès, avec leur précision comme sur l'arbre (`ca 1849`, `< 1917`) ; le baptême remplace une date de naissance manquante et l'inhumation une date de décès manquante ;
- qui est la personne : ses conjoints s'il y en a, sinon ses parents, suivis du nombre d'enfants ;
- le lieu de naissance, s'il est connu.

Cliquez sur un résultat pour retrouver la personne dans la vue d'où vous avez lancé la recherche : l'arbre, centré sur elle, si vous cherchiez depuis l'arbre ; sa fiche si vous cherchiez depuis la fiche d'une personne ou d'un couple.

## Filtrer les résultats

Cliquez sur **Filtres** pour ouvrir le panneau des filtres. Les filtres s'appliquent au fil de la saisie, et chaque filtre actif apparaît aussi sous forme d'étiquette au-dessus des résultats : cliquez sur le × d'une étiquette pour retirer ce filtre, ou sur **Effacer les filtres** pour les retirer tous.

Le panneau compte trois groupes, **Individu**, **Événement** et **Relations** :

| Filtre | Ce qu'il retient |
|---|---|
| **Nom**, **Prénom(s)** | Les personnes dont les noms contiennent le texte, sans tenir compte des majuscules ni des accents. Ce sont les mêmes champs que ceux de la barre supérieure : saisir dans l'un met l'autre à jour. |
| **Sexe** | **Tous**, **Masculin**, **Féminin** ou **Inconnu** |
| **Profession** | Les personnes dont une profession correspond au texte |
| **Né(e) entre**, **Décédé(e) entre** | Les personnes nées ou décédées entre les deux années, incluses |
| **Lieu** | Les personnes ayant un événement, individuel ou de couple, dans un lieu correspondant |
| **Type d'événement** | Les personnes ayant un événement de ce type : Naissance, Décès, Baptême, Inhumation, Mariage, Résidence, Profession ou Recensement |
| **Événement entre** | Les personnes ayant un événement entre les deux années, incluses |
| **Conjoint(e)** | Les personnes ayant un conjoint de ce nom et de ces prénoms |
| **Père**, **Mère** | Les personnes dont le père ou la mère porte ce nom et ces prénoms |
| **Avec un média** | Les personnes ayant au moins un média rattaché |

Les champs de nom, de profession et des proches proposent les noms et professions présents dans votre arbre. Ils ne listent que les valeurs de l'arbre : un terme que personne ne porte ne trouverait personne.

Si rien ne correspond, la page l'indique et propose **Effacer les filtres**.

## Trier et changer de présentation

Le sélecteur de tri au-dessus des résultats propose :

- **Pertinence** : d'abord les noms de famille qui commencent par ce que vous avez saisi, puis les prénoms correspondants, puis les correspondances trouvées plus loin dans le nom ;
- **Nom A → Z** (par défaut) et **Nom Z → A**, par nom de famille puis prénoms ;
- **Date de naissance ↑** (les plus anciens d'abord) et **Date de naissance ↓** (les plus récents d'abord).

Les deux boutons à droite basculent entre la **Vue liste** et la **Vue grille d'ascendances**. Dans la grille, chaque personne est une carte avec son nom, ses années et une petite ascendance de ses parents et grands-parents. Cliquez sur l'en-tête de la carte, ou sur une personne de la petite ascendance, pour vous rendre à cette personne de la même façon.

La liste affiche 25 personnes par page et la grille 20. Les boutons de page en bas permettent de passer d'une page à l'autre ; changer de présentation ramène à la première page.

## Imprimer les résultats

Le bouton d'impression de la barre supérieure imprime la page de résultats affichée, dans la présentation et l'ordre courants. La feuille porte votre recherche en titre, indique le nombre de résultats et les filtres actifs, et précise quelle page elle contient. Pour imprimer d'autres résultats, affichez d'abord la page voulue.

Pour aller plus loin : [spécification de la page des résultats](@/tech/ui-search-results.md) (en anglais).
