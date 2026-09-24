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
* [Assistant Access (MCP)](mcp.md) - Model Context Protocol server built into the desktop binary: one tree per session, read-only tools, stdio transport, launch, consent, and its relation to REST and GraphQL.
* [Roadmap](roadmap.md) - Current delivery status, active priorities, and future milestones for OxidGene.
* [Geneanet Media Import](geneanet-media-import.md) - Recovering the person↔photo links a Geneanet export drops, through the media API, the GeneWeb join key, and size matching.
* [Geneanet Upload API](geneanet-upload-api.md) - Reverse-engineered reference for the Geneanet Upload app's api.geneanet.org surface, Cloudflare behavior per HTTP client, originals versus renditions, and login.

# Cross-cutting

* [Cross-cutting Rules](cross-cutting.md) - Rules shared by all OxidGene frontends, backends, APIs, tests, and documentation.
* [Common UI](ui-common.md) - Shared layout, navigation, design tokens, components, accessibility, and responsive behavior.

# UI Pages

* [Homepage](ui-home.md) - Tree dashboard with tree cards, search and sort, and the create and delete modals.
* [Genealogy Tree](ui-genealogy-tree.md) - Pedigree canvas with person cards, connectors, navigation, and the events sidebar.
* [Person Profile](ui-person-profile.md) - Full person detail view with identity, timeline, family connections, media, and notes.
* [Search Results](ui-search-results.md) - Filterable person search results page.
* [Dictionary](ui-dictionary.md) - Read-only index of family names, sources, places, and occupations with usage counts.
* [Tree Settings](ui-settings.md) - Tree settings page for roots, privacy, date display, entry options, tools, export, and AI assistant access.
* [App Settings](ui-app-settings.md) - Application-level preferences page for appearance (theme) and interface language.

# UI Modals and Flows

* [Person Edit Modal](ui-person-edit-modal.md) - Modal to create and edit a person in every context, edit a couple, manage media, and delete.
* [Person Merge](ui-merge.md) - Three-step wizard to select a duplicate person, compare both records side by side, and confirm the merge.
* [Import](ui-import.md) - The import modal for GEDCOM, GEDZIP, GeneWeb, and Geneanet trees with media.
