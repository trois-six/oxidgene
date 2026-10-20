+++
title = "Nettoyer un arbre après un import"
description = "Vérifier un arbre importé, corriger ses anomalies et ses lieux, fusionner les doublons, harmoniser les noms de famille et annuler une modification malheureuse."
weight = 40
+++

Un arbre importé arrive rarement impeccable : des dates saisies à la hâte, des lieux écrits de trois façons, le même ancêtre entré deux fois depuis deux branches. Ce parcours vous guide pour vérifier et nettoyer un arbre, dans un ordre qui évite de refaire le travail : d'abord les erreurs évidentes, puis les lieux, puis les doublons, enfin les noms.

Rien ici ne presse ni ne se fait d'un bloc. Chaque étape peut se mener quelques fiches à la fois, et chaque liste est recalculée quand vous y revenez.

## 1. Faire le point

Ouvrez les [statistiques](@/guide/statistics.fr.md) avec l'icône de graphique de la barre d'icônes à gauche. L'onglet **Vue d'ensemble** donne la mesure du travail :

- les tuiles de **Complétude** indiquent la part des personnes avec une naissance datée, un décès daté, un parent connu ;
- les **Effectifs** donnent le nombre de personnes, de lieux et de noms de famille distincts. Bien plus de noms de famille que prévu trahit souvent des variantes d'orthographe.

## 2. Corriger les anomalies

1. Ouvrez les outils avec l'icône de clé de la barre d'icônes à gauche, puis l'onglet **Anomalies**.
2. Commencez par les règles de type **Erreur** : décès avant la naissance, parent né après son enfant, personne parmi ses propres ancêtres. Ce sont presque toujours des fautes de frappe ou une personne rattachée à la mauvaise famille.
3. Dépliez une règle, suivez le lien d'une personne vers sa fiche et corrigez-la.
4. Passez ensuite aux règles de type **Avertissement**. Elles signalent l'improbable, pas l'impossible : une mère de 56 ans peut être une faute de frappe ou une naissance tardive bien réelle. Laissez en l'état ce que les actes confirment.

Une date imprécise ne soulève jamais une anomalie que son imprécision suffirait à expliquer : ce qui s'affiche mérite donc un coup d'œil. Une anomalie trahit souvent un doublon ou un mauvais lien plutôt qu'une mauvaise date : gardez-le en tête pour l'étape 4.

{{ <screenshot name="tools-anomalies" alt="Onglet Anomalies listant les règles qui ont trouvé quelque chose" lang={lang} /> }}

## 3. Localiser les lieux

1. Dans les outils, ouvrez **Lieux non localisés**. Les lieux sont classés du plus utilisé au moins utilisé : les premières lignes rapportent le plus.
2. Cliquez sur **Qui l'utilise** si vous hésitez sur le lieu visé.
3. Cliquez sur **Corriger**, choisissez la bonne suggestion du dictionnaire des lieux, puis cliquez sur **Enregistrer**.

Le lieu est renommé, pas remplacé : tous les événements et médias qui l'utilisent suivent. Dès qu'il porte un nom du dictionnaire, il est situé sur la carte des statistiques et quitte la liste.

## 4. Fusionner les doublons

1. Dans les outils, ouvrez **Doublons potentiels**. Les paires sont classées des plus probables aux moins probables, avec une confiance (**Très probable**, **Probable**, **Possible**) et ce que les deux fiches ont en commun.
2. Pour une paire qui désigne à l'évidence deux personnes (un père et son fils homonyme, par exemple), cliquez sur **Deux personnes différentes**. Elle ne sera plus proposée.
3. Pour une paire qui pourrait être une seule personne, cliquez sur **Comparer**. L'assistant de fusion s'ouvre sur les deux fiches côte à côte.
4. Choisissez la fiche à conserver avec **Garder cette fiche**, puis, ligne par ligne, les valeurs que garde la fiche fusionnée. Cochez les événements et les médias à reprendre de l'autre fiche, puis cliquez sur **Suivant**.
5. Vérifiez le récapitulatif et cliquez sur **Fusionner**.

Une fusion est irréversible : vérifiez la comparaison avec soin. La liste est recalculée après chaque réponse. Vous pouvez aussi fusionner deux fiches repérées par vous-même, avec **Fusionner avec…** sur la fiche d'une personne ou dans le menu d'actions du pedigree. La page [Outils](@/guide/tools.fr.md) détaille l'assistant.

{{ <screenshot name="merge" alt="Assistant de fusion sur deux fiches d'une même personne" lang={lang} /> }}

## 5. Harmoniser les noms de famille en une fois

Le dictionnaire liste chaque nom de famille exactement tel qu'il est écrit : les variantes sautent aux yeux. « Landrevel » et « LANDREVEL », ou « Landrevel » et « Landreval », y forment des lignes distinctes.

1. Ouvrez le [dictionnaire](@/guide/dictionary.fr.md) avec l'icône de livre de la barre d'icônes à gauche. Il s'ouvre sur l'onglet **Noms de famille**.
2. Parcourez la liste à l'aide de l'index alphabétique et du filtre. Dépliez une ligne pour voir qui porte le nom.
3. Ouvrez le menu **⋮** de la ligne de la variante à corriger et choisissez **Modifier**. La fenêtre **Modifier le nom de famille** s'ouvre.
4. Tapez la bonne graphie dans **Nom**. Si ce nom existe déjà, la fenêtre prévient que les deux seront fusionnés.
5. Cliquez sur **Renommer**. Toutes les personnes qui portaient la variante comme nom principal portent désormais la bonne graphie.

Les personnes qui ne portent la variante que comme nom secondaire (alias, nom d'épouse) la gardent ; la fenêtre indique combien elles sont.

La même fenêtre corrige une particule mal découpée, comme un « Le » ou un « de » pris pour une partie du nom, ou l'inverse. Laissez le nom inchangé, renseignez le champ **Particule**, puis cliquez sur **Appliquer à tous**.

{{ <screenshot name="dictionary" alt="Onglet Noms de famille du dictionnaire" lang={lang} /> }}

## 6. Annuler une modification malheureuse

Si une correction se révèle fausse, l'historique permet de revenir en arrière.

- Pour une personne : ouvrez sa fiche, cliquez sur **Historique**, sélectionnez la version d'avant l'erreur et cliquez sur **Restaurer cette version**.
- Pour une modification repérée dans le journal de l'arbre : ouvrez les paramètres de l'arbre avec l'icône d'engrenage, choisissez **Historique**, repérez la modification et cliquez sur **Restaurer la version précédente**.

Un renommage de nom de famille donne une nouvelle version à chaque personne renommée : on l'annule donc personne par personne, depuis son historique. Une restauration est elle-même enregistrée et peut donc être annulée à son tour. Voir [Historique](@/guide/history.fr.md).

## Et ensuite

Le nettoyage n'est jamais tout à fait fini. Revenez aux onglets **Anomalies** et **Doublons potentiels** après chaque import ou longue séance de saisie : tous deux sont recalculés à partir de l'arbre à chaque ouverture. Pour voir quels ancêtres manquent encore, ouvrez **Complétude de l'ascendance** ; [Saisir un nouvel ancêtre](@/workflows/record-a-new-ancestor.fr.md) explique comment combler les vides.
