![[OxidGene.png]](OxidGene.png)

---

## 1. Context and Project Objectives

### 1.1 General Context

The project aims to develop a multiplatform genealogy application, built in Rust, based on:

- a WebAssembly (WASM) frontend, and
- a backend exposing an API simultaneously in REST (JSON) and GraphQL, with all features available through both protocols.

The application is designed to be:

- compiled as a desktop client running on Windows, Linux and macOS, and
- deployable as a web application through Docker containers:
    - frontend container,
    - backend container,
    - database container
    - queing application container (for EPIC F – Asynchronous Pipeline (Post‑MVP))

### 1.2 Nature of the Application

OxidGene is a genealogy platform enabling users to create, view, edit, and share family trees and associated genealogical data (individuals, relationships, events, sources, media).

### 1.3 Main Objectives

- Deliver a modern, high‑performance, portable genealogy application.
- Provide an open API (REST + GraphQL) aligned with the design principles of the FamilySearch API.
- Ensure a user experience comparable to leading genealogy platforms.
- Allow progressive evolution toward advanced and paid features.

### 1.4 Differentiation

- Made in Rust!
- A theme‑based UX system reproducing the experience of Geneanet, Filae, Ancestry, and MyHeritage.
- A unified Rust + WASM architecture.
- A dual REST/GraphQL API.
- A fully offline‑capable desktop client.
- Advanced collaboration and tree‑matching features (post‑MVP).

---

## 2. Target Users and Roles

### 2.1 Target Users

- Individuals practicing genealogy.
- Genealogy associations.
- Professional or advanced users.
- Paid subscribers (future phases).

### 2.2 User Roles (per tree)

- Guest: limited access, contemporary individuals hidden.
- Full Read‑only: full tree access.
- Editor: read + create/modify/delete.

### 2.3 Access Control

- Trees can be private, shared, or public.
- Access rights defined per tree.

---

## 3. Core Features

### 3.1 Tree Management

- Create trees from scratch or via GEDCOM import.
- Manage multiple trees (web version).

### 3.2 GEDCOM Import/Export

- Full import/export using Rust crate `ged_io`.
- Error logging and normalization.
- Support for GEDCOM > 5.5 and > 7.

### 3.3 Collaborative Editing (Web)

- Simultaneous editing.
- Conflict detection and resolution.

### 3.4 Tree Matching (Post‑MVP)

- Suggest merges between user trees.

### 3.5 Themes / UX

- Switch between multiple UX themes inspired by major genealogy platforms.

### 3.6 Interface Language

- Configurable UI language.
- User‑level (web) or app‑level (desktop).
- Runtime switching.

### 3.7 REST & GraphQL APIs

- Full feature parity.
- FamilySearch‑inspired structure.

### 3.8 Media Management

- Upload images/PDF/videos.
- Metadata and viewer integration.
- Async upload pipeline (post‑MVP).

### 3.9 Statistics & Reports

- Frequent surnames, birth stats, geographic distribution, etc.
- Graphs, tables, PDF export.

### 3.10 Visualization & Printing

- Multiple tree layouts.
- Export high‑resolution PDFs.

---

## 4. Technical Architecture

### 4.1 Databases

- Web: PostgreSQL.
- Desktop: SQLite via Turso.
- No sync in MVP.

### 4.2 Data Model

- Person, Family, Event, Source, Media, Place.
- Graph relationships.
- Closure table for optimized traversal.

### 4.3 Backend

- Rust core.
- REST + GraphQL.
- `ged_io` wrapper for GEDCOM module.

### 4.4 Frontend

- Modular WASM interface.
- Theme support.

### 4.5 Asynchronous Processing (Post‑MVP)

- Message queue container (Redis/RabbitMQ/NATS).
- `document‑queue` orchestration service.
- Rust workers (scalable).
- Temporary + persistent object storage.

---

## 5. Security & Privacy

- Mask contemporary individuals (<100 years).
- Optional surname masking.
- Full audit logging.

---

## 6. Performance

- Lazy loading.
- Caching.

---

## 7. Build & Testing

- Unified `justfile`.
- Full test suite.
- CI/CD pipelines.

---

## 8. Deployment

- Kubernetes deployment (dev & prod).
- GitOps with FluxCD.
- Liveness/readiness probes.

---

## 9. Premium Features (Post‑MVP)

- Assisted tree matching.
- OCR.
- Image enhancement.
- External data source plugins.

---

## 10. MVP Scope

- Interactive tree.
- Person selection.
- Editing.
- GEDCOM import/export.
- Language switching.

---

## 11. EPICs, Sprints & Milestones

### EPIC A – Technical Foundation

- Data model, backend skeleton, frontend skeleton.

### EPIC B – GEDCOM Engine

- Import/export.
- Round‑trip tests.
- Error logs.
- Performance.

### EPIC C – Tree Editing

- Visual tree.
- Person sheet.
- Relationship editing.

### EPIC D – UX, Languages, Performance

- Themes.
- i18n.
- Caching.

### EPIC E – Security & Deployment

- Access rights.
- Data masking.
- Kubernetes deployment.

### EPIC F – Asynchronous Pipeline (Post‑MVP)

- Chunked uploads.
- Message queue.
- `document‑queue` service.
- Rust workers.
- Async media + GEDCOM processing.
- Notifications.

---

