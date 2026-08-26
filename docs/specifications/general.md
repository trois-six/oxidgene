---
type: "Product Specification"
title: "General — Vision, Users & Features"
description: "Product vision, target users, feature scope, and MVP boundaries for OxidGene."
tags: [oxidgene, specification, product, mvp]
timestamp: 2026-06-17T00:00:00Z
---


![OxidGene](../assets/OxidGene.png)

# General — Vision, Users & Features

> Part of the [OxidGene Specifications](index.md).
> See also: [Architecture](architecture.md) · [Data Model](data-model.md) · [API Contract](api.md) · [Roadmap](roadmap.md)

---

## 1. Context and Project Objectives

### 1.1 General Context

The project aims to develop a multiplatform genealogy application, built entirely in Rust, based on:

- a **Dioxus** frontend compiled to WebAssembly (WASM) for web and desktop, and
- a backend powered by **Axum** exposing an API simultaneously in REST (JSON) and GraphQL, with all features available through both protocols.

The application is designed to be:

- compiled as a **desktop client** running on Windows, Linux and macOS (single binary embedding an Axum server + SQLite + Dioxus WebView via Wry), and
- deployable as a **web application** through Docker containers:
    - frontend container (static WASM assets served by a lightweight HTTP server),
    - backend container (Axum server),
    - database container (PostgreSQL),
    - optional worker infrastructure for EPIC H asynchronous processing.

For technical details, see [Architecture](architecture.md).

### 1.2 Nature of the Application

OxidGene is a genealogy platform enabling users to create, view, edit, and share family trees and associated genealogical data (individuals, relationships, events, sources, media).

### 1.3 Main Objectives

- Deliver a modern, high-performance, portable genealogy application.
- Provide an open API (REST + GraphQL) aligned with the design principles of the FamilySearch API. → see [API Contract](api.md)
- Ensure a user experience comparable to leading genealogy platforms.
- Allow progressive evolution toward advanced and paid features.

### 1.4 Differentiation

- Made in Rust — performance, safety, and a single language across the entire stack.
- A theme-based UX system reproducing the experience of Geneanet, Filae, Ancestry, or MyHeritage.
- A unified Rust + WASM architecture with a single Dioxus codebase for web and desktop.
- A dual REST/GraphQL API.
- A fully offline-capable desktop client.
- Advanced collaboration and tree-matching features (post-MVP).

---

## 2. Target Users and Roles

### 2.1 Target Users

- Individuals practicing genealogy.
- Genealogy associations.
- Professional or advanced users.
- Paid subscribers (future phases).

### 2.2 Planned User Roles (per tree)

These roles describe the authorization target for EPIC G. They are not
implemented in the current MVP, which has no authentication or viewer-aware
access control.

- **Guest**: limited access, contemporary individuals hidden. → see [Settings](ui-settings.md) (privacy section)
- **Full Read-only**: full tree access.
- **Editor**: read + create/modify/delete.

### 2.3 Access Control

- Target behavior: trees can be private, shared, or public, with access rights
    defined per tree.
- Current behavior: privacy values record the user's intent but do not restrict
    any API query, search, export, media download, or UI view.
- Authentication, authorization, and privacy enforcement are deferred to EPIC
    G (not in MVP). → see [Roadmap](roadmap.md)

---

## 3. Core Features

### 3.1 Tree Management

- Create trees from scratch or via GEDCOM import.
- Manage multiple trees.
- Rename a tree from its homepage card or its Tree & Roots settings.
- Mark the person who represents the current user for visual orientation in the pedigree and reach that setting from their profile.
- → see [Homepage spec](ui-home.md)

### 3.2 GEDCOM Import/Export

- Import GEDCOM 5.5.1 and 7.0 with automatic version detection through
    `ged_io`; export the whole tree or a selected subtree.
- Import GeneWeb `.gw` files through `geneweb`, converted into the same domain
    mapping as GEDCOM.
- Import and export GEDZIP `.gdz` archives. Embedded media use the ordinary
    storage, validation, thumbnail, and linking pipeline; missing or unsupported
    files produce warnings without discarding valid genealogy.
- Recover Geneanet media links through a guided desktop flow. Authentication
    and collection run in an incognito browser window, media shared by several
    people are stored once, and event evidence links are created only on an
    unambiguous match.
- Preserve supported source metadata and report unsupported or ambiguous data
    explicitly rather than inventing a mapping.
- See [Import](ui-import.md), [Geneanet Media Import](geneanet-media-import.md),
    [API Contract](api.md), and [Settings](ui-settings.md).

### 3.3 Collaborative Editing (Web) — Post-MVP

- Simultaneous editing (deferred to post-MVP).
- Conflict detection and resolution.

### 3.4 Tree Matching — Post-MVP

- Suggest merges between user trees.

### 3.5 Themes / UX

- Switch between multiple UX themes inspired by major genealogy platforms from the settings.
- → see [Settings](ui-settings.md)

### 3.6 Interface Language

- Configurable UI language, without restart.
- User-level (web) or app-level (desktop).

### 3.7 REST & GraphQL APIs

- Full feature parity between both protocols.
- FamilySearch-inspired structure.
- Available from EPIC A onward.
- → see [API Contract](api.md)

### 3.8 Media Management

- Upload images/PDF/videos.
- Metadata and viewer integration. The viewer fills the available overlay area;
    its media stays contained within that full frame rather than shrinking the
    reader to the media's intrinsic dimensions.
- Identify someone in a subpart of an image: the reader draws a region, chooses the person through search, and sees the identification as a linked vignette in the viewer and the person's media gallery.
- A vignette's context menu can remove that identification immediately; if it was a portrait, the person falls back to no portrait rather than retaining a stale region.
- A profile gallery can permanently delete an unshared media after confirmation; the viewer can force-delete it with all associated information. REST and GraphQL expose the same conditional-deletion contract.
- Async upload pipeline (post-MVP).
- → see [Person Edit Modal](ui-person-edit-modal.md) (media section)

### 3.9 Statistics & Reports

- Frequent last/first names, frequent occupations, birth distribution by months, parents age at birth, avg date at first union, birth/death stats, demographic pyramid, distribution of marriage days, avg duration of an union, avg children per union, avg duration between two children, avg age difference between first and last child in a couple, age diff between spouses, geographic distribution, last 100 births, last 100 deaths, last 100 unions, top 100 alive oldest, top 100 older...
- Graphs, tables, PDF export.

### 3.10 Visualization & Printing

- Multiple tree layouts (ancestor chart, descendant chart, fan chart).
- Export high-resolution PDFs.
- → see [Tree View spec](ui-genealogy-tree.md)

---

## 4. Security & Privacy

Privacy metadata is persisted now, but it is **not a security boundary in the
current MVP**. Marking a tree, person, family, or media item private does not
hide it from reads, searches, exports, media downloads, or the UI.

The following behavior is planned for EPIC G:

- Mask contemporary individuals (< 100 years old) for guest users. → see [Settings](ui-settings.md) (privacy section)
- Optional last/first name masking.
- Authentication and per-tree authorization.
- Privacy enforcement across REST, GraphQL, exports, media, and UI views.
- Full audit logging.

See [Roadmap](roadmap.md) for delivery status.

---

## 5. Performance

- Lazy loading of tree branches.
- Durable database read projections with transactional refresh; no cache tier.
    → see [Data Model](data-model.md) §4
- Recursive CTE over the family links for ancestor/descendant queries. → see [Data Model](data-model.md) (Ancestry traversal)
- Streaming GEDCOM parser for large files.
- Cursor-based pagination to avoid expensive offset scans. → see [API Contract](api.md) (pagination)
- Desktop window remapping preserves every page's mounted state. Duplicate GTK
    geometry events are discarded before they can reallocate the WebView; real
    size, position and display-scale changes still propagate normally.

---

## 6. Premium Features — Post-MVP

- Assisted tree matching.
- OCR on scanned documents.
- Image enhancement.
- External data source plugins.

---

## 7. MVP Scope

The MVP covers EPICs A through D (see [Roadmap](roadmap.md)):

- Interactive tree visualization. → [Tree View](ui-genealogy-tree.md)
- Person selection and detail view, whose identity dates retain both the real
    event label (birth/baptism, death/burial) and French gender agreement.
- Full CRUD editing (persons, families, events, sources, media, places, notes). → [Person Edit Modal](ui-person-edit-modal.md)
- GEDCOM import/export.
- Language switching.
- Theme support. → [Settings](ui-settings.md)
- REST + GraphQL API. → [API Contract](api.md)
- Desktop and web deployment. → [Architecture](architecture.md)

**Not in MVP**: authentication, access control, collaborative editing, tree matching, async pipeline.

---

## 8. Common User Interface

Shared layout, navigation, components, design tokens, accessibility, responsive
behavior, loading, and error presentation are defined once in
[Common UI](ui-common.md) and [Cross-cutting Rules](cross-cutting.md). Each page
specification documents only its own behavior.

## 9. Respect of norms and standards

The project must respect the norms and standards:

- GEDCOM 5.5 and 7.0
- XDG base directories for cache, config...
- REST and GraphQL
- OpenAPI
- OAuth 2.0 / OpenID Connect (eventually SAML if we decide to use it)
