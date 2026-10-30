+++
title = "Installer OxidGene"
description = "Télécharger et lancer l’application de bureau sous Linux, macOS ou Windows, savoir où sont rangées vos données, ou héberger vous-même la version web."
weight = 10
+++

Cette page explique comment installer l’application de bureau OxidGene, où elle range votre généalogie sur votre ordinateur, et où regarder si vous préférez la compiler vous-même ou installer la version web pour votre famille.

L’application de bureau est un programme unique qui contient tout ce dont elle a besoin : vos arbres sont enregistrés dans une base de données sur votre propre ordinateur, et elle fonctionne hors connexion. Elle n’a besoin du réseau que pour ouvrir un registre cité sur le site d’un service d’archives et pour importer un arbre depuis Geneanet.

## Vérifier votre ordinateur

- Un système Linux, macOS (11 ou plus récent) ou Windows 64 bits, avec un environnement graphique.
- Le composant d’affichage web du système : WebKitGTK sous Linux, WebView2 sous Windows, WebKit sous macOS.
- Au moins 4 Gio de mémoire et 500 Mio d’espace disque libre, plus la place pour vos arbres et toutes les photos et tous les documents que vous importerez.
- Pour un grand arbre ou une grande médiathèque, un SSD et 8 Gio de mémoire sont recommandés.

Aucun serveur de base de données, aucun système de conteneurs ni aucun autre logiciel n’est nécessaire.

## Télécharger l’application

OxidGene est publié sur sa [page GitHub Releases](https://github.com/trois-six/oxidgene/releases). Ne le téléchargez nulle part ailleurs.

1. Ouvrez la [page des versions](https://github.com/trois-six/oxidgene/releases) et choisissez la plus récente.
2. Téléchargez le fichier correspondant à votre système :

   | Système | Fichier | Contenu |
   |---|---|---|
   | Linux (x86-64) | `oxidgene-desktop-linux-x86_64.tar.gz` | Le programme |
   | macOS 11 ou plus récent (Apple silicon et Intel) | `oxidgene-desktop-macos-universal.dmg` | `OxidGene.app` |
   | Windows (x86-64) | `oxidgene-desktop-windows-x86_64.zip` | Le programme |

3. Téléchargez aussi `SHA256SUMS` et vérifiez que votre fichier lui correspond avant de le lancer. Sous Linux, par exemple, exécutez `sha256sum -c SHA256SUMS --ignore-missing` dans le dossier de téléchargement. Si vous utilisez la ligne de commande GitHub, `gh attestation verify <fichier> --repo trois-six/oxidgene` confirme en plus que le fichier a été produit par le workflow de publication du projet lui-même.

Ce sont des programmes portables, pas des installeurs : rien n’est ajouté à votre système en dehors du programme lui-même et des dossiers décrits plus bas.

Si la page des versions n’en propose encore aucune, compilez l’application à partir des sources (voir plus bas).

## Le lancer sous Linux

1. Décompressez l’archive, par exemple avec `tar -xzf oxidgene-desktop-linux-x86_64.tar.gz`.
2. Placez le programme `oxidgene-desktop` là où vous rangez vos applications.
3. Lancez-le, par exemple avec `./oxidgene-desktop`.

## Le lancer sous macOS

1. Ouvrez le fichier `.dmg` et faites glisser `OxidGene.app` dans votre dossier Applications.
2. Ouvrez OxidGene une première fois. macOS refuse de l’ouvrir, car l’application n’est pas encore signée avec un identifiant de développeur Apple. Fermez l’avertissement.
3. Ouvrez **Réglages Système → Confidentialité et sécurité**, repérez le message concernant OxidGene et choisissez **Ouvrir quand même**.
4. Confirmez. macOS retient votre choix, et OxidGene s’ouvre ensuite normalement.

## Le lancer sous Windows

1. Décompressez le fichier `.zip` et placez `oxidgene-desktop.exe` où vous voulez.
2. Double-cliquez dessus.
3. Le programme n’est pas encore signé (les notes de chaque version indiquent quand il le sera) : Windows SmartScreen peut donc afficher un avertissement. Choisissez **Informations complémentaires**, puis **Exécuter quand même**.

## Où sont rangées vos données

OxidGene répartit les fichiers qu’il écrit dans quatre dossiers. Sous Linux, ce sont :

| Dossier | Contenu | Peut-on le supprimer ? |
|---|---|---|
| `~/.local/share/oxidgene/` | Votre base de données (`oxidgene.db`) et le dossier `media/` avec vos photos et documents | **Non.** C’est votre généalogie, elle ne peut pas être reconstituée |
| `~/.config/oxidgene/` | Vos propres thèmes (`themes/`) et langues (`languages/`) | Non : c’est vous qui les avez écrits |
| `~/.local/state/oxidgene/` | Les préférences de la fenêtre (langue, thème, réglages du pedigree) et les cookies des sites d’archives que vous avez consultés | Seulement quand OxidGene est fermé ; vos préférences sont alors réinitialisées |
| `~/.cache/oxidgene/` | Les fichiers temporaires des imports et des exports | Oui, quand OxidGene est fermé |

Sous macOS, votre base et vos médias se trouvent dans `~/Library/Application Support/oxidgene/` ; sous Windows, dans `%APPDATA%\oxidgene\`. Les sections Apparence et Langue des paramètres indiquent le dossier exact de vos propres thèmes et langues (voir [Paramètres](@/guide/settings.fr.md)).

Incluez le dossier des données dans vos sauvegardes habituelles. Pour emporter un seul arbre avec ses médias, exportez-le en archive GEDZIP (voir [Import et export](@/guide/import-export.fr.md)).

## Le compiler à partir des sources

Si vous préférez compiler l’application vous-même, il vous faut Git, la chaîne d’outils Rust stable et les paquets de développement du composant web de votre système. Ensuite :

```bash
git clone https://github.com/trois-six/oxidgene.git
cd oxidgene
just build-desktop-release
./target/release/oxidgene-desktop
```

Sans `just`, lancez `cargo build --locked --release --package oxidgene-desktop`. Sous Windows, le programme est `target\release\oxidgene-desktop.exe`. La liste complète des paquets nécessaires, système par système, figure dans le guide de démarrage technique.

## Installer la version web pour votre famille

OxidGene peut aussi fonctionner comme une application web, utilisée depuis un navigateur, avec PostgreSQL et un stockage compatible S3. Deux façons de la déployer :

- **Docker Compose**, pour essayer la version web sur une seule machine ;
- **Kubernetes avec Helm**, pour un déploiement sur serveur.

La version web n’a pas encore de comptes utilisateurs : quiconque peut l’atteindre peut lire, modifier et supprimer tous les arbres. Gardez-la sur un réseau privé ou derrière une passerelle d’authentification. Quelques fonctions sont réservées à l’application de bureau, dont l’import des photos depuis Geneanet et la connexion d’un assistant IA.

En savoir plus : [guide de démarrage technique](@/tech/quickstart.md) (en anglais).
