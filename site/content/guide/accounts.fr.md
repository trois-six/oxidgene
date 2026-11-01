+++
title = "Comptes et profil"
description = "Se connecter à la version web d’OxidGene, suivre ses notifications, gérer son profil, son avatar, sa langue, ses courriels de notification et ses sessions, télécharger ou supprimer son compte et, pour un administrateur, gérer les utilisateurs de l’instance."
weight = 155
+++

Quand OxidGene fonctionne comme application web pour une famille ou une association, chacun se connecte avec son propre compte. Cette page explique comment se connecter et se déconnecter, ce que contient votre profil, comment voir et terminer vos sessions, comment télécharger ou supprimer votre compte, et comment un administrateur gère les utilisateurs de l’instance.

Les comptes n’existent que dans la version web, et seulement si la personne qui l’exploite l’a reliée à un fournisseur d’identité. L’application de bureau n’a pas de compte : elle a un seul utilisateur, vous, qui a accès à tout. Une version web exploitée sans fournisseur d’identité se comporte de la même façon, et ne montre rien de ce que décrit cette page.

## Se connecter

1. Cliquez sur **Se connecter** en haut à droite de la page d’accueil.
2. Votre navigateur se rend sur la page du fournisseur d’identité — le service qu’utilise votre instance pour vérifier qui vous êtes. Connectez-vous-y comme il le demande : adresse e-mail et mot de passe, second facteur, ou compte extérieur qu’il propose.
3. Vous revenez, connecté, sur la page d’où vous êtes parti.

OxidGene ne voit jamais votre mot de passe : le fournisseur d’identité le conserve, comme votre second facteur et la récupération d’un mot de passe oublié. Sa page s’ouvre dans la langue de l’interface quand il la propose.

Tant que vous n’êtes pas connecté, les pages qui montrent les arbres et votre profil proposent **Se connecter** à la place de leur contenu. Les paramètres de l’application restent accessibles : ils appartiennent à votre appareil.

Si la connexion échoue, la page où vous revenez en donne la raison : vous ou le fournisseur l’avez refusée, l’instance n’admet que les personnes qu’elle a invitées, votre compte a été désactivé, ou le fournisseur était injoignable. **Réessayer** recommence.

## Le menu du compte

Une fois connecté, votre avatar remplace **Se connecter** en haut à droite de la page d’accueil et des paramètres de l’application. Cliquez dessus pour ouvrir le menu du compte : votre nom et votre adresse e-mail, **Profil** et **Se déconnecter**.

**Se déconnecter** termine votre session dans OxidGene, puis chez le fournisseur d’identité : la connexion suivante vous redemande qui vous êtes.

## Quand votre session se termine

Une session se termine après un temps d’inactivité, au bout d’une durée maximale, quand vous utilisez **Se déconnecter partout** sur un autre appareil, ou quand un administrateur désactive votre compte. Si elle se termine alors qu’une page est ouverte, la fenêtre **Votre session est terminée** apparaît à votre prochaine action. **Se reconnecter** vous ramène sur la même page ; **Fermer** laisse la page en l’état, pour que vous puissiez copier ce que vous étiez en train de saisir avant de vous reconnecter.

## Notifications

OxidGene vous dit ce qui vous arrive, à vous et à vos arbres : un arbre partagé avec vous, votre accès à un arbre modifié ou terminé, quelqu’un qui accepte votre invitation, un membre qui quitte votre arbre, un arbre qui change de propriétaire, un import ou un export que vous avez lancé qui se termine.

- **La cloche**, à gauche de votre avatar, indique combien de notifications vous n’avez pas lues. Cliquez dessus pour voir les dernières, de la plus récente à la plus ancienne, les non lues en gras. Cliquez sur l’une d’elles pour ouvrir ce dont elle parle — l’arbre, ou ses paramètres pour une invitation acceptée et un membre parti — et elle compte comme lue. **Tout marquer comme lu** remet le compteur à zéro.
- **Les nouvelles notifications arrivent d’elles-mêmes** tant que la page est ouverte : rien à recharger. Un onglet en arrière-plan ne demande rien jusqu’à ce que vous y reveniez.
- Une notification sur un arbre que vous ne voyez plus indique *Un arbre n’est plus partagé avec vous*, sans son nom. Les notifications sont conservées 90 jours.

Quand l’instance envoie des courriels, certaines notifications vous parviennent aussi par courriel, comme vous le choisissez dans votre profil (ci-dessous). Chaque courriel se termine par **Ne plus recevoir ces courriels** : le lien ouvre une page qui demande confirmation avant de désactiver ce type de courriel, et le bouton *Se désabonner* de votre messagerie fait de même aussitôt. Les courriels de connexion de l’instance — vérification d’adresse, mot de passe oublié — viennent du fournisseur d’identité, pas d’OxidGene, et ces réglages ne les changent pas.

## Votre profil

Ouvrez **Profil** depuis le menu du compte. Son menu de gauche compte cinq sections.

### Profil

- **Avatar** : cliquez sur **Téléverser**, choisissez une image JPEG, PNG ou WebP de 5 Mo au plus, puis choisissez le carré qui deviendra votre avatar : il part du milieu de l’image ; faites-le glisser, ou déplacez-le avec les flèches du clavier, et utilisez **Zoom** pour cadrer une plus petite partie. Cliquez sur **Utiliser comme avatar**. OxidGene ne garde que ce carré, redimensionné, et rien d’autre du fichier : ni lieu, ni date, ni appareil. **Retirer** rétablit l’avatar généré.
- Sans image, votre avatar montre vos initiales — la première lettre des deux premiers mots de votre nom — sur une couleur choisie d’après votre compte, toujours la même sur chaque appareil.
- **Nom affiché** : le nom que voient les personnes avec qui vous partagez des arbres. Saisissez-le et cliquez sur **Enregistrer**. **Utiliser le nom du fournisseur** revient au nom que donne votre fournisseur d’identité.
- **E-mail** : telle que la donne votre fournisseur d’identité, avec la mention vérifiée ou non. Modifiez-la chez le fournisseur.

### Langue

Choisissez la langue de l’interface pour votre compte. Elle s’applique aussitôt sur cet appareil, et sur tout appareil où aucune langue n’a été choisie dans les [paramètres de l’application](@/guide/settings.fr.md). Les courriels qu’OxidGene vous envoie sont écrits dans cette langue aussi.

### Notifications

Les notifications apparaissent toujours sous la cloche. Cette section choisit celles qui vous parviennent aussi par courriel :

- **M’envoyer des courriels** : désactivé, aucune notification ne vous parvient par courriel, quels que soient les choix ci-dessous.
- Pour chaque type de notification : **Pas par courriel**, **Par courriel, aussitôt**, ou **Dans un résumé quotidien par courriel** — un courriel par jour, tôt le matin (6 h UTC), qui liste ce que vous n’avez pas encore lu. Par défaut, un arbre partagé avec vous et une invitation acceptée sont envoyés aussitôt, le reste pas du tout.

OxidGene n’écrit qu’à l’adresse que votre fournisseur d’identité a vérifiée. Quand l’instance n’envoie pas de courriel, la section le dit, et ne propose aucun choix.

### Sessions

La liste des navigateurs connectés à votre compte : quel navigateur, quand il s’est connecté et quand il a été actif pour la dernière fois. **Ce navigateur** désigne celui que vous utilisez.

- **Terminer** déconnecte ce navigateur. Sur le navigateur actuel, le bouton s’appelle **Se déconnecter**.
- **Se déconnecter partout** termine toutes les sessions de votre compte, celle-ci comprise.

Terminez toute session que vous ne reconnaissez pas, puis changez votre mot de passe chez votre fournisseur d’identité.

### Compte

- **Mot de passe et validation en deux étapes** : **Ouvrir** mène à la page de compte de votre fournisseur d’identité, où vous les modifiez.
- **Télécharger les données de mon compte** : un fichier JSON avec tout ce qu’OxidGene conserve sur votre compte, vos réglages de notifications compris.
- **Supprimer mon compte** : après confirmation, votre compte, vos sessions, votre avatar, vos notifications et les courriels en attente sont effacés définitivement, et vous êtes déconnecté. La généalogie que vous avez saisie reste dans ses arbres. L’historique présente vos modifications comme celles d’un ancien membre.

## Gérer les utilisateurs de l’instance

Un administrateur de l’instance trouve un groupe **Instance** dans les [paramètres de l’application](@/guide/settings.fr.md), avec une section **Utilisateurs** : chaque compte, avec son avatar, son nom, son adresse e-mail, son rôle, s’il peut créer des arbres, s’il est désactivé, et sa dernière connexion. **Afficher plus** affiche les suivants.

Le menu **⋮** d’un utilisateur propose :

- **Nommer administrateur** ou **Retirer le rôle d’administrateur** ;
- **Autoriser la création d’arbres** ou **Ne plus autoriser la création d’arbres** ;
- **Désactiver**, qui déconnecte l’utilisateur partout et l’empêche de se connecter, ou **Réactiver** ;
- **Changer d’identité…**, qui relie le compte à une autre identité chez le fournisseur — son émetteur et son sujet — après le passage de l’instance à un autre fournisseur d’identité ;
- **Effacer**, qui supprime définitivement le compte après confirmation ; l’historique présente alors l’utilisateur comme un ancien membre.

Votre propre compte n’a pas de menu ici : modifiez-le depuis votre profil. Quand le fournisseur d’identité décide des rôles — les administrateurs de l’instance viennent alors d’un groupe chez le fournisseur —, changer un rôle ici est refusé, et le changement se fait chez le fournisseur.
