+++
title = "Outils"
description = "Vérifier et travailler un arbre avec la page des outils : anomalies, lieux non localisés, complétude de l'ascendance, doublons, fusion et dates."
weight = 130
+++

La page des outils réunit ce qui sert à vérifier un arbre et à le travailler plutôt qu'à le consulter : les anomalies de dates et de liens familiaux, les lieux impossibles à situer, la complétude de votre ascendance, les doublons potentiels et leur fusion, ainsi que deux outils de dates. Cette page passe en revue chaque onglet.

## Ouvrir les outils

Cliquez sur l'icône de clé dans la barre d'icônes à gauche de n'importe quelle page de l'arbre, juste sous l'icône de graphique des statistiques. La page présente un outil par onglet : **Anomalies**, **Lieux non localisés**, **Complétude de l'ascendance**, **Doublons potentiels**, **Convertisseur de dates** et **Date en toutes lettres**. Elle rouvre le dernier onglet consulté.

Chaque onglet ne charge ses données qu'à son ouverture et les recalcule à chaque fois à partir de l'arbre : une correction apparaît dès que vous revenez sur l'onglet.

## Repérer les anomalies

L'onglet **Anomalies** passe tout l'arbre au crible à la recherche de dates, filiations, unions, témoins et fiches impossibles (**Erreur**) ou improbables (**Avertissement**).

1. Ouvrez l'onglet **Anomalies**. Une rangée de tuiles indique le nombre d'anomalies par catégorie : **Dates**, **Filiation**, **Unions**, **Témoins et parrains** et **Fiches**.
2. Cliquez sur une tuile pour n'afficher que cette catégorie ; cliquez à nouveau pour tout revoir.
3. Dans chaque catégorie, chaque règle qui a trouvé quelque chose forme un bloc replié avec sa gravité, son titre et son nombre. Dépliez un bloc pour lire ce que vérifie la règle et voir une ligne par anomalie.
4. Chaque ligne nomme les personnes concernées, chacune liée à sa fiche, avec **Ouvrir dans l'arbre** pour centrer le pedigree sur elle, et **Couple** pour une anomalie d'union. À côté figure ce que la règle a mesuré : un âge, un écart, le texte saisi.
5. Cliquez n’importe où ailleurs sur une ligne, ou appuyez sur **Entrée** dessus, pour ouvrir la fiche de sa première personne, où vous corrigez l’anomalie.
5. Corrigez la fiche, puis revenez sur l'onglet : la liste est recalculée.

Une règle liste au plus 500 anomalies et indique combien d'autres elle a trouvées. Un arbre sans anomalie l'annonce, avec le nombre de personnes vérifiées.

Utilisez la liste **Étiquette** au-dessus des anomalies pour ne garder que celles qui nomment une personne portant l’étiquette. Les compteurs suivent ce choix.

### Comment les dates sont lues

Les contrôles sont conçus pour ne pas crier au loup :

- La naissance est la naissance, ou le baptême si la naissance n'est pas datée ; le décès est le décès, ou l'inhumation si le décès n'est pas daté.
- Une date imprécise vaut pour tous les jours qu'elle peut désigner : une année seule pour toute l'année, un mois pour tout le mois. Une date vers, calculée ou estimée s'élargit de deux ans de chaque côté. Les dates avant, après, peut-être, ou entre deux dates ne sont pas utilisées.
- Une règle ne se déclenche que si elle se vérifie pour tous les jours que ces dates peuvent désigner. Une date vague ne soulève donc jamais une anomalie que son imprécision suffirait à expliquer.
- Les parents adoptifs, nourriciers et beaux-parents ne sont pas soumis aux règles d'âge et de décès qui s'appliquent aux parents biologiques.

### Le catalogue des règles

| Catégorie | Ce qui est vérifié |
|---|---|
| **Dates** | Décès avant la naissance, inhumation avant le décès, événement avant la naissance, baptême après le décès, événement après le décès ou après l'inhumation (hors inhumation, crémation, funérailles, homologation de testament et événements libres), décès à plus de 105 ans, centenaire né avant 1900, date dans le futur, et âge noté lors d'un événement contredit par les dates de plus de deux ans |
| **Filiation** | Parent ou ancêtre né après son enfant ou son descendant, personne figurant parmi ses propres ancêtres, parent de moins de 11 ans, père de plus de 70 ans ou mère de plus de 55 ans à une naissance, enfant né après le décès de sa mère ou plus de 300 jours après celui de son père, enfants nés à entre 11 jours et 7 mois d'intervalle, ou à plus de 50 ans d'intervalle |
| **Unions** | Union avant la naissance ou après le décès d'un conjoint, conjoint de moins de 12 ans ou de plus de 100 ans, conjoints nés à plus de 50 ans d'intervalle, les deux mêmes personnes unies plusieurs fois, plusieurs conjoints homonymes, union avec son propre parent, union à plus de deux conjoints |
| **Témoins et parrains** | Témoin ou parrain pas encore né ou déjà décédé lors de l'événement, parrain enregistré comme femme ou marraine comme homme |
| **Fiches** | Mari enregistré comme femme ou épouse comme homme, deux maris ou deux épouses dans une même union, date illisible, intervalle de dates qui finit avant de commencer, personne sans aucun nom |

Parrains et marraines sont reconnus d'après la relation notée sur un témoin, en plusieurs langues (parrain, godfather, Taufpate, padrino…). La liste complète des règles, avec leurs seuils et les contrôles de Geneanet correspondants, figure dans la [spécification des outils](@/tech/ui-tools.md) (en anglais).

{{ <screenshot name="tools-anomalies" alt="Onglet Anomalies de la page des outils" lang={lang} /> }}

## Localiser les lieux

L'onglet **Lieux non localisés** recense les lieux utilisés dans votre arbre que la [carte des statistiques](@/guide/statistics.fr.md) ne sait pas situer : des lieux sans coordonnées dont le dictionnaire des lieux ne reconnaît pas le nom. Ils sont classés du plus utilisé au moins utilisé, avec leur nombre d'utilisations.

1. Cliquez sur **Qui l'utilise** pour voir les personnes dont les événements ou les médias citent ce lieu ; chacune ouvre le pedigree sur elle.
2. Cliquez sur **Corriger**. Le nom devient un champ de lieu, avec les suggestions du dictionnaire des lieux.
3. Choisissez une suggestion du dictionnaire, ou tapez le nom corrigé, puis cliquez sur **Enregistrer**.

Le lieu est renommé, pas remplacé : ses événements et ses médias le conservent. Choisir une suggestion du dictionnaire lui apporte aussi ses coordonnées : il est alors localisé et quitte la liste. Le renommage est inscrit dans l'historique de l'arbre comme toute autre modification.

## Mesurer la complétude de l'ascendance

L'onglet **Complétude de l'ascendance** suit votre ascendance génération par génération depuis la racine SOSA de l'arbre (SOSA 1) : qui est connu à chaque numéro SOSA, et quels faits essentiels sont renseignés. Si l'arbre n'a pas encore de racine SOSA, l'onglet le signale et propose **Choisir une racine SOSA**, qui mène aux paramètres de l'arbre.

- **Générations** fixe la profondeur, de 4 à 15 générations (8 par défaut).
- Le résumé compte une ligne par génération : ancêtres trouvés sur ancêtres attendus (1, 2, 4, 8…), la proportion en barre (verte au-dessus de 70 %, orange à partir de 40 %, rouge en dessous), et combien, parmi ceux trouvés, ont une naissance, un décès et une union renseignés.
- En dessous, chaque génération liste ses ancêtres par numéro SOSA, chacun lié à sa fiche avec ses dates et une marque par fait essentiel : verte s'il est renseigné, rouge et barrée s'il manque. Une génération qui a des lacunes s'ouvre d'elle-même ; une génération complète reste repliée.
- **N'afficher que ce qui manque** ne garde que les ancêtres manquants et ceux à qui il manque un fait.

Les faits essentiels sont une naissance ou un baptême, un décès ou une inhumation, et une union avec l'autre parent de la lignée (mariage, union civile, publication de bans, contrat, etc.). Une personne sans décès noté et née il y a moins de 120 ans est peut-être encore en vie : aucun décès n'est attendu pour elle.

Seuls les parents manquants d'ancêtres connus sont listés comme **Ancêtre manquant**. Leurs propres ancêtres sont simplement comptés sous chaque génération (« 12 autres ancêtres manquent parce que leurs enfants manquent aussi »). Un ancêtre atteint par deux lignées (implexe) apparaît à chacun de ses numéros SOSA.

## Examiner les doublons potentiels

L'onglet **Doublons potentiels** liste les paires de personnes qui pourraient être une même personne saisie deux fois, des plus probables aux moins probables.

Deux personnes sont comparées quand leurs noms et leurs premiers prénoms se prononcent de la même façon (« Martins » et « Martin », « Dupond » et « Dupont ») ; seules les personnes ayant à la fois un nom et un prénom sont comparées. Une paire est écartée si les sexes diffèrent, si les naissances ou les décès sont à plus de 5 ans d'écart, si l'une est décédée avant la naissance de l'autre, si l'une est le conjoint, le parent ou l'enfant de l'autre, s'il s'agit de frères et sœurs nés à des jours connus différents, ou si vous avez déjà indiqué qu'il s'agit de deux personnes différentes.

Chaque paire reçoit un score selon les indices que les deux fiches partagent : même nom, même date ou même année de naissance, même lieu de naissance, même année de décès, mêmes parents ou parents de même nom, conjoint de même nom. Une paire n'est listée qu'à partir de 40 points : un nom courant ne suffit jamais à lui seul. La confiance indique **Très probable** à partir de 70 points, **Probable** à partir de 55, et **Possible** en dessous. La liste compte au plus 500 paires.

Pour chaque paire, vous voyez les deux personnes avec leurs dates, leurs proches et leur lieu de naissance, la confiance, le score et ce que les deux fiches ont en commun. Ensuite :

- Cliquez sur **Deux personnes différentes** s'il ne s'agit pas de la même personne. La paire quitte la liste définitivement.
- Cliquez sur **Comparer** pour ouvrir l'assistant de fusion sur la paire et trancher.

Après l'une ou l'autre réponse, la liste est recalculée.

{{ <screenshot name="tools-duplicates" alt="Onglet Doublons potentiels avec des paires de fiches à comparer" lang={lang} /> }}

## Fusionner deux fiches d'une même personne

L'assistant de fusion réunit deux fiches d'une même personne en une seule. Pour l'ouvrir :

- **Comparer** sur une paire de doublons potentiels (il s'ouvre directement sur la comparaison) ;
- **Fusionner avec…** dans le menu d'actions du pedigree (clic droit sur une carte, ou son crayon) ;
- **Fusionner avec…** dans l'en-tête de la fiche d'une personne.

L'assistant compte trois étapes.

1. **Choisir l'autre fiche**. Recherchez l'autre fiche de la personne dans l'arbre courant. La choisir fait passer à l'étape suivante.
2. **Comparer et choisir**. Les deux fiches sont présentées côte à côte. Choisissez celle à conserver avec **Garder cette fiche** (la première par défaut) ; l'autre sera absorbée. Dans le tableau de comparaison, les lignes dont les valeurs diffèrent sont mises en évidence ; là où la fiche absorbée a quelque chose à apporter, choisissez la valeur que garde la fiche fusionnée : nom, prénoms, sexe, et naissance, baptême, décès, inhumation et crémation. Par défaut, la valeur la plus précise (date plus complète, lieu renseigné) est retenue. En dessous, cochez les autres événements et les médias de la fiche absorbée à reprendre sur la fiche conservée. Ceux qu'elle possède déjà sont signalés « déjà sur la fiche conservée » et laissés décochés. Les noms, liens familiaux, notes et sources sont toujours rattachés à la fiche conservée. Cliquez sur **Suivant**.
3. **Vérifier et confirmer**. Un récapitulatif présente la fiche conservée, le nom principal qu'elle portera, ce qui est repris et ce qui est laissé. Lisez l'avertissement, puis cliquez sur **Fusionner**.

{{ <screenshot name="merge" alt="Assistant de fusion comparant deux fiches côte à côte" lang={lang} /> }}

Ce qui se passe ensuite :

- La fiche absorbée est supprimée ; tout ce que vous avez retenu passe sur l'autre. Les événements laissés sont supprimés ; les médias laissés restent dans la médiathèque, simplement détachés. L'ancien nom principal de la fiche conservée devient un nom secondaire.
- La fusion est irréversible, comme le rappelle l'assistant. Prenez le temps de bien faire vos choix à l'étape 2.
- Si la fiche absorbée était la racine de l'arbre, la fiche conservée prend sa place.
- Deux personnes conjointes dans une même famille, ou dont l'une est l'ancêtre de l'autre, ne peuvent pas être fusionnées.
- Depuis la liste des doublons, l'assistant propose aussi **Deux personnes différentes**, et **Annuler** laisse la paire dans la liste.

## Convertir une date d'un calendrier à l'autre

Le **Convertisseur de dates** lit une date dans chacun des calendriers où l'application enregistre des dates : grégorien, julien, hébraïque et républicain.

1. Saisissez la date avec le champ de date habituel : calendrier, qualificatif, jour, mois et année (et une seconde date pour « ou » et « entre »).
2. Une tuile par calendrier affiche la date telle que l'application l'écrit dans ce calendrier, qualificatif compris. Le calendrier de saisie est signalé **Telle que saisie**.

Une date partielle garde sa précision : une année seule devient l'année qu'elle recouvre le plus. Un calendrier qui ne peut pas exprimer la date, comme le calendrier républicain avant le 22 septembre 1792, affiche **Inexprimable dans ce calendrier**. Une date unique, exacte et complète donne aussi son jour de la semaine.

## Écrire une date en toutes lettres, et la relire

L'onglet **Date en toutes lettres** écrit une date comme le faisaient les registres et les actes, et relit une date écrite pour la remettre en chiffres. Il est précieux pour transcrire un acte paroissial ou notarié.

Pour écrire une date en toutes lettres :

1. Saisissez la date avec le champ de date. Une date hébraïque ou républicaine est écrite à partir du même jour du calendrier grégorien, ce que la page précise. Les années de 1 à 3999 peuvent être écrites.
2. Choisissez la **Forme** : **Tout en lettres** ou **Chiffres et nom du mois**.
3. Choisissez quand **L'année commence le** : **1er janvier**, ou **25 mars (style de l'Annonciation)**, que suivaient bien des registres. Dans ce style, une date du 1er janvier au 24 mars porte le millésime de l'année précédente.
4. Lisez la date dans chaque langue (anglais, français, allemand, espagnol, italien, néerlandais, polonais et portugais) et en **Latin**. Chaque ligne a un bouton de copie.

Les lignes latines ajoutent le **Latin, comput romain** d'une date complète (compté jusqu'aux calendes, nones ou ides suivantes, comme *ante diem IV Nonas Februarii*) et **Latin, partie par partie**, qui décompose le jour, le mois et l'année en toutes lettres et en chiffres romains.

Pour relire une date écrite, collez-la sous **Lire une date écrite** et cliquez sur **Lire**. Le champ de date du haut est alors rempli. L'outil comprend les nombres en toutes lettres dans chacune de ces langues ou en latin (cardinaux ou ordinaux), les chiffres, les chiffres romains, les noms de mois à tous les cas, les abréviations GEDCOM comme *FEB*, et le comput romain. S'il ne parvient pas à lire le texte, il dit pourquoi : rien à lire, aucune année trouvée, ou un jour qui n'existe pas dans ce mois. Une date relue est considérée comme grégorienne.

## Imprimer un onglet

Cliquez sur l'icône d'imprimante (**Imprimer**) dans la barre d'icônes à gauche pour imprimer l'onglet affiché avec ses résultats ; les champs de saisie et les boutons ne sont pas imprimés.

Pour aller plus loin : [spécification des outils](@/tech/ui-tools.md) et [spécification de la fusion](@/tech/ui-merge.md).
