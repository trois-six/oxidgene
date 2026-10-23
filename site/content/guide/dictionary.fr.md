+++
title = "Dictionnaire"
description = "Parcourir les noms de famille, sources, dépôts, lieux et professions d'un arbre, et les renommer ou les fusionner en une fois."
weight = 110
+++

Le dictionnaire recense toutes les valeurs distinctes saisies dans un arbre — noms de famille, sources, dépôts, lieux et professions —, chacune une seule fois, avec son nombre d'utilisations. C'est l'endroit où repérer un patronyme écrit de deux façons, une source citée à une douzaine d'endroits ou un lieu saisi un peu différemment à chaque fois, et où les corriger pour toutes les fiches qui les utilisent à la fois : renommer ou fusionner un nom de famille ou une profession, modifier ou fusionner un lieu ou un dépôt.

## Ouvrir le dictionnaire

Cliquez sur l'**icône livre** (**Dictionnaire**) dans la barre d'icônes de gauche de n'importe quelle page de l'arbre : l'arbre lui-même, la fiche d'une personne ou d'un couple, les statistiques. La page s'ouvre sur l'onglet **Noms de famille**.

Les onglets en haut de la page sont **Noms de famille**, **Sources**, **Dépôts**, **Lieux**, **Professions**, **Médias** et **Étiquettes**. L'onglet **Médias** est la médiathèque de l'arbre, décrite dans [Médias](@/guide/media.fr.md). L'onglet **Étiquettes** liste les étiquettes de l'arbre, décrites dans [Étiquettes](@/guide/tags.fr.md).

Si vous revenez au dictionnaire avec le bouton retour, il se rouvre exactement comme vous l'avez laissé : même onglet, même filtre, même lettre, même page et même ligne dépliée.

{{ <screenshot name="dictionary" alt="Onglet Noms de famille du dictionnaire, filtré sur LANDREVEL, avec la liste des personnes qui portent ce nom" lang={lang} /> }}

## Parcourir et filtrer une liste

Les onglets **Noms de famille**, **Lieux** et **Professions** fonctionnent de la même façon :

- **Lettres** : cliquez sur une lettre pour n'afficher que les entrées qui commencent par elle, ou sur **Tout** pour tout réafficher. Les lettres sans entrée sont grisées.
- **Filtre** : tapez dans le champ **Filtrer...** pour resserrer la liste au fil de la saisie. Le filtre se combine avec la lettre choisie.
- **Nombre** : le nombre d'entrées correspondantes s'affiche à droite des lettres, par exemple « 142 entrées ».
- **Taille de page** : choisissez 25, 50, 100 ou **Tout**. Afficher toutes les entrées d'une très longue liste peut être lent, et la page vous prévient au-delà de 500.

Les entrées sont regroupées sous leur première lettre. Cliquez sur une entrée, ou sur la flèche au bout de sa ligne, pour déplier les personnes qui l'utilisent. Chaque personne est un lien.

Quand rien ne correspond, **Effacer le filtre** fait revenir toute la liste.

## Noms de famille

Chaque ligne est un nom de famille exactement tel qu'il est écrit, avec le nombre de personnes qui le portent, que ce soit leur nom principal ou un autre nom (alias, nom d'épouse). La liste ne regroupe pas les graphies : « Martin » et « MARTIN » font deux lignes, tout comme « Cruz » et « de la Cruz ». C'est voulu, pour que vous voyiez les variantes ; le renommage (ci-dessous) permet de les unifier.

Dépliez un nom de famille pour lister les personnes qui le portent ; cliquez sur une personne pour l'ouvrir dans l'arbre.

Le classement d'un nom à particule comme « de la » ou « d' » dépend de l'option **Tri des patronymes** des [paramètres de l'application](@/guide/settings.fr.md). Avec **Particule incluse**, « de la Cruz » est classé à la lettre D. Avec **Particule ignorée**, il est classé à la lettre C et s'affiche « Cruz (de la) », pour que le groupe C se lise par le mot sur lequel il est trié.

## Renommer ou fusionner un nom de famille

Ouvrez le menu **⋮** en bout de ligne d’un nom de famille et choisissez **Modifier** pour ouvrir **Modifier le nom de famille**.

1. Modifiez le champ **Nom**. Si l'arbre écrit les noms de famille en capitales, le champ le fait aussi.
2. Lisez la ligne en dessous : elle indique combien de personnes seront renommées.
3. Cliquez sur **Renommer**.

Seules les personnes qui portent ce nom comme nom **principal** sont renommées. Celles qui ne le portent que comme nom secondaire (alias, nom d'épouse) le gardent, et la boîte de dialogue indique combien elles sont ; le nom reste alors dans le dictionnaire pour elles.

Si le nouveau nom existe déjà dans l'arbre, la boîte de dialogue vous prévient que les deux seront fusionnés. Les personnes renommées rejoignent le nom existant et prennent sa particule ; pour changer cette particule, modifiez la ligne du nom existant.

Un renommage est inscrit dans l'[historique](@/guide/history.fr.md) de l'arbre comme une seule entrée, et chaque personne renommée reçoit une nouvelle version. Le renommage ne s'annule pas d'un bloc : pour rétablir une personne, restaurez-la depuis son propre historique.

## Corriger la coupure d'une particule

À l'import d'un arbre, OxidGene devine quels premiers mots d'un nom de famille forment une particule (« de », « le », « van der »…). Une erreur touche d'un coup toutes les personnes qui portent ce nom, par exemple des noms en « Le … » classés à leur second mot. Pour la corriger :

1. Ouvrez le menu **⋮** de la ligne du nom de famille et choisissez **Modifier**.
2. Laissez le **Nom** inchangé et modifiez le champ **Particule**. Videz-le quand le premier mot fait partie du nom.
3. Vérifiez l'aperçu, qui montre la **Particule**, le **Nom** et la lettre de classement (**Classé à**).
4. Cliquez sur **Appliquer à tous**.

La particule doit déjà figurer au début du nom : OxidGene n'ajoute jamais un mot que le nom ne contient pas. Le nom lui-même ne change pas, seulement l'endroit où il se coupe et la lettre sous laquelle il se classe.

## Sources

Un arbre contient souvent des milliers de sources, dont beaucoup commencent de la même façon (« AD44 … »). Plutôt qu'une simple liste de A à Z, l'onglet **Sources** permet de les resserrer étape par étape :

1. **Choisissez une lettre :** seules les lettres par lesquelles commence au moins une source sont proposées.
2. Tant qu'il reste plus de 250 sources, l'onglet propose les suites possibles (par exemple « AD4 », puis « AD44 - »), chacune avec son nombre de sources. Les étapes qui n'offrent qu'une seule suite possible sont franchies pour vous.
3. Dès qu'il reste 250 sources ou moins, elles sont toutes listées d'un coup, sans pagination.

Le chemin parcouru s'affiche sous forme de fil d'Ariane commençant par **Toutes les sources** ; cliquez sur n'importe quelle partie pour remonter. Le champ **Filtrer...** resserre le niveau en cours.

Un groupe dont l'intitulé est exactement celui du niveau au-dessus regroupe les sources qui portent exactement ce titre, sans caractère de plus. Cliquez dessus pour ne lister que ces sources : le fil d'Ariane affiche alors « Titre exactement « … » ».

Chaque ligne de source affiche son titre, son auteur et les dépôts qui la conservent, ainsi que son nombre de citations. Dépliez-la pour voir les personnes que la source documente : celles citées directement, la personne d'un événement cité, et les deux conjoints d'une union ou d'un événement d'union cités.

### Modifier une source

Ouvrez le menu **⋮** en bout de ligne d’une source et choisissez **Modifier** pour ouvrir **Modifier la source** :

- **Titre** (obligatoire), **Auteur**, **Abréviation** et **Publication** ;
- sous **Plus de détails** : l'**Organisme responsable** et les **Dépôts** qui conservent la source. Choisissez un dépôt de l'arbre, ou **Nouveau dépôt…** pour en créer un, indiquez au besoin une **Cote** et un **Support**, puis cliquez sur **Ajouter le dépôt**. Une source conservée sous deux cotes dans le même dépôt a deux lignes.

Les lignes de dépôt sont ajoutées ou retirées immédiatement ; les autres champs attendent **Enregistrer**.

## Dépôts

L'onglet **Dépôts** liste les services d'archives, bibliothèques et administrations qui conservent vos sources, triés par nom, avec la première ligne de leur adresse, leur téléphone et leur site web. Tapez dans **Filtrer...** pour resserrer la liste.

- Dépliez un dépôt pour voir les sources qu'il conserve, avec leurs cotes et leur support.
- Choisissez **Modifier** dans le menu **⋮** d’un dépôt pour le modifier, ou sur **Ajouter un dépôt** pour en créer un. Un dépôt a un **Nom** (obligatoire), une **Adresse**, un **Téléphone**, un **Courriel**, un **Site web** et des notes.
- Quand vous donnez à un dépôt le nom d'un autre (majuscules, accents et ponctuation mis à part), le formulaire propose de les fusionner : voir [Fusionner deux lieux ou deux dépôts](#fusionner-deux-lieux-ou-deux-depots).
- **Supprimer le dépôt**, en bas de son formulaire, le supprime après confirmation. Les sources qu'il conservait restent dans l'arbre ; elles n'y sont simplement plus rattachées. Annuler la suppression depuis l'historique rétablit ces liens.

## Lieux

Chaque ligne est un lieu tel qu'il a été saisi, avec une punaise pleine lorsque le lieu a des coordonnées géographiques et estompée sinon, et le nombre d'événements et de médias qui l'utilisent. Dépliez un lieu pour lister les personnes concernées : celles de ses événements (les deux conjoints pour un événement de couple) et celles liées à ses médias.

### Modifier un lieu

Ouvrez le menu **⋮** de la ligne d'un lieu (ou faites un clic droit dessus) et choisissez **Modifier** pour ouvrir **Modifier le lieu**. Changez son **Nom**, sa **Latitude** et sa **Longitude** (laissez-les vides pour aucune coordonnée), puis cliquez sur **Enregistrer** : chaque événement et chaque média situés en ce lieu affichent le changement.

### Fusionner deux lieux ou deux dépôts

Quand un lieu ou un dépôt a été saisi deux fois, donnez à l'un le nom de l'autre dans son formulaire. Les majuscules, les accents et la ponctuation ne comptent pas : « SAINT-ALBAN » correspond à « Saint Alban ». Le formulaire alors :

1. Signale que ce nom est celui d'un autre lieu (avec son nombre d'utilisations) ou d'un autre dépôt. Si plusieurs correspondent, choisissez celui avec lequel fusionner dans **Fusionner avec**.
2. Coche **Fusionner avec « … »**. La case reste décochée quand les deux portaient déjà le même nom, car deux lieux peuvent partager un nom et être différents : cochez-la vous-même pour les fusionner.
3. Liste les détails que les deux portent différemment, le nom et les coordonnées d'un lieu, le nom, l'adresse, le téléphone, l'e-mail et le site d'un dépôt, chacun avec un choix entre **Celle-ci** et l'autre fiche. Un détail que seule l'une des deux possède est conservé.
4. Cliquez sur **Fusionner**.

L'autre fiche reste, avec les détails choisis. Un lieu fusionné reprend tous les événements et médias de celui-ci, et ses étiquettes. Un dépôt fusionné reprend les sources que celui-ci conservait, ses notes et ses étiquettes ; une source déjà conservée là sous la même cote n'apparaît pas deux fois. Celui-ci est ensuite supprimé. La fusion forme une seule entrée de l'[historique](@/guide/history.fr.md) de l'arbre.

## Professions

Chaque ligne est une profession telle qu'elle a été saisie, avec le nombre de personnes qui l'ont exercée. Dépliez-la pour les lister.

### Renommer ou fusionner une profession

Ouvrez le menu **⋮** au bout de la ligne d'une profession et choisissez **Modifier** pour ouvrir **Modifier la profession**. Changez le champ **Profession**, lisez la ligne en dessous, qui indique combien de personnes seront concernées, et cliquez sur **Renommer**. Chaque événement de profession qui la mentionne prend le nouveau libellé. Si ce libellé existe déjà, le formulaire vous prévient que les deux seront fusionnées.

Un renommage forme une seule entrée de l'[historique](@/guide/history.fr.md) de l'arbre, et chaque personne renommée reçoit une nouvelle version, que vous pouvez restaurer personne par personne.

## Imprimer une liste

Le bouton d'impression de la barre supérieure imprime la liste de l'onglet en cours. Une liste paginée imprime la page affichée ; choisissez d'abord **Tout** comme taille de page pour imprimer toute la liste.

Pour aller plus loin : [spécification du dictionnaire](@/tech/ui-dictionary.md) (en anglais).
