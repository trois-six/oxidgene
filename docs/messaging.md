---
type: "Product Specification"
title: "Messaging and Notifications"
description: "Communication between the users of a web deployment: in-app notifications and their preferences and email sent through the operator's SMTP relay from a worker-processed outbox, with translated templates, daily digests and one-click unsubscribe, implemented; and, planned, end-to-end encrypted one-to-one and group conversations over Messaging Layer Security with per-device browser keys, a public key directory, passphrase-protected key backup and key verification, delivered by long poll, with blocking, reporting, moderation, rate limits, retention, erasure, API, interface outline, dependencies and tests."
tags: [oxidgene, specification, messaging, notifications, email, encryption, mls, privacy, api, ui]
generated: { by: human:maintainer, at: 2026-11-03T14:30:00Z }
---

# Messaging and Notifications

> Part of the [OxidGene Specifications](index.md).
> See also: [Authentication](authentication.md) · [Authorization](authorization.md) ·
> [Privacy](privacy.md) · [API Contract](api.md) · [Data Model](data-model.md) ·
> [Cross-cutting Rules](cross-cutting.md) · [Roadmap](roadmap.md)

**Status: notifications and email (§2, §3, §6) are implemented; the
conversations, their encryption and moderation (§4 to §9) are planned.**
[Roadmap §8](roadmap.md#8-planned-security-release-and-deployment)
tracks the phases. Notifications, email and messaging exist only between the
users of a web deployment, so they are compiled and active only in the web
build — server, worker and web frontend. The desktop has one user, sends no
mail and has no messaging ([Authentication §7](authentication.md#7-web-build-only-the-desktop-is-unchanged)).

---

## 1. Principles

- **Messages are end-to-end encrypted.** Only the devices of a
  conversation's members can read it. The server stores ciphertext, routes
  it, enforces who belongs where and limits rates; it never holds a key that
  decrypts a message (§5).
- **A standard protocol.** Messaging Layer Security (RFC 9420), not a scheme
  of our own.
- **Text and links only.** A message is text and links to records; there are
  no attachments.
- **Access is never widened by a message.** A link to a person shows the
  reader that person only if the reader may see them
  ([Privacy §6.4](privacy.md#64-effects)).
- **Mail says little.** OxidGene's mail carries no message content and no
  genealogy beyond a tree's name.
- **No polling at fixed intervals.** Clients wait on a long poll that answers
  when something changes (§6).

---

## 2. Notifications

### 2.1 Kinds

A notification ([§10](#10-data-model)) is a row for one user, holding the
identifiers of what it is about; the frontend renders its text in the
reader's language, and resolves the tree or person it names under the
reader's current access — a notification about a tree they no longer see
reads *A tree is no longer shared with you*, without its name.

| Kind | Sent to | When |
|---|---|---|
| `tree_shared` | The new member | A membership is created for them ([Authorization §6.1](authorization.md#61-sharing-with-a-user-of-the-instance)) |
| `membership_changed`, `membership_ended` | The member | Their role, scope or expiry changes; they are removed or their membership expires |
| `invitation_accepted` | The owner | An invitation becomes a membership |
| `ownership_transferred` | The new and the former owner | |
| `member_left` | The owner | A member leaves |
| `job_finished` | The job's actor | An import or export they started completes or fails |
| `messages` | Each member of a conversation | New messages arrived; one notification per conversation, its count rising, never their content |
| `report_resolved` | The reporter | A moderator resolved their report (§7) |
| `api_client_expiring` | The client's owner | Seven days before an API client or credential expires |

The first seven exist (`oxidgene_api::service::notification`); `messages`
and `report_resolved` arrive with the conversations, `api_client_expiring`
with the API clients ([Roadmap §8](roadmap.md#8-planned-security-release-and-deployment),
phases 7 and 6). A notification is written in the transaction of the change
that causes it — the share, the member update or removal, the acceptance,
the transfer, the maintenance pass that deletes an expired membership, the
end of a job — and its readers' long polls are woken once that commits.
`ownership_transferred` names the new owner as its subject, and reads *You
are now the owner* to them; an administrator's owner for an ownerless tree
notifies the new owner alone. `job_finished` names the job, whose outcome —
completed or failed, import or export — is read when the notification is; a
cancelled job, which its actor stopped, notifies nobody.
### 2.2 Delivery in the application

The navbar's bell shows the unread count; its panel lists the latest
notifications, newest first, each opening what it is about and marked read
when opened, with **Mark all as read**. New ones arrive through the long poll
(§6). Notifications are kept 90 days.

### 2.3 Preferences

Per user and per kind: in the application always; by email *off*,
*immediately* or in a *daily digest*. Defaults: `tree_shared` and
`invitation_accepted` immediately, `messages` daily, the rest off. One switch
turns all email off; it is stored as the preference of the kind `all`. Every
notification mail carries a one-click unsubscribe (RFC 8058) for its kind,
through a signed token valid without signing in: its `List-Unsubscribe`
header names `POST /api/v1/mail/unsubscribe?token=`, which a mail client
posts to, and its body links to the page `/unsubscribe?token=`, which asks
before posting it, so that a scanner following the link unsubscribes nobody.
A digest's token turns off every kind the user had in a digest. The token is
the user's id and the kind, signed with HMAC-SHA256 under a key derived from
the session keys (`OXIDGENE_SESSION_KEYS`): the first key signs, any key
opens, so that rotating them keeps the links of mail already sent working
until the old key goes. An invitation's mail carries none: its address has no
account to keep a preference in, and it is sent once per send, at the
owner's request.

---

## 3. Email

### 3.1 Who sends what

| Sender | Mail |
|---|---|
| The identity provider | Its own: address verification, password reset, its sign-in notices. Configured in the provider ([Authentication §8.2](authentication.md#82-rauthy-mode), `auth.idp.rauthy.smtp`) |
| OxidGene | Invitations by email ([Authorization §6.2](authorization.md#62-invitations-by-email)) and notifications per the recipient's preferences (§2.3), single or as a digest |
| The desktop | Nothing |

### 3.2 SMTP relay

OxidGene does not deliver mail itself: it hands it to the operator's SMTP
relay.

| Variable | Helm value | Description |
|---|---|---|
| `OXIDGENE_SMTP_HOST` | `mail.smtp.host` | The relay. Unset disables mail (§3.5) |
| `OXIDGENE_SMTP_PORT` | `mail.smtp.port` | Default 587 |
| `OXIDGENE_SMTP_TLS` | `mail.smtp.tls` | `starttls` (default) or `implicit` (port 465); `none` only towards a loopback relay; `danger-insecure`, plain SMTP towards any relay, for a development relay such as the Compose stack's Mailpit, warned about at every start |
| `OXIDGENE_SMTP_USERNAME`, `OXIDGENE_SMTP_PASSWORD` | `mail.smtp.credentialsSecret` | From a Kubernetes Secret, both or neither; the password may instead be read from the file `OXIDGENE_SMTP_PASSWORD_FILE` names, and never reaches a log |
| `OXIDGENE_MAIL_FROM` | `mail.from` | The sender address, such as `OxidGene <no-reply@genealogy.example.invalid>`; required with a relay |

SPF, DKIM and DMARC for the sender's domain, and bounce handling, are the
operator's; OxidGene neither signs mail nor processes bounces. The relay's
certificate is checked by the platform's verifier, as the identity
provider's is, and the greeting names the public URL's host.

The relay is read in `oidc` mode only: with `OXIDGENE_AUTH_MODE=none` these
settings are ignored, nothing is mailed and no relay is needed. The process
that sends — the worker, or the server's embedded worker under SQLite or
filesystem media — also reads `OXIDGENE_PUBLIC_URL`, which its links start
with, and `OXIDGENE_SESSION_KEYS`, which sign the unsubscribe tokens; a
worker without a relay needs neither. An invalid setting stops the process
with its category (`mail_config_missing`, `mail_config_invalid`) and its
variable, never its value.

### 3.3 Outbox

A mail is a row of `mail_outbox` (§10), written in the transaction of what
causes it — the invitation, the share — so a rolled-back write sends nothing.
The job worker sends it: claims due rows with a lease, sends, marks them
sent; a failure retries after 1, 5 and 30 minutes, then 2 and 12 hours, and
is marked failed after six attempts with its error category. Rows are deleted
seven days after they are sent or failed, and the recipient's address with
them. The daily digest is one outbox row per user per day, assembled when the
worker sends it. Logs record the kind, the outcome and the attempt, never an
address.

- **Ids only.** A row holds the recipient's address and language and the ids
  it is rendered from: the invitation, the notification, or the time a
  digest starts from. It is rendered when it is sent, under the recipient's
  access then, so a renamed tree goes out under its new name and a tree the
  recipient no longer sees under none.
- **What no longer applies is dropped**, status `dropped` with its reason: an
  invitation revoked, answered or expired before its mail left, a
  notification read meanwhile, a kind turned off since, a recipient
  deactivated or erased, a digest with nothing unread left.
- **Who is mailed.** A notification is mailed to an active account with a
  verified address, as its preferences say; a user whose provider did not
  verify the address gets notifications in the application only.
- **Lease and refusals.** A claimed row is leased for five minutes; a worker
  that stops while sending leaves it to another once the lease runs out, and
  a worker whose lease ran out records nothing once another claimed the row
  again: what the latest claim records stands. A
  permanent refusal of the relay (an SMTP `5xx` reply, a message that cannot
  be built) is failed at once, `rejected` or `message`; a transient one
  (`deferred`, `timeout`, `tls`, `connection`) follows the retries.
- **The digest's hour.** A notification whose kind is wanted daily opens the
  user's digest row if none is pending, due at the next 06:00 UTC — users
  keep no time zone — and joins the pending one otherwise; the digest lists
  the unread notifications of the kinds the user still wants daily since it
  opened.
- **No polling.** The sender wakes as the job worker does
  ([Architecture §6.5](architecture.md#65-waking-the-worker)): a row queued
  in its process wakes it in memory once its transaction commits, one queued
  elsewhere through PostgreSQL's `pg_notify('oxidgene_mail', '')`, sent in
  the inserting transaction; idle, it reads when the earliest pending row
  falls due — a retry, a digest, a lease run out — and a minute after it
  went to sleep at the latest: three statements a minute while idle.
- **Retention.** The worker's maintenance pass deletes the rows ended more
  than seven days ago, and the notifications older than 90 days.

### 3.4 Templates

Each kind has a plain-text and a simple HTML template, its strings in the
translation tables of the eight interface languages, at key parity. A mail is
written in the recipient's language — their account's locale, else the
instance's default, English — and contains the actor's display name, the
tree's name where relevant, and a link to the application; a messages
digest says only *You have N new messages in M conversations*.

The tables are `assets/mail/<language>.json`, apart from the interface's,
which the frontend embeds and a mail never needs; a test keeps them at key
parity with the same `{placeholders}`. Names and tree names are escaped in
the HTML and stripped of control characters in the subject. An invitation
goes out in the instance's default, its address having no account; it says
who invites, to which tree and as what, until when it waits, how to accept
it, and to ignore it when the sender is unknown.

### 3.5 Without SMTP

With no relay configured, nothing is mailed: invitations give the owner a
link to copy ([Authorization §6.2](authorization.md#62-invitations-by-email)),
notifications stay in the application, and the email preferences are hidden.

The development Compose stack runs Mailpit as its relay, which keeps every
mail for inspection and delivers none: Rauthy's own and OxidGene's, which the
stack's worker sends to `mailpit:1025` with `OXIDGENE_SMTP_TLS=danger-insecure`
([Authentication §8.5](authentication.md#85-development-stack)).

---

## 4. Conversations

### 4.1 Who can message whom

A user may start a conversation with

- anyone they share a tree with — members and owner of a common tree;
- anyone they name by exact username or verified email, as for sharing
  ([Authorization §6.1](authorization.md#61-sharing-with-a-user-of-the-instance)):
  this one arrives as a **message request**, which the recipient accepts,
  ignores or blocks; until accepted, the sender sees no read state and can
  send no more than three messages.

Visitors without an account, link holders and API clients take no part.

### 4.2 One-to-one and groups

A one-to-one conversation has two users; a group has up to 50 and a creator
who may add and remove members, any member being able to leave. Each user
takes part through every device they enrolled (§5.2). A one-to-one
conversation exists once per pair.

### 4.3 About a tree or a person

A conversation may be about a tree or a person — *about this ancestor*. The
reference is part of the encrypted conversation information, not a column the
server reads. A member's client resolves it through the API under that
member's own access: if they may not see the person, the conversation shows
no subject, and the person's page shows no conversation about them.

### 4.4 Messages

A message is a JSON document, encrypted as a whole: `text` (plain text, at
most 4,000 characters, rendered without HTML, its web addresses as
`noopener noreferrer` links), `links` (at most 20 references
`{ tree_id, kind, id }` to persons, families, sources or places), and
optionally the message it replies to. Record links are rendered as chips the
reader's client resolves under the reader's access, showing nothing for a
record the reader may not see. A sender may edit or delete their message for
everyone during 24 hours; deletion sends an encrypted tombstone and lets the
server drop the original ciphertext.

### 4.5 Read state

Each member has a read cursor per conversation, stored by the server — a
member sees how far the others have read. A member may turn read receipts off
for themselves, which also hides others' from them.

---

## 5. End-to-end encryption

### 5.1 Protocol

Every conversation is an MLS group (RFC 9420) with ciphersuite
`MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`; each member device is a leaf.
MLS gives forward secrecy and post-compromise security for groups of any
size, with membership changes in logarithmic cost, and is an IETF standard
with several interoperable implementations. The clients use OpenMLS (MIT),
which compiles to WebAssembly with its RustCrypto provider.

| | MLS (chosen) | Signal's double ratchet | Megolm (Matrix) |
|---|---|---|---|
| Groups | Native, one tree of keys | Pairwise sessions, or sender keys for groups | One outbound ratchet per sender and device |
| Post-compromise security in groups | Yes, at each commit | Pairwise only | No |
| Standard | IETF RFC 9420 | Published specifications | Matrix specification; Matrix is moving to MLS |
| Rust for WebAssembly | OpenMLS (MIT) | libsignal (AGPL-3.0, server-oriented builds) | vodozemac (Apache-2.0) |

### 5.2 Devices and keys

- **Device keys.** A browser enrolling for messaging generates its MLS
  signature key and key packages in the WASM client. They never leave the
  browser unencrypted.
- **Storage.** OpenMLS's state — keys, group secrets, epochs — is kept in
  IndexedDB, encrypted with AES-GCM under a wrapping key that is a
  non-extractable WebCrypto key stored in IndexedDB itself: a script reading
  the storage gets ciphertext, and no code can export the wrapping key. The
  MLS private keys are decrypted into WASM memory while in use, since OpenMLS
  computes with them; WebCrypto cannot hold them as non-extractable keys and
  still let MLS use them.
- **User identity.** Each user has an identity signing key, which signs each
  of their device keys. The server's directory (§5.3) holds the public
  identity key, the signed device keys and each device's key packages.
- **Adding a device.** A new browser shows a QR code (or a short code) holding
  its device key's fingerprint; an existing device of the user scans or types
  it, signs the new device key with the identity key, and sends it the
  identity key and the archive key (§5.4), sealed with HPKE (RFC 9180) to the
  new device's key, through the server. Without another device, the new one is
  enrolled from the key backup (§5.4). The conversations' members' clients
  then add the new device to each group, at their next activity, after
  checking its signature by the user's identity key.
- **Removing a device.** From the user's device list: the device is revoked
  in the directory and removed from every group by the next commit; a lost
  device is removed the same way.
- **Signing out** of OxidGene does not erase the browser's messaging keys;
  **Forget this device** does, and removes it.

### 5.3 Key directory and server role

The server:

- holds public keys only: identity keys, signed device keys, key packages
  (single-use, replenished by each device);
- stores and routes MLS messages — handshakes, welcomes, application
  messages — per device, as ciphertext;
- orders each group's commits: it accepts one commit per epoch, the first,
  and refuses the others, which their senders rebase;
- enforces membership: only a member's device may post to a group, and a
  commit adding a user is accepted only when that user may be messaged by the
  adder (§4.1) and has not blocked them; clients reject a welcome or an add
  the server did not accept;
- is a registered external sender of every group, so it can propose the
  removal of a deactivated, erased or blocked user's devices, which the next
  member commit applies;
- limits rates (§8), and never decrypts.

It sees metadata: who is in which conversation, who sends when, sizes, read
cursors. It does not see subjects, references, text or links.

### 5.4 Backup and recovery

- **Archive.** Forward secrecy means a device cannot read messages sent
  before it joined a group. So that a user's new devices see their history,
  each device, after decrypting a message, stores it re-encrypted under the
  user's **archive key** — a random symmetric key — in the user's server-side
  archive. Devices read their history from there.
- **Key backup.** The identity key and the archive key are backed up on the
  server, encrypted with a key derived in the browser from a **passphrase**
  the user chooses at enrolment (Argon2id, 64 MiB, 3 passes, a random salt),
  or from a generated **recovery key** the user writes down. The server stores
  the ciphertext, salt and parameters, and never sees the passphrase or the
  derived key; fetching the backup is rate-limited (§8). The passphrase must
  be at least four words or 14 characters, since a stolen backup can be
  attacked offline.
- **Recovery.** A new device given the passphrase or recovery key restores
  the identity and archive keys, then joins its groups as any new device.
- **Everything lost.** A user who loses every device and their passphrase and
  recovery key cannot read their history again: they enrol with a new
  identity, and their contacts are warned that it changed. The interface says
  this at enrolment, and before a device is forgotten.

### 5.5 What encryption changes

- **Moderation** works on what a reporter discloses: the reporting client
  attaches the messages it reports, decrypted, each with its MLS signed
  content, which the moderator verifies against the sender's device key in the
  directory — a report cannot invent a message, and reveals only what the
  reporter chose (§7).
- **Email digests** carry no content, only counts (§3.4).
- **Search** of messages runs on the client, over what it has decrypted; the
  server cannot search them.
- **Record links** are resolved by the client under the reader's access
  (§4.4); the server cannot tell which records a conversation mentions.
- **Erasure** deletes ciphertext and keys (§9); copies already decrypted on
  other members' devices are beyond the server's reach.
- **Backups and legal requests.** The operator's database backups hold
  ciphertext and metadata; an operator can disclose metadata, never content.
  The instance's privacy notice says so.

### 5.6 Verification

Each pair of users can compare a safety number — 60 digits derived from both
identity keys — or scan each other's QR code. A verified contact is marked on
the devices that verified them; when a contact's identity key changes, every
conversation with them shows a warning until verified again. New devices of a
user are trusted through their identity key's signature, so verifying a user
once covers their devices.

---

## 6. Delivery without polling

`GET /api/v1/me/updates?after={cursor}&wait=25` answers at once when events
newer than the cursor exist, otherwise holds the request until one arrives or
`wait` seconds pass (at most 30), then answers with what it has and the next
cursor — the same wait-until-changed long poll used to follow a job's
progress. Events are typed and carry ids, ciphertext for MLS messages, and
nothing else: `notification`, `mls_message`, `welcome`, `commit`,
`device_added`, `read_cursor`, `conversation_changed`. One process wakes its
own waiters; with several backend replicas, or a worker in a process of its
own, the wake-up goes through PostgreSQL's notifications, as the job status
reads' does ([Architecture §6.5](architecture.md#65-waking-the-worker)):
the transaction writing a notification sends
`pg_notify('oxidgene_updates', user_id)`, which leaves when it commits, and
each server listens on that channel with one connection of its own, waking
every waiter after a lost connection. A deployment with several replicas runs
on PostgreSQL in any case — SQLite forces one — so Redis is not needed for
it. The frontend keeps one such request open while it is shown, and backs
off after an error.

The cursor is opaque — today the newest notification's id — and `notification`
is the only event until the conversations bring theirs. A waiting request
reads nothing: it sleeps on a channel of its user and reads once more only
when woken, so an idle page costs two statements per 25 seconds — its
session's user and one read of what is new, when the request starts — and
none while it waits, as a test measures. At most 10,000 requests wait at once in one process;
past them a request answers at once without events, and the page pauses a
second before asking again, as it does after every answer without events.

---

## 7. Blocking, reporting and moderation

- **Blocking** a user stops them from starting conversations with the blocker,
  adding them to groups or sending them message requests; their messages in
  shared groups are not delivered to the blocker's devices; the one-to-one
  conversation is hidden. The blocked user is not told. Unblocking is
  possible at any time.
- **Reporting** a conversation or messages sends a report with a reason
  (spam, harassment, other) and the disclosed messages (§5.5) to the
  instance's administrators. The reporter may block in the same step.
- **Moderation.** Administrators see reports — reason, disclosed messages
  with their verification, the reported user — and resolve them: dismiss,
  warn, suspend the user's messaging for a period, or deactivate the account
  ([Authentication §6.5](authentication.md#65-deactivation)). Every decision
  is an access event ([Authorization §9.2](authorization.md#92-access-log)).
  Reports are deleted a year after resolution.

---

## 8. Rate limits

| What | Default |
|---|---|
| Messages sent | 30 per minute per user |
| New conversations and message requests | 20 per day per user towards people they share no tree with |
| Members added to groups | 50 per hour per user |
| Key-package and backup fetches | 60 per hour per user; backup fetches 10 per hour |
| Reports | 20 per day per user |

Past a limit, `429 rate_limited`
([Authentication §9.2](authentication.md#92-rate-limits-and-brute-force)).

---

## 9. Retention and erasure

- The server keeps a message's ciphertext until every member's devices have
  fetched it and for at most 30 days, and each user's archive until they
  delete it; `OXIDGENE_MESSAGE_ARCHIVE_RETENTION` may bound the archive.
- Leaving a conversation deletes the user's archive of it. A conversation
  whose last member leaves is deleted.
- **Erasure** of a user ([Authentication §6.6](authentication.md#66-erasure-and-account-data))
  deletes their identity and device keys, key packages, backup, archive,
  pending messages, read cursors, blocks, notifications, preferences and
  queued mail; the server proposes their removal from every group (§5.3); the
  messages they sent remain only on the devices of those who already
  decrypted them. Reports keep the disclosed messages until their own
  deletion, the reporter having disclosed them.

---

## 10. Data model

The tables, added to the single initial migration until the first
release ([Data Model §6.11](data-model.md#611-notifications-mail-and-messaging)).
Every key is a UUID v7. `notification`, `notification_preference` and
`mail_outbox` exist; the others are planned.

| Table | Columns |
|---|---|
| `notification` | `id`, `user_id`, `kind`, `tree_id?`, `actor_user_id?`, `subject_id?`, `count`, `created_at`, `read_at?` |
| `notification_preference` | `user_id`, `kind`, `email` (`off`, `immediate`, `daily`); one row per changed kind |
| `mail_outbox` | `id`, `user_id?`, `to_address`, `kind`, `language`, `params` (ids only), `status`, `attempts`, `next_attempt_at`, `lease_until?`, `sent_at?`, `error_category?`, `created_at` |
| `messaging_identity` | `user_id`, `identity_public_key`, `created_at`, `replaced_at?` |
| `messaging_device` | `id`, `user_id`, `signature_public_key`, `identity_signature`, `label`, `created_at`, `last_seen_at`, `revoked_at?` |
| `key_package` | `id`, `device_id`, `data`, `created_at`, `claimed_at?` |
| `key_backup` | `user_id`, `ciphertext`, `kdf`, `salt`, `params`, `version`, `updated_at` |
| `conversation` | `id`, `kind` (`direct`, `group`), `mls_group_id`, `epoch`, `created_by`, `created_at`, `last_activity_at` |
| `conversation_member` | `conversation_id`, `user_id`, `role` (`creator`, `member`), `state` (`requested`, `active`, `left`), `read_cursor?`, `read_receipts`, `joined_at`, `left_at?` |
| `mls_message` | `id`, `conversation_id`, `epoch`, `content_type` (`application`, `commit`, `proposal`, `welcome`), `sender_device_id`, `recipient_device_id?` (welcomes), `ciphertext`, `created_at`, `deleted_at?` |
| `mls_delivery` | `message_id`, `device_id`, `fetched_at?` |
| `message_archive` | `id`, `user_id`, `conversation_id`, `message_id`, `ciphertext`, `created_at` |
| `user_block` | `blocker_id`, `blocked_id`, `created_at` |
| `message_report` | `id`, `reporter_id`, `reported_user_id`, `conversation_id`, `reason`, `disclosed` (the disclosed messages and their signed content), `status`, `resolution?`, `resolved_by?`, `created_at`, `resolved_at?` |

---

## 11. API

All under `/api/v1`, mirrored in GraphQL; ciphertext is base64 on both
surfaces. Each operation moves into [API Contract §2](api.md#2-rest-api) and
the parity table when implemented: the first six, the notifications', are
there ([API Contract](api.md#notifications)).

| Method | Path | GraphQL |
|---|---|---|
| `GET` | `/me/updates?after=&wait=` | `updates(after, wait)` |
| `GET` | `/me/notifications?first=&after=` | `notifications` |
| `POST` | `/me/notifications/read` | `markNotificationsRead` |
| `GET` | `/me/notification-preferences` | `notificationPreferences` |
| `PUT` | `/me/notification-preferences` | `updateNotificationPreferences` |
| `POST` | `/mail/unsubscribe` | `unsubscribe` (signed token, no session) |
| `PUT` | `/me/messaging/identity` | `setMessagingIdentity` |
| `GET` | `/me/messaging/devices` | `messagingDevices` |
| `POST` | `/me/messaging/devices` | `enrollMessagingDevice` |
| `DELETE` | `/me/messaging/devices/{device_id}` | `revokeMessagingDevice` |
| `POST` | `/me/messaging/devices/{device_id}/key-packages` | `uploadKeyPackages` |
| `GET` | `/me/messaging/backup` | `keyBackup` |
| `PUT` | `/me/messaging/backup` | `setKeyBackup` |
| `GET` | `/users/{user_id}/messaging-keys` | `messagingKeys(userId)`: identity, devices, one claimed key package per device; only for users the caller may message |
| `GET` | `/conversations?first=&after=` | `conversations` |
| `POST` | `/conversations` | `createConversation` |
| `GET` | `/conversations/{conversation_id}` | `conversation` |
| `POST` | `/conversations/{conversation_id}/members` | `addConversationMembers` |
| `DELETE` | `/conversations/{conversation_id}/members/{user_id}` | `removeConversationMember` (oneself: leave) |
| `POST` | `/conversations/{conversation_id}/accept` | `acceptMessageRequest` |
| `POST` | `/conversations/{conversation_id}/messages` | `postMlsMessage`: a commit, proposal, welcome or application message |
| `DELETE` | `/conversations/{conversation_id}/messages/{message_id}` | `deleteMlsMessage` (the sender, within 24 hours) |
| `PUT` | `/conversations/{conversation_id}/read` | `markConversationRead` |
| `GET` | `/me/messaging/archive?conversation_id=&first=&after=` | `messageArchive` |
| `POST` | `/me/messaging/archive` | `archiveMessages` |
| `GET` | `/me/blocks` | `blockedUsers` |
| `PUT` | `/me/blocks/{user_id}` | `blockUser` |
| `DELETE` | `/me/blocks/{user_id}` | `unblockUser` |
| `POST` | `/reports` | `reportMessages` |
| `GET` | `/admin/reports?first=&after=&status=` | `messageReports` |
| `PUT` | `/admin/reports/{report_id}` | `resolveMessageReport` |

Errors follow [Authorization §4.3](authorization.md#43-errors-401-403-and-404):
a conversation the caller is not in is `404`; a stale epoch is
`409 conflict`; a refused add is `403`.

---

## 12. User interface

Outline; each surface is translated in every interface language and exists
in the web build only.

- **Navbar.** A bell with the unread count and its panel (§2.2), and a
  messages icon with the number of conversations holding unread messages.
- **Inbox** (`/messages`): conversations by last activity, each with its
  members, subject if any, unread count and last activity time; message
  requests in a tab of their own with **Accept**, **Ignore**, **Block**;
  **New conversation** with the people the user shares trees with, or an
  exact username or email.
- **Conversation** (`/messages/{id}`): messages with their sender, time,
  edited mark and read state; a composer with a record-link picker searching
  the trees the user can see; the subject (a tree or person) as a chip;
  members, verification state and **Verify**, **Leave**, **Block**,
  **Report** in a side panel; a banner when a member's identity changed.
- **Person page.** *Discuss this person* starts a group with chosen members
  of the tree, about that person.
- **Messaging security** and **Blocked users** among the account sections of
  App Settings
  ([Authentication §10](authentication.md#10-user-interface)): enrolment with
  the passphrase or recovery key and its warning; the devices list with
  **Add a device** (QR) and **Remove**; **Forget this device**; the user's own
  safety number; the blocked users with **Unblock**.
- **Administration.** A **Reports** list in the Instance section for
  administrators.

---

## 13. Dependencies and size

| Need | Choice | Where | Cost |
|---|---|---|---|
| MLS | `openmls` with `openmls_rust_crypto` (X25519, Ed25519, AES-GCM, HKDF, SHA-256 from RustCrypto) | `oxidgene-ui` behind a `messaging` feature that only `oxidgene-web` enables | Several hundred KB to a few MB of WASM before compression, to be measured; the web binary's budget in `scripts/budgets.json` is raised by the measured amount, no more |
| Passphrase derivation | `argon2` (RustCrypto) | Same feature | Small |
| Sealing to a device | HPKE as OpenMLS already provides it | Same feature | None more |
| MLS framing on the server | The public header (group, epoch, content type) parsed with `tls_codec`, which OpenMLS uses; no decryption code | `oxidgene-api` `auth` feature | Small |
| Mail | `lettre` 0.11 without its defaults: `smtp-transport`, `tokio1-rustls`, `builder`, `aws-lc-rs` (the provider rustls already builds) and `rustls-platform-verifier` (the verifier reqwest already uses); no native TLS, no connection pool, no `hostname` | `oxidgene-api` `auth` feature: server and worker | Measured at its landing: `lettre` and three small crates of its message builder (`email-encoding`, `email_address`, `quoted_printable`), the rest — `mime`, `httpdate`, `fastrand`, `socket2`, `url`, `rustls`, `tokio-rustls`, `rustls-platform-verifier` — already in the graph; no duplicate |
| Long-poll wake-up across replicas | PostgreSQL's `LISTEN`/`NOTIFY` through the `sqlx` pool SeaORM already holds, as the job status reads do | Server | None more |

Each crate is weighed against AGENTS.md's dependency discipline when it
lands: `cargo tree` with the duplicate-crate budget, the binary budgets of
the server, worker and web bundle, and a guard keeping `openmls`, `argon2`
and `lettre` out of the desktop's dependency graph and out of the frontend's
non-`messaging` build. Message encryption adds nothing to the desktop.

---

## 14. Tests

- **Protocol.** Two-, three- and multi-device groups in the test harness with
  OpenMLS: adds, removes, concurrent commits rebased after a `409`, device
  enrolment by QR flow and by backup, the archive, safety numbers, an
  identity change warning. Run natively, with a WASM build check.
- **Server.** The server never accepts plaintext: a test posts application
  messages and asserts the stored rows are the ciphertext posted; membership
  enforcement; external-sender removals on erasure; ordering of commits;
  retention.
- **Notifications and mail** against the outbox: transactional creation,
  retries with a fake relay that fails, digests, unsubscribe tokens,
  templates at key parity in every language, no address in logs
  (`tests/notification_test.rs`), and the SMTP relay itself against a fake
  SMTP server on the loopback, its permanent and transient refusals
  included. End to end, an invitation mailed through the Compose stack's
  Mailpit, accepted, and the owner's bell and mail saying so
  (`e2e/auth/invitation-mail.spec.ts`, `just e2e-auth`).
- **Long poll**: answers at once, on change, and at timeout; across two
  processes through PostgreSQL's notifications (`just test-postgres`).
- **REST and GraphQL** parity for every operation of §11, with
  [Authorization §12](authorization.md#12-tests)'s viewer matrix.
- **Leaks.** A record link to an excluded or hidden person resolves to
  nothing for a member who may not see it.

---

## 15. Open decisions

1. **Message requests from strangers.** Recommended: allowed by exact
   username or email, as requests. Alternative: only people who share a tree,
   which removes most spam and most reach.
2. **Server-side history archive.** Recommended: the per-user archive under
   the archive key (§5.4), so new devices see history. Alternative: no
   archive — each device reads only what arrives after it joins, as strict
   forward secrecy implies.
3. **Read receipts default.** Recommended: on, switchable per user.
   Alternative: off by default.
4. **Default email preference for messages.** Recommended: a daily digest.
   Alternatives: off, or immediate.
5. **Ciphersuite.** Recommended: X25519, AES-128-GCM, Ed25519, the suite
   every MLS implementation must offer. Alternative: a post-quantum hybrid
   suite once OpenMLS offers one to WebAssembly clients.
