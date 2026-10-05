---
type: "Roadmap Specification"
title: "Roadmap — Delivery Status and Milestones"
description: "Current delivery status, active priorities, and future milestones for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-05T21:00:00Z }
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
| D | Shared UX, themes, languages, runtime settings, printing | In progress: the Date Display and Entry Options sections of Tree Settings are being built | [Common UI](ui-common.md), [Cross-cutting Rules](cross-cutting.md), [Settings](ui-settings.md) |
| E | Read projections, search, dictionary | Complete except dictionary descent (§7) | [Data](data-model.md), [Search](ui-search-results.md), [Dictionary](ui-dictionary.md) |
| F | Media and Geneanet recovery | In progress | [Data](data-model.md), [API](api.md), [Import](ui-import.md), [Geneanet Pipeline](geneanet-media-import.md) |
| G | Security, privacy enforcement, deployment | Planned | [General](general.md), [Architecture](architecture.md), [Settings](ui-settings.md) |
| H | Asynchronous and large-scale processing | Durable import and export jobs delivered; the rest post-MVP (§12) | [Architecture](architecture.md), [API](api.md) |
| I | Assistant access through MCP | First delivery complete; later phases planned | [Assistant Access](mcp.md), [App Settings](ui-app-settings.md) |
| J | Change history: audit log of every tree write, person, place, source and settings versions, side-by-side comparison, restore | Complete; entries record no author until EPIC G | [Data](data-model.md#5-change-history), [API](api.md), [Person History](ui-person-history.md), [Settings](ui-settings.md#11-section-history), [Homepage](ui-home.md) |

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
- [ ] Decide whether PDF page rendering justifies a native rasterizer and its
  cross-platform binary cost.
- [ ] Prefer an event-linked vignette over the whole media as its illustration.
- [ ] Add per-media progress and cancellation to the final Geneanet write pass.
- [ ] Test large media libraries and close media-specific error-state gaps.
- [ ] Run the complete Geneanet flow against an authorized test account using
  anonymized captures and committing no session or genealogy data.

## 3b. Active: archive viewer

The desktop opens a normalized citation at the cited register view through an
archive catalogue and one driver per portal platform ([Person
Profile](ui-person-profile.md#opening-a-cited-register)).

- [x] Drive the Loire-Atlantique archives (Arkothèque) for births and
  baptisms.
- [ ] Move the catalogue and the citation parser into `oxidgene-archives`,
  resolve Arkothèque citations through the portal's request interface, and
  have the archive window load the resolved view instead of driving the
  portal's form ([Archive Portals §10](archives.md#10-delivery-phases)).
- [x] Add the Mnesys adapter.
- [x] Resolve a cited source on the backend, through REST and GraphQL, and
  open its target from the web client in a new tab
  ([Archive Portals §5.3](archives.md#53-api), [§6.2](archives.md#62-web)).
- [x] Show `iiif` archives' views in the shared viewer and attach cited views
  as a remote multi-page document that the region tool can crop
  ([Archive Portals §6.3–6.4](archives.md#63-oxidgenes-viewer)).
- [x] Cite and resolve series beyond acts — population censuses, military
  registers, conscription lists, tables of successions and absences — named
  in words or by code, through Arkothèque and Ligeo collections
  ([Archive Portals §3.1, §5.1](archives.md#51-citation-parsing)).
- [x] Wait out anti-bot checks in the desktop archive window, ask the reader
  to answer one that stays, land on the filtered search results after a
  check or a block, name the view to go to on a portal without an address
  per view, and complete on Linux a portal certificate served without its
  issuer ([Archive Portals §6.1](archives.md#61-desktop)).
- [ ] Catalogue every French departmental archive, adding a driver for each
  portal platform they use.
- [ ] Extend the catalogue to Swiss cantonal archives.
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
Remaining tabs:

- [ ] The proposed anomaly rules of [Tools §3.3](ui-tools.md): a unique event
  recorded twice and a child of several families (they need the events and
  family links rather than the projections), contradicting qualified dates,
  no death recorded past 110 years, a union of siblings, a given name of the
  other sex (from the reference sheets), two living siblings of one given
  name, place spellings that differ only in form, and distant places on one
  day.
- [ ] Decide whether the anomaly thresholds should become tree settings.
- [ ] Dates in words ([Tools §8.3](ui-tools.md)): double dating for the
  Annunciation style, other old styles, the liturgical feast of the day,
  the classical subtractive Latin forms, Hebrew and Republican dates
  written out.

## 6b. Planned: upstream GEDCOM fixes

OxidGene works around `ged_io` 0.16 and the `geneweb` crate (pre-parse repairs
in `oxidgene-gedcom/src/sanitize.rs`, post-write additions in `finish.rs`).
Each workaround goes once its upstream fix ships. Patches to propose, against
upstream `main` (mentioning the overlap with `ged_io`'s pending CRUD-API
refactor):

- [ ] `ged_io`: read an `AGE` leniently — free text and an empty value as a
  phrase rather than a failed file, `1y6m` without spaces, approximate wording
  kept as text.
- [ ] `ged_io`: write a source's `REPO` citations with their `CALN`, `MEDI`
  and `NOTE`; keep several `CALN` (each with its `MEDI`) and read `MEDI` only
  under `CALN`.
- [ ] `ged_io`: write a `REPO` record's `PHON`, `EMAIL`, `FAX`, `WWW`, `NOTE`,
  `REFN`, `RIN` and custom tags, and a source's `DATA` (`EVEN`, `DATE`,
  `PLAC`, `AGNC`), `PUBL`, `TEXT`, `OBJE`, `REFN` and `RIN`.
- [ ] `ged_io`: write `PHRASE` only in GEDCOM 7 output; 5.5.1 uses parentheses.
- [ ] `ged_io`: keep every `NOTE` of a record or structure (not the last), and
  write them all; parse 7.0 `SNOTE` and resolve note pointers.
- [ ] `ged_io`: keep and write custom tags everywhere; read `CONC`/`CONT`
  without trimming the delimiting space and never split a line beside a space;
  read continuations of every long value; write shared `NOTE` records with
  continuations.
- [ ] `ged_io`: parse `INDI.SUBM` and `FAM.SUBM`; write a `SUBM` record's
  `EMAIL`, `PHON` and `WWW`; a structured date model with per-bound calendars.
- [ ] `geneweb`: keep event witnesses where they belong and report unresolved
  ones; emit a union event for family-line witnesses; keep the death reason
  and titles' places and ends; map `rel` relations to their GEDCOM forms.

## 7. Planned: dictionary descent

- [ ] Define descent grouping, including incomplete parentage and children who
  do not carry the surname.
- [ ] Add symmetric REST and GraphQL operations.
- [ ] Add the recursive view to the existing Dictionary page and specification.
- [ ] Cover SOSA badges, limits, empty states, and large surname groups.

## 7b. Planned: subtree export

- [ ] Export a selected subtree — a person's ancestors, descendants, or both,
  to a chosen depth — as GEDCOM or GEDZIP, on REST and GraphQL alike, from
  the export section of [Settings](ui-settings.md).

## 8. Planned: security, release, and deployment

- [ ] Implement authentication and session management.
- [ ] Implement per-tree guest, read-only, and editor authorization.
- [ ] Enforce person, family, and media privacy according to viewer access,
  as one piece of work with the tree's visibility and contemporary-person
  settings ([Settings §8](ui-settings.md#8-section-privacy)): the age
  threshold, the display mode, navigation to hidden persons and their
  photos, and the export's *Include contemporary persons* option. A
  contemporary person is a privacy rule, so it is built and enforced with
  the rest of privacy, not before it.
- [ ] Record the author of every audit entry ([Data Model §5](data-model.md#5-change-history))
  and add access audit logging with anonymized operational output.
- [ ] Mirror security behavior and errors across REST and GraphQL.
- [x] Build and publish versioned desktop binaries for Linux, Windows, and
  macOS from repository tags, with SHA-256 checksums.
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
- [ ] Build all release artifacts in CI, publish checksums and provenance, and
  smoke-test the container and desktop deliverables before release.
- [x] Run unit, functional, browser JavaScript, performance and end-to-end
  tests as separate CI jobs, the Playwright suite also nightly
  ([Development §2.7](development.md#27-test-categories)).
- [ ] Make the E2E job a required check of the CI gate once it has run
  reliably.
- [ ] Try desktop printing through each platform's native dialog (WebKitGTK,
  macOS, WebView2): whether it raises `beforeprint`, and whether it honours
  the pedigree's landscape page.

Privacy fields currently record intent but do not hide data. The UI must state
this clearly until authorization is enforced.

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

- [ ] Mutating tools with `destructiveHint`, the same transactions, and
  projection refresh; deletions last.
- [ ] Streamable HTTP on `/mcp` with MCP authorization mapped to per-tree
  access, after EPIC G.
- [ ] Bounded thumbnail image content.
- [ ] A read-only tool over the kinship operation, so an assistant can say how
  two persons are related.
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

## 12. Asynchronous processing

The durable job queue is delivered: imports and exports run as background
jobs held in the database, claimed under expiring leases by the worker
(`apps/oxidgene-worker` on the web, embedded in the desktop), and resumed
after a restart ([Architecture §6](architecture.md)). Post-MVP:

- [x] Define queue and worker architecture without a second source of truth.
- [x] Move imports and exports to durable background jobs.
- [x] Recover interrupted jobs after a restart.
- [ ] Let the user cancel a queued or running job.
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
