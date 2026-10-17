+++
title = "Fiches de personne et de couple"
description = "Tout ce que l’application sait d’une personne ou d’un couple, sur une seule page : identité, famille, événements, sources, médias, notes et ancêtres."
weight = 50
+++

La fiche de personne rassemble tout ce qui est connu d’un individu : qui il était, sa famille, les événements de sa vie avec leurs sources, ses photos et documents, ses notes et ses ancêtres. La fiche de couple présente deux conjoints côte à côte et ne dessine qu’une fois ce qu’ils partagent. Cette page explique comment lire l’une et l’autre, et ce que vous pouvez y faire.

{{ <screenshot name="person" alt="Fiche d’Étienne Marie Landrevel avec portrait, notes et galerie" lang={lang} /> }}

## Ouvrir la fiche d’une personne

- Depuis la [vue arbre](@/guide/pedigree.fr.md), sélectionnez une carte puis cliquez sur **Vue profil** dans la barre latérale gauche.
- Depuis les [résultats de recherche](@/guide/search.fr.md), cliquez sur une personne.
- Depuis une autre fiche de personne ou de couple, cliquez sur le nom d’un proche : chaque personne nommée sur ces pages renvoie à sa propre fiche.
- Depuis le panneau des événements de la vue arbre, cliquez sur l’un des événements de la personne.

La barre du haut affiche le nom de l’arbre, qui ramène à la vue arbre, puis le nom de la personne. La barre latérale gauche est celle de la vue arbre : **Vue arbre** revient au graphique centré sur cette personne, **Vue couple** ouvre son premier couple.

## L’en-tête d’identité

L’en-tête présente le portrait (ou une silhouette s’il n’y en a pas), le nom de famille en capitales, les prénoms et les autres noms portés par la personne, comme un nom marital ou un nom de jeune fille.

En dessous, un court résumé donne la naissance et le décès avec la date complète et le lieu, l’âge au décès quand les deux dates sont connues, le sexe et les professions. Les dates gardent leur précision : « vers 1796 », « entre 11 nov. 1691 et 20 août 1693 ». Quand la naissance n’est pas datée, c’est le baptême qui s’affiche, et la phrase le dit (« Baptisé le ») ; de même, l’inhumation remplace un décès non daté (« Inhumé le »).

### Les badges

- Une personne qui a un numéro SOSA porte le badge **SOSA**.
- La personne choisie comme **Qui suis-je ?** dans les paramètres de l’arbre porte en plus un badge **Moi**. Cliquez dessus pour modifier ou effacer ce choix dans **Arbre et racines**.

### Fiches de profession et de prénom

Survolez une profession ou un prénom dans l’en-tête. Quand OxidGene possède une fiche à son sujet, une bulle l’explique : en quoi consistait le métier, ou l’origine et le sens du prénom. Plus de 3 000 professions anciennes et plus de 2 000 prénoms sont couverts, reconnus quelle que soit la langue de l’acte d’où ils ont été relevés. Une fiche pas encore rédigée dans votre langue s’affiche en anglais. Les termes sans fiche restent du texte simple.

### Les actions de l’en-tête

Les petits boutons à icône, à droite de l’en-tête (leur nom s’affiche au survol) :

- **Modifier** ouvre le formulaire de la personne : voir [Saisir et modifier les personnes](@/guide/editing.fr.md).
- **Fusionner avec…** ouvre l’assistant de fusion, pour réunir une autre fiche de la même personne avec celle-ci. Après la fusion, la page affiche la fiche conservée. Voir [Outils](@/guide/tools.fr.md).
- **Historique** liste toutes les versions enregistrées de la personne, comparées côte à côte, et permet d’en restaurer une. Voir [Historique](@/guide/history.fr.md).

## Les liens familiaux

**Liens familiaux** raconte la famille de la personne en phrases que l’on pourrait lire à voix haute : de qui elle est le fils ou la fille, puis chacune de ses unions, avec la date et le lieu du mariage et les enfants qui en sont nés, enfin ses frères et sœurs. Les demi-frères et demi-sœurs sont regroupés selon le parent qu’ils ont en commun. Les unions sont classées selon leur premier événement daté, en général le mariage.

Chaque nom renvoie à la fiche de la personne. La racine SOSA et ses ancêtres y portent aussi leur marque : vous voyez d’un coup d’œil quels proches sont dans votre ascendance directe.

## La chronologie

{{ <screenshot name="person-timeline" alt="Événements d’Étienne Marie Landrevel avec ceux de ses parents, de son épouse et de ses enfants" lang={lang} /> }}

**Événements** liste par ordre de date les événements de la personne et, autour d’eux, ceux de sa famille proche : ses unions, et des événements de ses parents, de ses frères et sœurs et de ses enfants. Les événements sans date viennent en dernier. Chaque ligne précise à qui appartient l’événement :

| Mention | Signification |
|---|---|
| **Individuel** | Un événement de la personne elle-même |
| **Famille conjugale** | Un événement de l’une de ses unions, comme le mariage |
| **Enfant** | Un événement de l’un de ses enfants, dont le nom suit |
| **Famille parentale** | Un événement de ses parents ou de ses frères et sœurs, dont le nom suit |

Les événements propres à la personne et ceux de ses unions ressortent, pour que le contexte ne noie pas sa propre vie.

Chaque événement indique sa date complète, son lieu, sa description et, quand l’acte le donne, l’âge de la personne à ce moment (« à 34 ans », « nourrisson », « mort-né »). L’organisme responsable, s’il est connu, figure sur une ligne à part. Sous l’événement, **Sources** donne la source de chaque citation avec la page ou la référence citée.

Les documents qui prouvent l’événement apparaissent en petites vignettes en dessous. Cliquez sur l’une d’elles pour l’ouvrir dans la visionneuse.

### Ouvrir le registre cité

Quand une citation désigne un registre d’un service d’archives connu d’OxidGene (toutes les archives départementales françaises sont cataloguées), sa source devient un bouton, dont l’infobulle nomme le service d’archives.

1. Cliquez sur la source.
2. Sur l’application de bureau, le registre s’ouvre sur le site du service d’archives, dans une fenêtre d’archives, à la vue citée. Sur le web, il s’ouvre dans un nouvel onglet du navigateur.
3. Si la citation désigne le service d’archives sans suffire à trouver le registre, une fenêtre **Rechercher dans les archives** demande les détails manquants (commune, type de registre, année, vue). Complétez-les puis cliquez sur **Rechercher**. Cochez **Ajouter ces détails à la citation** pour les enregistrer dans la citation.

La page [Sources et archives](@/guide/sources-archives.fr.md) explique comment les citations sont reconnues et que faire quand un portail ne répond pas.

## Les médias

**Médias** présente toutes les photos et tous les documents liés à la personne : ceux qui lui sont rattachés, ceux de ses couples, et les détails de photos où elle a été identifiée, montrés sous la forme de la zone découpée plutôt que de l’image entière. Une étoile marque la photo qui sert de portrait.

- Cliquez sur une vignette pour l’ouvrir dans la visionneuse.
- Cliquez sur le **+** à côté du titre pour ajouter un document.
- Faites un clic droit sur une vignette et choisissez **Lier un événement** pour indiquer quel événement de la personne elle documente, ou **Délier un événement** pour annuler ce lien.

La réorganisation de ce qui est rattaché (choix du portrait, découpage, retrait d’un document de la personne) se fait dans le formulaire de la personne. La page [Photos et documents](@/guide/media.fr.md) décrit la visionneuse et le formulaire de document.

## Notes et ancêtres

**Notes** affiche les notes rédigées sur la personne. La section est masquée s’il n’y en a aucune.

**Ancêtres** dessine un petit arbre de la personne et d’au plus deux générations au-dessus, dans le thème de graphique que vous avez choisi. Survolez une carte pour lire le nom complet et les dates, cliquez dessus pour ouvrir la fiche de cette personne.

## La fiche de couple

{{ <screenshot name="couple" alt="Fiche d’un couple Landrevel avec la photographie du mariage" lang={lang} /> }}

La fiche de couple présente un couple sur une seule page. Ouvrez-la avec **Vue couple** dans la barre latérale gauche, depuis la vue arbre ou une fiche de personne ; le bouton n’apparaît que si la personne a un conjoint connu, et ouvre son premier couple.

Le mari est à gauche, l’épouse à droite. Chaque conjoint a sa colonne, avec les mêmes sections que sur sa fiche :

- son identité, avec un bouton **Profil** pour ouvrir sa propre fiche ;
- ses propres notes ;
- ses propres médias uniquement (ce qui est rattaché au couple n’apparaît qu’une fois, sur toute la largeur) ;
- sa famille : parents, frères et sœurs, et ses **autres** unions avec leurs enfants ;
- ses événements, sans ceux de ce couple ;
- ses ancêtres.

Ce que le couple partage n’est dessiné qu’une fois, sur toute la largeur, en tête de chaque groupe :

- **Union** : une phrase comme « Marié(e) le … à …, dont : », suivie des enfants de cette union ;
- **Événements du couple** : les événements de l’union (mariage, contrat, divorce…) et les naissances, baptêmes, décès et inhumations de ses enfants, avec leurs sources et leurs documents ;
- **Médias du couple** : les photos et documents rattachés au couple, avec un **+** pour en ajouter ;
- **Notes du couple** : les notes sur le couple.

### Passer à un autre couple

Au-dessus des colonnes, un sélecteur surmonte chaque conjoint. Celui de gauche liste les couples du conjoint de droite, celui de droite les couples du conjoint de gauche. Choisissez un nom pour ouvrir ce couple : pour voir le second mari d’une femme, choisissez-le dans le sélecteur de gauche. Un sélecteur est désactivé quand il n’y a pas d’autre couple à proposer.

Quand un couple n’a qu’un seul conjoint enregistré, l’autre colonne indique **Conjoint inconnu**.

### Modifier le couple

**Modifier le couple**, en haut à droite, ouvre le formulaire du couple : l’union et ses événements, les enfants, la confidentialité et les documents. Vous pouvez aussi y détacher un enfant ou supprimer le couple ; la suppression retire seulement l’union, jamais les personnes, et vous ramène à la vue arbre. Voir [Saisir et modifier les personnes](@/guide/editing.fr.md#modifier-un-couple).

## Imprimer une fiche

Cliquez sur **Imprimer** dans la barre latérale gauche, ou appuyez sur Ctrl+P (Cmd+P sur Mac). La fiche s’imprime en entier sur autant de feuilles que nécessaire, en couleurs claires, sous un en-tête portant le nom de la personne ou du couple, le nom de l’arbre et la date. Les boutons et la barre latérale ne s’impriment pas.

Pour aller plus loin : spécifications de la [fiche de personne](@/tech/ui-person-profile.md) et de la [fiche de couple](@/tech/ui-couple-profile.md) (en anglais).
