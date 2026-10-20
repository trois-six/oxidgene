+++
title = "Import et export"
description = "Faire entrer dans OxidGene des arbres GEDCOM, GEDZIP, GeneWeb et Geneanet, et exporter vos arbres pour d’autres logiciels ou pour les mettre à l’abri."
weight = 30
+++

OxidGene lit les fichiers produits par les autres logiciels et sites de généalogie, et écrit des fichiers qu’ils savent lire. Cette page traite de l’import d’un fichier GEDCOM, GEDZIP ou GeneWeb, de l’import d’un arbre Geneanet avec ses photos, et de l’export d’un arbre.

## Quel format choisir

| Format | Extension | Contenu |
|---|---|---|
| GEDCOM | `.ged` | La généalogie, sous forme de texte. Les photos et documents n’y figurent que par le chemin de leur fichier ; les fichiers eux-mêmes n’y sont pas. |
| GEDZIP | `.gdz` | Un fichier GEDCOM 7.0 et les fichiers de médias auxquels il renvoie, réunis dans une seule archive. |
| GeneWeb | `.gw` | La généalogie telle que l’exportent GeneWeb et Geneanet. |

Si le logiciel que vous quittez sait écrire un GEDZIP, choisissez-le : c’est le seul format de fichier qui apporte vos photos et documents avec la généalogie.

## Importer un fichier

Un import se fait toujours dans un arbre existant. Créez d’abord l’arbre (voir [Les arbres](@/guide/trees.fr.md)) ; importer dans un arbre neuf et vide tient les données importées à l’écart de ce que vous avez déjà.

{{ <screenshot name="import" alt="Fenêtre d’import d’un arbre, avec ses deux onglets" lang={lang} /> }}

1. Sur la page d’accueil, ouvrez le menu à trois points de la carte de l’arbre et choisissez **Importer**.
2. La fenêtre **Importer dans « … »** s’ouvre sur l’onglet **Un fichier**.
3. Déposez votre fichier sur **Déposez un fichier ici, ou cliquez pour parcourir**, ou cliquez dessus pour le choisir. Elle accepte les fichiers `.ged`, `.gdz` et `.gw` jusqu’à 1 Gio.
4. Cliquez sur **Importer**.
5. Patientez pendant que la barre de progression passe par ses étapes : **Envoi du fichier**, **Lecture de la généalogie…**, **Enregistrement des médias** pour un GEDZIP, **Écriture de la généalogie…** puis **Préparation de l’arbre…**. La fenêtre ne peut pas être fermée tant que l’import est en cours.
6. Quand **Import terminé** s’affiche, lisez le bilan : personnes, familles, événements, sources, lieux, images et documents.

Si l’import a produit des avertissements, une ligne comme **3 avertissements** apparaît sous le bilan ; cliquez dessus pour les lire. Ils signalent ce que l’import n’a pas pu lire ou relier : un âge qui n’est pas écrit comme un âge, une note qui ne renvoie à rien, un fichier qu’un GEDZIP mentionne sans le contenir. Le reste du fichier est importé normalement.

Si l’import échoue, la fenêtre en donne la raison et garde votre fichier sélectionné, pour que vous puissiez réessayer.

Pendant l’import, la carte de l’arbre sur la page d’accueil affiche **Import en cours** et ne peut pas être ouverte. Vous pouvez recharger la page ou revenir plus tard : la carte redevient disponible une fois l’import terminé.

### Ce qui est importé

OxidGene lit les versions 5.5.1, 7.0 et 7.1 de GEDCOM, dans n’importe quel jeu de caractères, et reconnaît la version tout seul. Il importe les personnes et leurs noms, les familles avec leurs conjoints et leurs enfants (enfants adoptés et placés compris), les événements et attributs avec leurs dates, lieux, âges, causes et autorités, les sources et citations avec leur qualité, les dépôts d’archives, les notes, les témoins et associations, ainsi que les médias.

Ce qu’OxidGene ne sait pas représenter — une balise propre à un logiciel, une religion notée dans un événement plutôt que sur la personne, le temple d’une ordonnance LDS — n’est pas affiché, mais reste conservé avec la fiche à laquelle il appartient et réécrit à chaque export : un fichier importé puis exporté de nouveau ne perd rien d’autre que son en-tête.

Un fichier GeneWeb `.gw` est converti de la même façon. Un bloc abîmé est ignoré et signalé par un avertissement, et le reste du fichier est tout de même importé.

### Photos et documents

- Depuis un **GEDZIP**, les photos et documents arrivent avec la généalogie, prêts à être affichés, recadrés et reliés, comme si vous les aviez ajoutés vous-même.
- Depuis un fichier **GEDCOM** ou **GeneWeb**, chaque média devient une fiche qui retient le nom de son fichier, mais sans l’image. Vous pourrez y joindre le fichier plus tard (voir [Médias](@/guide/media.fr.md)).

La première image reliée à une personne dans le fichier devient son portrait, et les documents de plusieurs pages exportés par OxidGene reviennent sous la forme d’un seul document, pages dans l’ordre.

## Importer un arbre Geneanet avec ses photos

Les exports de Geneanet n’indiquent pas quelle photo appartient à quelle personne. L’onglet **Depuis Geneanet** de la même fenêtre reconstitue ces liens en se connectant à Geneanet depuis l’application de bureau, en quelques étapes guidées.

Dans la version web, cet onglet peut importer la généalogie d’un fichier `.gw`, mais pas les photos : les étapes qui lisent vos médias Geneanet nécessitent l’application de bureau.

La démarche complète est décrite pas à pas dans [Récupérer un arbre Geneanet avec ses photos](@/workflows/geneanet-with-photos.fr.md).

## Exporter un arbre

L’export se trouve dans les paramètres de l’arbre.

1. Ouvrez les **Paramètres** de l’arbre, puis **Exporter l’arbre** dans le menu de gauche.
2. Choisissez le format : **GEDCOM (.ged)** ou **GEDZIP (.gdz)**.
3. Pour un GEDCOM, choisissez le **Logiciel de destination** (voir plus bas).
4. Réglez les options.
5. Cliquez sur **Télécharger .ged** ou **Télécharger .gdz**, puis choisissez où enregistrer le fichier.

Le message **Export terminé avec succès.** confirme l’export, et chaque export est consigné dans l’historique de l’arbre.

### Choisir le logiciel de destination

Un fichier GEDCOM est écrit pour le logiciel ou le site qui le lira. La liste **Logiciel de destination** propose :

- **GEDCOM 5.5.1 standard**, par défaut : la version que lisent tous les logiciels et sites de généalogie, sans aucun remaniement ;
- **GEDCOM 7.0 standard** : la dernière version, pour les logiciels qui la lisent ;
- un logiciel ou un site désigné par son nom, comme Geneanet, Ancestry, Heredis ou Gramps : le fichier est mis en forme comme ce logiciel sait le lire. Une ligne sous la liste précise ce qui change. Certains logiciels sont accompagnés d’un avertissement : leur profil reste à vérifier sur leur dernière version.

Après un export destiné à un logiciel nommé, OxidGene indique ce qui s’est perdu en route : soit « Écrit pour … sans rien perdre. », soit une ou deux listes à déplier, comme **3 limites de … dépassées** et **12 modifications ont perdu de l’information pour …**, qui citent chaque fiche concernée.

Le choix de la destination ne vaut que pour cet export ; il n’est pas conservé. Un GEDZIP n’a pas de destination : il contient toujours du GEDCOM 7.0 standard.

### Options d’export

- **Inclure les notes et les sources**, activée par défaut. Désactivée, le fichier ne contient ni notes, ni sources avec leurs citations, ni dépôts d’archives.
- **Inclure les médias**, pour le GEDZIP seulement, activée par défaut. Désactivée, l’archive ne contient que la généalogie, sans aucune référence aux photos ou documents.

Ces options ne valent que pour l’export suivant, et reprennent leur valeur par défaut quand vous rechargez la page.

### Étendue de l’export

Le sélecteur **Étendue** limite l’export à une partie de l’arbre :

- **Tout l’arbre**, par défaut.
- **Une branche** : choisissez la personne racine, puis si vous prenez ses descendants, ses ascendants, ou les deux. Indiquez le nombre de générations (vide pour toutes) et si vous incluez la personne racine et, pour les descendants, leurs conjoints. Le fichier garde ces personnes, les familles auxquelles elles appartiennent, et les sources, lieux, notes et médias qu’elles atteignent.
- **Personnes portant une étiquette** : choisissez une étiquette. Le fichier garde les personnes qui la portent, avec leurs familles et les fiches qu’elles atteignent.

Une branche ou une étiquette doit être choisie avant de lancer l’export. L’étendue ne vaut que pour l’export suivant, comme les autres options.

### L’émetteur

La carte **Émetteur** indique de qui proviennent les fichiers exportés : **Nom**, **Courriel** et **Adresse**, enregistrés par **Enregistrer**. Sans nom, le fichier désigne la personne choisie comme **Qui suis-je ?** (voir [Les arbres](@/guide/trees.fr.md)), et à défaut « Not Provided ». Lors d’un import, les champs encore vides sont remplis à partir de l’émetteur du fichier.

### Télécharger à nouveau un GEDZIP

Une archive GEDZIP est conservée une heure après sa création. Pendant cette heure, la section d’export indique sa taille et jusqu’à quand elle reste disponible, avec un bouton **Télécharger à nouveau** : servez-vous-en si l’enregistrement a échoué ou s’il vous faut une seconde copie, sans réemballer l’arbre. Passé ce délai, exportez de nouveau l’arbre.

### Ce que voient les autres logiciels

- Un fichier GEDCOM ne contient pas les photos elles-mêmes, seulement leur chemin. Utilisez un GEDZIP pour déplacer ou sauvegarder un arbre avec ses médias.
- Un document de plusieurs pages est écrit sous forme d’images séparées, chacune reliée aux mêmes personnes. OxidGene les rassemble de nouveau quand il réimporte le fichier ; les autres logiciels les gardent séparées.
- Un portrait, ou une identification, découpé dans une photographie voyage sous la forme d’un découpage standard (`CROP`, GEDCOM 7). Les logiciels qui lisent les découpages les affichent comme une partie de la photographie, et les découpages écrits par d’autres logiciels sont importés comme des identifications.

En savoir plus : [spécification de l’import](@/tech/ui-import.md) (en anglais).
