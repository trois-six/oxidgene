+++
title = "Saisir et modifier les personnes"
description = "Créer et modifier personnes et couples : état civil, événements, dates, lieux, notes, sources, témoins, et les contrôles qui évitent les doublons."
weight = 60
+++

Un même formulaire crée ou modifie une personne, quel que soit le contexte : nouveau conjoint, enfant, frère ou sœur, parent, personne isolée ou fiche existante. Un second formulaire modifie un couple. Cette page explique comment les ouvrir, comment remplir chaque partie et comment OxidGene vous aide à ne pas saisir deux fois la même personne.

{{ <screenshot name="person-edit" alt="Formulaire d’un membre de la famille Landrevel, avec les sections état civil et naissance" lang={lang} /> }}

## Ouvrir le formulaire

| Pour… | Faites ceci |
|---|---|
| Modifier une personne | Dans la vue arbre, cliquez sur le **⋮** sous sa carte sélectionnée (ou faites un clic droit sur la carte) et choisissez **Modifier l’individu** ; sur sa fiche, cliquez sur **Modifier** |
| Ajouter un conjoint, un enfant, un frère ou une sœur | Dans le choix d’actions de la vue arbre, choisissez **Ajouter un conjoint**, **Ajouter un enfant** ou **Ajouter un frère/sœur** |
| Ajouter un parent | Dans la vue arbre, cliquez sur le **+** de la case vide en pointillés, à la place du père ou de la mère |
| Ajouter une personne sans lien familial | Cliquez sur **Ajouter une personne** dans la barre latérale gauche |

**Ajouter un conjoint**, **Ajouter un enfant** et **Ajouter un frère/sœur** ouvrent d’abord un petit panneau : « Rechercher une personne existante à lier, ou en créer une nouvelle : ». Tapez un nom pour retrouver une personne déjà présente et la lier, ou cliquez sur le bouton en dessous (par exemple **Créer une nouvelle personne comme enfant**) pour ouvrir le formulaire d’une nouvelle personne. Quand l’option **Proposer les personnes existantes** de l’arbre est désactivée, seule la nouvelle personne est proposée.

Le formulaire pré-remplit ce qu’il peut deviner, et tout reste modifiable :

- un nouveau père reçoit le nom de l’enfant et le sexe masculin ; une nouvelle mère reçoit le sexe féminin ;
- un nouvel enfant, frère ou sœur reçoit le nom de la personne sélectionnée ;
- un nouveau conjoint reçoit le sexe opposé à celui de la personne sélectionnée ;
- quand la personne sélectionnée a plusieurs unions, l’ajout d’un enfant demande d’abord à quelle union il appartient.

Après **Créer**, la nouvelle personne est rattachée à la famille et devient la personne centrale de l’arbre.

## Lier un parent déjà présent dans l’arbre

Quand vous ajoutez un parent depuis une case vide et que l’option **Proposer les personnes existantes** de l’arbre est active, un panneau au-dessus du formulaire liste jusqu’à cinq personnes dont le nom correspond à ce que vous tapez (« Personnes existantes portant ce nom : »). Pour un père, le panneau apparaît aussitôt, son nom étant déjà rempli.

- Cliquez sur **Lier cette personne** pour en faire le parent. Aucune personne n’est créée et le formulaire se ferme.
- Ou ignorez le panneau et continuez à remplir le formulaire : « Ou continuez à créer une nouvelle personne ci-dessous. »
- Fermez le panneau avec son **✕** s’il vous gêne.

L’enfant, les parents déjà enregistrés et les personnes de l’autre sexe sont exclus de la liste. L’option se trouve dans les [paramètres](@/guide/settings.fr.md) de l’arbre, sous **Options de saisie**.

## L’état civil

La section **État civil** regroupe l’identité de la personne :

- **Nom de naissance** et **Prénom(s)**, marqués d’un astérisque, doivent être remplis pour enregistrer. Le nom s’écrit en capitales au fil de la frappe, sauf si la mise en majuscules automatique des noms est désactivée pour l’arbre. Le champ des prénoms complète un prénom à la fois : dans « Jean Ma », il propose des prénoms pour « Ma ».
- **Sexe** : **Masculin**, **Féminin** ou **Inconnu**.
- **Source** : la source de l’identité dans son ensemble (voir [Sources](#sources) plus bas).

La section propose aussi les listes ci-dessous. Pour une personne en cours de création, leurs boutons d’ajout — comme **Ajouter un événement** et **Ajouter un média** plus bas — créent d’abord la personne avec ce que contient le formulaire, puis le formulaire devient sa fiche de modification, avec cette partie ouverte : la personne existe dès lors, même si vous annulez ensuite.

- **Profession(s)** : cliquez sur **Ajouter une profession**, tapez le métier tel que l’acte le nomme, ajoutez une date si vous en avez une, puis **Créer la profession**. Chaque profession est enregistrée comme un événement, elle apparaît donc dans la chronologie et suit les exports GEDCOM.
- **Autres informations** : cliquez sur **Ajouter une information** pour enregistrer un autre nom : nom marital, nom de jeune fille, alias, surnom ou sobriquet, nom religieux, préfixe ou suffixe, entre autres.
- **Notes** : cliquez sur **Ajouter une note**, rédigez-la, puis **Créer la note**. Chaque note se modifie ou se supprime séparément.

## Naissance et décès

Les sections **Naissance** et **Décès** ont chacune une **Date**, un **Lieu**, des **Notes** et une **Source**, enregistrés avec le formulaire. Quand la date de naissance manque, OxidGene affiche à la place celle du baptême sur les cartes et les fiches ; de même pour l’inhumation quand la date de décès manque.

### Témoins, parrains et marraines

Sous **Témoins**, liez les personnes présentes à la naissance ou au décès : témoins, parrain, marraine.

1. Cliquez sur **Ajouter un témoin**.
2. Indiquez son rôle dans le champ de relation, par exemple « Marraine ».
3. Recherchez la personne dans l’arbre et choisissez-la.

Le témoin doit déjà être une personne de l’arbre, et l’événement doit déjà être enregistré : pour une nouvelle personne, la section indique « Enregistrez d'abord cet événement pour ajouter des témoins. » Cliquez sur × à côté d’un témoin pour le retirer. Tout autre événement lie ses témoins de la même façon, dans son panneau (voir plus bas).

## Les autres événements

**Autres événements** liste son baptême, son inhumation, ses recensements, ses résidences, son service militaire, son testament et tous ses autres événements. Pour en ajouter un :

1. Cliquez sur **Ajouter un événement**.
2. Choisissez son **Type**. Les types sont regroupés par nature : sacrements, vie civile, actes légaux, attributs et autres.
3. Remplissez la **Description** (la valeur propre de l’événement, comme une profession ou une adresse), la **Date** et le **Lieu**.
4. Ajoutez au besoin une **Cause** ou l’**Âge** indiqué par l’acte. Les âges s’écrivent comme en GEDCOM : `34`, `1y 6m` (y années, m mois, w semaines, d jours), `< 1y`, `> 80y`, `CHILD`, `INFANT` ou `STILLBORN`.
5. Ajoutez des **Notes** et une **Source**. **Plus de détails** fait apparaître l’**Organisme responsable**.
6. Cliquez sur **Créer l’événement**.

Un événement enregistré affiche sur une ligne son type, sa description, sa date et son lieu. Cliquez sur **Modifier** pour ouvrir son panneau : les mêmes champs avec leur propre bouton **Enregistrer**, la **Fiabilité de la source** de sa citation (de **Peu fiable** à **Certain**, ou **Non évalué**) sous **Plus de détails** dès qu’une source est saisie, les **Témoins**, liés comme pour la naissance (une profession n’en a pas), et **Preuves**, les documents qui prouvent l’événement. Cliquez sur **Supprimer** pour retirer l’événement.

## Les dates

Tous les champs de date fonctionnent de la même façon, pour les personnes, les couples et les documents.

1. Choisissez le calendrier : **Grégorien**, **Julien**, **Hébreu** ou **Républicain**.
2. Choisissez la précision : **Exact**, **Environ**, **Calculée**, **Estimée**, **Peut-être**, **Avant**, **Après**, **Ou**, **Entre** ou **D’après un âge**.
3. Remplissez le jour, le mois et l’année. Les dates partielles sont acceptées : laissez vide le jour, ou le jour et le mois, quand l’acte ne donne qu’un mois ou une année.

**Ou** et **Entre** font apparaître une seconde date (« 1841 ou 1842 », « entre 1840 et 1845 »). **D’après un âge** demande l’âge et l’année où il a été relevé, par exemple un âge donné par un recensement.

Les mois hébreux et républicains se choisissent dans une liste de leurs propres noms, qui commence par une entrée vide pour une date connue seulement par son année (« an VI »). Changer le calendrier d’une date déjà saisie la convertit au lieu de simplement la renommer : le 11 mars 1796 devient le 21 ventôse an IV. Une date que le calendrier choisi ne sait pas exprimer reste telle que vous l’avez tapée.

Une date impossible est signalée juste sous le champ, par exemple « Cette date n’a jamais existé. » ou « La seconde date ne doit pas précéder la première. »

L’ordre des champs jour, mois et année, leurs séparateurs et le calendrier de départ d’un champ vide se règlent pour chaque arbre sous **Options de saisie** dans les [paramètres](@/guide/settings.fr.md) de l’arbre. L’affichage des dates est lui aussi un réglage de l’arbre.

## Les lieux

Les champs de lieu font des suggestions pendant la frappe et acceptent toujours le texte libre (indication : « Commune, village, paroisse… »).

- Les lieux de l’arbre viennent en premier, les plus utilisés d’abord.
- À partir du troisième caractère, le dictionnaire de lieux intégré prend la suite. Il couvre la France, le Royaume-Uni, l’Allemagne, l’Italie, l’Espagne, la Suisse, la Pologne, les États-Unis, le Portugal, la Belgique, le Luxembourg et les Pays-Bas, et fonctionne sans connexion réseau.
- Chaque suggestion donne le nom du lieu puis le reste de son libellé : code, département ou comté, région, pays. Une commune fusionnée ou renommée depuis est aussi proposée sous son ancien nom, avec la mention « jusqu'en » suivie de l’année où elle a disparu.

À l’enregistrement, un lieu choisi dans la liste est utilisé tel quel. Un texte tapé devient le lieu de l’arbre portant le même nom, sans tenir compte des majuscules ; si l’arbre n’en a pas, il est créé. Un lieu créé depuis le dictionnaire en reprend les coordonnées, et figure donc sur la carte des statistiques.

## Les suggestions dans les autres champs

Les champs de nom, de prénom, de profession et de source font eux aussi des suggestions : d’abord les valeurs de l’arbre, chacune avec le nombre de personnes qui l’utilisent, puis les termes des fiches de référence, dans n’importe quelle langue. Une ligne marquée **info** a une fiche explicative, que vous lirez en survolant le terme sur la fiche de la personne. Une suggestion choisie est saisie telle que l’arbre ou la fiche l’écrit.

Les suggestions ne limitent jamais ce que vous tapez. Désactivez-les pour un arbre avec **Suggestions de saisie** sous **Options de saisie** dans les paramètres de l’arbre.

## Sources

Chaque champ **Source** est un simple texte : tapez la source telle que vous la lisez sur l’acte, par exemple « AD29, état civil de Quimper, naissances 1843, vue 12, acte 26 ». Le champ propose les titres de sources déjà présents dans l’arbre.

À l’enregistrement, un titre identique à une source existante de l’arbre, sans tenir compte des majuscules, la réutilise ; tout autre titre crée une nouvelle source. Corriger une faute de frappe reporte la citation sur le titre corrigé, et l’ancienne source disparaît si plus rien ne l’utilise.

Une source saisie dans **Naissance**, **Décès** ou le panneau d’un événement est rattachée à cet événement. Quand elle désigne un registre d’archives connu d’OxidGene, la fiche de la personne en fait un bouton qui ouvre le registre : voir [Fiches de personne et de couple](@/guide/persons.fr.md#ouvrir-le-registre-cite) et [Sources et archives](@/guide/sources-archives.fr.md).

## Confidentialité

**Confidentialité** indique si la personne doit être **Par défaut**, **Public** ou **Privé**. Ce choix est enregistré dès maintenant mais ne masque encore rien, comme le précise le formulaire : il prendra effet avec les comptes et la publication.

## Enregistrer, ou partir sans enregistrer

- Cliquez sur **Créer** (nouvelle personne) ou **Enregistrer** (personne existante).
- **Annuler**, la croix, Échap ou un clic en dehors du formulaire le ferment sans enregistrer. Si vous avez modifié quelque chose, OxidGene demande d’abord « Abandonner les modifications ? ».

Les photos et documents ne font pas partie de l’enregistrement du formulaire : ils sont écrits dès que vous les ajoutez. Voir [Photos et documents](@/guide/media.fr.md).

### Le contrôle des homonymes

Après chaque enregistrement, OxidGene cherche dans l’arbre les autres personnes qui portent exactement les mêmes nom et prénoms, sans tenir compte des majuscules ni des accents. S’il en trouve, une fenêtre **Homonyme trouvé** demande si la personne que vous venez d’enregistrer est l’une d’elles.

- Laissez **Une autre personne** sélectionné et cliquez sur **Conserver séparément** : les deux fiches restent, et la question ne sera plus posée pour elles.
- Choisissez l’une des personnes proposées, chacune présentée avec ses dates, sa famille et son lieu de naissance, puis cliquez sur **Fusionner** : la fiche que vous venez d’enregistrer est fusionnée avec cette personne puis supprimée. Cette action est irréversible.
- Cliquez sur **Décider plus tard** (ou appuyez sur Échap) pour répondre une autre fois ; la question reviendra au prochain enregistrement.

## Supprimer une personne

1. Ouvrez le formulaire de la personne.
2. Tout en bas, cliquez sur **Supprimer cette personne**.
3. Lisez l’avertissement et cliquez sur **Confirmer la suppression**.

La personne et ses événements sont supprimés, ainsi que ses liens avec ses parents, ses enfants et ses conjoints. Une personne supprimée peut encore être restaurée depuis son [historique](@/guide/history.fr.md).

## Modifier un couple

Ouvrez le formulaire du couple avec **Modifier l’union** dans le choix d’actions de la vue arbre (choisissez d’abord l’union si la personne en a plusieurs), ou avec **Modifier le couple** sur la fiche de couple.

- **Chaque conjoint** a un bloc replié avec les mêmes champs que le formulaire de personne. Dépliez-le pour modifier ce conjoint sans quitter le couple.
- **Événements de l’union** : cliquez sur **Ajouter un événement d’union** et choisissez le type : mariage, divorce, annulation, fiançailles, bans, contrat, licence, convention matrimoniale, ou un autre événement comme une résidence. Chaque événement a une date, un lieu, des notes et une source. Le panneau d’un événement d’union enregistré propose aussi l’**Âge de** chaque conjoint, tel que l’acte de mariage le donne.
- **Enfants** : cliquez sur **Ajouter un enfant** pour lier une personne existante ou en créer une. **Détacher** retire un enfant de cette union sans le supprimer ; ce retrait ne s’applique qu’à l’enregistrement, et **Annuler** le défait.
- **Confidentialité** et **Médias** fonctionnent comme dans le formulaire de personne.

Cliquez sur **Enregistrer** pour tout appliquer d’un coup. **Supprimer le couple** retire l’union et ses événements après confirmation ; les deux personnes restent dans l’arbre, et leurs enfants ne sont plus liés à cette union.

Pour aller plus loin : [spécification du formulaire de personne](@/tech/ui-person-edit-modal.md) (en anglais).
