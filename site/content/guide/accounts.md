+++
title = "Accounts and profile"
description = "Sign in to the web version of OxidGene, follow your notifications, manage your profile, avatar, language, notification emails and sessions, download or delete your account, and, as an administrator, manage the instance's users."
weight = 155
+++

When OxidGene runs as a web application for a family or an association, each person signs in with their own account. This page explains how to sign in and out, what your profile holds, how to see and end your sessions, how to download or delete your account, and how an administrator manages the users of the instance.

Accounts exist in the web version only, and only when whoever runs it has connected it to an identity provider. The desktop application has no account: it has one user, you, with access to everything. A web version run without an identity provider behaves the same way, and shows nothing of what this page describes.

## Sign in

1. Click **Sign in** at the top right of the home page.
2. Your browser goes to the identity provider's page — the service your instance uses to check who you are. Sign in there as it asks: email and password, a second factor, or an outside account it offers.
3. You come back to the page you started from, signed in.

OxidGene never sees your password: the identity provider keeps it, along with your second factor and the recovery of a forgotten password. The provider's page opens in the interface language when it has it.

Until you sign in, the pages that show trees and your profile offer **Sign in** instead of their content. The application settings stay available: they belong to your device.

If the sign-in fails, the page you return to says why: you or the provider refused it, the instance only admits the people it invited, your account was deactivated, or the provider could not be reached. **Try again** starts over.

## The account menu

Once signed in, your avatar replaces **Sign in** at the top right of the home page and of the application settings. Click it to open the account menu: your name and email, **Profile**, and **Sign out**.

**Sign out** ends your session in OxidGene, then at the identity provider, so the next sign-in asks who you are again.

## When your session ends

A session ends after a while without activity, after a maximum lifetime, when you sign out on another device with **Sign out everywhere**, or when an administrator deactivates your account. If it ends while a page is open, the dialog **Your session has ended** appears at your next action. **Sign in again** brings you back to the same page; **Close** leaves the page as it is, so you can copy what you were typing before signing in again.

## Notifications

OxidGene tells you what happens to you and your trees: a tree shared with you, your access to a tree changed or ended, someone accepting your invitation, a member leaving your tree, a tree changing owner, an import or export you started finishing.

- **The bell**, left of your avatar, shows how many notifications you have not read. Click it for the latest ones, newest first, the unread in bold. Click one to open what it is about — the tree, or its settings for an accepted invitation and a member who left — and it counts as read. **Mark all as read** clears the count.
- **New notifications arrive by themselves** while the page is open: there is nothing to reload. A tab in the background asks nothing until you come back to it.
- A notification about a tree you no longer see reads *A tree is no longer shared with you*, without its name. Notifications are kept 90 days.

When the instance sends email, some notifications also reach you by email, as you choose in your profile (below). Each email ends with **Stop these emails**: the link opens a page that asks before it turns that kind of email off, and your mail application's own *Unsubscribe* button does the same at once. The instance's sign-in emails — address check, password reset — come from the identity provider, not from OxidGene, and these settings do not change them.

## Your profile

Open **Profile** from the account menu. Its left menu has five sections.

### Profile

- **Avatar**: click **Upload**, choose a JPEG, PNG or WebP image of at most 5 MB, then choose the square that becomes your avatar: it starts in the middle of the picture; drag it, or move it with the arrow keys, and use **Zoom** to frame a smaller part. Click **Use as avatar**. OxidGene keeps only that square, resized, and nothing else of the file: no location, date or camera details. **Remove** brings back the generated avatar.
- Without an image, your avatar shows your initials — the first letter of the first two words of your name — on a colour chosen from your account, always the same on every device.
- **Display name**: the name the people you share trees with see. Type it and click **Save**. **Use my provider's name** goes back to the name your identity provider gives.
- **Email**: as your identity provider gives it, with whether it is verified. Change it at the provider.

### Language

Choose the interface language for your account. It applies at once on this device, and on any device where no language has been chosen in the [application settings](@/guide/settings.md). OxidGene's emails to you are written in this language too.

### Notifications

Notifications always appear under the bell. This section chooses which ones also reach you by email:

- **Send me emails**: off, no notification reaches you by email, whatever the choices below.
- For each kind of notification: **Not by email**, **By email, at once**, or **In a daily email summary** — one email a day, early in the morning (6:00 UTC), listing what you have not read yet. By default, a tree shared with you and an accepted invitation are emailed at once, the rest not at all.

OxidGene emails only the address your identity provider has verified. When the instance sends no email, the section says so, and offers no choice.

### Sessions

The list of the browsers signed in to your account: which browser, when it signed in and when it was last active. **This browser** marks the one you are using.

- **End** signs that browser out. On the current one, the button reads **Sign out**.
- **Sign out everywhere** ends every session of your account, this one included.

End any session you do not recognise, then change your password at your identity provider.

### Account

- **Password and two-step verification**: **Open** goes to your identity provider's account page, where you change them.
- **Download my account data**: a JSON file holding everything OxidGene stores about your account, your notification settings included.
- **Delete my account**: after a confirmation, your account, sessions, avatar, notifications and pending emails are erased for good and you are signed out. The genealogy you entered stays in its trees. The history shows your changes as those of a former member.

## Manage the instance's users

An administrator of the instance finds an **Instance** group in the [application settings](@/guide/settings.md), with a **Users** section: every account, with its avatar, name, email, role, whether it may create trees, whether it is deactivated, and its last sign-in. **Load more** shows the next ones.

The **⋮** menu of a user offers:

- **Make administrator** or **Remove the administrator role**;
- **Allow creating trees** or **Stop allowing tree creation**;
- **Deactivate**, which signs the user out everywhere and stops them from signing in, or **Reactivate**;
- **Change identity…**, which binds the account to another identity at the provider — its issuer and subject — after the instance moves to another identity provider;
- **Erase**, which deletes the account for good after a confirmation; the history then shows the user as a former member.

Your own account has no menu here: change it from your profile. When the identity provider decides the roles — the instance's administrators then come from a group at the provider — changing a role here is refused, and the change is made at the provider.
