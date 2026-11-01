+++
title = "Share a tree"
description = "Share a tree on the web version of OxidGene: with members who build or read it, by invitation, with share links, or by making it visible to the site or the web, and limit any share to a branch."
weight = 157
+++

On the web version of OxidGene, the owner of a tree decides who sees it and what they may do. This page explains the roles, how to share a tree with people who have an account or not yet, how share links and the tree's visibility let visitors in, how to limit a share to a branch, and what the people you share with see.

Sharing exists in the web version only, when it is connected to an identity provider (see [Accounts and profile](@/guide/accounts.md)). The desktop application, and a web version run without an identity provider, have one user who sees everything: they show nothing of this page.

## The roles

| Role | Who | May |
|---|---|---|
| **Owner** | The account the tree belongs to | Everything: sharing, settings, exclusions, deleting the tree |
| **Editor** | Someone who builds the tree with you | Add, change and delete records, export, see living persons |
| **Guest** | A relative you trust with the tree | Read the whole tree, living persons included, and change nothing |
| **Visitor** | Whoever the tree's visibility or a share link lets in | Read the tree, living persons and private records hidden |

Persons you [exclude from sharing](@/guide/settings.md#exclusions-from-sharing) stay yours alone, whatever the role or the share.

## Share with someone

Open the tree's settings, then **Members** — a section only the owner sees.

1. In **Share the tree**, type the person's exact **Email address or username**, or pick them in **Or someone you already share with**, which lists the members of your other trees.
2. Choose the **Role**: **Guest** or **Editor**.
3. Under **What is shared**, keep **Whole tree** or choose **A branch** (see [Share a branch](#share-a-branch)).
4. Optionally, set the day the share **Ends on**.
5. Click **Share**.

Someone with an account becomes a member at once, and finds the tree on their home page. An address that has no account yet receives an **invitation** instead. When the instance sends no email, the section says so and shows the **Invitation link** to send yourself: it carries no secret, since only the invited address can accept it.

Looking someone up by address or username is limited to thirty times an hour.

**Members of the tree** lists everyone with their role, what they see and when their share ends. For each member, you can change the role, **Make owner** to hand the tree over — you then become an editor — or **Remove** them.

**Pending invitations** lists the invitations nobody has answered yet, each with its link, **Resend** and **Revoke**. An invitation waits fourteen days.

## Accept an invitation

Sign in with the invited address — the instance lets you create an account for it — and open the home page. **Invitations** lists the trees you are invited to: **Accept** adds the tree to your list, **Decline** forgets the invitation.

## Share links

In the tree's settings, **Sharing** lists the **Share links**. A link lets whoever opens it visit the tree without an account, living persons hidden.

1. Under **New link**, give it a **Label** that tells you who it is for, choose **What is shared** and, optionally, when it **Ends on**.
2. Click **Create the link** and copy it at once: it is shown this once.

Each link shows how many times it was opened. **Revoke** ends it immediately for everyone who opened it.

## Make the tree visible

**Visibility**, in the **Sharing** section, decides who visits the tree without being a member:

- **Private**: nobody but the members and the holders of a share link;
- **Visible to the site's users**: every signed-in user of the instance;
- **Public on the web**: anyone, without signing in.

A visible tree is reached by its address: it is listed nowhere. **What visitors see** can limit visitors to a branch, and the section counts the persons they will see before you **Save**.

## Share a branch

Instead of **Whole tree**, choose **A branch**, then its root person, its side — **Descendants**, **Ancestors** or **Ancestors and descendants** — and how many **Generations** it covers. You can add the spouses of the persons reached and their brothers and sisters. The section counts the persons the branch holds and how many of them visitors see.

A branch follows the tree: a child you add under a shared ancestor is shared at once. If the root is merged into another person, the share follows the person kept; if the root is deleted, the share is empty until it is restored.

An editor limited to a branch changes only the persons in it, and the persons they add to it. Places, sources, repositories and tags are shared by the whole tree: they use them but do not change them.

## What visitors see

When you visit a tree as a visitor, a banner says **You are viewing this tree as a visitor**, with **Sign in** when you are not signed in. On the home page, each card shows your role on the tree, and **My trees** and **Shared with me** keep the trees you own or those others share with you.

To stop seeing a tree someone shared with you, open its settings and click **Leave** under **Leave this tree**.
