+++
title = "Explorer et partager votre histoire familiale"
description = "Regarder votre arbre sous forme de graphiques, trouver le lien de parenté entre deux personnes, situer votre famille sur une carte, exporter l'arbre et le faire lire par un assistant."
weight = 50
+++

Une fois votre arbre bien avancé, vous aurez envie de le regarder sous de nouveaux angles et de le partager avec vos proches. Ce parcours montre comment le dessiner en éventail ou en sablier, trouver le lien de parenté entre deux personnes, voir sur une carte où vivait votre famille, imprimer un graphique, exporter l'arbre vers un autre logiciel et le faire lire par un assistant IA.

## 1. Dessiner l'arbre en éventail, en sablier ou en roue

La vue arbre peut dessiner votre généalogie de plusieurs façons. Ce choix est une préférence d'affichage de votre appareil, pas de l'arbre.

1. Ouvrez les paramètres de l'application avec l'icône d'engrenage dans l'en-tête de la page d'accueil (les mêmes options se trouvent sous **Préférences globales** dans les paramètres d'un arbre), puis choisissez **Pedigree**.
2. Sous **Graphique**, choisissez une vue :
   - **Arbre** : les ancêtres au-dessus, les descendants au-dessous ;
   - **Roue des ancêtres** et **Éventail** : les ancêtres en couronnes autour de la personne, en cercle complet ou en demi-cercle, le côté paternel toujours à gauche ;
   - **Roue des descendants** et **Éventail des descendants** : les descendants en couronnes, leurs unions entre les générations ;
   - **Lignée** et **Lignée des descendants** : une colonne par génération, comme dans Gramps ;
   - **Sablier** : les descendants à gauche, la personne au centre, les ancêtres à droite ;
   - **Papillon** : la lignée paternelle à gauche, la lignée maternelle à droite.
3. Revenez à l'arbre : la nouvelle vue s'applique aussitôt.

Dans chaque vue, cliquez sur une personne pour la placer au centre du graphique, faites glisser pour vous déplacer, et zoomez avec la molette ou les boutons. Dans les roues et les éventails, les générations lointaines sont écrites plus petit : zoomez pour les lire.

{{ <screenshot name="fan-chart-dark" alt="Éventail de la famille Landrevel dans le thème sombre" lang={lang} /> }}

## 2. Choisir une apparence

Deux réglages changent l'allure des graphiques, tous deux dans les paramètres de l'application :

- **Thème du pedigree**, sous **Pedigree** : **Classique** (cartes sobres et connecteurs courbes) ou **Médiéval** (un tracé gravé, avec blasons et traits à l'équerre). Il s'applique à tous les pedigrees de l'appareil.
- **Thème**, sous **Apparence** : les couleurs et les polices de toute l'application, par exemple **Clair** ou **Sombre**. Les graphiques le suivent.

{{ <screenshot name="hourglass-medieval" alt="Sablier dessiné avec le thème de pedigree médiéval" lang={lang} /> }}

## 3. Imprimer un graphique

1. Zoomez et déplacez le graphique pour cadrer ce que vous voulez sur papier.
2. Cliquez sur l'icône d'imprimante (**Imprimer**) dans la barre d'icônes à gauche, ou appuyez sur Ctrl+P (Cmd+P sur Mac).
3. Si le graphique dépasse une feuille à ce niveau de zoom, choisissez **Ce qui est à l'écran** (une feuille) ou **Tout le graphique à ce zoom**, qui indique le nombre de feuilles nécessaires. Les feuilles se chevauchent de 1 cm, avec un trait pointillé pour découper ou aligner : vous pouvez ainsi assembler un grand tableau mural.

Le papier utilise toujours les couleurs claires, quel que soit le thème à l'écran. La boîte de dialogue d'impression permet aussi d'enregistrer le graphique en PDF.

## 4. Trouver le lien de parenté entre deux personnes

1. Dans l'arbre, cliquez sur le **⋮** sous la carte sélectionnée d'une personne (ou faites un clic droit sur la carte) et choisissez **Lien de parenté avec…**.
2. Choisissez l'autre personne : **Moi**, la **Racine SOSA** de l'arbre, ou toute personne trouvée avec la recherche.
3. La page de parenté s'ouvre. Elle indique ce qu'est la seconde personne pour la première (« Cousin germain », « Cousin issu de germain ») et dessine le chemin génération par génération, des ancêtres communs jusqu'aux deux personnes.

Quand il existe plusieurs liens (des cousins par deux lignées, par exemple), un résumé les liste du plus proche au plus éloigné, avec le nombre de générations parcourues en montant et en descendant ; cliquez sur l'un d'eux pour le dessiner. Utilisez **Modifier** pour remplacer l'une ou l'autre personne, ou le bouton d'inversion pour voir le lien depuis l'autre côté : un oncle devient un neveu. Quand les deux personnes n'ont aucun ancêtre commun, la page montre leur **Parenté par alliance**, étape par étape à travers les unions.

Voir [Parenté](@/guide/kinship.fr.md).

{{ <screenshot name="kinship" alt="Page de parenté montrant le chemin entre deux cousins" lang={lang} /> }}

## 5. Voir où vivait votre famille

1. Ouvrez les statistiques avec l'icône de graphique de la barre d'icônes à gauche.
2. Ouvrez l'onglet **Lieux**.

La carte de chaleur montre les régions où se sont déroulés les événements de votre arbre, avec à côté les dix lieux les plus utilisés et, en dessous, les naissances par pays, par région et par département. Cliquez sur un lieu de la liste pour zoomer dessus. Les autres onglets racontent la suite de l'histoire : prénoms, durée de vie, âge au mariage, taille des familles siècle après siècle. Voir [Statistiques](@/guide/statistics.fr.md).

{{ <screenshot name="statistics-map" alt="Carte de chaleur des lieux de la famille Landrevel" lang={lang} /> }}

## 6. Exporter l'arbre pour un autre logiciel ou un proche

1. Ouvrez les paramètres de l'arbre avec l'icône d'engrenage de la barre d'icônes à gauche et choisissez **Exporter l’arbre**.
2. Choisissez le format :
   - **GEDCOM (.ged)**, le format d'échange que lisent tous les logiciels de généalogie. Sous **Logiciel de destination**, choisissez le logiciel qui l'importera (Geneanet, Ancestry, Heredis, Gramps et d'autres), ou **GEDCOM 5.5.1 standard** (par défaut) ou **GEDCOM 7.0 standard**. Le fichier est alors écrit comme ce logiciel sait le lire.
   - **GEDZIP (.gdz)**, une archive unique réunissant un fichier GEDCOM 7.0 et vos photos et documents. Elle est idéale pour une sauvegarde complète ou un partage avec les pièces jointes, vers un logiciel compatible GEDCOM 7.0.
3. Réglez les options : **Inclure les notes et les sources**, et pour un GEDZIP **Inclure les médias**. Elles ne valent que pour cet export.
4. Si vous le souhaitez, remplissez la carte **Émetteur** (nom, courriel, adresse) : elle indique de qui provient le fichier.
5. Cliquez sur **Télécharger .ged** ou **Télécharger .gdz**.

Après un export GEDCOM destiné à un logiciel précis, la page indique ce que ce logiciel n'a pas pu recevoir, ou confirme que rien n'a été perdu. Une archive GEDZIP reste disponible une heure : **Télécharger à nouveau** l'enregistre de nouveau sans la reconstruire.

À garder en tête :

- Un export contient toujours l'arbre entier. Exporter seulement une partie (les ancêtres ou les descendants d'une personne) n'est pas encore possible.
- Les personnes vivantes sont exportées comme les autres : il n'existe pas encore d'option pour écarter les contemporains. Vérifiez ce que vous partagez.

Voir [Import et export](@/guide/import-export.fr.md).

## 7. Faire lire votre arbre par un assistant IA

Dans l'application de bureau, un assistant IA comme Claude Desktop ou Claude Code peut lire vos arbres grâce au Model Context Protocol (MCP), pour répondre à des questions sur votre famille, rédiger une biographie à partir d'une fiche ou aider à repérer des incohérences.

1. Ouvrez les paramètres de l'application et choisissez **API**.
2. Sous **Assistant IA (MCP)**, lisez l'avertissement, puis copiez la commande ou la configuration client dans votre assistant.

L'accès est en lecture seule, sauf si vous ajoutez `--allow-writes` à la commande : l'assistant peut alors modifier vos arbres, et chacune de ses modifications entre dans l'historique de l'arbre. Dans tous les cas, il peut lire tous les arbres de l'application, personnes vivantes, notes et sources comprises, et il envoie ce qu'il lit au fournisseur du modèle qu'il utilise. Ne connectez qu'un assistant à qui vous confiez ces données. La page [Assistant IA](@/guide/assistant.fr.md) détaille la mise en place.
