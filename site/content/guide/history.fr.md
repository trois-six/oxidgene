+++
title = "Historique"
description = "Parcourir chaque état enregistré d'une personne, comparer des versions champ par champ et restaurer une version antérieure."
weight = 140
+++

OxidGene enregistre chaque modification apportée à un arbre et conserve les états antérieurs de vos fiches. Cette page explique comment lire l'historique d'une personne, comparer deux versions, restaurer une version antérieure, faire revenir une personne supprimée et consulter le journal des modifications de l'arbre.

## Ce que conserve l'historique

Chaque écriture dans un arbre est inscrite dans son journal des modifications : données, paramètres, médias, imports et exports, restaurations.

Pour une personne, chaque modification conserve l'état qu'elle avait juste avant. Une version contient tout ce que montre sa fiche, sauf les médias : noms, sexe, ses propres événements avec leurs témoins, notes et sources, ses notes et ses sources, ses parents, et ses unions avec leurs événements. Les médias figurent dans le journal, mais ne sont jamais versionnés.

Bon à savoir :

- Une personne que personne n'a modifiée depuis sa création ou son import n'a qu'une version, sa version actuelle. Un import ne conserve lui-même aucun état antérieur.
- Une modification qui touche plusieurs personnes conserve une version pour chacune de celles qu'elle a réellement changées. Ajouter un enfant à une famille conserve l'état antérieur des deux conjoints et de l'enfant ; renommer le père ne conserve que le sien.
- Les lieux, les sources, les dépôts et les paramètres de l'arbre ont eux aussi des versions.
- Le journal n'indique pas qui a fait une modification.

## Ouvrir l'historique d'une personne

1. Ouvrez la fiche de la personne.
2. Cliquez sur **Historique** dans l'en-tête de la fiche.

La page a pour titre « Historique — » suivi du nom de la personne. À gauche, les versions sont listées de la plus récente à la plus ancienne, par lots de 100 (**Afficher plus** montre les plus anciennes). Chacune indique :

- **Version** et son numéro, avec *actuelle* pour la personne telle qu'elle est aujourd'hui ;
- le moment de la modification, dans votre langue et à votre heure locale ;
- ce qui l'a produite, par exemple *Modification — événement (Naissance)*, *Import* ou *Restauration* ; la plus ancienne version, quand aucune modification enregistrée ne l'a produite, indique **État initial** ;
- les détails éventuels : le fichier d'un import, *version 3 restaurée*, *fusionné avec* une autre personne.

{{ <screenshot name="history" alt="Versions d'une personne comparées côte à côte sur la page d'historique" lang={lang} /> }}

## Comparer deux versions

La version actuelle est sélectionnée à l'ouverture. Cliquez sur une autre version de la liste pour l'afficher.

- **Comparer avec** choisit la version antérieure de référence ; par défaut, celle qui précède immédiatement.
- **Modifications seulement**, activé par défaut, masque les champs inchangés. Décochez-le pour voir la fiche entière.

La comparaison est un tableau, groupé par rubrique (**Identité**, **Noms**, **Événements**, **Notes**, **Sources**, **Parents**, **Unions**), avec la version la plus ancienne à gauche et la plus récente à droite. Chaque changement est marqué **Ajouté**, **Modifié** ou **Retiré**. La plus ancienne version n'a rien à quoi se comparer : tout y apparaît comme ajouté.

Les lieux, les sources et les proches apparaissent sous le nom qu'ils portaient au moment où la version a été conservée : une ancienne version se lit toujours comme à l'époque, même si un lieu ou un proche a été renommé depuis.

## Restaurer une version antérieure

1. Sélectionnez la version à laquelle revenir.
2. Cliquez sur **Restaurer cette version**.
3. Lisez la confirmation : la personne sera remise exactement dans l'état de cette version (noms, événements, notes, sources, parents et unions). Confirmez.

La restauration apparaît en tête de liste comme nouvelle version actuelle, marquée *Restauration* et *version N restaurée*. Comme une restauration est elle-même enregistrée, vous pouvez l'annuler de la même façon : en restaurant la version qu'elle a remplacée.

Une restauration ne fait jamais revenir une autre personne. Si un témoin, un conjoint ou un enfant cité dans l'ancienne version a été supprimé depuis, ce lien est abandonné plutôt que la personne ressuscitée.

## Faire revenir une personne supprimée

La fiche d'une personne supprimée n'existe plus, mais son historique reste accessible.

1. Ouvrez les paramètres de l'arbre avec l'icône d'engrenage de la barre d'icônes à gauche, puis choisissez **Historique** dans le menu de gauche.
2. Repérez la suppression dans le journal et cliquez sur le nom de la personne (ou sur **Historique de la personne**) pour ouvrir son historique.
3. La page affiche un badge **Supprimé** à côté du titre. La version actuelle est l'état supprimé ; elle ne peut pas être restaurée elle-même.
4. Sélectionnez la version juste avant la suppression et cliquez sur **Restaurer cette version**.

La personne revient telle qu'elle était avant la suppression, et son nom dans la barre du haut redevient un lien vers sa fiche.

## Consulter le journal des modifications de l'arbre

La rubrique **Historique** des paramètres de l'arbre est le journal des modifications de tout l'arbre, de la plus récente à la plus ancienne, par lots de 100.

- Filtrez-le avec **Tout**, **Données**, **Paramètres**, **Médias**, **Imports**, **Exports** ou **Restaurations**.
- Chaque entrée donne l'heure, un badge de catégorie, l'action effectuée (par exemple *Modification — événement (Naissance)*, *Ajout — nom*, *Import*) et le nom de ce qu'elle concerne, tel qu'il était alors. Le nom d'une personne mène à son historique.
- Une entrée qui a modifié des fiches propose **Voir la modification** (ou **Voir les N modifications**). Dépliez-la pour comparer, pour chaque fiche modifiée, son état avant et après.
- Chaque modification propose **Restaurer la version précédente**, qui annule cette modification pour cette fiche après confirmation. Une modification de personne mène aussi à son historique complet avec **Historique de la personne**.

Les créations, imports, exports et modifications de médias ne remplacent aucun état antérieur : ils n'ont rien à montrer ni à restaurer.

Une fusion est irréversible ; l'assistant de fusion vous en avertit avant confirmation (voir [Outils](@/guide/tools.fr.md)). Un renommage de nom de famille dans le dictionnaire est inscrit comme une seule entrée, mais donne une nouvelle version à chaque personne renommée : on l'annule donc personne par personne (voir [Dictionnaire](@/guide/dictionary.fr.md)).

Pour aller plus loin : [spécification de l'historique d'une personne](@/tech/ui-person-history.md).
