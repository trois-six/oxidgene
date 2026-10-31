+++
title = "Assistant IA"
description = "Permettre à un assistant IA comme Claude de lire vos arbres depuis l’application de bureau avec votre accord explicite, et de les modifier seulement si vous autorisez aussi l’écriture."
weight = 160
+++

L’application de bureau OxidGene peut laisser un assistant IA lire vos arbres généalogiques : vous pouvez alors l’interroger sur un arbre, lui faire rédiger une biographie à partir de la fiche d’une personne, ou lui demander de repérer des incohérences. Si vous l’y autorisez, l’assistant peut aussi modifier vos arbres, par exemple pour saisir les personnes et les événements d’un acte que vous transcrivez ensemble. Cette page explique ce que l’assistant peut voir et faire, comment le connecter, comment l’autoriser à écrire et comment le déconnecter.

La connexion passe par le Model Context Protocol (MCP), une norme qui permet aux assistants d’utiliser des outils extérieurs. Tout logiciel d’assistant compatible MCP peut s’en servir, par exemple Claude Desktop ou Claude Code.

## Ce que l’assistant peut faire, et ce qu’il ne peut pas faire

- **Il lit.** L’assistant peut rechercher des personnes, ouvrir des fiches, des pedigrees et des familles, trouver le lien de parenté entre deux personnes, lister des événements, des témoins, des citations et des notes, consulter des lieux et des sources, et parcourir le dictionnaire d’un arbre.
- **Il n’écrit que si vous l’y autorisez.** Par défaut, il ne peut rien ajouter, modifier ni supprimer. Si vous ajoutez `--allow-writes` à sa commande (voir « L’autoriser à modifier vos arbres » ci-dessous), il peut aussi créer et modifier des personnes, des noms, des événements, des familles, des lieux, des sources, des citations, des notes et des étiquettes, et les supprimer.
- **Il voit tous les arbres** de l’application. Chacune de ses demandes à OxidGene désigne l’arbre concerné, si bien que ses réponses ne mélangent jamais deux arbres.
- **Il voit tout leur contenu**, y compris les personnes vivantes, les notes et les sources. Les réglages de confidentialité de vos personnes et de vos arbres ne s’appliquent pas à lui.
- **Ce qu’il lit quitte votre ordinateur.** L’assistant envoie ce qu’il lit à l’entreprise qui exploite son modèle de langage. OxidGene ne maîtrise pas ce que cette entreprise conserve.
- Il ne reçoit ni vos photos ni vos documents, et ne peut ni importer, ni exporter, ni fusionner, ni restaurer une version, ni accéder à Geneanet, même quand l’écriture est autorisée.

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

Copier la commande ne change rien dans OxidGene. L’assistant n’obtient l’accès qu’une fois la commande ajoutée chez lui. Cette commande est en lecture seule.

## L’autoriser à modifier vos arbres

L’écriture est désactivée tant que vous ne l’activez pas : l’assistant peut mal lire un acte ou une parenté, et le texte de vos arbres, souvent écrit par d’autres, pourrait tenter de lui donner des instructions. Pour autoriser l’écriture :

1. Sous la commande, dans la carte **Assistant IA (MCP)**, lisez l’indication sur l’écriture.
2. Ajoutez `--allow-writes` après `mcp` dans la configuration de votre assistant : `…/oxidgene-desktop mcp --allow-writes`, ou `"args": ["mcp", "--allow-writes"]` dans un fichier de configuration.
3. Redémarrez l’assistant s’il vous le demande.

L’assistant dispose alors d’outils pour créer et modifier des fiches, et d’outils pour supprimer des fiches ou retirer des liens. Ces derniers sont signalés comme destructeurs : un logiciel d’assistant qui demande confirmation avant de telles actions vous la demande d’abord ; vérifiez ses réglages, chaque logiciel en décide lui-même.

Chaque modification de l’assistant est enregistrée dans l’historique de l’arbre, comme une modification que vous faites vous-même : vous pouvez l’y examiner et restaurer la version précédente (voir [Paramètres](@/guide/settings.fr.md)). L’historique n’indique pas quelles modifications viennent de l’assistant : fiez-vous à leur heure.

Vous pouvez autoriser l’écriture pour un logiciel d’assistant et en laisser un autre en lecture seule : l’option appartient à la configuration de chaque logiciel. Retirez-la pour remettre un assistant en lecture seule.

## S’en servir

Posez vos questions à l’assistant en langage courant, par exemple :

- « Quels arbres ai-je dans OxidGene ? »
- « Dans l’arbre Landrevel, résume la vie d’Étienne Marie Landrevel. »
- « Liste les ancêtres du SOSA 1 de l’arbre Landrevel dont le décès n’est pas renseigné. »
- Avec l’écriture autorisée : « Dans l’arbre Landrevel, ajoute le baptême d’Étienne Marie Landrevel vers 1790 et cite le registre paroissial. »

L’assistant commence par lister vos arbres, puis demande à OxidGene ce dont il a besoin dans l’arbre que vous avez nommé. Les dates lui parviennent avec leur précision (« vers », « avant »…), et quand une naissance ou un décès n’a pas de date, le baptême ou l’inhumation peut en tenir lieu, comme partout dans OxidGene. Les numéros SOSA partent de la personne racine de l’arbre.

La fenêtre d’OxidGene peut rester ouverte pendant que l’assistant travaille : ils partagent la même base de données. Une modification de l’assistant apparaît dans la fenêtre une fois la page rechargée.

Les noms, notes et sources proviennent souvent de fichiers importés, écrits par d’autres, et peuvent contenir du texte qui tente de donner des instructions à l’assistant. Tant que l’accès est en lecture seule, rien de ce qu’ils contiennent ne peut l’amener à modifier vos arbres par l’intermédiaire d’OxidGene. Quand vous autorisez l’écriture, OxidGene demande à l’assistant de ne jamais agir sur un tel texte, et chaque modification reste dans l’historique : vérifiez ce qu’il a fait.

## Le déconnecter

Retirez l’entrée OxidGene des serveurs MCP de votre logiciel d’assistant. Il ne peut alors plus lire vos arbres. Il n’y a rien à annuler du côté d’OxidGene ; les modifications qu’il a faites, si vous aviez autorisé l’écriture, restent dans vos arbres et leur historique.

En savoir plus : [spécification de l’accès des assistants](@/tech/mcp.md) (en anglais).
