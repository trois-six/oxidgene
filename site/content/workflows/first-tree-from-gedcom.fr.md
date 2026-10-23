+++
title = "Quitter un autre logiciel"
description = "Exporter votre arbre depuis votre logiciel de généalogie actuel, l’importer dans OxidGene, désigner ses racines et vérifier le résultat."
weight = 10
+++

Ce parcours vous mène d’un arbre tenu dans un autre logiciel ou site de généalogie à un arbre vérifié dans OxidGene : l’export depuis l’ancien logiciel, la création de l’arbre, l’import du fichier, le choix de la racine SOSA et de « Qui suis-je ? », puis le repérage de ce qui mérite votre attention.

Si votre arbre est sur Geneanet et que vous voulez aussi ses photos, suivez plutôt [Récupérer un arbre Geneanet avec ses photos](@/workflows/geneanet-with-photos.fr.md).

## 1. Exporter l’arbre depuis votre logiciel actuel

Servez-vous de la fonction d’export de votre logiciel actuel, en choisissant le meilleur format qu’il propose, dans cet ordre :

1. **GEDZIP (`.gdz`)**, parfois appelé « GEDCOM 7 avec médias » : le seul format qui emporte vos photos et vos documents avec la généalogie.
2. **GEDCOM (`.ged`)**, en version 5.5.1 ou 7. OxidGene lit les deux, quel que soit le jeu de caractères : inutile de changer l’encodage de l’export.
3. **GeneWeb (`.gw`)**, si vous utilisez GeneWeb.

Avec un simple fichier GEDCOM, vos photos restent en arrière : OxidGene crée une fiche pour chacune, avec le nom de son fichier, mais sans l’image. Conservez votre dossier de médias ; vous pourrez joindre chaque fichier à sa fiche plus tard (voir [Médias](@/guide/media.fr.md)).

Le fichier peut atteindre 1 Gio.

## 2. Créer un arbre

1. Installez et ouvrez OxidGene (voir [Installer OxidGene](@/guide/install.fr.md)).
2. Sur la page d’accueil, cliquez sur **Nouvel arbre**.
3. Donnez-lui un **Nom**, par exemple « Famille Landrevel », et, si vous le souhaitez, une description.
4. Cliquez sur **Créer**.

Importez dans un arbre neuf et vide : un import ajoute ce qu’il lit à l’arbre, il ne le fusionne pas avec les personnes déjà présentes.

## 3. Importer le fichier

1. Sur la carte du nouvel arbre, ouvrez le menu à trois points et choisissez **Importer**.
2. Dans l’onglet **Un fichier**, déposez votre fichier sur la zone prévue, ou cliquez dessus pour le choisir.
3. Cliquez sur **Importer** et patientez. La fenêtre reste ouverte jusqu’à la fin de l’import ; un grand arbre avec beaucoup de médias peut demander plusieurs minutes.
4. Quand **Import terminé** s’affiche, comparez le bilan (personnes, familles, événements, sources, lieux, images, documents) avec ce qu’indiquait votre ancien logiciel.
5. Si une ligne comme **12 avertissements** apparaît, cliquez dessus et lisez-les. Ils signalent ce qui n’a pas pu être lu ou relié, comme un âge écrit en texte libre ou une note qui ne renvoie à rien. Tout le reste a été importé.

Si le résultat n’est pas celui attendu, par exemple parce que l’export était incomplet, supprimez l’arbre, exportez de nouveau depuis votre ancien logiciel et recommencez dans un nouvel arbre. Voir [Import et export](@/guide/import-export.fr.md) pour ce qui est importé et ce qui ne l’est pas.

## 4. Désigner la racine SOSA et dire qui vous êtes

1. Ouvrez les **Paramètres** de l’arbre depuis le menu de sa carte.
2. Dans **Arbre et racines**, sous **Personne racine (SOSA 1)**, cliquez sur **Modifier** et choisissez la personne à partir de laquelle votre ascendance est numérotée, souvent vous-même ou l’un de vos enfants.
3. Sous **Qui suis-je ?**, cliquez sur **Modifier** et choisissez votre propre fiche dans l’arbre.

Les deux choix sont enregistrés immédiatement. La vue arbre s’ouvre désormais sur la racine SOSA, ses ancêtres portent leur numéro SOSA, et les liens de parenté peuvent se calculer à partir de vous. Voir [Les arbres](@/guide/trees.fr.md).

Passez ensuite, dans les **Préférences globales** de la même page, par **Affichage des dates** et **Options de saisie**, et choisissez comment écrire et saisir les dates sur cet appareil (voir [Paramètres](@/guide/settings.fr.md#affichage-des-dates)).

## 5. Faire le tour du propriétaire

Ouvrez l’arbre depuis la page d’accueil. Parcourez dans le pedigree quelques branches que vous connaissez bien, ouvrez quelques fiches, et vérifiez que noms, dates, lieux, sources et notes sont arrivés comme prévu (voir [Le pedigree](@/guide/pedigree.fr.md) et [Les personnes](@/guide/persons.fr.md)).

## 6. Rechercher les anomalies

Chaque logiciel enregistre les données un peu à sa façon, et un déménagement est une bonne occasion de débusquer de vieilles erreurs.

1. Dans la barre latérale gauche de l’arbre, cliquez sur l’icône en forme de clé pour ouvrir les **Outils**.
2. Ouvrez l’onglet **Anomalies**. Des tuiles comptent les anomalies par catégorie : **Dates**, **Filiation**, **Unions**, **Témoins et parrains** et **Fiches**. Une **Erreur** est une situation impossible, comme un décès avant la naissance ; un **Avertissement**, une situation improbable, comme une mère de plus de 55 ans à la naissance d’un enfant.
3. Ouvrez une règle pour voir chaque cas et les personnes concernées. Cliquez sur une personne pour ouvrir sa fiche et corriger les données.

Une date vague ne déclenche jamais une anomalie que son imprécision suffirait à expliquer : ce qui est listé mérite donc d’être regardé.

## 7. Localiser vos lieux

1. Dans les **Outils**, ouvrez l’onglet **Lieux non localisés**. Il liste les lieux que la carte des statistiques ne sait pas situer, les plus utilisés d’abord.
2. Cliquez sur **Qui l'utilise** pour voir quelles personnes y font référence.
3. Cliquez sur **Corriger** pour rectifier le nom. Choisir une suggestion du dictionnaire des lieux intégré donne aussi au lieu ses coordonnées, et il quitte la liste. Ses événements et médias le conservent : le lieu est renommé, pas remplacé.

## 8. Chercher les doublons

Un import révèle souvent une même personne saisie deux fois. L’onglet **Doublons potentiels** des **Outils** liste les paires de fiches qui pourraient désigner une seule personne, pour les fusionner ou les déclarer distinctes. Voir [Outils](@/guide/tools.fr.md) et [Nettoyer un arbre](@/workflows/clean-up-tree.fr.md).

## 9. Garder une copie

Une fois satisfait de l’arbre, exportez-le en GEDCOM 7.0 (GEDZIP) depuis **Paramètres → Exporter l’arbre** : un seul fichier contenant votre généalogie et ses médias, qu’OxidGene et les autres logiciels compatibles GEDCOM 7 savent relire (voir [Import et export](@/guide/import-export.fr.md)).
