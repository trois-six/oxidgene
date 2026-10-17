+++
title = "OxidGene"
description = "Une application de généalogie libre pour l’ordinateur et le web : importez, explorez, modifiez et interrogez vos arbres."

[extra]
headline = "Votre histoire familiale, entre vos mains"
platforms = "Windows · macOS · Linux · votre propre serveur web — logiciel libre"
start = "@/guide/install.md"
tour_alt = "Visite d’OxidGene : arbre, fiche individuelle, roue, couple, carte des lieux, parenté, recherche, médiathèque, historique et réglages"

steps_title = "Commencez votre généalogie en trois étapes"
steps = [
    { title = "Importez votre arbre", text = "Un fichier GEDCOM, GEDZIP ou GeneWeb de votre logiciel actuel, ou votre arbre Geneanet avec ses photos — ou partez de zéro.", link = "@/workflows/first-tree-from-gedcom.md" },
    { title = "Retrouvez vos ancêtres", text = "Ajoutez les personnes trouvées dans les registres, avec leurs sources, et ouvrez chaque registre cité à la bonne vue sur le site des archives.", link = "@/workflows/record-a-new-ancestor.md" },
    { title = "Racontez l’histoire familiale", text = "Arbres, fiches rédigées comme un récit, statistiques sur une carte, parenté entre cousins : partagez vos découvertes.", link = "@/workflows/explore-and-share.md" },
]

features_title = "Tout ce dont le généalogiste a besoin"
features = [
    { title = "Importez depuis n’importe où", text = "GEDCOM 5.5.1 et 7.0, GEDZIP avec ses médias, GeneWeb, et les arbres Geneanet avec leurs photos.", link = "@/guide/import-export.md" },
    { title = "Des arbres de toutes formes", text = "Un arbre interactif jusqu’à dix générations dans chaque sens, roues, éventails, sablier et nœud papillon, en style classique ou héraldique.", link = "@/guide/pedigree.md" },
    { title = "Des fiches qui racontent", text = "La famille rédigée comme un récit, une chronologie avec les événements des proches, les sources et des fiches explicatives.", link = "@/guide/persons.md" },
    { title = "Les archives en un clic", text = "Un registre cité s’ouvre à la vue citée sur le site même des archives, pour toutes les archives départementales.", link = "@/guide/sources-archives.md" },
    { title = "Photos et documents", text = "Portraits découpés dans les photos de groupe, documents de plusieurs pages, étiquettes et médiathèque.", link = "@/guide/media.md" },
    { title = "Recherche et parenté", text = "Retrouvez n’importe qui grâce aux filtres, et voyez tous les liens entre deux personnes.", link = "@/guide/kinship.md" },
    { title = "Statistiques et cartes", text = "Complétude, carte de chaleur des lieux, noms, métiers, durées de vie et records de votre arbre.", link = "@/guide/statistics.md" },
    { title = "Des outils pour un arbre propre", text = "Anomalies, lieux à localiser, doublons à fusionner et convertisseur de calendriers.", link = "@/guide/tools.md" },
    { title = "Rien ne se perd", text = "Chaque modification est conservée : comparez les versions d’une personne et restaurez-en une plus ancienne.", link = "@/guide/history.md" },
    { title = "Des lieux sans rien taper", text = "Un dictionnaire intégré des lieux de douze pays, avec leurs anciens noms, disponible hors ligne.", link = "@/guide/editing.md" },
    { title = "Dans votre langue et vos couleurs", text = "Huit langues, des thèmes clair et sombre, et vos propres thèmes.", link = "@/guide/settings.md" },
    { title = "Interrogez un assistant IA", text = "Laissez Claude ou un autre assistant lire votre arbre, en lecture seule, depuis l’application de bureau.", link = "@/guide/assistant.md" },
]

values_title = "Votre généalogie en toute liberté"
values = [
    { title = "Logiciel libre.", text = "OxidGene est publié sous licence GNU Affero General Public License : pas de licence à acheter, pas d’abonnement, et son code source est ouvert." },
    { title = "Vos données restent chez vous.", text = "L’application de bureau garde vos arbres sur votre ordinateur et fonctionne hors ligne. Pas de compte, pas de pistage : elle ne se connecte que pour ouvrir le site d’archives ou importer depuis Geneanet." },
    { title = "Des formats ouverts.", text = "GEDCOM et GEDZIP en entrée comme en sortie, avec un export adapté au logiciel qui le lira : rien n’enferme vos recherches." },
    { title = "Sur votre ordinateur ou votre serveur.", text = "Une application pour Windows, macOS et Linux, ou un déploiement web pour votre famille ou votre association." },
]

gallery_title = "Aperçu de l’application"
gallery = [
    { name = "pedigree", caption = "L’arbre et son panneau d’événements", link = "@/guide/pedigree.md" },
    { name = "fan-chart-dark", caption = "Une roue dans le thème sombre", link = "@/guide/pedigree.md" },
    { name = "person", caption = "La fiche d’une personne", link = "@/guide/persons.md" },
    { name = "couple", caption = "La fiche d’un couple", link = "@/guide/persons.md" },
    { name = "statistics-map", caption = "Les lieux sur une carte de chaleur", link = "@/guide/statistics.md" },
    { name = "kinship", caption = "La parenté entre deux cousins", link = "@/guide/kinship.md" },
    { name = "tools-anomalies", caption = "Les anomalies de l’arbre", link = "@/guide/tools.md" },
    { name = "media-library", caption = "La médiathèque", link = "@/guide/media.md" },
    { name = "history", caption = "L’historique d’une personne, comparé", link = "@/guide/history.md" },
]

download_title = "Télécharger OxidGene"
downloads = [
    { title = "Windows", text = "Windows 10 et 11, x86-64 : l’exécutable dans une archive .zip.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "macOS", text = "macOS 11 ou plus récent, Apple silicon et Intel : l’application dans une image disque.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "Linux", text = "x86-64 : l’exécutable dans une archive .tar.gz, avec le WebKitGTK du système.", link = "https://github.com/trois-six/oxidgene/releases" },
    { title = "Serveur web", text = "Docker Compose ou chart Helm, avec PostgreSQL et un stockage compatible S3.", link = "@/guide/install.md" },
]
download_note = "Aucune version n’est encore publiée : d’ici la première, compilez l’application de bureau depuis ses sources comme l’explique le guide d’installation. Chaque version publie ses sommes de contrôle SHA-256."
switching = "@/workflows/first-tree-from-gedcom.md"
switching_label = "Vous venez d’un autre logiciel de généalogie ?"

docs_title = "Documentation"
+++
