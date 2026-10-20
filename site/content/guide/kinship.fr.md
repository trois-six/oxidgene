+++
title = "Liens de parenté"
description = "Trouver tous les liens qui unissent deux personnes d'un arbre, dessinés génération par génération."
weight = 100
+++

La page de parenté répond à la question « quel est le lien entre ces deux personnes ? » pour deux personnes quelconques d'un arbre. Elle liste tous les liens trouvés, du plus proche au plus éloigné, et dessine chacun génération par génération à partir des ancêtres communs, ou par les mariages lorsque les deux personnes n'en ont aucun.

## Trouver le lien entre deux personnes

1. Dans l'arbre, faites un clic droit sur la carte de la première personne, ou cliquez sur le **⋮** sous la carte sélectionnée, puis choisissez **Lien de parenté avec…**.
2. Choisissez la seconde personne dans le panneau qui s'ouvre :
   - **Moi**, la personne qui vous représente dans les paramètres de l'arbre (**Qui suis-je ?**) ;
   - **Racine SOSA**, la racine SOSA de l'arbre ;
   - ou n'importe qui d'autre, grâce au champ de recherche.

   Ces deux raccourcis n'apparaissent que s'ils sont définis et qu'ils ne désignent pas la personne cliquée.
3. La page de parenté s'ouvre avec la première personne sous **De** et la seconde sous **À**.

Cliquez sur **Annuler** dans le panneau pour rester sur l'arbre.

La page a sa propre adresse : vous pouvez mettre un résultat en favori ou le partager. Le bouton retour du navigateur, ou celui d'OxidGene, ramène à l'arbre.

{{ <screenshot name="kinship" alt="Page de parenté montrant deux membres de la famille Landrevel cousins germains, leurs grands-parents communs en haut" lang={lang} /> }}

## Changer ou intervertir les deux personnes

- Cliquez sur **Modifier** à côté de l'une ou l'autre personne pour en choisir une autre avec la recherche de personnes. Annuler la recherche conserve la personne actuelle.
- Cliquez sur le bouton **⇄** placé entre elles pour intervertir les deux personnes.

Les liens sont toujours nommés du point de vue de la première personne : le libellé dit ce qu'est la seconde pour la première. Intervertir les deux personnes change un oncle en neveu, un grand-père en petit-fils.

## Lire le résultat

Une ligne indique d'abord combien de liens ont été trouvés, par exemple « 2 liens de parenté trouvés ».

S'il y en a plusieurs, un sommaire les liste du plus proche au plus éloigné. Chaque ligne donne :

- son numéro et son nom, comme « Cousin germain » ou « Cousin issu de germain – 1 génération d'écart » ;
- le nombre de générations remontées depuis la première personne jusqu'aux ancêtres communs, puis redescendues jusqu'à la seconde, par exemple « 2 en montant, 2 en descendant » ;
- les noms des ancêtres communs.

Cliquez sur une ligne pour dessiner ce lien sous le sommaire. Le lien le plus proche est dessiné à l'ouverture de la page et chaque fois que vous changez de personne. S'il n'y a qu'un seul lien, il est dessiné directement, sans sommaire.

Le dessin est une grille, une rangée par génération :

- La colonne étroite à gauche donne la génération de chaque rangée par rapport à la première personne : `0` pour sa propre génération, `+1` pour celle de ses parents, `+2` pour celle de ses grands-parents, `−1` pour celle de ses enfants.
- La rangée du haut contient l'ancêtre commun, ou les deux membres du couple d'ancêtres communs.
- En dessous, la lignée de la première personne descend à gauche et celle de la seconde à droite, chaque personne renvoyant à sa fiche.
- Lorsque l'une des personnes est l'ancêtre direct de l'autre, il n'y a qu'une seule lignée.

Un lien qui ne passe que par l'un des deux parents du haut — demi-frères, demi-cousins — porte la mention **Par un seul parent** à côté de son nom.

Plusieurs liens entre les deux mêmes personnes sont fréquents dans les arbres anciens : lorsque les mêmes ancêtres apparaissent par des lignées différentes (implexe), chaque lignée est un lien distinct. Un ancêtre commun plus lointain atteint en passant par un ancêtre plus proche ne compte pas comme un lien supplémentaire.

## Parenté par alliance

Lorsque les deux personnes n'ont aucun ancêtre commun, OxidGene cherche à la place les chemins les plus courts passant par des mariages, ceux qui comptent le moins d'unions en premier.

Un tel lien est intitulé **Parenté par alliance** et présente les étapes sous forme de chaîne, chaque étape étant un proche de la précédente en partant de la première personne — par exemple « Oncle › Épouse › Frère ». La ligne du sommaire indique aussi le nombre d'unions.

Le dessin montre chaque tronçon de sang sous forme de grille distincte, séparée par une ligne **⚭ Union** là où une personne a épousé la suivante. Les générations restent comptées depuis la première personne tout au long du chemin, et les conjoints se trouvent sur la même génération.

## Comment les liens sont nommés

Le nom dépend du nombre de générations que le chemin remonte puis redescend, et du sexe de la seconde personne (avec une forme neutre si le sexe est inconnu) :

| Chemin | Nom |
|---|---|
| En descendant uniquement | Enfant, petit-enfant, arrière-petit-enfant, puis « Descendant à la n-ième génération » |
| En remontant uniquement | Parent, grand-parent, arrière-grand-parent, puis « Ancêtre à la n-ième génération » |
| Une génération en montant, une en descendant | Frère ou sœur, ou demi-frère ou demi-sœur |
| Deux ou trois en montant, une en descendant | Oncle ou tante, grand-oncle ou grand-tante ; au-delà, « Frère ou sœur d'un ancêtre à la n-ième génération » |
| Une en montant, deux ou trois en descendant | Neveu ou nièce, petit-neveu ou petite-nièce ; au-delà, « Descendant d'un frère ou d'une sœur à la n-ième génération » |
| Tout autre chemin | Cousin germain, cousin issu de germain, puis « Cousin au n-ième degré », suivi du nombre de générations d'écart lorsque les deux personnes ne sont pas de la même génération |

## Quand aucun lien n'apparaît

- « Aucun lien enregistré ne relie ces deux personnes. » signifie que ni un ancêtre commun ni une chaîne de mariages ne les relie dans votre arbre. Les personnes et familles supprimées ne relient personne.
- « Choisissez deux personnes différentes. » s'affiche lorsque la même personne se trouve aux deux bouts.
- Au plus 32 liens sont listés. Dans un arbre à fort implexe, les plus proches sont conservés et la page indique **Seuls les liens les plus proches sont listés ; il en existe d'autres.**

Pour aller plus loin : [spécification de la parenté](@/tech/ui-kinship.md) (en anglais).
