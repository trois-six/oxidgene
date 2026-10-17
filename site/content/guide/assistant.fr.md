+++
title = "Assistant IA"
description = "Permettre à un assistant IA comme Claude de lire vos arbres depuis l’application de bureau, en lecture seule et avec votre accord explicite."
weight = 160
+++

L’application de bureau OxidGene peut laisser un assistant IA lire vos arbres généalogiques : vous pouvez alors l’interroger sur un arbre, lui faire rédiger une biographie à partir de la fiche d’une personne, ou lui demander de repérer des incohérences. Cette page explique ce que l’assistant peut voir, comment le connecter et comment le déconnecter.

La connexion passe par le Model Context Protocol (MCP), une norme qui permet aux assistants d’utiliser des outils extérieurs. Tout logiciel d’assistant compatible MCP peut s’en servir, par exemple Claude Desktop ou Claude Code.

## Ce que l’assistant peut faire, et ce qu’il ne peut pas faire

- **Il ne fait que lire.** L’assistant peut rechercher des personnes, ouvrir des fiches et des pedigrees, lister des événements, des citations et des notes, consulter des lieux et des sources, et parcourir le dictionnaire d’un arbre. Il ne peut rien ajouter, modifier ni supprimer.
- **Il voit tous les arbres** de l’application. Chacune de ses demandes à OxidGene désigne l’arbre concerné, si bien que ses réponses ne mélangent jamais deux arbres.
- **Il voit tout leur contenu**, y compris les personnes vivantes, les notes et les sources. Les réglages de confidentialité de vos personnes et de vos arbres ne s’appliquent pas à lui.
- **Ce qu’il lit quitte votre ordinateur.** L’assistant envoie ce qu’il lit à l’entreprise qui exploite son modèle de langage. OxidGene ne maîtrise pas ce que cette entreprise conserve.
- Il ne reçoit ni vos photos ni vos documents, et ne peut ni importer, ni exporter, ni accéder à Geneanet.

Cette fonction n’existe que dans l’application de bureau. La version web affiche à la place une note qui le signale.

## Avant de commencer

- Installez l’application de bureau OxidGene et ouvrez-la au moins une fois, pour que votre base de données existe (voir [Installer OxidGene](@/guide/install.fr.md)).
- Installez le logiciel d’assistant de votre choix et vérifiez qu’il accepte les serveurs MCP.

## Connecter un assistant

1. Cliquez sur l’icône en forme d’engrenage en haut à droite de la page d’accueil pour ouvrir les paramètres de l’application.
2. Ouvrez la section **API** et repérez la carte **Assistant IA (MCP)**.
3. Lisez l’avertissement en haut de la carte : c’est l’accord que vous vous apprêtez à donner.
4. Copiez ce que demande votre logiciel d’assistant :
   - **Commande** : la commande complète, `…/oxidgene-desktop mcp`, avec l’emplacement du programme sur votre ordinateur déjà renseigné ;
   - **Configuration client (JSON)** : la même commande, sous la forme qu’attendent de nombreux logiciels d’assistant dans leur fichier de configuration.
5. Ajoutez-la aux serveurs MCP de votre logiciel d’assistant en suivant sa documentation, puis redémarrez-le s’il vous le demande.

Avec Claude Code, par exemple, la commande ressemble à :

```bash
claude mcp add oxidgene -- /chemin/vers/oxidgene-desktop mcp
```

et un logiciel qui utilise un fichier de configuration reçoit :

```json
{
  "mcpServers": {
    "oxidgene": {
      "command": "/chemin/vers/oxidgene-desktop",
      "args": ["mcp"]
    }
  }
}
```

Copiez toujours la version affichée dans vos propres paramètres : elle contient l’emplacement réel du programme.

Copier la commande ne change rien dans OxidGene. L’assistant n’obtient l’accès qu’une fois la commande ajoutée chez lui.

## S’en servir

Posez vos questions à l’assistant en langage courant, par exemple :

- « Quels arbres ai-je dans OxidGene ? »
- « Dans l’arbre Landrevel, résume la vie d’Étienne Marie Landrevel. »
- « Liste les ancêtres du SOSA 1 de l’arbre Landrevel dont le décès n’est pas renseigné. »

L’assistant commence par lister vos arbres, puis demande à OxidGene ce dont il a besoin dans l’arbre que vous avez nommé. Les dates lui parviennent avec leur précision (« vers », « avant »…), et quand une naissance ou un décès n’a pas de date, le baptême ou l’inhumation peut en tenir lieu, comme partout dans OxidGene. Les numéros SOSA partent de la personne racine de l’arbre.

La fenêtre d’OxidGene peut rester ouverte pendant que l’assistant travaille : ils partagent la même base de données.

Les noms, notes et sources proviennent souvent de fichiers importés, écrits par d’autres, et peuvent contenir du texte qui tente de donner des instructions à l’assistant. L’accès étant en lecture seule, rien de ce qu’ils contiennent ne peut l’amener à modifier vos arbres par l’intermédiaire d’OxidGene.

## Le déconnecter

Retirez l’entrée OxidGene des serveurs MCP de votre logiciel d’assistant. Il ne peut alors plus lire vos arbres. Il n’y a rien à annuler du côté d’OxidGene.

En savoir plus : [spécification de l’accès des assistants](@/tech/mcp.md) (en anglais).
