---
type: "Security Specification"
title: "Privacy and Contemporary Persons"
description: "Planned enforcement of privacy for the viewers of a shared tree: how a record's privacy resolves, the precise contemporary-person rule with its inference from relatives, what a Visitor sees of a hidden person in each display mode, the owner's exclusions of persons and subtrees from every share, the server-side redaction points from projections and search to statistics, exports, MCP and messaging, the tree's privacy settings, GDPR context and prior art, and tests."
tags: [oxidgene, specification, privacy, security, gdpr, export]
generated: { by: human:maintainer, at: 2026-11-02T18:00:00Z }
---

# Privacy and Contemporary Persons

> Part of the [OxidGene Specifications](index.md).
> See also: [Authorization](authorization.md) · [Authentication](authentication.md) ·
> [Tree Settings §8](ui-settings.md#8-section-privacy) · [Data Model](data-model.md) ·
> [API Contract](api.md) · [Cross-cutting Rules](cross-cutting.md)

**Status: partly built.** Records default to public (§3.1); the
contemporary rule (§4) is derived and, in the web build, materialized in
`person_visibility`; the exclusions (§6) are stored and materialized in the
web build; the tree settings (§9) and the export options (§8) work. Every
enforcement point of §7 redacts what the viewer may not see — the owner's
exclusions, confidential notes and tags without the permission, private and
contemporary persons for Visitors and share links' holders, and what lies
outside a viewer's access scope
([Authorization §6.5](authorization.md#65-access-scopes)) — through one
policy built from the viewer's access. Viewer redaction and exclusions are compiled and active only in the
web build; the desktop has one user who sees everything, and keeps today's
behaviour ([Authentication §7](authentication.md#7-web-build-only-the-desktop-is-unchanged)).
The export option that leaves out contemporary and private persons (§8) is
the one part both builds share.

---

## 1. Principles

- **Redaction happens on the server, before serialization.** A hidden value
  never reaches a response, a count, a search match, a cache, an export or a
  log; the frontend never hides what it received.
- **One policy.** The rules below are one module, used by every surface
  through the `ViewerContext` ([Authorization §4](authorization.md#4-viewercontext)).
- **Public by default, the living protected.** A record nobody classified is
  public; the contemporary rule hides living persons from Visitors, and an
  explicit `private` or an exclusion hides more (§3.1).
- **Fail closed where it matters.** A person no record of their own or of a
  relative shows to be long dead is treated as contemporary (§4); a medium
  touching a hidden person is hidden (§3.2).
- **The owner decides.** The owner sees everything; Editors and Guests see
  contemporaries and private records; Visitors see neither; exclusions hide
  from everyone but the owner (§6).

---

## 2. Who is redacted

| Viewer | Private records and contemporaries | Excluded persons (§6) | Confidential notes |
|---|---|---|---|
| Owner | Shown | Shown | Shown |
| Editor | Shown | Not shared | Shown |
| Guest | Shown | Not shared | Withheld |
| Visitor (link holder, `site` or `public` visibility) | Redacted (§5) | Not shared | Withheld |

Each viewer is further limited to their scope
([Authorization §6.5](authorization.md#65-access-scopes)).

---

## 3. Record privacy

`Person`, `Family` and `Media` carry `privacy` (`default`, `public`,
`private`) and the tree `default_privacy` (`public` or `private`)
([Data Model](data-model.md#media), "Privacy"). A `Note` carries
`confidential` ([Data Model](data-model.md#note)), set in the note's editor
where the tree has other viewers and changed with `privacy_edit`.

### 3.1 Precedence

**A record with no explicit privacy is public.** The tree's
`default_privacy` defaults to `public`, so a person, family or medium whose
`privacy` is `default` is public unless the owner sets the whole tree to
`private`. For any viewer but
the owner, the first rule that applies decides:

1. **Exclusion** ([§6](#6-exclusions-from-sharing)): not shared, with any
   role, scope or visibility.
2. **Explicit private**: the record's `private`, or `default` on a tree whose
   `default_privacy` is `private` — hidden from Visitors.
3. **Contemporary rule** ([§4](#4-contemporary-persons)): a contemporary person
   is hidden from Visitors, even one whose own `privacy` is `public`.
4. **Explicit or default public**: shown.

Editors and Guests stop at rule 1: they see private and contemporary records.

### 3.2 Records

For a Visitor:

| Record | Hidden when |
|---|---|
| Person | Rules 1, 2 or 3 of §3.1 apply |
| Family | Rule 1 or 2 applies to it, or one of its spouses is hidden. A hidden family keeps the links between its visible members, so parents and children still connect; its events, notes, citations and media are withheld |
| Medium | Rule 1 or 2 applies to it; or a person it shows — linked to it directly, through a vignette, through one of their events or a family of theirs — is hidden, unless the only reason is that person being contemporary and the tree shows contemporaries' photos; or it shows an excluded person. Pages follow their document |
| Note | `confidential` (withheld from Guests too); or it belongs to a hidden record |
| Event, citation | It belongs to a hidden person or family |
| Source, repository | Only cited or held by hidden records: a source is shown through a visible citation, a repository through a visible source |
| Place | Only used by hidden events and media |
| Tag | Always, for Visitors ([Authorization §3.2](authorization.md#32-permission-matrix), `read_tags`) |

With the tree's default `public`, a Visitor sees every record nobody
classified, living persons excepted; switching it to `private` limits them to
what was published explicitly.

The same rules hide the records of an excluded person from Editors and
Guests (§6.4), and a confidential note from Guests. Where the table leaves a
case open, the records' redaction decides:

- A medium's own persons are those linked to it, those on its vignettes,
  and those of the events linked to it or its vignettes; its families, those
  linked to it and those of its events. A vignette on a page shows that
  person in the whole document, so it decides for the document and every
  page.
- A medium shown although it shows a hidden person — the tree shows
  contemporaries' photos — keeps it unnamed: the link to that person and
  the vignette of them stay hidden.
- A source no citation draws on, and a repository holding no source, stay
  visible: only being cited or held by hidden records hides them. A media
  link to a hidden source is hidden, its medium deciding for itself.
- Tags are content: a record hidden, or a family whose content is withheld,
  carries none and counts in no tag; a viewer without `read_tags` gets an
  empty vocabulary and knows no tag (`404`).
- A medium's usage count counts the records it is linked to that the viewer
  sees.
- A deletion cascades to nothing, so the records of a deleted person — their
  events, notes, citations, media links and vignettes, the media showing
  them — and those of a deleted event outlive them: they are hidden from
  every viewer anything is hidden from, as the records of a hidden person
  are.
- A person shown in full is drawn with no portrait taken from a withheld
  medium or vignette — the chosen one, or the first linked photograph the
  portrait falls back to — on their page, their card, a search row or a
  list of portraits; their media and note counts leave out the links to
  withheld media and the notes the viewer may not read.

---

## 4. Contemporary persons

### 4.1 Definition

With the tree's contemporary rule on (default), a person is **contemporary**
unless one of these holds:

1. a death, burial or cremation is recorded for them, dated or not;
2. their latest possible birth year (§4.2, §4.3) is at most the current year
   minus the tree's threshold (`contemporary_years`, 100 by default, from 50
   to 150).

So a person is contemporary when nothing shows they died and no bound —
from their own dates or their relatives' (§4.3) — shows they were born at
least `contemporary_years` years ago. A person with no usable date whose
descendants place their birth further back than the threshold is therefore
not contemporary; only a person for whom neither their own records nor any
relative's give a bound old enough stays contemporary. The current year is
the server's, in UTC, so a person turns non-contemporary on 1 January without
any write.

### 4.2 The reference birth year

The birth, else the baptism or christening, gives the bound, read with its
qualifier ([Data Model](data-model.md#event)) and the calendar converted to
the Gregorian year:

| Date | Latest possible birth year |
|---|---|
| Exact, `from`, `to` (single bound) | Its year |
| `about`, `estimated`, `calculated` | Its year + 5 |
| `before Y` | Y − 1 |
| `between Y1 and Y2`, `from Y1 to Y2` | Y2 |
| `after Y`, a phrase with no year | No bound |

A range's far end is free text: one whose year no stored date can hold
(outside the range of `date_sort`) is unreadable and bounds nothing, so the rule
stays closed.

### 4.3 Inference from relatives

Every source below gives a *latest possible birth year*; the smallest wins,
and the person is old enough once it is at most the current year minus the
threshold. The margins are deliberately wide: an inference may make a person
older than their own records prove only when no reasonable life fits
otherwise.

**The person's own other events.** A dated event other than birth or
baptism — marriage, occupation, residence, census — at its latest possible
year E (the last year its date allows: a marriage `between 1900 and 1910`
gives 1910, one `after 1900` nothing): born at the latest in E, or E − 14
for their own marriage. The latest, not the earliest, year: the person was
born before the event, which may have happened as late as its date allows,
so only that bound fails closed.

**Descendants.** A descendant at depth d (a child is 1, a grandchild 2…)
whose own latest possible birth year is Y — from their birth, else their
baptism (§4.2), else their own other events as above — places the person's
birth at the latest in **Y − d × G**, where **G = 15 years** is the minimum
generation gap. Fifteen is the youngest age at which parenthood is common
enough to bound a generation from below: an earlier birth occurs, but
rarely, and a whole chain of them almost never, so `d × 15` is a span the
descent certainly took — "certainly before", not an average, which would be
nearer 30. The walk follows live family links down to 10 generations and
uses only descendants' recorded dates, never another estimate. For example, a
person without dates whose grandchild was baptized in 1890 was born at the
latest in 1890 − 2 × 15 = 1860.

**Other relatives**, one link away, from their recorded dates:

| Evidence | Latest possible birth year |
|---|---|
| A parent's birth or baptism in Y | Y + 70 |
| A parent's death or burial in Y | Y + 1 |
| A spouse's birth or baptism in Y | Y + 50 |
| A sibling's birth or baptism in Y | Y + 30 |

**Earliest bounds.** A parent's birth P with a minimum parental age of 15
gives the person's *earliest* possible birth, P + 15. It can never show a
person old; it shows them recent, and is kept beside the latest bound so the
Tools page and the export can say why a person is contemporary.

Only a person for whom none of these yields a latest bound, nor their own
birth or baptism, has no bound at all; they are contemporary (fail closed).

### 4.4 Overrides

`privacy: private` hides a person whatever their dates; `public` does not
turn the rule off for them. A person's `public` shows them on a tree whose
`default_privacy` is `private`, once they are no longer contemporary: a
record cannot publish a living person on its own. The only way to show
contemporaries to Visitors is to turn the rule off for the whole tree (§9),
a deliberate decision with its own confirmation. GEDCOM `RESN confidential` or `privacy`, and
GeneWeb's restricted access, already import as `private`
([API Contract §4](api.md#round-trip-fidelity)).

### 4.5 Stored inputs

The rule needs relatives' dates, which a request listing thousands of persons
cannot gather one by one. `person_visibility`
([Data Model §6.8](data-model.md#68-person_visibility)) stores per person its
`privacy`, `deceased` (rule 1), `latest_birth_year` and `earliest_birth_year`
(§4.2, §4.3), and what refreshing them reads: the bound the person's own
recorded dates give their ancestors, the bound of their own and their close
relatives' dates, and the bounds their descendants give by depth, one per
generation down to ten. It is refreshed in the same transaction as every write changing
a person's privacy or events, the dated events of a parent, child, spouse or
sibling, or the links between them; because descendants count, such a write
also refreshes the rows of the person's ancestors up to 10 generations, one
bounded walk: generation by generation, each parent's bounds by depth are
computed again from their children's stored rows, and the walk goes up only
while they change. Imports and merges rebuild the persons they touch, and a change
to the derivation rebuilds every tree, as the projections are: each row
records the version of the rule it was derived by
(`VISIBILITY_RULE_VERSION`), and the first read of a tree holding none, or
an older one, rebuilds the tree's rows on the writer. Only the web build
writes them; the desktop's export derives the rule from the records it
reads. The threshold
and the year are applied at read time, so changing the threshold rewrites
nothing:
`contemporary = NOT deceased AND (latest_birth_year IS NULL OR latest_birth_year > current_year - contemporary_years)`.

The derivation is a pure function in `oxidgene-core`, which the export of
both builds also applies to the records it reads (§8).

---

## 5. What a redacted viewer sees

A person hidden from a Visitor appears according to the tree's display mode:

| Mode | A hidden person is |
|---|---|
| `stub` (default) | A placeholder keeping the tree's shape: sex, family links, and — with navigation allowed — an identifier; marked `placeholder: private` and labelled *Private* by the frontend in the reader's language |
| `initials` | The stub plus initials of their first given name and surname (`J. D.`), computed on the server |
| `hidden` | Absent: in no list, chart, count or link. A child whose parent is hidden shows one parent; a family with a hidden spouse shows the other alone |

Whatever the mode, a hidden person has no name or alternate name, nickname,
date, place, age, event, occupation, note, citation, source, medium, portrait,
tag, SOSA number, or kinship label. A stub's page shows the stub and its
visible relatives. With navigation off (`hidden_navigation: false`), a stub
has no identifier, so the charts stop there.

Visible persons keep everything, except what names a hidden one: a witness, a
spouse or a parent who is hidden shows as a stub (or nothing, in `hidden`
mode), and relative counts follow the mode — `hidden` counts only visible
children.

---

## 6. Exclusions from sharing

### 6.1 Objects

The owner can make records **not shareable**, whatever the share, the role or
the visibility. An exclusion ([Data Model §6.9](data-model.md#69-privacy_exclusion-and-excluded_person))
is a first-class object:

| Kind | Excludes |
|---|---|
| `person` | One person. Excluding several creates one exclusion per person |
| `ancestors` | The ancestors of a root person, to `depth` generations or all; the root too with `include_root` (default `false`) |
| `descendants` | The descendants of a root person, to `depth` or all; the root too with `include_root` (default `true`); their spouses with `include_spouses` (default `true`) |

They are managed from the person menu (*Exclude from sharing*: this person,
their ancestors, their descendants) and from the tree's privacy settings,
which list them with their root, kind, depth and the number of persons they
cover, and remove them. Creating and removing an exclusion is recorded in the
tree's history (`access` category,
[Authorization §9.1](authorization.md#91-authors-in-the-change-history)). Only
the owner sees exclusions and the records they cover (open decision 1).

### 6.2 Precedence

An exclusion wins over everything but ownership: over a share's scope that
includes the person, over the Editor and Guest roles, over the tree's
visibility, over a person's `public` privacy, and over every API client but
the owner's own. An excluded family member makes the family's events, notes,
citations and media withheld, as a hidden spouse does for Visitors (§3).

### 6.3 How excluded persons appear

Absent, in every display mode, as in the `hidden` mode (§5): a stub of an
excluded person could have no initials, no identifier and no navigation, so
it would be a blank, unreachable card that only says someone is there. A
family with an excluded spouse shows the other alone, and a child of an
excluded parent shows one parent.

### 6.4 Effects

- **Reads** by anyone but the owner: absent from search, recently modified
  persons, statistics, the dictionary's counts and usage lists, value
  suggestions, tags' records and counts, the history (entries about them are
  left out; snapshot labels naming them become stubs), the tools, MCP
  results, and record links in messages, which resolve to nothing
  ([Messaging §5.5](messaging.md#55-what-encryption-changes)). A direct read
  is `404 not_found`.
- **Kinship.** A path through an excluded person is not returned; two persons
  related only through one are shown as unrelated.
- **Writes** by an Editor naming an excluded person are `404 not_found`; one
  touching a family with an excluded member is `403 forbidden`.
- **Exports** (§8): never in an export made by anyone but the owner; the
  owner's own export includes them unless the owner leaves them out.

### 6.5 Kept in sync

The union of a tree's exclusions is materialized in `excluded_person`, rebuilt
like an access scope when the tree's ancestry generation moves
([Authorization §6.5](authorization.md#65-access-scopes)): a child added under
an excluded root is excluded at the next read, and a person linked as a new
ancestor of an excluded root is excluded too. A merge keeps a person excluded
when either record was; a deleted root keeps its exclusion, which applies again
if it is restored, and stays excluded itself meanwhile, so that its records,
which outlive it, stay hidden (§3.2).

---

## 7. Enforcement points

| Point | Rule |
|---|---|
| Person, family, event, media reads | §3, §5, §6 in the service; a hidden record in `hidden` mode, an excluded or out-of-scope one is `404` |
| `person_denorm` profiles and pedigrees | Read viewer-independently, redacted per §5 before serialization; batches redact every node |
| Search (`person_search_fts`) | For a Visitor, the query joins `person_visibility`, `excluded_person` and the scope and drops hidden persons entirely, in every display mode, so a hidden name never matches. Relative columns (spouses, parents) of a returned row are rebuilt from visible relatives; the `spouse_*`, `father_*` and `mother_*` filters match visible relatives only. `total_count` counts returned rows |
| Recently modified persons, homepage cards | Visible persons only |
| Statistics | Computed over the persons the viewer sees, every distribution, list and total alike (open decision 2) |
| Kinship | Endpoints must be visible; paths through excluded or out-of-scope persons are not returned; hidden persons on a path are stubs |
| Dictionary, value and place suggestions | Values and counts from visible records only: a surname borne only by hidden persons is not listed |
| Tools | Owner and Editors only; Editors without excluded persons |
| History | Owner and Editors; Editors without entries about excluded persons, scoped Editors without entries outside their scope |
| Media bytes | The same decision as the medium's metadata, on every direct read |
| MCP | The token's user's context; no MCP-specific rule ([Assistant Access §7.2](mcp.md#72-privacy-values-are-not-applied)) |
| Messaging | Record links resolve under the reader's own access (§6.4) |
| HTTP caching | `no-store` on tree JSON, `private` and `Vary: Cookie, Authorization` on media; nothing cached across viewers ([Authentication §9.1](authentication.md#91-origin-headers-and-caching)) |
| Logs and traces | Counts and categories only, never which person was redacted ([Cross-cutting Rules §5.3](cross-cutting.md#53-sensitive-data)) |

**Decisions for the tree-wide reads.** The dictionary, the suggestions and
the tools count and list the persons shown in full only: a placeholder, in
any display mode, carries no value, so it adds nothing to a count, a usage
list or a descent, and a descent through a hidden person stops there. A
source is hidden when it is cited and none of its citations is visible; a
place when it is used and none of its events and media is visible; a source
or a place nobody uses stays shown, since no hidden record reaches it. In
the history, a snapshot label naming a hidden person is left out rather
than replaced: the frontend renders an unlabelled record as its unknown
placeholder, and the hidden person's identifier is the redaction's
pseudonym, or absent, so nothing else names them.

**Decisions for the person reads.**

- Lists, search, recently modified persons, portraits, ancestors and
  descendants hold the persons shown in full only, in every display mode: a
  placeholder appears where a person names it (a family member, a pedigree
  node, a kinship path), never as a list entry of its own. Paged lists
  leave hidden persons out in SQL, so pages and `total_count` agree.
- A list, a count or a search leaving hidden persons or records out selects
  them by subqueries over `excluded_person`, `access_scope_person` and
  `person_visibility` (the contemporary rule's stored inputs, with the year
  and the threshold bound as values) and over the records' own links, never
  by a list of their identifiers: its bound parameters stay the same few
  however many persons are hidden, under SQLite's 32,766 and PostgreSQL's
  65,535. The person rules are the same as the projections' redaction
  applies in memory. A direct read or a nested list deciding record by
  record reads the hidden records of every kind in one statement whose
  sets are common table expressions, so that a restricted viewer's read
  costs the owner's plus a fixed few statements. The redaction itself
  reads the tree's families, with their privacy and live spouses, in one
  statement over the tree rather than by the identifiers of its families
  or of its excluded persons.
- The redaction and the hidden records are decided once per viewer class
  — the redacting permissions of the access, its scope and the year — and
  kept by the tree cache while no write touches what they read
  ([Architecture §4.1](architecture.md#41-tree-cache)): every request of a
  restricted viewer reads the tree's redaction generation first, and
  builds the redaction again when it has moved, when the year has
  changed, or when the generation cannot be read. Two viewers share one
  only when their classes are equal; nothing a client sends is part of
  the class.
- A placeholder that may not be navigated through carries a pseudonym drawn
  afresh for each request: within one response it names the same
  placeholder everywhere; across requests it links nothing, whether the
  redaction was built for the request or kept from an earlier one — the
  pseudonyms are the one part of it never kept. Its only structure is a
  version-7 time prefix, which says when the request ran.
- Ancestors, descendants and the branch targets of bulk tagging walk the
  family links through the persons who appear to the viewer only, as
  kinship does: a person reached only through an absent one is not
  reached, counted or tagged, and a listed or walked-from hidden person is
  `404 not_found`.
- Search filters on spouses and parents match the relatives the viewer sees:
  the candidates matching the other filters, at most 10,000, are read and
  filtered on their redacted relatives.
- A person's page is `404 not_found` unless the viewer reaches the person
  under its own identifier; a placeholder's page holds its record alone.
  The person record of a placeholder keeps its sex and privacy, not its
  portrait or tags.
- Statistics count the persons shown in full, and the growth chart adds or
  removes none of the hidden ones on any day.
- An access scope is one more input of the same policy: a person outside it
  is absent, or a *Not shared* stub when a parent, spouse or child of a
  person inside, with a request's pseudonym and no navigation. The
  exclusions win over the scope, the scope over the privacy rules, so a
  boundary person never shows more than that stub.

---

## 8. Exports

| Who exports | Persons written |
|---|---|
| Owner, default | Everything, excluded persons included; `private` records written with `RESN confidential` as today |
| Owner, *Leave out excluded persons* | Without them, as for an Editor |
| Owner or Editor, *Apply visitors' privacy rules* | As a Visitor would see the tree (§3–§6) |
| Editor, Guest (`guests_may_export`) | Everything they see: never an excluded person, nothing outside their scope |
| Desktop, *Leave out contemporary and private persons* | Hidden persons per §3 and §4 with the tree's settings, the rest unchanged: the one privacy option of both builds |

A person left out is written according to the display mode: in `hidden` mode
not at all, the links through them dropped as a scoped export drops them
([API Contract](api.md#export-scope)); otherwise as an `INDI` with its sex and
family links, no name (or the initials, in `initials` mode), and `RESN
privacy` in GEDCOM 5.5.1 — information withheld for privacy — or `RESN
CONFIDENTIAL` in GEDCOM 7, which discourages `PRIVACY` for its ambiguity.
Excluded persons are never stubbed with initials. Media of hidden persons stay
out of a GEDZIP, and so does a medium linked to a record left out with them —
an event of theirs, a withheld family or one of its events. The export's audit
entry records which option applied.

An export job, its status and its archive are its author's: since the archive
holds what its author sees — the owner's, everything — nobody else finds it
among the downloadable exports, polls it, cancels it or downloads it (`404`).

Confidential notes are left out of an export by an exporter without
`read_confidential` (a Guest) and by *Apply visitors' privacy rules*; what
the exporter's access hides is applied whatever the request asks, and a
job resolves it again from its actor when it runs. *Apply visitors' privacy
rules* is the tree a viewer without `read_private` sees, limited to the
tree's visitor scope ([Authorization §6.4](authorization.md#64-tree-visibility))
when it has one, as an access scope limits a scoped exporter's export
([Authorization §6.5](authorization.md#65-access-scopes)); it keeps the
tags, which a Visitor does not read but which are no privacy rule. A Guest exports only where `guests_may_export` is on,
the setting being read for a Guest's access only.

---

## 9. Tree settings

New columns of the tree ([Data Model §6.10](data-model.md#610-changes-to-existing-tables)),
edited in [Tree Settings §8](ui-settings.md#8-section-privacy) by the owner,
through the existing tree update on both surfaces:

| Setting | Values | Default | Build |
|---|---|---|---|
| `default_privacy` | `public`, `private` | `public` (§3.1) | Both |
| `contemporary_rule` | on, off | on | Both: exports on the desktop, viewers too on the web |
| `contemporary_years` | 50–150 | 100 | Both |
| `hidden_display` | `stub`, `initials`, `hidden` | `stub` | Both: exports on the desktop |
| `hidden_navigation` | on, off | on | Web |
| `contemporary_photos` | on, off | off | Web |
| `guests_may_export` | on, off | off | Web |
| `visibility`, visitor scope | [Authorization §6.4](authorization.md#64-tree-visibility) | `private`, none | Web |

Turning the contemporary rule off shows contemporaries to Visitors; the page
asks for confirmation and says that living people become visible to whoever
the tree is visible to.

---

## 10. Legal context and prior art

- **GDPR.** Data about deceased persons is outside the regulation (Recital 27);
  data about living persons is personal data. The household exemption
  (Art. 2(2)(c)) covers a purely private family tree, not one published to an
  indefinite audience — the European Court of Justice's *Lindqvist* ruling on
  publishing on the web. OxidGene therefore treats publication of living
  persons as the owner's deliberate act: the records of the dead public by
  default, contemporaries hidden from Visitors whatever their own privacy, unless the
  owner turns the rule off for the whole tree, exclusions for what must never be shared, and erasure of
  accounts ([Authentication §6.6](authentication.md#66-erasure-and-account-data)).
  The tree owner is responsible for the genealogy they publish; OxidGene
  provides the means.
- **GeneWeb** gives each person an access level (public, private,
  semi-public), decides who is old enough to show from configurable year
  thresholds on birth, death and marriage, distinguishes visitors from
  password-holding friends and wizards, and shows a withheld person as a
  placeholder or hides them altogether.
- **Geneanet** treats a person with no recorded death born within about a
  century as living, hides them from visitors unless the owner decides
  otherwise, and shares a tree at three levels — visitor, guest and editor —
  which [Authorization §3.1](authorization.md#31-roles) follows.
- **GEDCOM** marks restrictions with `RESN` (`confidential`, `locked`,
  `privacy`), which OxidGene imports and writes as §4.4 and §8 say.

---

## 11. Tests

- **Rule tables.** §4.2 and §4.3 as table tests: each qualifier, each kind of
  evidence, descendants at several depths with `Y − d × 15`, the
  10-generation limit, the smallest bound winning, an undated person with
  and without any dated relative, a person turning non-contemporary at a year
  boundary with a fixed clock, and the precedence of §3.1.
- **Refresh.** A parent's death dated, a great-grandchild's birth added, a
  link removed: the rows of the person and of their ancestors change in the
  same transaction.
- **Redaction.** Each display mode against a fixture of fictitious families:
  pedigrees keep or lose their shape as specified; no name, date, place,
  note, medium or count of a hidden person in any response.
- **Exclusions.** Each kind, depth and option; precedence over scopes, roles,
  visibility and `public`; a descendant added under an excluded root; merges;
  kinship through an excluded person; Editor writes refused.
- **Leak suite.** The canary persons of [Authorization §12](authorization.md#12-tests)
  — contemporary, private, excluded, out of scope — never appear in any REST,
  GraphQL, MCP or export output for a viewer who may not see them, in
  bodies, counts, ranks, suggestions or statistics. The suite enumerates
  every REST read from the OpenAPI document and every GraphQL query field
  from introspection, so a new route is covered without being listed, and
  runs once per restricted kind of viewer: Editors and Guests today, with
  an excluded person and a confidential note as canaries; Visitors join it
  with one entry.
- **Kept redactions.** A restricted viewer's redaction kept by the tree
  cache, then each of its inputs changed — an exclusion, a scope, a
  person's privacy, a date making someone contemporary or not, the tree's
  privacy settings and visibility, a membership's role, the membership or
  a share link revoked, the year, and a write made by another process:
  the very next request, over REST and GraphQL, answers as if nothing had
  been kept. Two viewer classes never receive each other's redaction, the
  same placeholder carries different pseudonyms in two responses, and the
  owner's requests keep nothing.
- **Exports.** Each row of §8 on both builds, with `RESN` per version, and
  re-import of a redacted export.

---

## 12. Open decisions

1. **Editors and exclusions.** Recommended: only the owner sees excluded
   persons. Alternative: Editors see them but cannot share them
   ([Authorization §13](authorization.md#13-open-decisions), decision 3).
2. **Statistics for redacted viewers.** Recommended: computed over visible
   persons only. Alternative: include hidden persons in totals and in
   distributions that identify nobody, which needs a minimum cell size to stay
   safe on small trees.
3. **The default display mode.** Recommended: `stub`, which keeps charts
   readable. Alternatives: `hidden`, the most discreet; `initials`, the
   friendliest for relatives browsing as Visitors.
4. **The default threshold.** Recommended: 100 years, Geneanet's custom and a
   safe bound for a lifetime. Alternative: a shorter span such as 80 years,
   which publishes more of the twentieth century.
5. **Inference margins.** Recommended: the margins of §4.3, with a
   generation gap of 15 years. Alternative:
   margins configurable per tree, which few owners could set well.
