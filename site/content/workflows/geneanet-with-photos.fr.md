+++
title = "Récupérer un arbre Geneanet avec ses photos"
description = "Importer un arbre Geneanet dans l’application de bureau avec ses photos et ses documents, chacun rattaché aux bonnes personnes."
weight = 20
+++

Ce parcours importe un arbre que vous tenez sur Geneanet dans l’application de bureau OxidGene, avec ses photos et ses documents numérisés rattachés aux bonnes personnes. Il détaille chaque étape, le rôle de la fenêtre de connexion Geneanet, ce que vous obtenez à la fin, et comment enregistrer votre progression pour terminer plus tard ou sur un autre ordinateur.

## Pourquoi un import particulier

L’export d’arbre de Geneanet contient au plus un portrait par personne, souvent sous la forme d’un lien qui ne fonctionne que si vous êtes connecté. L’export de vos données contient vos fichiers d’origine, mais sans dire qui y figure. Le lien entre une photo et une personne n’existe que dans votre compte Geneanet.

L’import **Depuis Geneanet** d’OxidGene réunit les deux : il lit votre arbre à partir d’un export GeneWeb, puis se connecte à Geneanet par une fenêtre de connexion pour lire la liste de vos médias et savoir à qui ils appartiennent.

Il vous faut :

- l’application **de bureau** OxidGene. La version web peut importer la généalogie d’un fichier `.gw`, mais pas les photos ;
- votre compte Geneanet et son mot de passe ;
- un export GeneWeb (`.gw`) de votre arbre ;
- éventuellement, vos archives de données Geneanet (voir l’étape 2).

## Avant de commencer

Sur la page d’accueil, créez un nouvel arbre, vide, pour accueillir l’import (voir [Les arbres](@/guide/trees.fr.md)). L’import ajoute un arbre complet ; il ne fusionne pas avec les personnes déjà présentes, et le relancer dans le même arbre ne met pas à jour ce que le premier passage a apporté.

Ouvrez ensuite le menu de la carte de cet arbre, choisissez **Importer**, puis l’onglet **Depuis Geneanet**. Les étapes s’affichent les unes sous les autres, une seule étant ouverte à la fois. Une étape terminée se replie en un résumé d’une ligne, avec un bouton **Modifier** pour y revenir au besoin.

## 1. Le fichier de votre arbre

1. Sur Geneanet, exportez votre arbre comme l’indique l’étape :
   - Allez sur Geneanet → Mon arbre → Opérations → Exporter.
   - Choisissez le format GeneWeb (.gw).
   - Cochez « Liens web vers les photos principales des individus » et « Images de la chronique familiale ».
   - Téléchargez le fichier.
2. Dans OxidGene, cliquez sur **Choisir le fichier .gw** et sélectionnez-le. OxidGene le lit aussitôt et affiche le nombre de personnes qu’il contient.

Utilisez l’export `.gw`, pas le GEDCOM : seul le `.gw` distingue deux personnes de même nom à la manière de Geneanet, et c’est ce qui permet de placer chaque photo sur la bonne personne. Si vous choisissez un fichier GEDCOM, l’étape vous le signale.

### Quels médias voulez-vous ?

La même étape vous demande quelle copie garder pour chaque photo :

- **Les copies de Geneanet** (par défaut) : chaque photo est importée dans la plus grande copie que Geneanet publie. Geneanet les redimensionne et les recompresse, et un PDF que vous aviez déposé arrive en image JPEG. Rien d’autre n’est nécessaire : c’est la voie la plus rapide, et la seule possible si vous n’avez jamais téléchargé vos données Geneanet.
- **Les fichiers que vous avez déposés** : vos fichiers d’origine, en pleine qualité, pris dans vos archives de données Geneanet partout où ils peuvent y être identifiés, téléchargés sinon. Cela ajoute une étape pour les archives.

Les deux choix importent la même généalogie, les mêmes liens entre personnes et médias, et les mêmes zones d’identification sur les photos de groupe. Seuls les fichiers d’image diffèrent.

## 2. Vos archives de médias (fichiers d’origine seulement)

Cette étape n’apparaît que si vous avez choisi **Les fichiers que vous avez déposés**.

1. Sur Geneanet, demandez vos données comme l’indique l’étape : allez sur Geneanet → Mes données → Tableau de bord, demandez le téléchargement de vos données, attendez le courriel, puis téléchargez les fichiers ZIP.
2. Cliquez sur **Ajouter des archives…** et sélectionnez tous les fichiers ZIP. Ne les décompressez pas : OxidGene les lit là où ils sont, et Geneanet découpe un gros export en plusieurs fichiers.

Vous pouvez retirer une archive de la liste, et une archive ajoutée deux fois n’est prise qu’une fois. Si vous n’avez pas encore les archives, cliquez sur **Passer cette étape** : les médias seront téléchargés à la place. C’est plus lent, mais rien n’est perdu.

## 3. Connexion à Geneanet

1. Cliquez sur **Ouvrir la fenêtre de connexion Geneanet**.
2. Une vraie fenêtre de navigateur s’ouvre sur Geneanet. Connectez-vous comme d’habitude, y compris si Geneanet demande un captcha ou une vérification. OxidGene ne voit jamais votre mot de passe.
3. Une fois connecté, la fenêtre devient un panneau d’état, **OxidGene lit vos médias Geneanet**, tandis que la fenêtre d’import affiche la progression.

Ne fermez pas vous-même la fenêtre de connexion : la fermer annule la collecte et réinitialise l’étape. OxidGene la masque, puis la ferme, de lui-même quand il n’en a plus besoin. Si votre session Geneanet expire en cours de route, vous pouvez vous reconnecter sans perdre ce qui a déjà été collecté.

La fenêtre de connexion est privée : elle ne garde ni cookies ni connexion après sa fermeture, et le prochain import vous demandera de vous reconnecter.

## 4. Ce qui sera importé

Cette étape n’écrit encore rien. Elle montre ce que l’import s’apprête à faire : les personnes du fichier, les médias trouvés, les personnes avec média, et des remarques comme :

- les photos montrant plusieurs personnes, qui seront rattachées à chacune ;
- les documents de plusieurs pages, qui seront importés en entier, avec toutes leurs pages ;
- les médias identifiant des personnes que Geneanet marque « hors de l’arbre » : nommées sur une photo mais volontairement rattachées à personne dans votre arbre. Une personne est créée pour chacune, avec ses photos ;
- les médias rattachés à personne, qui sont ignorés, et ceux qui nomment des personnes absentes de votre fichier `.gw`.

Si presque aucun média ne correspond aux personnes de votre fichier, l’export et le compte Geneanet ne sont sans doute pas le même arbre. OxidGene s’arrête et vous demande : **Revenir à l’étape 1** pour choisir un autre fichier, ou **Importer quand même** si vous êtes sûr de vous.

OxidGene récupère ensuite les médias par la fenêtre de connexion : tous avec les copies de Geneanet, seulement ceux que vos archives ne contiennent pas avec vos fichiers d’origine.

## 5. Import

À ce stade, plus rien n’est demandé à Geneanet : vos médias sont déjà sur votre ordinateur. Cliquez sur **Lancer l’import** et laissez la fenêtre ouverte jusqu’à la fin. Enregistrer beaucoup de médias prend quelques minutes.

Quand **Import terminé** s’affiche, le bilan compte les personnes, familles, événements, sources, lieux, images et documents importés, ainsi que les rattachements créés entre les médias et les personnes, les couples et les événements. Il indique aussi combien de personnes ont été créées pour des gens identifiés sur des photos mais absents de votre arbre, et combien de zones d’identification ont été conservées pour voir qui est qui sur une photo. Les médias qui n’ont pas pu être importés sont listés, et les éventuels avertissements se trouvent derrière une ligne à déplier.

### Homonymes dans votre arbre

Si une personne créée à partir d’une photo porte le même nom qu’une personne déjà présente dans votre arbre, le bilan la liste sous **Homonymes dans votre arbre**. Selon Geneanet, ce sont deux personnes différentes : c’est pourquoi OxidGene ne les fusionne jamais de lui-même. Pour chacune :

- laissez **Une nouvelle personne** sélectionnée et cliquez sur **Conserver comme nouvelle personne**,
- ou choisissez la personne existante dans la liste et cliquez sur **Fusionner**.

## Terminer plus tard, ou sur un autre ordinateur

Lire votre liste de médias sur Geneanet prend un moment et, avec vos fichiers d’origine, rapprocher vos archives coûte en plus une requête par média. Vous pouvez enregistrer ce qui a été collecté et éviter de le redemander à Geneanet. À côté de **Réutiliser une connexion enregistrée**, dans l’étape de connexion et dans celle du récapitulatif, vous trouverez :

- **Enregistrer ce qui a été collecté…**, qui enregistre la liste des médias une fois connecté. Un import ultérieur qui charge ce fichier ouvre encore la fenêtre de connexion pour récupérer les médias.
- **Tout enregistrer, prêt à importer hors ligne…**, qui apparaît une fois les médias récupérés. Un import ultérieur qui charge ce fichier n’a plus besoin d’aucune connexion à Geneanet.
- **Charger un fichier enregistré…**, qui reprend un fichier enregistré. OxidGene regarde ce qu’il contient et ne demande que ce qui manque.

Vous pouvez ainsi reprendre après une interruption, ou collecter sur un ordinateur et importer sur un autre. Le fichier enregistré contient votre liste de médias, et parfois vos photos : gardez-le aussi confidentiel que votre arbre.

## Bon à savoir

- Une fois lancé, l’import ne peut pas être annulé. S’il est interrompu pendant l’enregistrement des médias, la généalogie est complète mais certains médias peuvent manquer ; supprimez l’arbre et relancez l’import, en chargeant un fichier enregistré pour ne pas tout redemander à Geneanet.
- OxidGene n’écrit jamais rien sur Geneanet.
- Une seule fenêtre de connexion Geneanet peut être ouverte à la fois.
- Un événement ne reçoit sa photo que s’il peut être identifié avec certitude ; sinon, la photo est rattachée à la personne.

Pour conserver une copie du résultat avec tous ses médias, exportez l’arbre en GEDZIP (voir [Import et export](@/guide/import-export.fr.md)).
