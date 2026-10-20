+++
title = "Statistiques"
description = "Ce que la page de statistiques révèle d'un arbre dans son ensemble, onglet par onglet."
weight = 120
+++

La page de statistiques montre ce que votre arbre raconte dans son ensemble : son degré de complétude, les régions où vivaient vos ancêtres, leurs prénoms, leur durée de vie, l'âge auquel ils se mariaient, le nombre de leurs enfants, les records qui méritent un coup d'œil, et la façon dont l'arbre lui-même s'est construit. Cette page la parcourt onglet par onglet.

## Ouvrir les statistiques

Cliquez sur l'icône de graphique dans la barre d'icônes à gauche de n'importe quelle page de l'arbre : le pedigree, une fiche de personne ou de couple, le dictionnaire. La première fois, la page s'ouvre sur **Vue d'ensemble** ; ensuite, elle rouvre le dernier onglet consulté.

La page est en lecture seule. Tout est recalculé à partir de votre arbre à chaque visite : elle reflète donc toujours vos dernières modifications.

Chaque graphique porte un titre et une aide **?** qui explique comment ses valeurs sont comptées. Survolez un point d'une courbe pour en lire la valeur.

## Tenir compte des dates approximatives

Par défaut, âges et moyennes n'utilisent que les dates exactes. Cochez **Dates approximatives** dans la barre du haut pour y inclure aussi les dates notées vers, calculées ou estimées. Le réglage vaut pour tous les onglets. Les dates notées avant, après, peut-être, ou entre deux dates, ne servent jamais au calcul d'un âge.

Pour ne compter que les personnes portant une étiquette, choisissez-la dans la liste **Étiquette** de la barre du haut : chaque chiffre ne porte alors que sur ces personnes. Les sources et l’onglet Croissance couvrent toujours tout l’arbre, et l’indiquent.

Quand une date de naissance manque, celle du baptême la remplace ; quand une date de décès manque, celle de l'inhumation la remplace.

## Vue d'ensemble

L'onglet **Vue d'ensemble** réunit les chiffres clés sur trois rangées de tuiles :

- **Effectifs** : personnes (hommes, femmes et sexe inconnu), unions, lieux, sources, années couvertes du premier au dernier événement daté, et nombre de noms de famille et de premiers prénoms distincts.
- **Complétude**, en part du total des personnes : avec une naissance datée, avec un décès daté, sans parent connu, sans enfant, jamais en union. Vous voyez d'un coup d'œil où les recherches restent à faire.
- **Moyennes** : âge au décès et âge à la première union (aussi pour les hommes et pour les femmes), intervalle intergénérationnel (l'âge des parents à la naissance de chaque enfant) et nombre d'enfants par union. Chacune est accompagnée de sa médiane, de son écart-type et de son étendue.

En dessous, le bloc **Événements** présente un anneau des dix types d'événements les plus fréquents. Un événement familial, comme un mariage, n'est compté qu'une fois, quel que soit le nombre de conjoints qui le partagent.

{{ <screenshot name="statistics-overview" alt="Onglet Vue d'ensemble des statistiques avec effectifs, complétude et moyennes" lang={lang} /> }}

## Lieux

L'onglet **Lieux** s'ouvre sur la **Carte des lieux** : une carte de chaleur des endroits où se sont déroulés les événements de votre arbre. Chaque lieu localisé ajoute une tache diffuse, d'autant plus intense que le lieu est souvent utilisé ; les zones chaudes montrent ainsi les régions où votre famille a vécu. La précision est volontairement grossière : la carte montre des régions, pas des villages. Elle fonctionne hors ligne, sans faire appel à aucun service de cartographie.

- Zoomez avec la molette ou les boutons **Zoomer** et **Dézoomer**, faites glisser la carte pour la déplacer, et utilisez **Ajuster aux lieux** pour revoir tous les lieux.
- À côté de la carte, les **10 lieux les plus utilisés** sont listés avec leur nombre d'utilisations et un numéro repris en repère sur la carte. Cliquez sur un repère ou sur un lieu de la liste pour centrer la carte dessus.
- En bas de la liste, une ligne indique combien de lieux n'ont pas pu être localisés. La [page des outils](@/guide/tools.fr.md) les recense et permet de les corriger.

Un lieu est localisé par ses propres coordonnées ou, à défaut, en reconnaissant son nom dans le dictionnaire des lieux intégré. Un hameau écrit avant sa commune (« Le Moulin, Saint-Aubin, … ») est placé sur la commune.

Sous la carte, **Naissances par zone** présente trois anneaux : les naissances par pays, par région et par département (ou comté, district…), les dix premiers de chaque, avec le nombre de pays, de régions ou de départements distincts que couvre votre arbre.

{{ <screenshot name="statistics-map" alt="Carte de chaleur des lieux de l'arbre de la famille Landrevel" lang={lang} /> }}

## Noms et professions

L'onglet **Noms et professions** présente des anneaux pour les dix noms de famille, professions, premiers prénoms masculins et premiers prénoms féminins les plus courants. Seul le premier mot des prénoms compte, et les noms qui ne diffèrent que par les majuscules sont regroupés, affichés dans leur graphie la plus fréquente.

## Population et Familles : les courbes dans le temps

Les onglets **Population** et **Familles** contiennent des graphiques qui suivent votre famille de siècle en siècle. Tous deux s'ouvrent sur une barre qui règle leur axe du temps :

- **Intervalle** : la largeur de chaque période, 10, 25, 50 ou 100 ans (25 par défaut).
- La règle des années : faites glisser ses deux poignées, ou utilisez les flèches du clavier, pour choisir la première et la dernière année affichées. La vue initiale écarte les rares actes très anciens qui étireraient l'axe sur des siècles presque vides ; **Toutes les années** l'élargit à toute la période.

La barre reste en haut pendant le défilement. L'intervalle est mémorisé sur votre appareil ; les années choisies valent pour la visite.

**Population** contient :

- **Naissances et décès** : naissances, baptêmes, unions, décès et inhumations par période ; rapport de masculinité à la naissance (garçons pour 100 filles) ; répartition des naissances par mois ; mortalité infantile et juvénile (décès avant un an et avant cinq ans).
- **Durée de vie** : l'âge moyen au décès par période de décès et par génération (période de naissance), pour les hommes et les femmes, et la pyramide des âges au décès par tranches de cinq ans.

**Familles** contient :

- **Unions** : âge moyen à la première union, durée moyenne d'une union, répartition des unions par jour de la semaine et par mois, et différence d'âge entre conjoints.
- **Enfants** : enfants par union, un histogramme des unions par nombre d'enfants, le temps écoulé entre deux naissances, l'écart entre le premier et le dernier enfant, et l'âge des parents au premier, au dernier et à chaque enfant.

Une moyenne, une part ou un rapport n'est tracé que pour une période d'au moins 10 valeurs : une poignée d'actes ne passe pas pour une tendance. Les périodes sans donnée sont sautées plutôt que tracées à zéro. Un graphique sans rien à montrer affiche « Pas assez de données datées ».

{{ <screenshot name="statistics-charts" alt="Graphiques par période sous la règle des années" lang={lang} /> }}

## Records et listes

L'onglet **Records et listes** présente d'abord les **Records** : une carte par record que détient votre arbre, avec la personne ou le couple concerné (lien vers sa fiche), la valeur et sa date. On y trouve notamment l'homme et la femme ayant vécu le plus longtemps, la plus ancienne et la plus récente naissance, le plus jeune et le plus âgé à une union, l'union la plus longue, le plus d'enfants, le veuvage le plus long, le plus grand écart entre frères et sœurs, et le plus de générations de descendants.

En dessous, **Données remarquables** propose des listes de 100 personnes au plus, avec date et lieu : **Dernières naissances**, **Dernières unions**, **Derniers décès**, **Les plus âgés peut-être encore vivants** (aucun décès noté et nés il y a moins de 120 ans), **Ayant vécu le plus longtemps** et **Familles les plus nombreuses**.

## Évolution de l'arbre

L'onglet **Évolution** suit votre travail plutôt que vos ancêtres : combien de personnes l'arbre comptait chaque jour depuis l'ajout de la première.

- **Personnes dans l’arbre** compte les personnes à la fin de chaque période. Chaque import y forme une marche, signalée par un trait pointillé et une pastille numérotée ; survolez la pastille pour voir combien de personnes il a apportées. Les imports sont listés sous le graphique avec leur date, leur nombre de personnes et le nom du fichier.
- **Personnes ajoutées et retirées** montre, par période, les personnes créées, importées ou restaurées, et celles supprimées ou fusionnées avec une autre.

L'échelle de temps s'adapte à l'âge de l'arbre : un point par jour, par semaine, par mois ou par an, comme l'indique une note au-dessus des graphiques.

## Imprimer un onglet

Cliquez sur l'icône d'imprimante (**Imprimer**) dans la barre d'icônes à gauche, ou appuyez sur Ctrl+P (Cmd+P sur Mac), pour imprimer l'onglet affiché, avec ses graphiques et les années choisies. La boîte de dialogue d'impression de votre système permet aussi de l'enregistrer en PDF.

Pour aller plus loin : [spécification des statistiques](@/tech/ui-statistics.md).
