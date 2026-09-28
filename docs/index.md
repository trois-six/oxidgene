---
okf_version: "0.2"
---

# Foundation

* [General](general.md) - Product vision, target users, feature scope, and MVP boundaries for OxidGene.
* [Quickstart](quickstart.md) - Requirements and procedures for running OxidGene as a downloaded desktop application, a source build, a Compose stack, or a Kubernetes deployment.
* [Architecture](architecture.md) - Technical architecture, crate boundaries, stack choices, and deployment model for OxidGene.
* [Development](development.md) - Local development, secure coding practices, verification workflows, and just command reference for OxidGene.
* [Data Model](data-model.md) - Canonical domain entities, enums, and relationship model used by OxidGene services and UI.
* [API Contract](api.md) - REST and GraphQL contract for OxidGene, including endpoints, pagination, and payload conventions.
* [Assistant Access (MCP)](mcp.md) - Model Context Protocol server built into the desktop binary: read-only tools that each name their tree, stdio transport, launch, consent, and its relation to REST and GraphQL.
* [AI Features (Bring Your Own LLM)](ai.md) - Planned AI features on media — transcript, record and photo analysis, enhancement, colorization — run with LLM providers and keys each user brings, never the host's: provider presets and protocols, where keys live, the backend relay, privacy and consent, API contract, and delivery phases.
* [DNA Kits](dna.md) - Planned import of consumer DNA raw data attached to persons, processed locally only: supported kit formats, haplogroups, matching between the kits of one tree cross-checked with its kinship paths, continental ancestry estimates, storage and encryption, consent and deletion, reference data and licences, API, and delivery phases.
* [Roadmap](roadmap.md) - Current delivery status, active priorities, and future milestones for OxidGene.
* [Geneanet Media Import](geneanet-media-import.md) - Recovering the person↔photo links a Geneanet export drops, through the media API, the GeneWeb join key, and size matching.
* [Place Dictionary](place-dictionary.md) - The place dictionary of France, the United Kingdom, Germany, Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium, Luxembourg and the Netherlands: its Geneanet-compatible CSV layout, the open-data sources and licences it is generated from, and the rules that file each place under every name it has borne.
* [Geneanet Upload API](geneanet-upload-api.md) - Reverse-engineered reference for the Geneanet Upload app's api.geneanet.org surface, Cloudflare behavior per HTTP client, originals versus renditions, and login.

# Cross-cutting

* [Cross-cutting Rules](cross-cutting.md) - Rules shared by all OxidGene frontends, backends, APIs, tests, and documentation.
* [Common UI](ui-common.md) - Shared layout, navigation, design tokens, components, accessibility, and responsive behavior.

# UI Pages

* [Homepage](ui-home.md) - Tree dashboard with tree cards listing recently modified persons, search and sort, and the create and delete modals.
* [Genealogy Tree](ui-genealogy-tree.md) - Pedigree canvas with person cards, connectors, navigation, and the events sidebar.
* [Person Profile](ui-person-profile.md) - Full person detail view with identity, timeline, family connections, media, and notes.
* [Couple Profile](ui-couple-profile.md) - Side-by-side view of both spouses of a couple, with the union, its events, media, and notes shared across the two.
* [Person History](ui-person-history.md) - Every recorded version of a person, compared field by field side by side, with the restore of an earlier one.
* [Search Results](ui-search-results.md) - Filterable person search results page.
* [Kinship](ui-kinship.md) - Every way two persons of a tree are related, each path drawn generation by generation from the ancestors they share, or through unions when they share none.
* [Dictionary](ui-dictionary.md) - Index of family names, sources, places, and occupations with usage counts, and the bulk family-name editor (rename, merge, particle).
* [Statistics](ui-statistics.md) - Tree statistics page in tabs: an overview with completeness and averages, a heat map of places with births by country, region and subdivision, names, demographic charts per period under a year ruler, event and family distributions, the tree's records, notable lists, and the number of persons the tree held over the days it was worked on.
* [Tools](ui-tools.md) - Tree tools page in tabs, one tool each: the anomalies of dates, filiations, unions, witnesses and records with their catalogue, the places the statistics cannot locate, the completeness of the ancestry from the SOSA root, and a converter of dates between calendars.
* [Tree Settings](ui-settings.md) - Tree settings page for roots, privacy, date display, entry options, tools, and export.
* [App Settings](ui-app-settings.md) - Application-level preferences page for appearance, language, pedigree, names, API connection details, and the AI assistant connection.

# UI Modals and Flows

* [Person Edit Modal](ui-person-edit-modal.md) - Modal to create and edit a person in every context, edit a couple, manage media, and delete.
* [Person Merge](ui-merge.md) - Three-step wizard to select a duplicate person, compare both records side by side, and confirm the merge.
* [Import](ui-import.md) - The import modal for GEDCOM, GEDZIP, GeneWeb, and Geneanet trees with media.
