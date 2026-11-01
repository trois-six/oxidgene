---
type: "Security Specification"
title: "Authorization and Sharing"
description: "Authorization of the web deployment, owned by OxidGene rather than the identity provider: the Owner, Editor, Guest and Visitor roles and their permission matrix, tree visibility, the ViewerContext resolved once per request and passed to every service, the 401, 403 and 404 contract shared by REST and GraphQL, sharing with users of the instance, email invitations, share links, subtree scopes and how exclusions override them, API clients over the client credentials grant, MCP over Streamable HTTP, authors in the change history, the access log, and tests."
tags: [oxidgene, specification, security, authorization, sharing, api, mcp, audit]
generated: { by: human:maintainer, at: 2026-11-01T21:00:00Z }
---

# Authorization and Sharing

> Part of the [OxidGene Specifications](index.md).
> See also: [Authentication](authentication.md) · [Privacy](privacy.md) ·
> [Messaging and Notifications](messaging.md) · [API Contract](api.md) ·
> [Data Model](data-model.md) · [Assistant Access](mcp.md) ·
> [Roadmap](roadmap.md)

**Status: in progress.** The roles and their enforcement are in place
(phase 2): memberships with the Owner, Editor and Guest roles, ownership and
its transfer, ownerless trees, the `ViewerContext` of every request, the
requirement of every operation with the 401, 403 and 404 contract on both
surfaces, the authors of the history, the jobs' actors and the access log.
Sharing is in place (phase 5): Visitors from the tree's visibility and from
share links, sharing with users of the instance, invitations by email,
access scopes materialized and kept current, and a scoped Editor's write
rules. The redaction a Visitor reads through, which also shows a scope's
*Not shared* boundary, is phase 3's
([Privacy §7](privacy.md#7-enforcement-points)); API clients and MCP over
HTTP are planned; [Roadmap §8](roadmap.md#8-planned-security-release-and-deployment)
tracks the phases. Like everything of EPIC G, this is compiled and active
only in the web build — server, worker and web frontend; the desktop keeps
its single local user with full access and changes nothing
([Authentication §7](authentication.md#7-web-build-only-the-desktop-is-unchanged)).

---

## 1. Principles

- **OxidGene owns authorization.** The identity provider says who someone is;
  OxidGene's database says what they may do with each tree. No per-tree right
  is ever read from a token claim.
- **One decision point.** Every request resolves one `ViewerContext` (§4),
  and every service, projection read, search, statistic, chart, export, MCP
  tool and GraphQL resolver takes it. No surface filters on its own, so REST,
  GraphQL and MCP cannot disagree.
- **Fail closed.** A record the policy cannot place is hidden; a permission
  not granted is refused.
- **Roles for actions, relationships and attributes for records.** What a
  viewer may *do* is their role (§3). *Which persons* they see also depends
  on the tree's shape — a shared subtree (§6.5), the owner's exclusions
  ([Privacy §6](privacy.md#6-exclusions-from-sharing)) — and on each record's
  attributes and dates — privacy and contemporaries ([Privacy](privacy.md)).
  §3.4 says why roles alone cannot express this.
- **Precedence.** For every viewer but the owner: an exclusion, then an
  explicit `private`, then the contemporary rule, then the default — public.
  Editors and Guests are bound by exclusions only; Visitors by all four
  ([Privacy §3.1](privacy.md#31-precedence)).

---

## 2. Principals and instance roles

| Principal | Who | Authenticated by |
|---|---|---|
| User | A person with an account ([Data Model §6.1](data-model.md#61-app_user)) | A session or a trusted client's bearer token ([Authentication §5.1](authentication.md#51-accepted-credentials)) |
| API client | A program acting for a tree or a user (§7) | The client credentials grant |
| Link holder | Whoever opened a share link (§6.3) | A session holding the link's grant |
| Anonymous | Anyone else | Nothing |
| Local | The single user of `none` mode | Nothing ([Authentication §2](authentication.md#2-deployment-modes)) |

| Instance role | May |
|---|---|
| Administrator | Manage users: deactivate, reactivate, erase, rebind an identity, grant tree creation and the administrator role (unless the roles claim decides them, [Authentication §6.3](authentication.md#63-instance-roles-from-claims)); list trees with their name, owner, size and dates; assign an owner to an ownerless tree; delete a tree; read the instance's access log; moderate messaging ([Messaging §7](messaging.md#7-blocking-reporting-and-moderation)). **Not** read a tree's content: an administrator is a Visitor, Guest, Editor or Owner of a tree like anyone else |
| User | Sign in, be a member of trees, create trees when allowed (`can_create_trees`, from configuration or claims), own API clients, use messaging |

A tree created while the server ran in `none` mode has no owner; only an
administrator sees it, and assigns it one.

---

## 3. Tree roles and permissions

### 3.1 Roles

The roles follow the three sharing levels genealogists know from Geneanet,
plus the owner:

| Role | Who | Stored as |
|---|---|---|
| **Owner** | The one account the tree belongs to. Exactly one per tree | A membership with role `owner` |
| **Editor** | Builds the tree with the owner: adds, modifies and deletes data | A membership with role `editor` |
| **Guest** | A relative trusted with the whole tree, contemporaries included, who changes nothing | A membership with role `guest` |
| **Visitor** | Anyone the tree's visibility lets in, and share-link holders: browses under the privacy settings, contemporaries hidden | No membership: derived from the tree's visibility (§6.4) or a share link's grant (§6.3) |

A user holding several grants on one tree — a membership and a share link, or
a membership on a `site` tree — gets the highest role; at equal role, the one
without a scope wins.

The owner is the only one who manages the tree's members, shares, visibility,
exclusions and settings: a co-owning role is an open decision (1), and so is a
contributor role between Guest and Editor (2).

### 3.2 Permission matrix

Each permission is a stable identifier, listed in `viewer_access.permissions`
(§4.1) so the UI can hide what the viewer cannot do; the server checks it
regardless.

| Permission | Owner | Editor | Guest | Visitor |
|---|---|---|---|---|
| `read` — persons, families, events, places, sources, citations, notes and media within the viewer's scope | ✓ | ✓ | ✓ | ✓, redacted ([Privacy §5](privacy.md#5-what-a-redacted-viewer-sees)) |
| `read_private` — private records and contemporaries unredacted | ✓ | ✓ | ✓ | ✗ |
| `read_excluded` — persons excluded from sharing ([Privacy §6](privacy.md#6-exclusions-from-sharing)) | ✓ | ✗ (open decision 3) | ✗ | ✗ |
| `read_confidential` — confidential notes | ✓ | ✓ | ✗ | ✗ |
| `read_tags` — the tag vocabulary and the tags of records | ✓ | ✓ | ✓ | ✗ |
| `history_read` — the audit log and record versions | ✓ | ✓ | ✗ | ✗ |
| `tools` — anomalies, potential duplicates, ancestry completeness, unlocated places | ✓ | ✓ | ✗ | ✗ |
| `create`, `edit` — records, links, media uploads, notes, citations, tags on records | ✓ | ✓ | ✗ | ✗ |
| `privacy_edit` — a record's privacy and a note's confidentiality | ✓ | ✓ | ✗ | ✗ |
| `delete` | ✓ | ✓ | ✗ | ✗ |
| `merge` — person merges, dictionary renames and merges, bulk tagging | ✓ | ✓ | ✗ | ✗ |
| `history_restore` | ✓ | ✓ | ✗ | ✗ |
| `export` | ✓ | ✓ | With the tree's `guests_may_export` ([Privacy §9](privacy.md#9-tree-settings)) | ✗ |
| `import` — into the existing tree | ✓ | ✗ | ✗ | ✗ |
| `settings_manage` — name, roots, privacy settings, visibility | ✓ | ✗ | ✗ | ✗ |
| `exclusions_manage` | ✓ | ✗ | ✗ | ✗ |
| `members_manage` — members, invitations, share links | ✓ | ✗ | ✗ | ✗ |
| `api_clients_manage` — the tree's API clients | ✓ | ✗ | ✗ | ✗ |
| `access_log_read` — the tree's access events | ✓ | ✗ | ✗ | ✗ |
| `tree_duplicate` — with the instance's `can_create_trees` | ✓ | ✗ | ✗ | ✗ |
| `tree_delete`, `ownership_transfer` | ✓ | ✗ | ✗ | ✗ |
| `leave` — end one's own membership | — | ✓ | ✓ | — |

Operations that need no tree data — the date converter, the reference
bundles, the export destinations, the API description — need no permission,
and are open to an anonymous request. The Geneanet wizard's first steps,
which read what the user sends before a tree is chosen, and the tree list
need a signed-in user. Creating a tree needs the instance's
`can_create_trees`; its creator becomes its owner.

Every route and GraphQL root field declares its requirement — open, signed
in, tree creation, administrator, or a permission on the tree it names — in
`oxidgene-api`'s `authz::operations`, where the declaration is reviewed with
the operation; the API contract guard fails on a route or root field
without one and on a REST route whose GraphQL twin declares another (§12).
An undeclared operation is refused. Where the matrix leaves an operation's
permission open, the declaration settles it:

| Operation | Permission |
|---|---|
| The bulk target's preview (`tags/target`, `tagTarget`), which only bulk tagging uses | `merge` |
| Marking persons distinct, putting a tag on a record and taking it off | `edit` |
| Dropping and rebuilding the projections (`profiles`, `profiles/rebuild`) | `settings_manage` |
| An import or export job's status, cancellation and download | The job's `import` or `export` |
| A tree's members, a member's update and removal | `read`, which the service narrows (§10) |

### 3.3 Scoped members

A membership may carry a scope (§6.5): the member then sees and acts on the
scope's persons only. Scoping a Guest is plain. A scoped Editor follows two
more rules, because some records are shared by the whole tree:

- every person a write names — the one written, and every one it links to —
  must be in the scope; a new person is created only linked to one already in
  it (a parent, spouse or child), never standing alone, which would place it
  outside;
- places, sources, repositories and tags are read and linked, not edited,
  merged or deleted, since a change would reach records outside the scope; a
  scoped Editor may create new ones. Dictionary bulk operations and bulk
  tagging are refused (`403 forbidden`), and so is restoring a version.

The rules are checked once, before the operation runs, for REST and
GraphQL alike (`authz::scoped`): every identifier the write carries — its
path, its JSON body, its GraphQL arguments — that is a person, a family, an
event, a name, a witness, a note, a citation, a medium, a media link or a
vignette of the tree names the persons it is or belongs to, and each must be
in the scope (`404 not_found` otherwise, as a person outside it is to a
read). A person without any family link yet passes: it is the one the
Editor just created, standing alone only until it is linked to a person of
the scope, which brings it in at the next read.

### 3.4 Why roles alone are not enough

A role answers *what may this viewer do*. Three questions it cannot answer
decide *which records* they see:

- **The tree's shape.** "The descendants of this ancestor" is a set defined by
  family links, which grows when a child is added. That is a relationship
  rule, evaluated over the graph — materialized here (§6.5) and kept current
  by the triggers that already keep the SOSA ancestry current
  ([Data Model §4.8](data-model.md#48-sosa-ancestry-sosa_ancestor)).
- **The records' attributes and the calendar.** Whether a person is
  contemporary depends on their dates, their relatives' dates and the current
  year ([Privacy §4](privacy.md#4-contemporary-persons)); a person turns
  public without anyone touching the tree.
- **The owner's exceptions.** An exclusion withholds a person or a subtree
  from every role but the owner's, whatever the share.

So a viewer's access to a tree is a role (role-based), intersected with a
scope (relationship-based), minus exclusions, with redaction by attributes
for Visitors (attribute-based) — decided in one module, never per surface.

---

## 4. ViewerContext

### 4.1 Resolution

The authentication layer ([Authentication §4.8](authentication.md#48-one-session-for-rest-graphql-and-media))
builds the request's identity from a validated token: issuer, subject, user,
client id, scopes, session, API client
([Authentication §5.4](authentication.md#54-the-identity-a-request-carries)).
It loads the user row (one indexed read; a deactivated user is
`401 unauthenticated`). The tree part is resolved lazily, the first time a
request names a tree, and kept in the request for any later field naming the
same tree — a GraphQL document reading several trees resolves each once:

1. the membership of the user on the tree, with its role, scope and expiry
   (an expired membership counts as none);
2. otherwise a share-link grant held by the session (§6.3);
3. otherwise the tree's visibility: `public` makes anyone a Visitor, `site`
   any signed-in user (§6.4);
4. for an API client, its own role or its owner's, intersected with its
   scopes and the token's (§7.1).

The local owner of the desktop and of the `none` mode holds the owner's
access to every live tree, without a membership. A membership outranks
every Visitor's grant; among a Visitor's grants — a share link's and the
visibility's — the one without a scope wins. The share links a browser
holds are checked against their rows at every request, so a revoked or
expired one grants nothing at once. A bearer token carries no share link:
links are for people's browsers. API clients join the resolution with their
phase.

The result, `TreeAccess`, holds the role, the permission set, the scope id,
whether exclusions apply (always, unless the viewer is the owner or acts for
the owner), and the redaction policy for a Visitor. It never outlives the
request and is never shared between viewers. Its public part is returned on
every tree read as `viewer_access: { role, permissions, scoped }` (GraphQL
`viewerAccess`), so the frontend knows what to offer.

### 4.2 Propagation

`ViewerContext` lives in `oxidgene-core` (types) and is resolved in
`oxidgene-api`: REST's authorization middleware, after routing, builds it
from the principal the authentication layer resolved, checks the route's
requirement and hands it to the handler; the GraphQL handler puts it in the
request's data, an extension checks each root field's requirement and the
resolver of a tree-scoped field checks the tree it names; the stdio MCP
server and the desktop act for the local owner; a job acts for the actor it
recorded. Every service that writes takes it first and attributes its audit
entry to it. In phase 2 a viewer's access depends on the tree alone, so it is
decided once, before the operation runs, and the read services stay
viewer-independent; they take the context with the filters that narrow a
read — scopes, exclusions, redaction — in phases 3 and 5.

- **Writes** call `require(permission)` before opening their transaction,
  check every person they name against the scope and the exclusions, and
  attribute their audit entry to the viewer (§9.1).
- **Reads** pass a visibility filter to the repositories: the scope's
  materialized persons, the exclusions, and for Visitors the redaction
  inputs ([Privacy §7](privacy.md#7-enforcement-points)), as SQL predicates,
  so pagination, counts and ranking see only what the viewer may see.
- **Projections** (`person_denorm`, pedigrees, search rows) stay
  viewer-independent and are redacted after reading, before serialization, by
  one pure function of the record and the `TreeAccess`.
- **GraphQL nested fields** (a person's families, a family's children, an
  event's witnesses, a source's citations) resolve through the same services
  with the request's context: a nested list never shows what the root query
  would have refused.
- **MCP tools**, statistics, kinship, the dictionary, suggestions, exports and
  jobs all take the same context; jobs take the actor recorded at submission
  ([Authentication §5.5](authentication.md#55-background-jobs)).

### 4.3 Errors: 401, 403 and 404

| Situation | REST | GraphQL |
|---|---|---|
| An invalid, expired or revoked credential (cookie or bearer) | `401 unauthenticated`, before any handler; `WWW-Authenticate: Bearer` for a bearer token | HTTP `401` with the same envelope, before execution |
| No credential, on a tree that is not `public` — whether it exists or not | `401 unauthenticated` | `UNAUTHENTICATED` |
| A signed-in viewer on a tree they cannot see at all — whether it exists or not | `404 not_found` | `NOT_FOUND` (`null` for a singular query) |
| A viewer who sees the tree but lacks the permission | `403 forbidden` | `FORBIDDEN` |
| A record outside the viewer's scope, excluded, or hidden in the `hidden` display mode | `404 not_found` | `null` / `NOT_FOUND` |
| An API client's token lacking the scope | `403 forbidden`, `WWW-Authenticate: Bearer error="insufficient_scope", scope="…"` | `FORBIDDEN` |
| Over a rate limit | `429 rate_limited`, `Retry-After` | `RATE_LIMITED` |

An anonymous visitor cannot tell a private tree from a missing one, nor can a
signed-in viewer; only a viewer who sees the tree learns which actions are
refused. The envelope and codes are those of
[Cross-cutting Rules §4](cross-cutting.md#4-error-contract), which gains
`RATE_LIMITED`, `UNAUTHENTICATED` and `FORBIDDEN` in its GraphQL list.

---

## 5. Memberships and ownership

A membership ([Data Model §6.2](data-model.md#62-tree_membership)) holds a
role (`owner`, `editor` or `guest`), an optional scope, an optional expiry
and the member's own "Who am I?" person. A tree a signed-in user creates or
duplicates gets them as its owner; one the local owner creates has none.

- **One owner.** A unique index keeps one `owner` membership per tree.
  `POST /trees/{tree_id}/ownership` hands it to another member, who becomes
  owner while the former owner becomes an Editor, in one transaction; the
  new owner's membership loses any expiry. A user who is no member is
  `404`, a deactivated one `409`.
- **Ownerless trees.** An administrator lists every tree with its owner and
  size (`GET /admin/trees`) and gives an ownerless one an owner
  (`PUT /admin/trees/{tree_id}/owner`): a member is promoted, anyone else
  becomes its owner; a tree that has an owner is `409`.
- **Members.** The owner lists the members and changes a member's role,
  between Editor and Guest, and expiry; a member lists their own row and
  sets their own "Who am I?" person; another field is `403`.
- **Leaving.** An Editor or Guest may leave. The owner cannot (`409`): they
  transfer the tree or delete it. Erasing an account that owns a tree is
  refused alike ([Authentication §6.6](authentication.md#66-erasure-and-account-data)).
- **Expiry.** A membership past its expiry grants nothing and is deleted by
  the worker's maintenance pass, with a `membership_expired` access event.
- **"Who am I?"** becomes per member: each member marks the person they are
  in the tree, which drives their badge on the pedigree and the default
  submitter of their exports. `tree.self_person_id` remains the owner's.
- **Changes** to memberships, invitations, share links, exclusions and API
  clients are audited in the tree's history under the `access` category
  (§9.1) and notify the member concerned
  ([Messaging §2](messaging.md#2-notifications)).

---

## 6. Sharing

Only the owner shares. Every share names a role — Editor or Guest for people,
Visitor for links — and optionally a scope (§6.5). Exclusions win over every
share ([Privacy §6](privacy.md#6-exclusions-from-sharing)).

### 6.1 Sharing with a user of the instance

The owner shares the tree, or a scope of it, with someone who already has an
account. There is no user directory: the owner names the person by

- their **exact email**, compared with the verified addresses of accounts;
- their **exact username** (`preferred_username`); or
- picking them among the **people they already share with** — the members of
  the owner's other trees, and the people they message.

A match creates the membership at once; the user gets an in-app notification
and an email when they allow it ([Messaging §2](messaging.md#2-notifications)),
and the tree appears in their home under *Shared with me*. They may leave it.
An email that matches no account becomes an email invitation (§6.2); a
username that matches none answers `404 not_found`.

The lookup tells the owner whether an address they already know has an
account, as any invitation system does; it is limited to 30 lookups per hour
per user ([Authentication §9.2](authentication.md#92-rate-limits-and-brute-force))
and recorded in the access log, so it cannot be used to sweep addresses.
The limit is counted by each server process; picking among the people the
owner already shares with (`user_id`) is no lookup and is not counted.

### 6.2 Invitations by email

For people without an account. An invitation ([Data Model §6.3](data-model.md#63-tree_invitation))
holds the normalized address, the role, the scope, the membership's expiry if
any, and its own expiry (14 days by default, at most 90).

- **Delivery.** With SMTP configured, OxidGene mails it
  ([Messaging §3](messaging.md#3-email)); without, the owner copies the
  invitation link and sends it themselves. The link,
  `{public_url}/invitations`, carries no secret.
- **Sign-up.** The invitation opens the signup gate for that address
  ([Authentication §6.2](authentication.md#62-provisioning)).
- **Acceptance.** Signed in with a verified address equal to the invitation's,
  the user sees it on their home and accepts or declines it. Acceptance
  creates the membership; an address that does not match accepts nothing,
  whoever holds the link. A pending invitation is listed, revocable and
  re-sendable by the owner.
- An expired invitation is deleted by the worker's maintenance pass.

### 6.3 Share links

A share link ([Data Model §6.4](data-model.md#64-share_link)) gives Visitor
access to a tree without an account — to a relative without one, or for a
private tree whose owner does not want it `site` or `public`.

- The token is 256 random bits, base64url, prefixed `ogs_` so secret
  scanners recognize it; only its SHA-256 is stored, which suffices for a
  value of that entropy. It is shown once, as
  `{public_url}/auth/share/{token}`.
- Opening it adds the link's grant to the browser's share grants — the
  `__Host-oxidgene-share` cookie, `HttpOnly`, `SameSite=Strict`, its value's
  digest keying the grants in the session store beside the sessions, so a
  browser holds them whether anyone is signed in or not — and redirects to
  the tree, so the token leaves the address bar at once. A signed-in user
  keeps their own access and gains the link's where it is higher. Signing
  out drops the grants too, so a shared computer keeps none.
- A link has a label, an optional scope, an optional expiry, and counts its
  uses and last use. Revoking it ends every grant made from it at the next
  request. The session holding the grant expires with it, at most at the
  link's expiry: the grants last the session's absolute lifetime at most,
  and no longer than the latest expiry of the links they hold.
- A link grants Visitor access only: a holder is not identified, so
  contemporaries stay hidden. A link granting Guest access (open decision 4)
  and a link password (open decision 5) are not offered.

### 6.4 Tree visibility

| Visibility | Who is a Visitor |
|---|---|
| `private` (default) | Nobody but link holders |
| `site` | Every signed-in user of the instance, and link holders |
| `public` | Everyone on the web, signed in or not |

A visibility may carry a scope, the **visitor scope** — for instance the
ancestors of the SOSA root, so visitors see only the direct lineage. Records
nobody classified are public, so a `site` or `public` tree shows Visitors
every one of them except living persons, private records and exclusions
([Privacy §3.1](privacy.md#31-precedence)); the settings page says so, with the
number of persons Visitors will see, before saving. A `public` tree is reached by its
address; there is no directory of public trees (open decision 6), and
`GET /trees` lists a viewer's memberships, and the trees opened through a
link, only.

### 6.5 Access scopes

A scope ([Data Model §6.5](data-model.md#65-access_scope-and-access_scope_person))
names the persons a membership, an invitation, a share link, a visitor scope
or an API client is limited to. Its definition reuses the bulk targets of the
API ([API Contract](api.md#tags), Bulk targets):

| Kind | Persons |
|---|---|
| `ancestors_of` | A root person and their ancestors, to `depth` generations or all of them |
| `descendants_of` | A root person and their descendants, to `depth` generations or all |
| `ancestors_and_descendants_of` | Both, as one set (an hourglass) |

with `include_self` (default `true` for a scope), `include_spouses` (the
spouses of the persons the walk reaches, not the root's own, as the bulk
targets name them) and `collateral` (the siblings of the persons reached,
the root included, and the siblings' spouses). An unbounded depth stops at the traversal's 64
generations ([Data Model §1](data-model.md#ancestry-traversal-no-table)).
Tag-defined scopes are not offered: an Editor tagging a person would widen
what a share shows (open decision 7).

- **Materialized.** The persons of a scope are stored in
  `access_scope_person`, so filtering any list is an indexed join, not a
  walk per request.
- **Kept current.** The scope records the tree's ancestry generation it was
  built from. The triggers that move `sosa_state.generation` on every change
  to a family link, a family or a person's deletion
  ([Data Model §4.8](data-model.md#48-sosa-ancestry-sosa_ancestor)) also
  mark scopes stale; the first read that needs a stale scope rebuilds it on
  the writer, in one transaction: one read of the tree's live persons and
  one of its family links, the walk in memory as an exclusion's union is
  built, then the rows replaced. A child added under a shared ancestor
  enters the share at the next read. A scope belongs to the one membership,
  invitation, share link or visibility holding it: replaced, it is deleted
  once its holder has the new one, and deleting a scope ends what it
  limited rather than widening it; the worker's maintenance pass deletes the
  scopes nothing holds any more.
- **Root changes.** A merge that absorbs a scope's root moves the scope to the
  person kept; a deleted root leaves the scope empty until it is restored, and
  the owner's sharing page flags it.
- **Preview.** `POST /trees/{tree_id}/access-scopes/preview` returns the
  number of persons a definition reaches, as the bulk-tagging target does,
  how many of them a Visitor sees — neither excluded, private nor
  contemporary — and how many are excluded; without a definition, the
  whole tree's, which the visibility settings announce before saving.

**What respects a scope.** Every read: person, family, event and media reads,
profiles, pedigrees and every other chart, search, recently modified persons,
statistics, kinship, the dictionary and its usages, value suggestions, tags'
records, history entries and versions, exports, MCP tools, and links inside
messages. Families are seen when one of their spouses is in the scope; events,
notes, citations and media through the records they belong to; places,
sources and repositories through the records citing or using them.

**The boundary.** A person outside the scope who is a parent, spouse or child
of a person inside appears as a *Not shared* stub — no name, no dates, no
identifier, no navigation — so a chart keeps its shape; anyone further out
does not appear (open decision 8).

---

## 7. API clients

### 7.1 Design

Programs use the OAuth 2.0 client credentials grant against the identity
provider; OxidGene keeps the authority. An API client
([Data Model §6.6](data-model.md#66-api_client-and-api_client_credential)) is
one of two kinds:

| Kind | Created by | Acts as | Effective rights |
|---|---|---|---|
| Tree client | The tree's owner | The tree's own client, Guest or Editor, optionally scoped | Its role ∩ its scopes ∩ the token's scopes |
| User client | Any user | Its user, on all their trees or a listed subset | The user's current rights on each tree ∩ its scopes ∩ the token's scopes |

A user client loses what its user loses — a membership removed, an account
deactivated — at the next request. A tree client never reaches `read_excluded`
or the owner's permissions; a user client of the owner does, because it acts
as the owner.

### 7.2 Scopes

| Scope | Grants, within the role |
|---|---|
| `tree:read` | `read`, `read_private`, `read_confidential`, `read_tags`, `tools` |
| `tree:write` | `create`, `edit`, `privacy_edit`, `delete`, `merge`, `history_restore` |
| `media:read` | Direct media reads |
| `media:write` | Media uploads |
| `history:read` | `history_read` |
| `import` | `import` |
| `export` | `export` |
| `tree:admin` | `settings_manage`, `members_manage`, `exclusions_manage`, `api_clients_manage`, `access_log_read` — user clients only |
| `instance:admin` | The administrator's operations — user clients of an administrator only |

The scopes stored on the client are authoritative: a token carrying more is
cut down to them. An operation's required scope is declared beside its
permission in the parity table.

### 7.3 Provisioning

Creating an API client creates a credential at the provider, by the first of
these the deployment supports (`OXIDGENE_API_CLIENT_PROVISIONING`):

| Mode | How | Providers |
|---|---|---|
| `keycloak-admin` | An `external` Keycloak's admin REST API, through a confidential client `oxidgene-admin` its administrator creates, whose service account holds `manage-clients` and `view-clients` of `realm-management` — a role that reaches every client of the realm, which is therefore dedicated to OxidGene: a confidential client `oxidgene-api-<id>`, service account on, every other flow off, its scopes as optional client scopes, the `oxidgene-api` audience | Keycloak |
| `dcr` | RFC 7591 registration with an initial access token (`OXIDGENE_OIDC_REGISTRATION_TOKEN`), `grant_types: [client_credentials]`; RFC 7592 to delete it, its registration access token stored encrypted | Rauthy (its dynamic client registration, `dynamic_clients.enable` and its registration token), Keycloak, Auth0, Okta and others |
| `manual` | The creator registers a client in their provider, with the scopes and audience OxidGene shows, and enters its client id; the secret never passes through OxidGene | Every provider with client credentials |

The secret, when OxidGene provisioned it, is returned once with the client id
and the token endpoint, and never stored by OxidGene.

### 7.4 Credentials, rotation and revocation

- **Credentials.** A client has one or more credentials, each a client
  registration at the provider. Rotation creates a new one and gives the old
  one an end, seven days later by default (at most thirty): both work during
  the overlap, whatever the provider supports, then the worker deprovisions
  the old one.
- **Revocation** of a credential or a client is immediate: every request maps
  the token's client id to its row, and a revoked or expired row is
  `401 unauthenticated` at once. Tokens are short-lived (five minutes for the
  clients the chart bootstraps); deprovisioning at the provider follows, retried by the
  worker until it succeeds.
- **Expiry.** A client may have an expiry, a year at most by default.
- Opaque tokens are introspected ([Authentication §5.3](authentication.md#53-opaque-tokens)).

### 7.5 Rate limits and usage

Each client is limited to 600 requests a minute with bursts of 100, by
default (`429 rate_limited`). Every request of an API client is an access
event (§9.2) with its route template or GraphQL root fields and its status,
and every write it makes names it in the history (§9.1). Its last use is shown
beside it.

### 7.6 Personal access tokens compared

| | Client credentials (chosen) | OxidGene-issued personal access tokens |
|---|---|---|
| Secret held by | The provider | OxidGene (hashed) |
| Protocol | Standard OAuth; any OAuth library | A bearer string specific to OxidGene |
| Revocation | Immediate, through OxidGene's row | Immediate |
| Rotation with overlap | Through a second credential | Through a second token |
| Works with every provider | No: Google and some others offer no client credentials; `manual` needs a provider admin | Yes |
| Fits "OxidGene holds no credential" | Yes | No |

Client credentials keep OxidGene out of the credential business, as for
users. **OxidGene issues no personal access tokens**: a program always
authenticates as an API client with its client id and client secret, and
uses the short-lived access token the provider's token endpoint returns for
them. A deployment whose provider cannot issue client credentials offers no
API clients.

---

## 8. Assistant access over HTTP (MCP)

The MCP server's network transport ([Assistant Access §10](mcp.md#10-later-phases))
is a web-build feature authorized as the MCP specification's authorization
part requires:

- **Endpoint.** Streamable HTTP on `/mcp`, bearer tokens only — never the
  session cookie, so a page cannot drive it — with an `Origin` that, when
  present, must be the public origin.
- **Protected resource metadata** (RFC 9728) at
  `/.well-known/oauth-protected-resource/mcp` (and the root form): `resource`
  `{public_url}/mcp`, `authorization_servers` the issuer, the scopes of §7.2,
  `bearer_methods_supported: ["header"]`. A request without a valid token
  answers `401` with
  `WWW-Authenticate: Bearer resource_metadata="{public_url}/.well-known/oauth-protected-resource/mcp"`.
- **Tokens.** The assistant obtains a user token through its own registration
  (`oxidgene-mcp`, Authorization Code with PKCE and the provider's consent
  screen), asking for the resource with RFC 8707's `resource` parameter where
  the provider honours it, otherwise through an audience the provider adds
  to the client's tokens (Rauthy's `default_aud`, Keycloak's audience
  mapper); the provider's client registration or client metadata
  documents serve assistants that register themselves. `/mcp` accepts only a
  token whose `aud` contains `{public_url}/mcp` and whose client is trusted
  ([Authentication §5.1](authentication.md#51-accepted-credentials)); OxidGene
  never passes a token on.
- **Rights.** The token's user's `ViewerContext`, cut down by `tree:read`
  (read tools) and `tree:write` (mutating tools); `list_trees` returns the
  trees the user may read; every tool keeps its required `tree_id` and applies
  scope, exclusions and redaction as REST does. A token lacking the scope
  answers `403` with `insufficient_scope`.
- **Logging and audit** as for REST: tool name and outcome in logs, the
  client id in the access log and in the history of every write.

The desktop's stdio server is unchanged.

---

## 9. Audit

### 9.1 Authors in the change history

Every audit entry records its author ([Data Model §6.10](data-model.md#610-changes-to-existing-tables)):
`author_kind` (`user`, `api_client`, `local`), the user, the API client, and
the OAuth client id the write came through — the web client, an assistant,
an API client. A job's entries carry the actor recorded at submission
([Authentication §5.5](authentication.md#55-background-jobs)). `AuditEntry`
gains `author: { kind, user_id, display_name, client_id, api_client_id }` on
both surfaces, the display name read when the log is read, so an erased user
shows as *Former member* ([Authentication §6.6](authentication.md#66-erasure-and-account-data)).

The history gains the `access` category: memberships, invitations, share
links, visibility, exclusions and API clients, created, changed or removed.
These writes store no version. A membership change is entity `membership`,
about the tree, an exclusion `privacy_exclusion`, about its root; like every
write they name their author, and the desktop and the `none` mode record
author `local`.

### 9.2 Access log

`access_event` ([Data Model §6.7](data-model.md#67-access_event)) records what
the history does not: sign-ins and their failures, sign-outs and session ends
with their cause, back-channel logouts, refused requests (`401`, `403`, and
the `404`s the policy produced), share-link redemptions, invitations
accepted, lookups by email or username, API-client requests, user
deactivations and erasures, and administrator actions. Each row has the
principal's kind and ids, the client id, the tree, the route template or
GraphQL root fields, the permission refused, the outcome code and the request
id — never an email, a name, a token, a search term or an address.
Administrators read the instance's log, owners their tree's. Rows older than
`OXIDGENE_ACCESS_LOG_RETENTION` are deleted by the worker's maintenance pass.
An event is written in a task of its own, so a refusal is answered without
waiting for the database. Phase 2 records refusals (`request_refused`, with
the requirement refused), sign-ins (`login_succeeded`, `login_failed` with
its reason), sign-outs (`logout`, `logout_everywhere`), back-channel
logouts, expired memberships (`membership_expired`) and administrator
actions (`tree_owner_assigned`); the local owner, never refused, records
none.

---

## 10. API

Each operation moves into [API Contract §2](api.md#2-rest-api) and the
parity table when it is implemented; `/me`, `/me/export` and the
`/admin/users` operations have, with the profile operations of
[Authentication §6.7](authentication.md#67-profile-and-avatar)
([API Contract, Accounts](api.md#accounts)). All take the `/api/v1` prefix.

| Method | Path | GraphQL | Permission |
|---|---|---|---|
| `GET` | `/me` | `me` | Signed in |
| `DELETE` | `/me` | `deleteMyAccount` | Signed in; `409` while owning a tree |
| `GET` | `/me/export` | `myAccountData` | Signed in |
| `GET` | `/me/invitations` | `myInvitations` | Signed in |
| `POST` | `/me/invitations/{invitation_id}/accept` | `acceptInvitation` | Matching verified address |
| `POST` | `/me/invitations/{invitation_id}/decline` | `declineInvitation` | Matching verified address |
| `GET` | `/trees/{tree_id}/members` | `treeMembers` | `members_manage`; a member reads their own row |
| `POST` | `/trees/{tree_id}/members` | `shareTree` | `members_manage`. Body `{ email \| username \| user_id, role, scope?, expires_at? }`: a membership (`201`) or, for an unknown email, an invitation (`202`) |
| `PUT` | `/trees/{tree_id}/members/{user_id}` | `updateTreeMember` | `members_manage`; or the member, for their own "Who am I?" |
| `DELETE` | `/trees/{tree_id}/members/{user_id}` | `removeTreeMember` | `members_manage`, or `leave` for oneself |
| `POST` | `/trees/{tree_id}/ownership` | `transferTreeOwnership` | `ownership_transfer` |
| `GET` | `/trees/{tree_id}/share-candidates` | `shareCandidates` | `members_manage`: the people the owner already shares with (§6.1) |
| `GET` | `/trees/{tree_id}/invitations` | `treeInvitations` | `members_manage` |
| `POST` | `/trees/{tree_id}/invitations/{invitation_id}/resend` | `resendTreeInvitation` | `members_manage` |
| `DELETE` | `/trees/{tree_id}/invitations/{invitation_id}` | `revokeTreeInvitation` | `members_manage` |
| `GET` | `/trees/{tree_id}/share-links` | `shareLinks` | `members_manage` |
| `POST` | `/trees/{tree_id}/share-links` | `createShareLink` | `members_manage`; the URL is returned once |
| `PUT` | `/trees/{tree_id}/share-links/{link_id}` | `updateShareLink` | `members_manage` |
| `DELETE` | `/trees/{tree_id}/share-links/{link_id}` | `revokeShareLink` | `members_manage` |
| `POST` | `/trees/{tree_id}/access-scopes/preview` | `accessScopePreview` | `members_manage` |
| `GET` | `/trees/{tree_id}/visitor-scope` | `visitorScope` | `settings_manage`: the visitor scope's definition, whose root Visitors may not see |
| `GET` | `/trees/{tree_id}/exclusions` | `exclusions` | `exclusions_manage` ([Privacy §6](privacy.md#6-exclusions-from-sharing)) |
| `POST` | `/trees/{tree_id}/exclusions` | `createExclusions` | `exclusions_manage` |
| `DELETE` | `/trees/{tree_id}/exclusions/{exclusion_id}` | `deleteExclusion` | `exclusions_manage` |
| `GET` | `/trees/{tree_id}/access-log` | `treeAccessEvents` | `access_log_read` |
| `GET` | `/trees/{tree_id}/api-clients` | `treeApiClients` | `api_clients_manage` |
| `POST` | `/trees/{tree_id}/api-clients` | `createTreeApiClient` | `api_clients_manage` |
| `GET` | `/api-clients` | `apiClients` | Signed in: the user's own clients |
| `POST` | `/api-clients` | `createApiClient` | Signed in |
| `GET` | `/api-clients/{client_id}` | `apiClient` | Its owner (the user, or the tree's owner) |
| `PUT` | `/api-clients/{client_id}` | `updateApiClient` | Its owner |
| `DELETE` | `/api-clients/{client_id}` | `deleteApiClient` | Its owner |
| `POST` | `/api-clients/{client_id}/credentials` | `rotateApiClientCredential` | Its owner |
| `DELETE` | `/api-clients/{client_id}/credentials/{credential_id}` | `revokeApiClientCredential` | Its owner |
| `GET` | `/api-clients/{client_id}/usage` | `apiClientUsage` | Its owner |
| `GET` | `/admin/users` | `users` | Administrator |
| `GET` | `/admin/users/{user_id}` | `user` | Administrator |
| `PUT` | `/admin/users/{user_id}` | `updateUser` | Administrator: role, tree creation, status |
| `PUT` | `/admin/users/{user_id}/identity` | `rebindUserIdentity` | Administrator |
| `DELETE` | `/admin/users/{user_id}` | `eraseUser` | Administrator |
| `GET` | `/admin/trees` | `adminTrees` | Administrator: metadata only |
| `PUT` | `/admin/trees/{tree_id}/owner` | `assignTreeOwner` | Administrator, ownerless trees only |
| `GET` | `/admin/access-log` | `accessEvents` | Administrator |

Tree visibility, its visitor scope and the privacy settings are fields of the
existing tree update ([Privacy §9](privacy.md#9-tree-settings)). Lists page by
cursor, except `share-candidates` (at most 50). Created secrets and links are
returned once and never readable again. `/auth/*` and
`/.well-known/oauth-protected-resource` are transports of
[Authentication §4.2](authentication.md#42-endpoints) and §8.

---

## 11. User interface

Outline of the web build's surfaces; each is translated in every interface
language and absent from the desktop.

- **Homepage.** *My trees* and *Shared with me* filters; each card shows the
  viewer's role badge (Owner, Editor, Guest, Visitor); pending invitations
  above the cards, each with **Accept** and **Decline**; **New tree** only
  with tree creation.
- **Read-only views.** Edit controls are absent where `viewer_access` lacks
  their permission; a Visitor's tree view carries a banner — *You are viewing
  this tree as a visitor* — with **Sign in** when anonymous.
- **Tree Settings, owner only** ([Settings §4](ui-settings.md#4-left-navigation)):
  **Members** (members with role, scope, expiry, last change; **Share** by
  email, username or among the people they already share with; pending
  invitations with resend and revoke; transfer ownership); **Sharing**
  (visibility with its visitor scope, share links with copy, expiry, revoke
  and use count; a scope editor with the person picker, sides, generations,
  spouses and collateral, and a live count from the preview); **Exclusions**
  ([Privacy §6](privacy.md#6-exclusions-from-sharing)); **API clients** (create
  with name, role, scopes, scope and expiry; the secret shown once; rotate,
  revoke, usage); **Access log**. Members other than the owner see a
  **Leave this tree** button in Tree & Roots.
- **Profile page** ([Authentication §10](authentication.md#10-user-interface)):
  the user's own API clients among its sections. Names and avatars of other
  users appear only to those who may see them
  ([Authentication §6.7](authentication.md#67-profile-and-avatar)).
- **App Settings.** An **Instance** section for administrators: users, trees,
  the access log and messaging reports.

---

## 12. Tests

- **Policy unit tests.** The permission matrix as a table test, role by
  permission; scope and exclusion combinations; scoped-editor write rules.
- **Role × operation matrix.** For every row of the parity table, each
  viewer — owner, editor, guest, visitor, anonymous on a private and on a
  public tree, an outsider, a scoped member, a tree client, a user client
  with a missing scope — calls REST and GraphQL and gets the same outcome:
  success, `401`, `403` or `404`. A route without a declared permission fails
  the guard.
- **Leak tests.** A fictitious canary name on an excluded person, one on a
  contemporary, and one outside a share's scope; every read route and GraphQL
  query, as each restricted viewer, must never return the canaries — in
  bodies, counts, search, statistics, suggestions, exports and MCP results.
- **Scopes.** Materialization against the bulk-target walk; staleness after
  link changes; a merge of the root; preview counts.
- **API clients** against the mock provider's client credentials, DCR and
  admin endpoints: provisioning, rotation overlap, immediate revocation, rate
  limits, usage events.
- **MCP over HTTP:** metadata document, `401` with `resource_metadata`,
  audience checks, insufficient scope, per-viewer `list_trees`.
- **Audit:** every write names its author and client on both surfaces and
  from a job.

---

## 13. Open decisions

1. **A co-owner role.** Recommended: none — one owner manages sharing, which
   keeps the person accountable for publishing living relatives unique.
   Alternative: a `manager` role with the owner's permissions but ownership
   transfer and deletion, for associations running a tree together.
2. **A contributor role.** Recommended: none — Editor and Guest cover Geneanet's
   levels. Alternative: a contributor who adds records and media without
   deleting or merging, or whose changes wait for an Editor's review in a
   proposals queue, a feature of its own.
3. **Editors and excluded records.** Recommended: Editors do not see them; an
   exclusion is the owner's alone. Alternative: Editors see and edit them but
   cannot share them, which helps a co-maintained tree and weakens the
   exclusion to a sharing rule.
4. **Share links granting Guest access.** Recommended: no — an unidentified
   holder never sees contemporaries. Alternative: allow it, behind a warning.
5. **Share-link passwords.** Recommended: none; the link is the secret, with
   expiry and revocation. Alternative: an optional password, entered on a
   page before redemption, limited by the rate limit.
6. **A directory of public trees.** Recommended: none; a public tree is
   reached by its address. Alternative: an instance directory, opt-in per
   tree.
7. **Tag-defined scopes.** Recommended: not offered. Alternative: offer them,
   with tags on persons limited to the owner while a tag defines a scope.
8. **Out-of-scope relatives.** Recommended: *Not shared* stubs at the
   boundary. Alternative: absent, which hides even that a shared person has
   parents outside the share.
9. **Scope evaluation.** Recommended: materialized rows rebuilt when stale.
    Alternative: a recursive query per request, simpler to keep current and
    slower on every read of a large share.
