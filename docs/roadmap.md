---
type: "Roadmap Specification"
title: "Roadmap — Delivery Status and Milestones"
description: "Current delivery status, active priorities, and future milestones for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
generated: { by: human:maintainer, at: 2026-11-02T22:00:00Z }
---

# Roadmap — Delivery Status and Milestones

> Part of the [OxidGene Specifications](index.md). Product behavior belongs in
> its domain specification. This document records status and remaining work.

---

## 1. Policy

- Completed behavior is documented in the relevant product, API, data, or UI
  specification and linked below.
- This roadmap is not a commit-by-commit or day-by-day history.
- Milestones list open outcomes and sequencing constraints only.
- Git history is the authoritative chronological record.

## 2. Current status

| EPIC | Scope | Status | Canonical specifications |
|---|---|---|---|
| A | Foundation, persistence, APIs, server, desktop | Complete | [Architecture](architecture.md), [Data](data-model.md), [API](api.md) |
| B | GEDCOM, GEDZIP, and GeneWeb | Complete | [API](api.md), [Import](ui-import.md) |
| C | Tree browsing and editing, kinship, alternative charts | Complete | [Tree](ui-genealogy-tree.md), [Person](ui-person-profile.md), [Couple](ui-couple-profile.md), [Kinship](ui-kinship.md), [Person Edit](ui-person-edit-modal.md) |
| D | Shared UX, themes, languages, runtime settings, printing | Complete | [Common UI](ui-common.md), [Cross-cutting Rules](cross-cutting.md), [App Settings](ui-app-settings.md), [Settings](ui-settings.md) |
| E | Read projections, search, dictionary | Complete | [Data](data-model.md), [Search](ui-search-results.md), [Dictionary](ui-dictionary.md) |
| F | Media and Geneanet recovery | In progress | [Data](data-model.md), [API](api.md), [Import](ui-import.md), [Geneanet Pipeline](geneanet-media-import.md) |
| G | Security, privacy enforcement, sharing, messaging, deployment (web only) | Planned (§8) | [Authentication](authentication.md), [Authorization](authorization.md), [Privacy](privacy.md), [Messaging](messaging.md), [Architecture](architecture.md), [Settings](ui-settings.md) |
| H | Asynchronous and large-scale processing | Durable import and export jobs delivered; the rest post-MVP (§12) | [Architecture](architecture.md), [API](api.md) |
| I | Assistant access through MCP | First delivery complete; later phases planned | [Assistant Access](mcp.md), [App Settings](ui-app-settings.md) |
| J | Change history: audit log of every tree write, person, place, source and settings versions, side-by-side comparison, restore | Complete; entries record no author until EPIC G | [Data](data-model.md#5-change-history), [API](api.md), [Person History](ui-person-history.md), [Settings](ui-settings.md#9-section-history), [Homepage](ui-home.md) |

## 3. Active: media completion

- [x] Add an S3-compatible object-storage backend while retaining filesystem
  storage as the local default behind `MediaStore`.
- [x] Offer a Geneanet import that stores Geneanet's own renditions, skipping
  the data archives and the byte-length pass they need.
- [x] Reduce media entry to one document form, reachable identically from the
  person form, the couple form, and the profile, that writes nothing until it
  is saved and accepts remote addresses alongside uploaded files.
- [x] Browse the whole media library from the Dictionary, by a tag cloud and
  server-side filters on name, file type, category, linked person, linked
  event years, and date added.
- [ ] Exercise migrations and media workflows against PostgreSQL in CI.
- [x] Decide how PDFs are drawn without a native rasterizer and its
  cross-platform binary cost: the server reads them with the `hayro-syntax`
  parser alone, for a real page count and a thumbnail of the scan page 1
  paints ([API](api.md), Media).
- [x] View PDFs in the UI with pdf.js, which also draws a preview of a PDF the
  server could not thumbnail (a text or vector page) ([Common UI
  §4.5](ui-common.md#45-mediainput-mediagallery-and-documentform)).
- [x] Prefer an event-linked vignette over the whole media as its illustration
  ([Person Profile](ui-person-profile.md)).
- [x] Store the pictures cut out of a page for its vignettes once first
  viewed, deleted with the vignette, its rectangle or its page's file
  ([Architecture §4.2](architecture.md#42-stored-crops)).
- [x] Add per-media progress and cancellation to the final Geneanet write pass:
  every import is one transaction a cancellation rolls back
  ([Architecture §6.3](architecture.md)).
- [ ] Test large media libraries and close media-specific error-state gaps.
- [ ] Run the complete Geneanet flow against an authorized test account using
  anonymized captures and committing no session or genealogy data.

## 3b. Active: archive viewer

A cited source opens its register at the cited view on the archive's own
portal — in an archive window on the desktop, in a new tab on the web —
through a per-country archive catalogue and one adapter per portal platform
([Archive Portals](archives.md), [Person
Profile](ui-person-profile.md#opening-a-cited-register)).

- [x] Drive the Loire-Atlantique archives (Arkothèque) for births and
  baptisms.
- [x] Move the catalogue and the citation parser into `oxidgene-archives`,
  resolve Arkothèque citations through the portal's request interface, and
  have the archive window load the resolved view instead of driving the
  portal's form ([Archive Portals §10](archives.md#10-delivery-phases)).
- [x] Add the Mnesys adapter.
- [x] Search every form shape and series of Mnesys Expo portals and
  catalogue the seventeen Mnesys Expo departmental archives
  ([Archive Portals §4.4](archives.md#44-mnesys)).
- [x] Add the GAIA adapter and catalogue the five departmental archives
  running it — Ariège, Aude, Orne, Pyrénées-Orientales, Seine-et-Marne —
  with their censuses, military registers and succession tables
  ([Archive Portals §4.8](archives.md#48-gaia)).
- [x] Add the Bach adapter (Anaphore) and catalogue the six departmental
  archives running it — Gard, Haute-Marne, Tarn, Tarn-et-Garonne, Vaucluse,
  Guadeloupe — with their decennial tables, reformed churches' registers,
  censuses and military registers ([Archive Portals §4.11](archives.md#411-bach)).
- [x] Resolve a cited source on the backend, through REST and GraphQL, and
  open its target from the web client in a new tab
  ([Archive Portals §5.3](archives.md#53-api), [§6.2](archives.md#62-web)).
- [x] Attach `iiif` archives' cited views as a remote multi-page document that
  the region tool can crop, every archive opening alike on its portal — from
  the desktop's archive window, from beside the source on the web
  ([Archive Portals §6.3–6.4](archives.md#63-iiif-behind-the-scenes)). The
  offer is disabled since: viewing a cited source is not where documents are
  collected.
- [ ] Browse and search the archives freely from OxidGene, beyond the
  registers the tree cites, and offer there to attach a document found to a
  person of the tree, turning the attaching offer back on
  ([Archive Portals §6.3](archives.md#63-iiif-behind-the-scenes)).
- [ ] Attach an event to the archive register it comes from when a Geneanet
  person links an image of one of Geneanet's indexed registers
  (`/registres/view/<register>/<page>`, images listed by
  `www.geneanet.org/registres/api/images/<register>`): identify the archive,
  the register and the view, and record them as the event's citation so it
  opens through the archive catalogue like any other cited register
  ([Archive Portals §5.1](archives.md#51-citation-recognition), [Geneanet Media
  Import](geneanet-media-import.md)).
- [x] Cite and resolve series beyond acts — population censuses, military
  registers, conscription lists, tables of successions and absences — named
  in words or by code, and cemetery burial registers, through the
  collections of every adapter whose portals publish them
  ([Archive Portals §3.1, §5.1](archives.md#51-citation-recognition)).
- [x] Recognize citations written in any convention — the classic
  description, its short form, structured repository records with the cited
  event, portal addresses — with the words of each language as data, French
  shipped, and complete a partial one in a "Find in the archives" dialog
  ([Archive Portals §5.1, §6.5](archives.md#51-citation-recognition)).
- [x] Ship the citation vocabularies in every interface language, read
  together whatever the archive, and write the parts the reader completes in
  the interface language ([Archive Portals §5.1](archives.md#51-citation-recognition)).
- [x] Wait out anti-bot checks in the desktop archive window, ask the reader
  to answer one that stays, land on the filtered search results after a
  check or a block, name the view to go to on a portal without an address
  per view, and complete on Linux a portal certificate served without its
  issuer ([Archive Portals §6.1](archives.md#61-desktop)).
- [x] Cover the portal with a progress overlay that can cancel the lookup,
  refuse cookie consent on the reader's behalf, bring viewers without an
  address per view to the cited view, and say why a portal's server failed
  ([Archive Portals §6.1](archives.md#61-desktop)).
- [x] Catalogue the French departmental archives with an adapter per portal
  platform — every department — and the overseas
  civil status of the Archives nationales d'outre-mer, the one portal
  reached over plain `http`; open a register behind a portal's reuse
  licence once the reader has accepted it
  ([Archive Portals §4, §11](archives.md#11-french-departmental-portals)).
- [x] Catalogue the Bouches-du-Rhône archives once their portal passes its
  live check ([Archive Portals §11.5](archives.md#115-survey-by-department)).
- [x] Search an index of persons by the cited person's surname where the
  other parts leave several persons
  ([Archive Portals §4.5](archives.md#45-ligeo)).
- [ ] Catalogue the Bouches-du-Rhône military registers, searched by the
  cited person's surname, once their portal passes its live check
  ([Archive Portals §11.5](archives.md#115-survey-by-department)).
- [ ] Catalogue the Swiss cantonal archives and other countries.
- [x] Add a live end-to-end check per catalogued archive against its real
  portal, run weekly by a dedicated workflow rather than on commits, so
  vendor software upgrades are caught before users meet them
  ([Archive Portals §9.1](archives.md#91-live-checks)); each later adapter
  adds its probe and viewer description.

## 4. Person merge wizard

The merge operation, the distinct-person confirmations, the homonym check
after every person save and on the Geneanet receipt, and the three-step
[merge wizard](ui-merge.md) are delivered ([Data
Model](data-model.md#person-merge), [Person Edit
Modal](ui-person-edit-modal.md) §13):

- [x] Replace the tree action picker's placeholder "Merge with…" with the
  three-step wizard, choosing the record kept and the events and media taken
  from the other.
- [x] Offer "Merge with…" on the person profile.
- [x] Add the potential duplicates tab of [Tools](ui-tools.md), whose
  comparison opens the wizard at Step 2.
- [x] Let the comparison pick, row by row, the surname, the given names, the
  sex and each once-only event (birth, baptism, death, burial, cremation) of
  either record.
- [x] Reconcile events recorded twice in the comparison: one of each
  once-only event is kept, and a repeatable event the kept record already has
  is left unticked.

## 5. Entry suggestions

The [place dictionary](place-dictionary.md) of twelve countries
is generated, and entry fields suggest values ([Common UI §4.4](ui-common.md)).
Remaining:

- [x] Ship the dictionary with the application, compressed, and search it
  through REST and GraphQL alike.
- [x] Give every place field the shared `PlaceInput` of
  [Common UI §4.4](ui-common.md): suggestions from the tree's places and the
  dictionary, free text always accepted.
- [x] Suggest the tree's surnames, given names, occupations and source titles
  in the entry forms and the search filters, followed by the reference
  sheets' given names and occupations.
- [x] Let a tree turn entry suggestions off (the Entry options of
  [Settings](ui-settings.md)).
- [ ] Decide whether Northern Ireland townlands and French lieux-dits justify
  a separately downloaded database.

## 6. Statistics and tools

### Statistics

The [Statistics](ui-statistics.md) page (key figures, heat map of places
over an offline basemap with births by area, names, charts per period under
a year ruler, records, notable lists, optional approximate dates, and the
growth of the tree over the days it was worked on) is delivered. Remaining:

- [ ] PDF export of the statistics ([General §3.9](general.md)). Every tab
  already prints, and so saves as PDF through the print dialog
  ([Common UI §7](ui-common.md#7-printing)); decide whether a dedicated export
  holding every tab is still needed.
- [ ] Try the heat map and the charts on large trees (tens of thousands of
  persons) for load time and legibility.

### Tools

The [Tools](ui-tools.md) page, one tab per tool, holds the anomalies with
the catalogue of their rules, the places the statistics cannot locate, the
ancestry completeness from the SOSA root, the potential duplicates, the
date converter and the dates in words.
Delivered since:

- [x] The proposed anomaly rules of [Tools §3.3](ui-tools.md): a unique event
  recorded twice, a child of several families, contradicting qualified
  dates, no death recorded past 110 years, a union of siblings, a given name
  of the other sex, two living siblings of one given name, place spellings
  that differ only in form, and distant places on one day.
- [x] The anomaly thresholds as tree settings ([Settings
  §8b](ui-settings.md)).
- [x] Dates in words ([Tools §8.3](ui-tools.md)): double dating, the
  Christmas and Easter styles, the liturgical feast of the day, the
  classical subtractive Latin forms, Hebrew and Republican dates written
  out.

## 6b. Planned: upstream GEDCOM fixes

OxidGene reads and writes GEDCOM through `oxidged` (a lenient, lossless reader
and a conformant writer) and `geneweb` ported to it, both used from their
crates.io releases (`oxidged` 0.3, `geneweb` 0.6). Nothing remains that OxidGene does in
their place: `oxidged` decodes `CHAR ANSI` and 8-bit bytes under `CHAR ASCII`
as Windows-1252, counts the whole line against 255 characters, writes every
line break as `CONT` (extension values included), keeps each date bound's
wording, keeps an LDS status outside the enumeration with the ordinance's
extensions, keeps a 5.5.1 individual `EVEN`'s descriptor, and writes a 5.5.1
`FILE` of any format (a PNG, a PDF).

- [x] Use the published `oxidged` and `geneweb` releases instead of their git
  main branches.

## 7. Dictionary descent

The descent of a family name, by branches, unfolds in the Dictionary's
family-names tab beside its alphabetical list ([Dictionary
§7.2](ui-dictionary.md)).

- [x] Define descent grouping, including incomplete parentage and children who
  do not carry the surname.
- [x] Add symmetric REST and GraphQL operations.
- [x] Add the recursive view to the existing Dictionary page and specification.
- [x] Cover SOSA badges, limits, empty states, and large surname groups.

## 7b. Subtree export

- [x] Export a selected subtree — a person's ancestors, descendants, or both,
  to a chosen depth — as GEDCOM or GEDZIP, on REST and GraphQL alike, from
  the export section of [Settings](ui-settings.md). The export keeps the
  chosen persons with the families and records they reach and drops the
  pointers to the rest, as `oxidged`'s `Dataset::subset` does, applied to the
  records the exporter takes ([Tags §2.3](tags.md#23-exporting-part-of-the-tree)).

## 8. Planned: security, release, and deployment

EPIC G is specified in [Authentication](authentication.md),
[Authorization](authorization.md), [Privacy](privacy.md) and
[Messaging and Notifications](messaging.md). All of it is built for the web
deployment only — server, worker and web frontend — behind the `auth`
feature and UI capabilities the desktop does not enable; the desktop keeps
its single local user and its loopback guards, unchanged. Each phase ships
its REST and GraphQL operations together, with their tests, every user-visible
string in every language, and the user guide. In order:

1. **Identity and sessions.**
   - [x] The `auth` feature of `oxidgene-api`, enabled by the server and the
     worker only, with a guard keeping it out of the desktop
     ([Authentication §7](authentication.md#7-web-build-only-the-desktop-is-unchanged)).
   - [x] `OXIDGENE_AUTH_MODE` (`oidc`, `none`), the OIDC configuration,
     discovery and JWKS with back-channel URL, token validation
     ([Authentication §3, §5](authentication.md#5-access-tokens-and-the-identity-of-a-request)).
   - [x] The Backend-for-Frontend: `/auth/*`, the session store in memory and
     in Redis, CSRF, refresh with rotation, RP-initiated and back-channel
     logout ([Authentication §4](authentication.md#4-browser-sessions-backend-for-frontend)).
   - [x] Users: provisioning and the signup gate, instance roles from claims,
     deactivation, erasure, account data, the administrator's user list.
   - [x] The sign-in, account menu and session-end dialog of the web
     frontend, and its `none` mode identical to the desktop's interface
     ([Authentication §2](authentication.md#2-deployment-modes)).
   - [x] The profile API on REST and GraphQL: display name, the uploaded
     avatar with its crop, re-encoding and serving, language, sessions,
     account data and deletion
     ([Authentication §6.7](authentication.md#67-profile-and-avatar)).
   - [x] The profile page over it, with the generated initials avatar, and
     the administrator's Instance › Users section in App Settings; the
     profile's notification, API-client, messaging-security and
     blocked-users sections arrive with phases 4, 6 and 7.
   - [x] The Compose stack's Rauthy with its development setup and Mailpit,
     the web container's proxy, `just dev-idp-up`, and the chart's
     `auth.idp.mode` `external` and `none` with its values, Ingress paths
     and rendering tests ([Authentication §8](authentication.md#8-deployment)).
   - [x] The mock provider of the integration tests.
   - [x] The opt-in end-to-end sign-in suite (`just e2e-auth`): written
     against the Compose stack, it skips itself until the server answers
     `/auth/session` in `oidc` mode, and covers the web interface's sign-in,
     account menu, ended session, profile page and user list.
2. **Roles and enforcement.**
   - [x] Memberships with the Owner, Editor and Guest roles, tree creation
     rights, ownership transfer, ownerless trees
     ([Authorization §3, §5](authorization.md#5-memberships-and-ownership)).
   - [x] The `ViewerContext` resolved per request from the validated token,
     with its client id, taken by every service, projection read, GraphQL
     resolver and MCP tool ([Authorization §4](authorization.md#4-viewercontext)).
   - [x] The permission of every operation in the parity table, the
     401/403/404 contract, and the role × operation matrix test on both
     surfaces.
   - [x] The author and client of every audit entry, the job's recorded
     actor re-checked by the worker, and the access log with its retention
     ([Authorization §9](authorization.md#9-audit),
     [Authentication §5.5](authentication.md#55-background-jobs)).
3. **Privacy and exclusions.**
   - [x] Public as the default record privacy, with the precedence of
     [Privacy §3.1](privacy.md#31-precedence).
   - [x] `person_visibility` and the contemporary rule with its inference
     from descendants and other relatives
     ([Privacy §4](privacy.md#4-contemporary-persons)); the export's *Leave
     out contemporary and private persons* option, on both builds.
   - [x] Redaction in every enforcement point and display mode, confidential
     notes, media privacy, cache headers
     ([Privacy §5, §7](privacy.md#7-enforcement-points)), and the export's
     *Apply visitors' privacy rules*: for Editors, Guests, Visitors and
     share links' holders alike, through one policy.
   - [x] Exclusions of persons and subtrees, materialized and kept current,
     from the person menu and the privacy settings
     ([Privacy §6](privacy.md#6-exclusions-from-sharing)), and the owner's
     *Leave out excluded persons* export option.
   - [x] The tree's privacy settings in [Settings §8](ui-settings.md#8-section-privacy),
     and the canary persons of the privacy tests.
   - [x] The leak suite over the canary persons, over every REST and GraphQL
     read, for Editors, Guests, a scoped Guest and Visitors.
   - [x] Hidden persons and records left out of every list, count and search
     by subqueries over the visibility, exclusion and scope tables, whatever
     their number ([Privacy §7](privacy.md#7-enforcement-points)).
   - [x] A restricted viewer's hidden records read once per request in one
     statement: their record reads cost the owner's plus a fixed few.
4. **Notifications and email.**
   - [x] In-app notifications, preferences and the long poll
     ([Messaging and Notifications §2, §6](messaging.md#6-delivery-without-polling)):
     the kinds of the memberships, ownership, invitations and jobs, the
     navbar's bell, the profile's *Notifications* section, and the long poll
     woken in memory and through PostgreSQL's notifications.
   - [x] The SMTP relay, the outbox processed by the worker, translated
     templates, digests and unsubscribe; Mailpit in the Compose stack; the
     chart's `mail.smtp` values ([Messaging and Notifications §3](messaging.md#3-email)),
     invitations mailed through the outbox.
5. **Sharing.**
   - [x] Sharing with users of the instance, email invitations, share links,
     tree visibility with its visitor scope
     ([Authorization §6](authorization.md#6-sharing)); invitations are
     mailed through phase 4's outbox where the deployment has a relay, and
     the owner sends the link where it has none.
   - [x] Access scopes, materialized and kept current, with their preview,
     and a scoped Editor's write rules
     ([Authorization §3.3, §6.5](authorization.md#65-access-scopes)).
   - [x] Every read limited to the viewer's scope, with the *Not shared*
     boundary, through phase 3's redaction; exports too.
   - [x] Placeholders marked `not_shared` or `private`, labelled *Not shared*
     (never navigated) and *Private* on pedigree cards, in family lists and
     in kinship ([Authorization §6.5](authorization.md#65-access-scopes)).
   - [x] The homepage's roles and invitations, and the owner's Members and
     Sharing sections, the visitor banner and **Leave this tree**.
   - [x] Edit controls hidden throughout the tree pages and their edit
     forms where `viewer_access` lacks their permission, and the Tools and
     history pages answering their typed address with the forbidden page
     ([Authorization §11](authorization.md#11-user-interface), Read-only
     views, Pages reserved to a permission).
6. **API clients and assistant access over HTTP.**
   - [ ] Tree and user API clients over client credentials, with
     provisioning, rotation, revocation, rate limits and usage
     ([Authorization §7](authorization.md#7-api-clients)), the `media:read`
     and `media:write` scopes on direct media reads and uploads, and their
     `api_client_expiring` notification.
   - [x] The MCP tools act for any viewer and are checked as their REST
     operations are; `list_trees` lists the trees the viewer may read
     ([Assistant Access §7.2](mcp.md#72-privacy-values-are-not-applied)).
   - [ ] MCP over Streamable HTTP with protected resource metadata
     ([Authorization §8](authorization.md#8-assistant-access-over-http-mcp)).
   - [ ] Token exchange for the first worker task that calls a service on
     a user's behalf.
7. **Messaging.**
   - [ ] Device enrolment, the key directory, key backup and recovery,
     verification ([Messaging and Notifications §5](messaging.md#5-end-to-end-encryption)).
   - [ ] One-to-one and group conversations over MLS, message requests, read
     state, record links resolved under the reader's access.
   - [ ] Blocking, reporting, moderation, rate limits, retention and erasure;
     the inbox, conversation and Messaging security surfaces; the web
     bundle's measured size budget.
   - [ ] The `messages` and `report_resolved` notifications, the messages
     digest's counts, and the messaging events of the long poll.
8. **Rauthy mode.**
   - [x] `auth.idp.mode=rauthy`: the StatefulSet on Hiqlite or on its own
     CloudNativePG `Cluster`, the bootstrapped setup and the job completing
     the web client, with chart rendering tests
     ([Authentication §8.2, §8.3](authentication.md#83-the-bootstrapped-setup)).
   - [ ] Validate the Rauthy mode in a production-like cluster, three
     replicas included.

Releases:

- [x] Build and publish versioned desktop binaries for Linux, Windows, and
  macOS from repository tags, with SHA-256 checksums.
- [x] Publish the macOS desktop as a universal `OxidGene.app`, Apple silicon
  and Intel, in a disk image ([Development §6](development.md#6-release-automation)).
- [x] Sign the Windows executable through SignPath at each tagged release,
  with its product name and version in its resources, once the repository
  variable `SIGNPATH_ORGANIZATION_ID` is set.
- [ ] Obtain the SignPath Foundation's certificate, set up the SignPath
  project and its variable and secret, which turns Windows signing on.
- [ ] Sign the macOS application with an Apple Developer ID and notarize it,
  in place of its ad hoc signature, once the project has an Apple developer
  account.
- [x] Publish a documentation site on GitHub Pages: the user guide, workflows
  and FAQ in English and French, the specifications, the REST and GraphQL
  references and the rustdoc ([Development §7](development.md#7-documentation-site)).
- [x] Build and publish versioned OCI images for the static WASM frontend and
  the Axum backend, with immutable tags and documented configuration.
- [x] Provide a development Docker Compose stack for the frontend, backend, and
  PostgreSQL, including health checks, persistent local volumes, and a
  documented one-command startup workflow.
- [x] Provide a Helm chart for PVC-free frontend and backend pods, services,
  ingress and TLS references, health probes, disruption budgets, autoscaling,
  ephemeral SQLite or an externally managed PostgreSQL database or CloudNativePG,
  disabled, external, or operator-managed Redis session infrastructure, and
  ephemeral filesystem media or an existing S3 bucket or operator-managed RustFS.
- [ ] Validate the chart in a production-like Kubernetes cluster.
- [x] Publish the chart as an OCI artifact with the versioned application
  images.
- [x] Build every release artifact in CI behind the CI and nightly gates,
  with SHA-256 checksums of the desktop downloads and provenance and SBOM
  attestations of the images.
- [x] Attest the build provenance of the desktop downloads and the packaged
  chart ([Development §6](development.md#6-release-automation)).
- [ ] Smoke-test the container and desktop deliverables before release.
- [x] Run unit, functional, browser JavaScript, performance and end-to-end
  tests as separate CI jobs, the Playwright suite also nightly
  ([Development §2.7](development.md#27-test-categories)).
- [ ] Make the E2E job a required check of the CI gate once it has run
  reliably.
- [ ] Try desktop printing through each platform's native dialog (WebKitGTK,
  macOS, WebView2): whether it raises `beforeprint`, and whether it honours
  the pedigree's landscape page.

Privacy fields hide nothing from the members of a tree, who all hold
`read_private`: they take effect for Visitors and share links' holders.

## 9. Planned: assistant access (MCP)

First delivery: read-only, stdio, desktop, with a required `tree_id` on every
tool but `list_trees`.

- [x] Add the optional `mcp` feature to `oxidgene-api`, with `rmcp` limited to
  `server`, `macros`, and `transport-io`, and justify its transitive
  dependency cost measured with `cargo tree`.
- [x] Derive tool input schemas from the types REST already deserializes,
  behind that feature.
- [x] Add the `oxidgene-desktop mcp` subcommand, which starts no
  listener, WebView, job worker, purge worker, or job recovery, and writes only
  protocol messages to standard output.
- [x] Implement `list_trees` and the tree-scoped read-only tools of
  [Assistant Access §5](mcp.md).
- [x] Add the AI assistant entry to the App Settings API section: warning,
  command, JSON configuration, desktop capability injection, and its
  translation keys.
- [x] Test every tool over an in-process transport, including a missing
  `tree_id`, cross-tree IDs, a tree deleted mid-session, and output-stream
  cleanliness.

Later phases, in order:

- [x] Write tools over the person, name, event, witness, family, place,
  source, citation, note and tag services, with `destructiveHint` on
  removals and deletions, the same transactions, projection refresh and
  history entries, offered only behind `oxidgene-desktop mcp --allow-writes`
  (off by default) and announced by a hint in App Settings
  ([Assistant Access §5.2, §7.1](mcp.md)).
- [x] A read-only tool over the kinship operation, so an assistant can say how
  two persons are related, with `get_family` and `list_event_witnesses`
  giving the link IDs the removals take.
- [ ] Streamable HTTP on `/mcp` with MCP authorization mapped to per-tree
  access, in the web build: EPIC G phase 6 (§8).
- [ ] Bounded thumbnail image content.
- [ ] Output schemas, once the result types carry JSON Schema derives.

## 10. Planned: AI features (bring your own LLM)

Specified in [AI Features](ai.md). Each user connects their own providers and
keys; the host pays for no model.

- [ ] Phase 1: provider registry, OpenAI-compatible and Anthropic protocols,
  the App Settings **AI** section with client-side key storage and consent,
  the backend relay with its outbound protection, `/ai/test`, `/ai/status`,
  and the **AI transcript** button in the media viewer.
- [ ] Phase 2: record and photo analysis with the review panel.
- [ ] Phase 3: derived media, then enhance and colorize.
- [ ] Phase 4: Gemini native, Bedrock, streaming.

Deferred: photo animation, per-account key storage (EPIC G), local model
runtimes, sending tree context with an analysis.

## 11. Planned: DNA kits

Specified in [DNA Kits](dna.md): consumer raw data attached to persons,
processed locally only, desktop first.

- [ ] Phase 1: import and attach kits (five chip layouts and Y-STR),
  encrypted storage, consent, real deletion, export.
- [ ] Phase 2: Y and mtDNA haplogroups, Y-STR distance.
- [ ] Phase 3: matching between the tree's kits, cross-checked with kinship;
  X-DNA; chromosome browser; triangulation.
- [ ] Phase 4: continental ancestry estimate.

Before phase 2 and 3: confirm the licences of PhyloTree Build 17 and of the
HapMap genetic map.

## 11b. Tags

Specified in [Tags](tags.md): one vocabulary per tree, shared by every kind
of record, replacing the media-only tags.

- [x] Phase 1: `tag` and `record_tag` replacing `media_tag`; tags on persons,
  couples and media; the shared tag picker and badges; the Dictionary's
  Tags tab; the person-search tag filter; GEDCOM and GEDZIP extensions;
  history; the MCP `list_tags` tool.
- [x] Phase 2: highlighting a tag in the tree views; bulk tagging from the
  tree and from search results.
- [x] Phase 3: tags on events, sources and citations: the `tags` of their
  reads, the record menus and badges of the timelines and the Sources tab, and
  their GEDCOM `_OXIDGENE_TAG` placements.
- [x] Phase 4: tags on places, repositories and notes: the `tags` of their
  reads, the record menus and badges of the Dictionary's Places and Repositories
  tabs and of the Notes cards, and their GEDCOM placements, including the
  `PLAC` rule.
- [x] Phase 5: statistics and anomalies limited to a tag; export of the
  persons carrying a tag, with the subtree export (§7b): the tag of the
  statistics and anomalies APIs and pages, and the scope of the exports, on
  REST and GraphQL alike ([Tags §2](tags.md#2-uses-beyond-finding)).
- [x] Tags shown as badges while browsing: every list, the profile headers
  and the tree's events side panel; the tree's cards keep the highlight mode
  ([Common UI §4.18](ui-common.md#418-tags-picker-and-badges)).

## 12. Asynchronous processing

The durable job queue is delivered: imports and exports run as background
jobs held in the database, claimed under expiring leases by the worker
(`apps/oxidgene-worker` on the web, embedded in the desktop), and resumed
after a restart ([Architecture §6](architecture.md)). Post-MVP:

- [x] Define queue and worker architecture without a second source of truth.
- [x] Move imports and exports to durable background jobs.
- [x] Recover interrupted jobs after a restart.
- [x] Let the user cancel a queued or running job, and wait for a job's
  changes instead of polling ([Architecture §6.3, §6.4](architecture.md)).
- [ ] Add processing notifications.
- [ ] Add chunked and resumable media uploads.
- [ ] Validate 100,000-person trees and large media libraries.
- [x] Give file-backed SQLite one writer connection and a small read-only
  pool, routing read-only handlers to the pool, so that the application stays
  readable during an import ([Architecture §4](architecture.md)).

## 13. Definition of done

An item is complete only when implementation and specifications agree; i18n
keys exist in all eight languages (English, French, German, Spanish,
Italian, Dutch, Polish, Portuguese) at exact parity; examples and artifacts are anonymized; REST
and GraphQL behavior and tests match; obsolete code, CSS, endpoints,
translations, flags, and dependencies are removed; dependency cost is
justified; and `just check` passes before a detailed Conventional Commit.
