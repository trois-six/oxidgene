+++
title = "Sources et archives"
description = "Citer vos sources, noter ce que dit l'acte et ouvrir en un clic un registre cité sur le portail des archives qui le conservent."
weight = 80
+++

Cette page explique comment citer les sources de vos recherches dans OxidGene — l'âge indiqué par l'acte, l'organisme qui l'a tenu et la confiance que vous lui accordez — et comment un registre cité s'ouvre directement sur le portail des archives qui le conservent.

## Sources, citations et dépôts

OxidGene distingue trois notions liées :

- Une **source** est le document consulté : un registre paroissial, un registre d'état civil, un recensement, un livre. Elle a un titre et, au besoin, un auteur, une abréviation et une publication.
- Une **citation** rattache une personne, un événement ou une union à une source, et indique où se trouve l'information dans cette source (page, vue, numéro d'acte). Elle peut aussi contenir le texte que vous en avez relevé et un niveau de fiabilité.
- Un **dépôt** est le service d'archives, la bibliothèque ou l'administration qui conserve une source, sous une cote donnée.

Une même source est en général citée de nombreuses fois : un registre prouve les naissances de plusieurs enfants. Une citation, elle, appartient à une seule personne, un seul événement ou une seule union.

## Citer une source

Les sources se citent depuis le formulaire de modification d'une personne (voir [Modifier les personnes et les familles](@/guide/editing.fr.md)).

1. Ouvrez le formulaire de modification de la personne.
2. Repérez l'événement à documenter : la section **Naissance** ou **Décès**, ou un événement de la liste des événements du formulaire (dépliez un événement enregistré pour voir ses champs).
3. Saisissez la source dans son champ **Source**, telle que vous la lisez sur le document : registre, cote d'archives ou titre du document.
4. Enregistrez le formulaire, ou le volet propre à l'événement.

Pendant la saisie, le champ **Source** propose les titres des sources déjà présentes dans l'arbre. À l'enregistrement, un titre identique à celui d'une source existante (sans tenir compte des majuscules) la réutilise ; tout autre titre crée une nouvelle source. Si vous corrigez ensuite le titre, la citation est rattachée à la source corrigée, et l'ancienne source disparaît si plus rien ne l'utilise.

Le formulaire propose aussi un champ **Source** dans la partie état civil, pour une source qui concerne la personne dans son ensemble plutôt qu'un seul événement, ainsi qu'un champ **Source** sur chaque événement d'une union dans le formulaire du couple.

## Noter l'âge, l'organisme et la fiabilité

Un acte en dit souvent plus que la date et le lieu. Dans la liste des événements du formulaire, chaque événement peut aussi conserver :

- l'**Âge** : l'âge que l'acte donne à la personne, écrit à la manière de GEDCOM : `34` ou `34y`, `1y 6m`, `< 1y`, `> 80y`, `CHILD`, `INFANT` ou `STILLBORN` (`y` années, `m` mois, `w` semaines, `d` jours). Une valeur qui n'est pas un âge est refusée avec un message à l'enregistrement ;
- sur un mariage ou un autre événement d'union, un champ **Âge de …** pour chaque conjoint, puisqu'un tel acte donne l'âge de chacun ;
- sous **Plus de détails**, l'**Organisme responsable** : l'autorité qui a tenu l'acte, par exemple une paroisse ou un bureau d'état civil ;
- sous **Plus de détails**, une fois l'événement enregistré et pourvu d'une source, la **Fiabilité de la source** de sa citation : **Non évalué** (par défaut), **Peu fiable**, **Douteux**, **Secondaire**, **Direct** ou **Certain**.

**Plus de détails** s'ouvre de lui-même lorsqu'un événement a déjà un organisme ou une citation évaluée : rien de ce que vous avez saisi ne reste caché.

## Où apparaissent vos sources

Sur la fiche d'une personne, chaque événement de la chronologie liste ses sources après la mention **Sources** : le titre de la source suivi de l'endroit où se trouve l'information. L'âge noté apparaît en toutes lettres après le lieu, par exemple « à 34 ans », « nourrisson » ou « mort-né », et l'organisme sur sa propre ligne, **Organisme responsable : …**.

Le [Dictionnaire](@/guide/dictionary.fr.md) recense toutes les sources de l'arbre avec leur nombre de citations et les personnes qu'elles documentent. C'est là que vous modifiez une source elle-même — titre, auteur, abréviation, publication, organisme responsable — ainsi que les dépôts qui la conservent, avec leurs cotes.

## Ouvrir un registre cité sur le portail des archives

La plupart des sources généalogiques sont des pages de registres que les services d'archives ont numérisées et mises en ligne sur leur propre portail. Lorsqu'une citation désigne un tel registre, sa source devient un bouton sur la fiche de la personne. Survolez-le : l'infobulle indique **Ouvrir la vue citée aux …** suivi du nom des archives. Cliquez, et OxidGene trouve le registre sur le portail et l'ouvre à la vue citée, sans que vous ayez à refaire la recherche à la main.

{{ <screenshot name="archive-register" alt="La source d'une naissance désignant un registre des archives de Loire-Atlantique, encadrée sur la fiche d'une personne, et l'onglet du portail des archives qu'elle a ouvert sur le registre cité, dont la page est floutée" lang={lang} /> }}

Le catalogue couvre les archives départementales françaises et l'état civil des Archives nationales d'outre-mer. Il connaît les registres paroissiaux et d'état civil, les tables décennales et paroissiales, les recensements de population, les registres matricules, les listes de conscription, les tables des successions et absences, et les registres d'inhumation des cimetières.

### Comment un registre cité est reconnu

Inutile de suivre une convention précise. OxidGene lit le titre de la source et ses autres champs, la page de la citation, les dépôts qui conservent la source avec leurs cotes, ainsi que le type, l'année et le lieu de l'événement cité. Ces trois écritures désignent le même acte :

```text
AD72 - Exampleville - (aucun) - N - 1872 - 9E 999/9 - acte 31 - vue 45/200
AD72, état civil de Exampleville, naissances 1872, cote 9E 999/9, acte 31, vue 45/200
Registres paroissiaux et d'état civil — vue 45, acte 31
```

La première ligne est la forme normalisée (code des archives, commune, paroisse, acte, période, cote, numéro d'acte, vue), lue en premier et de la façon la plus stricte. La deuxième est la description rédigée habituelle. La troisième fonctionne lorsque la source a pour dépôt les archives concernées et que la citation est rattachée à la naissance à Exampleville en 1872 : l'événement fournit le type d'acte, l'année et la commune que le texte ne donne pas.

Quelques règles à connaître :

- Les archives doivent être désignées : par leur code (`AD72`), leur nom, un dépôt qui conserve la source ou l'adresse de leur site. Le lieu d'un événement ne désigne jamais un service d'archives, car un acte n'est pas toujours enregistré là où l'événement a eu lieu.
- La page de la citation complète la source : les vues et le numéro d'acte qu'elle indique l'emportent sur ceux du titre.
- Si la source ou la citation contient l'adresse d'une page du portail des archives, cette adresse s'ouvre telle quelle, sans recherche.
- Les citations peuvent être rédigées dans n'importe quelle langue de l'interface — français, anglais, allemand, espagnol, italien, néerlandais, polonais ou portugais : `AD72, civil registration of Exampleville, births 1872, image 45` ouvre le même registre que sa version française.

Une citation qui ne désigne aucun service d'archives, qui désigne des archives absentes du catalogue, ou un type de document que ces archives ne publient pas, reste du texte simple.

### Sur l'application de bureau : la fenêtre des archives

Dans l'application de bureau, le registre s'ouvre dans une fenêtre des archives distincte, à côté de la fenêtre principale qui reste là où vous étiez. Les portails d'archives n'acceptent pas d'être affichés à l'intérieur d'une autre application : la fenêtre montre donc le portail lui-même.

Pendant la recherche, la fenêtre affiche un écran d'attente avec les archives, la citation et l'étape en cours : **Connexion au portail des archives…**, **Recherche du registre…**, puis **Ouverture de la vue …**. Au bout de quelques secondes, le temps écoulé s'affiche à côté de l'étape. Cliquez sur **Annuler** pour arrêter la recherche : la fenêtre affiche alors la page de recherche du portail.

Une fois le registre affiché, vous naviguez librement sur le portail. Un bandeau au-dessus de la page vous prévient lorsqu'il faut votre attention :

- aucun registre, ou plusieurs, ne correspondent à la citation : la fenêtre affiche les résultats de recherche du portail filtrés selon la citation, pour que vous choisissiez ;
- la numérotation des vues a changé depuis la rédaction de la citation (registre renumérisé ou relié autrement) : le bandeau donne l'ancien et le nouveau nombre de vues ainsi que la vue estimée, qui n'est pas forcément la page citée ;
- la visionneuse du portail s'ouvre toujours sur la première vue : OxidGene saisit pour vous la vue citée dans le champ de numéro de page de la visionneuse, et ne vous demande d'y aller vous-même qu'en cas d'échec.

La fenêtre des archives franchit les petits obstacles des portails comme vous le feriez, sans jamais décider à votre place :

- **Bandeaux de cookies** : OxidGene refuse en votre nom le consentement aux cookies du portail et ne l'accepte jamais. Un bandeau qui ne propose que d'accepter vous est laissé.
- **Vérifications anti-robot** : certains portails vérifient que le visiteur n'est pas un robot. Une vérification qui se résout seule en quelques secondes est simplement attendue. Sinon, le bandeau vous demande d'y répondre dans la fenêtre, et la recherche reprend une fois que c'est fait. OxidGene ne répond jamais lui-même à une vérification.
- **Licences de réutilisation** : lorsqu'un registre est soumis à la licence de réutilisation des archives, la fenêtre ouvre la page de la licence et vous demande de l'accepter. Une fois acceptée, elle poursuit d'elle-même jusqu'au registre.

La fenêtre des archives conserve les cookies des portails d'une fois sur l'autre : une licence ou une vérification anti-robot n'est en général à passer qu'une seule fois. Sous Linux, si le certificat de sécurité d'un portail ne peut pas être vérifié, la fenêtre le signale et propose **Ouvrir dans le navigateur**.

### Sur le web : un nouvel onglet

Dans l'application web, le registre s'ouvre dans un nouvel onglet du navigateur. L'onglet s'ouvre aussitôt avec **OxidGene recherche le registre cité…**, puis se rend au registre une fois trouvé. Certains portails ne se laissent interroger que depuis un navigateur : l'onglet s'ouvre alors sur les résultats de recherche du portail filtrés selon la citation, et vous ouvrez le registre depuis ces résultats.

L'onglet ne peut pas porter le bandeau d'OxidGene : les mêmes messages s'affichent donc à côté de la source, sur la fiche — aucun registre ou plusieurs, numérotation modifiée, vue à atteindre. Pour un registre soumis à une licence de réutilisation, l'onglet ouvre la licence : acceptez-la sur le portail, puis recherchez-y le registre. Un bloqueur de fenêtres surgissantes strict peut empêcher l'onglet de s'ouvrir ; autorisez-les pour OxidGene le cas échéant.

### Quand la recherche échoue

Si le portail ne répond pas à temps, est injoignable, ou présente une vérification anti-robot restée sans réponse, la fenêtre ou l'onglet ouvre la page de recherche du portail filtrée selon la citation, avec un message, pour que vous poursuiviez à partir de là. Pour tout autre échec, c'est le site des archives qui s'ouvre. Rien n'est relancé dans votre dos : cliquez de nouveau sur la source pour réessayer.

## Compléter une citation partielle

Il arrive qu'OxidGene reconnaisse les archives mais pas l'acte ni la commune, par exemple pour une citation réduite à « AD72, vue 45 ». La source reste un bouton, mais elle ouvre alors la boîte de dialogue **Rechercher dans les archives**.

1. Cliquez sur la source dans la fiche de la personne.
2. Dans **Rechercher dans les archives**, vérifiez ce qui a été reconnu. Les archives sont affichées et ne peuvent pas être changées.
3. Complétez ce qui manque : **Commune**, **Type de registre** (seuls les types que ces archives publient sont proposés), **Année** et **Vue**.
4. Cliquez sur **Rechercher**. Le registre s'ouvre exactement comme pour une citation complète, dans la fenêtre des archives ou dans un nouvel onglet.

La commune et le type de registre sont obligatoires ; s'ils manquent, la boîte de dialogue reste ouverte et indique ce qu'il faut compléter.

Une recherche n'enregistre rien. Pour conserver ce que vous avez saisi, cochez **Ajouter ces détails à la citation** avant de cliquer sur **Rechercher** : vos précisions sont ajoutées à la fin de la page de la citation, dans la langue de l'interface (par exemple `Exampleville, naissance 1872, vue 45`), la modification est inscrite dans l'[historique](@/guide/history.fr.md), et le clic suivant ouvre directement le registre. La source elle-même, que d'autres citations peuvent partager, n'est jamais modifiée.

## Ce qu'OxidGene transmet aux archives

Les portails d'archives sont des services publics, et OxidGene les utilise comme le ferait un lecteur :

- Une recherche par clic. OxidGene ne parcourt, ne collecte ni ne précharge jamais de pages des portails de lui-même.
- Le portail reçoit ce dont son formulaire de recherche a besoin : la commune, le type d'acte, l'année ou la période et la cote. Une collection interrogée par nom de personne, comme les registres matricules, reçoit en plus le nom de famille, et seulement lorsque rien d'autre ne permet de retrouver la ligne de la personne.
- OxidGene ne télécharge, ne copie ni ne stocke les images des archives. Pour garder une copie d'un acte dans votre arbre, ajoutez-la vous-même comme document (voir [Médias](@/guide/media.fr.md)).

Pour aller plus loin : [spécification des portails d'archives](@/tech/archives.md) (en anglais).
