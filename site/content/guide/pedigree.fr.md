+++
title = "L’arbre généalogique"
description = "Lire l’arbre et s’y déplacer, agir sur une personne depuis sa carte, suivre ses événements, passer à un autre graphique et imprimer."
weight = 40
+++

La vue arbre est celle où vous passerez le plus de temps : elle dessine la personne que vous consultez, ses ancêtres au-dessus et ses descendants au-dessous, et vous permet de passer d’un parent à l’autre. Cette page explique comment lire les cartes, se déplacer, agir sur une personne, lire le panneau des événements, choisir un autre graphique ou un autre thème, et imprimer.

{{ <screenshot name="pedigree" alt="Arbre de la famille Landrevel avec portraits et panneau des événements" lang={lang} /> }}

## Lire l’arbre

La personne au centre de l’arbre occupe le milieu du canevas. Chaque génération d’ancêtres forme une rangée au-dessus d’elle, chaque génération de descendants une rangée au-dessous. Toutes les cartes affichées sont reliées à cette personne par une chaîne de parents, d’enfants ou de conjoints : une personne sans lien avec elle se retrouve par la [recherche](@/guide/search.fr.md), elle n’est jamais dessinée isolée.

Des traits relient chaque couple et descendent vers ses enfants. Quand une personne a plusieurs unions, chacune a son propre trait, et les enfants de chaque union se rattachent au bon conjoint.

### Ce que montre une carte

- **Portrait.** La photo de profil de la personne, ou une silhouette d’homme, de femme ou de sexe inconnu.
- **Noms.** Les prénoms, puis le nom de famille en capitales.
- **Dates.** Les seules années de naissance et de décès. Si la naissance n’est pas datée, le baptême la remplace ; si le décès n’est pas daté, l’inhumation le remplace.
- **Sexe.** Un court filet de couleur à côté du portrait dans le thème classique ; le contour de l’écu dans le thème médiéval.
- **Personne centrale.** Sa carte est remplie de couleur au lieu d’être simplement entourée.
- **Conjoints.** Les cartes des conjoints ont un fond légèrement différent.
- **Autres liens.** Un petit **+** bleu signale une personne qui a des proches en dehors de ce que le graphique dessine.

Une année seule peut masquer l’incertitude d’une date ; la carte ajoute donc la marque de précision portée par l’acte :

| Marque | Sens | Exemple |
|---|---|---|
| aucune | date exacte | `1849` |
| `ca` | vers, calculée, estimée ou déduite d’un âge | `ca 1849` |
| `?` | peut-être | `? 1849` |
| `<` | avant | `< 1917` |
| `>` | après | `> 1912` |
| `\|` | l’une ou l’autre date | `1849\|1852` |
| `..` | entre deux dates | `1691..1693` |

Ainsi, `ca 1849-< 1917` se lit « né vers 1849, décédé avant 1917 ». Quand un intervalle ne tient pas sur la carte, seule sa première année est conservée, avec sa marque (`.. 1691`). Survolez les dates pour les lire en toutes lettres dans votre langue.

### Les badges

- **Badge SOSA.** Un petit disque en bas à droite du portrait signale la racine SOSA de l’arbre (il porte le chiffre `1`) et chacun de ses ancêtres.
- **Badge « Moi ».** La personne choisie dans **Paramètres** → **Arbre et racines** → **Qui suis-je ?** porte un disque au centre plein. Il l’emporte sur le badge SOSA, pour que vous vous retrouviez toujours. Voir les [paramètres](@/guide/settings.fr.md).

### Les cases vides

Une carte en pointillés marquée d’un **+** tient la place d’un parent que l’arbre ne connaît pas encore, à chaque génération, ainsi que d’un conjoint non nommé dans un couple. Cliquez sur son **+** pour créer ce parent : voir [Saisir et modifier les personnes](@/guide/editing.fr.md) et le parcours [Ajouter un nouvel ancêtre](@/workflows/record-a-new-ancestor.fr.md).

## Se déplacer dans l’arbre

- **Changer de personne centrale.** Cliquez sur une carte. Le graphique se recentre sur elle, un crayon apparaît sous sa carte et le panneau des événements affiche ses événements.
- **Faire glisser.** Glissez n’importe où sur le canevas, cartes comprises.
- **Zoomer.** Utilisez la molette ou le pincement pour zoomer autour du pointeur, ou les boutons **Zoom avant** et **Zoom arrière** de la barre latérale gauche. La vue arbre zoome de 0,3× à 4×.
- **Ajuster.** **Ajuster à l’écran** cadre tout le graphique dans la fenêtre. Si l’arbre est trop grand pour tenir, il est centré sur la personne centrale.
- **Changer la profondeur.** Survolez **Profondeur des générations** dans la barre latérale gauche. Un petit panneau s’ouvre avec deux compteurs, ancêtres (flèche vers le haut) et descendants (flèche vers le bas), chacun de 0 à 10. Chaque **+** ou **−** redessine aussitôt le graphique.

Un nouvel arbre s’ouvre sur 4 générations d’ancêtres et 3 de descendants ; modifiez ces valeurs sous **Profondeur du pedigree** dans les [paramètres](@/guide/settings.fr.md) de l’application. Une fois que vous vous êtes déplacé dans un arbre, sa vue (personne centrale, profondeurs, position et zoom) est conservée tant que la fenêtre reste ouverte : allez sur une autre page puis revenez, le graphique est tel que vous l’avez laissé. Le bouton **Précédent** vous ramène lui aussi à la personne autour de laquelle l’arbre était dessiné.

## Trouver une personne depuis la barre du haut

La barre du haut comporte deux champs, **Nom / # SOSA** et **Prénom(s)**. Utilisez l’un, l’autre ou les deux.

1. Commencez à taper. Un panneau propose jusqu’à cinq noms de l’arbre, avec le nombre de personnes qui les portent, puis jusqu’à six personnes correspondantes.
2. Choisissez un nom pour compléter le champ, ou une personne pour centrer l’arbre sur elle.
3. Appuyez sur Entrée, ou cliquez sur la loupe, pour ouvrir tous les résultats.

Tapez un nombre seul dans **Nom / # SOSA** pour aller directement à ce numéro SOSA. La page [Recherche](@/guide/search.fr.md) décrit les résultats et leurs filtres.

## Agir sur une personne

Cliquez sur le crayon sous la carte centrale, ou faites un clic droit sur n’importe quelle carte, pour ouvrir le choix d’actions :

| Action | Effet |
|---|---|
| **Modifier l’individu** | Ouvre le formulaire de la personne |
| **Fusionner avec…** | Ouvre l’assistant de fusion pour réunir une autre fiche de la même personne avec celle-ci |
| **Modifier l’union** | Ouvre le formulaire du couple ; en cas de plusieurs unions, choisissez d’abord laquelle |
| **Ajouter un conjoint**, **Ajouter un enfant**, **Ajouter un frère/sœur** | Recherchez dans l’arbre le proche à lier, ou créez une nouvelle personne |
| **Lien de parenté avec…** | Ouvre la page de parenté entre cette personne et une autre |
| **Aller à…** | Dans les graphiques autres que l’arbre : liste les proches que le graphique ne dessine pas, comme les conjoints et les enfants dans un graphique d’ancêtres |

Les formulaires sont décrits dans [Saisir et modifier les personnes](@/guide/editing.fr.md), la fusion dans [Outils](@/guide/tools.fr.md), la parenté dans [Parenté](@/guide/kinship.fr.md).

## Suivre les événements d’une personne

Le panneau de droite présente la personne sélectionnée : son portrait, son nom et les mêmes dates que sa carte. En dessous, ses événements sont classés par année, les événements non datés en tête sous **Sans date**. La liste replace aussi sa vie dans son contexte : ses unions, les naissances et décès de ses enfants, les événements de la famille de ses parents, les décès et inhumations de ses parents, les naissances et décès de ses frères et sœurs. Un événement qui concerne quelqu’un d’autre nomme cette personne entre parenthèses ; un événement d’union nomme le conjoint.

Cliquez sur un événement pour ouvrir la fiche de la personne, où figurent ses sources et ses notes.

- **Redimensionnez** le panneau en faisant glisser son bord gauche. Le graphique se réajuste quand vous relâchez.
- **Masquez-le** avec le bouton situé sur son bord gauche (**Masquer les événements**), et cliquez à nouveau pour le rouvrir. Sa largeur et son état ouvert ou fermé sont mémorisés.
- Dans une fenêtre étroite, le panneau se replie de lui-même ; dans une fenêtre très étroite, il disparaît.

## La barre latérale gauche

De haut en bas : **Vue profil** (la fiche de la personne sélectionnée), **Vue couple** (son premier couple, affiché seulement si elle a un conjoint connu), **Vue arbre**, **Profondeur des générations**, **Zoom avant**, **Zoom arrière**, **Ajuster à l’écran**, **Ajouter une personne** (une personne encore sans lien familial), puis **Dictionnaire**, **Statistiques**, **Outils**, **Imprimer** et **Paramètres**.

La même barre figure sur les fiches de personne et de couple et sur les autres pages de l’arbre : vous passez ainsi toujours du profil au couple et à l’arbre d’une même personne.

## Choisir un autre graphique

L’arbre n’est qu’une des neuf façons de dessiner la même généalogie. Choisissez-en une dans les paramètres de l’application, sous **Pedigree** → **Graphique**. Ce choix vaut pour la vue arbre sur cet appareil ; les petits arbres de la fiche de personne et des résultats de recherche restent des arbres.

| Graphique | Ce qu’il dessine |
|---|---|
| **Arbre** | Les ancêtres au-dessus, les descendants au-dessous, une ligne par génération |
| **Roue des ancêtres** | La personne au centre, chaque génération d’ancêtres en couronne autour d’elle |
| **Éventail** | Les ancêtres déployés en demi-cercle, la personne à sa base |
| **Roue des descendants** | La personne au centre, chaque génération de descendants en couronne, les unions entre elles |
| **Éventail des descendants** | Les descendants déployés en demi-cercle vers le bas |
| **Lignée** | La personne à gauche, une colonne d’ancêtres par génération, comme dans Gramps |
| **Lignée des descendants** | La personne à gauche, une colonne de descendants par génération, les conjoints sous chacun |
| **Sablier** | Les descendants à gauche, la personne au centre, les ancêtres à droite |
| **Papillon** | La lignée paternelle à gauche, la lignée maternelle à droite, la personne entre les deux |

{{ <screenshot name="fan-chart-dark" alt="Éventail des ancêtres Landrevel dans le thème sombre" lang={lang} /> }}

Chaque graphique conserve la barre du haut, la barre latérale, le panneau des événements et le choix d’actions, et se manie de la même façon : glisser pour se déplacer, zoomer, ajuster, cliquer sur une personne pour la placer au centre, clic droit pour le choix d’actions. Quelques différences :

- Les graphiques qui ne dessinent que des ancêtres masquent le compteur des descendants de **Profondeur des générations**, et ceux qui ne dessinent que des descendants masquent celui des ancêtres.
- Dans les roues et les éventails, le côté paternel occupe la première moitié de chaque couronne et le côté maternel la seconde. Un parent manquant est un segment en pointillés, avec un **+** quand la place le permet. Les couronnes éloignées sont écrites plus petit : zoomez (jusqu’à 16×) pour les lire. Survolez un segment pour lire le nom complet et les dates.
- Dans la roue et l’éventail des descendants, une fine couronne entre deux générations nomme chaque conjoint ; cliquez dessus pour passer à ce conjoint.
- Dans la lignée, un bouton **‹** à côté de la carte centrale liste ses conjoints et ses enfants ; choisissez-en un pour passer à lui. Dans les autres graphiques, utilisez **Aller à…** dans le choix d’actions.

{{ <screenshot name="hourglass-medieval" alt="Sablier de la famille Landrevel dans le thème médiéval" lang={lang} /> }}

## Changer le thème du graphique

Sous **Pedigree** → **Thème du pedigree**, dans les paramètres de l’application, choisissez le dessin des cartes et des traits :

- **Classique** : cartes sobres et connecteurs courbes, suivant le thème de l’application.
- **Médiéval** : un tracé gravé, chaque personne dans un écu héraldique avec un portrait en médaillon, reliée par des traits à l’équerre.

Le thème s’applique sur cet appareil à tous les arbres : la vue arbre, les petits arbres de la fiche de personne et ceux des résultats de recherche. Il est indépendant du thème de couleurs de l’application, qui décide toujours du clair ou du sombre.

## Imprimer le graphique

Cliquez sur **Imprimer** dans la barre latérale gauche, ou appuyez sur Ctrl+P (Cmd+P sur Mac).

- Quand tout le graphique est à l’écran, il s’imprime directement sur une feuille en paysage, avec le nom de l’arbre et la date en en-tête.
- Quand une partie déborde de l’écran et qu’il dépasse une feuille au zoom actuel, OxidGene demande d’abord quoi imprimer : **Ce qui est à l'écran** (une feuille) ou **Tout le graphique à ce zoom**, avec le nombre de feuilles nécessaires. Les feuilles se chevauchent d’1 cm jusqu’à un trait pointillé : coupez ou alignez sur ce trait pour les assembler. Au-delà de 50 feuilles, dézoomez pour en imprimer moins.

Les barres latérales, les commandes de zoom et de profondeur, le panneau des événements et le crayon ne s’impriment pas. Pour imprimer plus ou moins de l’arbre, zoomez ou déplacez-vous d’abord.

Pour aller plus loin : [spécification de l’arbre généalogique](@/tech/ui-genealogy-tree.md) (en anglais).
