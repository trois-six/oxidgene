+++
title = "Partager un arbre"
description = "Partager un arbre dans la version web d’OxidGene : avec des membres qui le construisent ou le lisent, par invitation, avec des liens de partage, ou en le rendant visible du site ou du web, et limiter un partage à une branche."
weight = 157
+++

Dans la version web d’OxidGene, le propriétaire d’un arbre décide qui le voit et ce que chacun peut y faire. Cette page explique les rôles, comment partager un arbre avec des personnes qui ont un compte ou pas encore, comment les liens de partage et la visibilité de l’arbre laissent entrer des visiteurs, comment limiter un partage à une branche, et ce que voient les personnes avec qui vous partagez.

Le partage n’existe que dans la version web, reliée à un fournisseur d’identité (voir [Comptes et profil](@/guide/accounts.fr.md)). L’application de bureau, et une version web exploitée sans fournisseur d’identité, n’ont qu’un utilisateur qui voit tout : elles ne montrent rien de cette page.

## Les rôles

| Rôle | Qui | Peut |
|---|---|---|
| **Propriétaire** | Le compte à qui l’arbre appartient | Tout : le partage, les réglages, les exclusions, la suppression de l’arbre |
| **Éditeur** | Une personne qui construit l’arbre avec vous | Ajouter, modifier et supprimer des fiches, exporter, voir les personnes vivantes |
| **Invité** | Un proche à qui vous confiez l’arbre | Lire tout l’arbre, personnes vivantes comprises, sans rien modifier |
| **Visiteur** | Quiconque la visibilité de l’arbre ou un lien de partage laisse entrer | Lire l’arbre, personnes vivantes et fiches privées masquées |

Les personnes que vous [excluez du partage](@/guide/settings.fr.md#exclusions-du-partage) restent à vous seul, quels que soient le rôle et le partage.

## Partager avec quelqu’un

Ouvrez les réglages de l’arbre, puis **Membres** — une section que seul le propriétaire voit.

1. Dans **Partager l’arbre**, saisissez l’**Adresse électronique ou nom d’utilisateur** exacts de la personne, ou choisissez-la dans **Ou une personne avec qui vous partagez déjà**, qui liste les membres de vos autres arbres.
2. Choisissez le **Rôle** : **Invité** ou **Éditeur**.
3. Sous **Ce qui est partagé**, gardez **Tout l’arbre** ou choisissez **Une branche** (voir [Partager une branche](#partager-une-branche)).
4. Si vous le souhaitez, fixez le jour où le partage **Prend fin**.
5. Cliquez sur **Partager**.

Une personne qui a un compte devient membre aussitôt et trouve l’arbre sur sa page d’accueil. Une adresse qui n’a pas encore de compte reçoit à la place une **invitation**. Quand l’instance n’envoie pas de courriel, la section le dit et affiche le **Lien d’invitation** à envoyer vous-même : il ne contient aucun secret, puisque seule l’adresse invitée peut l’accepter.

La recherche d’une personne par adresse ou nom d’utilisateur est limitée à trente fois par heure.

**Membres de l’arbre** liste chacun avec son rôle, ce qu’il voit et la fin de son partage. Pour chaque membre, vous pouvez changer le rôle, **Rendre propriétaire** pour céder l’arbre — vous devenez alors éditeur — ou le **Retirer**.

**Invitations en attente** liste les invitations auxquelles personne n’a encore répondu, chacune avec son lien, **Renvoyer** et **Révoquer**. Une invitation attend quatorze jours.

## Accepter une invitation

Connectez-vous avec l’adresse invitée — l’instance vous laisse créer un compte pour elle — et ouvrez la page d’accueil. **Invitations** liste les arbres auxquels vous êtes invité : **Accepter** ajoute l’arbre à votre liste, **Refuser** oublie l’invitation.

## Les liens de partage

Dans les réglages de l’arbre, **Partage** liste les **Liens de partage**. Un lien permet à qui l’ouvre de visiter l’arbre sans compte, personnes vivantes masquées.

1. Sous **Nouveau lien**, donnez-lui un **Libellé** qui vous rappelle à qui il est destiné, choisissez **Ce qui est partagé** et, si vous le souhaitez, quand il **Prend fin**.
2. Cliquez sur **Créer le lien** et copiez-le aussitôt : il n’est affiché qu’une fois.

Chaque lien indique combien de fois il a été ouvert. **Révoquer** y met fin immédiatement pour tous ceux qui l’ont ouvert.

## Rendre l’arbre visible

**Visibilité**, dans la section **Partage**, décide qui visite l’arbre sans en être membre :

- **Privé** : personne d’autre que les membres et les détenteurs d’un lien de partage ;
- **Visible des utilisateurs du site** : tout utilisateur connecté de l’instance ;
- **Public sur le web** : n’importe qui, sans se connecter.

Un arbre visible s’atteint par son adresse : il n’est répertorié nulle part. **Ce que voient les visiteurs** peut limiter les visiteurs à une branche, et la section compte les personnes qu’ils verront avant que vous cliquiez sur **Enregistrer**.

## Partager une branche

Au lieu de **Tout l’arbre**, choisissez **Une branche**, puis sa personne racine, son côté — **Descendants**, **Ascendants** ou **Ascendants et descendants** — et le nombre de **Générations** qu’elle couvre. Vous pouvez ajouter les conjoints des personnes atteintes, et leurs frères et sœurs. La section compte les personnes de la branche et combien d’entre elles les visiteurs voient.

Une branche suit l’arbre : un enfant que vous ajoutez sous un ancêtre partagé est partagé aussitôt. Si la racine est fusionnée avec une autre personne, le partage suit la personne conservée ; si la racine est supprimée, le partage est vide jusqu’à sa restauration.

Un éditeur limité à une branche ne modifie que les personnes qu’elle contient, et celles qu’il y ajoute. Les lieux, sources, dépôts et étiquettes sont communs à tout l’arbre : il les utilise sans les modifier.

## Ce que voient les visiteurs

Quand vous visitez un arbre en tant que visiteur, un bandeau indique **Vous consultez cet arbre en tant que visiteur**, avec **Se connecter** si vous n’êtes pas connecté. Sur la page d’accueil, chaque carte indique votre rôle sur l’arbre, et **Mes arbres** et **Partagés avec moi** gardent les arbres qui vous appartiennent ou ceux que d’autres partagent avec vous.

Pour ne plus voir un arbre qu’on a partagé avec vous, ouvrez ses réglages et cliquez sur **Quitter** sous **Quitter cet arbre**.
