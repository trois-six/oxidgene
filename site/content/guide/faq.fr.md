+++
title = "Questions fréquentes"
description = "Des réponses courtes aux questions que l’on se pose avant et après avoir installé OxidGene."
weight = 170
+++

## OxidGene est-il gratuit ?

Oui. OxidGene est un logiciel libre sous licence GNU Affero General Public
License v3.0 : pas de licence à acheter, pas d’abonnement, et son code source
est public sur GitHub.

## Fonctionne-t-il sans connexion internet ?

L’application de bureau fonctionne hors ligne : vos arbres, leurs médias et le
dictionnaire des lieux sont tous sur votre ordinateur. Elle ne se connecte que
lorsque vous le lui demandez, pour ouvrir un registre cité sur le site d’un
service d’archives ou pour importer un arbre depuis Geneanet. Voir
[Installer OxidGene](@/guide/install.fr.md).

## Où sont gardés mes arbres, et comment les sauvegarder ?

Sur votre ordinateur, dans le dossier de données de l’application : la
[page d’installation](@/guide/install.fr.md#ou-sont-rangees-vos-donnees)
indique son emplacement sur chaque système. Pour garder une copie lisible
partout, exportez l’arbre en archive GEDZIP, qui emporte ses photos et ses
documents ([Exporter un arbre](@/guide/import-export.fr.md#exporter-un-arbre)).
Vous pouvez aussi copier tout le dossier de données pendant qu’OxidGene est
fermé.

## J’utilise un autre logiciel de généalogie. Puis-je y récupérer mon arbre ?

Oui. Exportez votre arbre depuis ce logiciel en GEDCOM (ou en GEDZIP s’il le
propose), ou en fichier GeneWeb `.gw`, puis importez-le dans un nouvel arbre.
Le parcours [Quitter un autre logiciel](@/workflows/first-tree-from-gedcom.fr.md)
vous guide pas à pas. Les arbres Geneanet s’importent avec leurs photos dans
l’application de bureau
([Récupérer un arbre Geneanet avec ses photos](@/workflows/geneanet-with-photos.fr.md)).

## Puis-je renvoyer mon arbre vers un autre logiciel ?

Oui. L’export écrit du GEDCOM 5.5.1 ou 7.0, ou un GEDCOM adapté au logiciel
qui le lira — Geneanet, Ancestry, Heredis, Gramps et d’autres — et signale ce
que ce logiciel ne pourra pas reprendre
([Choisir le logiciel de destination](@/guide/import-export.fr.md#choisir-le-logiciel-de-destination)).

## Je me suis trompé. Puis-je revenir en arrière ?

Chaque modification est conservée. Ouvrez l’historique de la personne pour
comparer ses versions et restaurer une version antérieure, même après une
suppression ([Historique](@/guide/history.fr.md)). Une fusion, en revanche, ne
peut pas être annulée depuis l’assistant de fusion.

## OxidGene masque-t-il les personnes vivantes ?

Pas encore. Vous pouvez déjà marquer un arbre, une personne, un couple ou un
média comme privé, et ce réglage est conservé, mais rien n’est masqué pour
l’instant : le masquage des personnes privées et contemporaines viendra avec
les comptes utilisateurs ([Paramètres](@/guide/settings.fr.md)).

## Ma famille peut-elle travailler sur le même arbre ?

La version web tourne sur un serveur et s’utilise depuis un navigateur :
plusieurs personnes peuvent donc ouvrir les mêmes arbres. Elle n’a pas encore
de comptes utilisateurs : toute personne qui peut l’atteindre peut modifier
tous les arbres. Gardez-la sur un réseau privé
([Installer la version web](@/guide/install.fr.md#installer-la-version-web-pour-votre-famille)).

## Existe-t-il une application mobile ?

Non. La version web adapte son affichage aux téléphones et aux tablettes.

## Dans quelles langues OxidGene est-il disponible ?

Anglais, français, allemand, espagnol, italien, néerlandais, polonais et
portugais. Vous changez de langue à tout moment dans les paramètres de
l’application, sans redémarrer ([Paramètres](@/guide/settings.fr.md)).

## macOS indique que l’application ne peut pas être ouverte.

L’application macOS n’est pas encore signée par Apple. Ouvrez-la une fois,
fermez l’avertissement, puis choisissez **Ouvrir quand même** dans **Réglages
Système → Confidentialité et sécurité**
([Le lancer sous macOS](@/guide/install.fr.md#le-lancer-sous-macos)).

## Où signaler un problème ou proposer une idée ?

Ouvrez un ticket sur le [dépôt GitHub](https://github.com/trois-six/oxidgene/issues).
Décrivez ce que vous avez fait et ce que vous avez vu, mais ne joignez jamais
votre véritable arbre : utilisez des noms inventés si vous avez besoin d’un
exemple.
