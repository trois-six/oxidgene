# OxidGene Development Guide

## Project

OxidGene is a multiplatform genealogy application written entirely in Rust. It
uses Dioxus for the WASM and desktop frontend, Axum for the REST and GraphQL
backend, SeaORM with PostgreSQL on the web and SQLite on desktop, `ged_io` for
GEDCOM, and `geneweb` for GeneWeb `.gw` imports.

## Development Principles

### Specifications are authoritative

- Read the relevant document in `docs/specifications/` before changing a
  feature, data model, API, workflow, or visual behavior.
- Update the affected specifications in the same change as the implementation.
- Specifications describe the current product, its contracts, and exceptional
  workflows. They are not a chronological development journal.
- Keep delivery status, active work, and future milestones in
  `docs/specifications/roadmap.md`, not in this file or in product
  specifications.
- Update `docs/specifications/index.md` whenever a specification is added,
  removed, renamed, or superseded.

### Language and internationalization

- Write all Git commit subjects and bodies, code comments, identifiers, logs,
  and technical documentation in English.
- Route every user-visible string through the existing i18n mechanism. This
  includes labels, tooltips, placeholders, validation messages, errors, empty
  states, accessibility text, and text produced by backend workflows for the
  UI.
- Keep English and French translation tables at exact key parity. Add or update
  both languages in the same change.
- Do not translate user-provided genealogical content, imported source data, or
  standard protocol values.

### Privacy and anonymization

- Anonymize every provided or committed artifact: tests, fixtures, examples,
  screenshots, logs, comments, commit messages, documentation, sample commands,
  and exported files.
- Do not commit real names, account names, email addresses, identifiers, family
  relationships, locations, archive references, or other personal data.
- Use clearly fictitious neutral data when an example needs people or places.
- Treat imported genealogy and media as sensitive data even when the source is
  publicly accessible.

### Git and delivery

- Follow the Conventional Commits specification.
- Use an imperative English subject and a detailed English body explaining the
  implementation, important design decisions, migrations, compatibility
  impact, and verification performed.
- Run `just check` before every commit. Do not commit until formatting, Clippy,
  and the full test suite pass and the expected CI checks are accounted for.
- Create a commit after each substantial feature. Split work earlier when the
  uncommitted change becomes difficult to review or when the work changes
  focus.
- Keep commits cohesive. Do not mix unrelated cleanup with a feature or fix.

### Rust quality and dependency discipline

- Keep dependencies to the strict minimum. Prefer the standard library and
  dependencies already present in the workspace when they solve the problem
  cleanly.
- Consider feature flags, transitive dependency cost, compile time, and release
  binary size before adding or expanding a dependency.
- Never hide compiler errors or warnings with broad `allow` attributes or loose
  lint configuration. Fix the underlying cause. A narrowly scoped exception
  requires a documented technical reason.
- Remove code, CSS, components, API endpoints, feature flags, translations, and
  dependencies made obsolete by a change. Do not leave dead compatibility paths
  unless a specification explicitly requires them.
- Keep changes minimal and consistent with existing crate boundaries and local
  patterns.

### API and test completeness

- Keep REST and GraphQL strictly symmetric: the same capabilities, validation,
  behavior, errors, and test coverage must be available through both surfaces.
- Add focused regression tests for bug fixes and behavior tests for new
  features. Use anonymized test data only.
- Keep benchmarks, performance tests, load tests, and tests requiring external
  infrastructure opt-in. Mark test-harness cases `#[ignore]` (or use an
  equivalent dedicated target) and run them only through an explicit command;
  `just check` must never execute them.
- Keep fast correctness assertions extracted from performance or load scenarios
  in the normal test suite when they provide useful regression coverage.
- Update API and data-model specifications whenever a public contract or stored
  shape changes.
- Bump `PROJECTION_SCHEMA_VERSION` whenever `PersonProfile` or any nested
  projection type changes. Existing payloads use `#[serde(default)]`, so a
  version bump is required to make the change visible on existing installs.

## Specification Map

All specifications live in `docs/specifications/`. Start with
`docs/specifications/index.md`, then read the documents relevant to the change:

| Area | Primary specification |
|------|-----------------------|
| Vision and product scope | `general.md` |
| Architecture and crate boundaries | `architecture.md` |
| Entities, enums, and relationships | `data-model.md` |
| REST and GraphQL contracts | `api.md` |
| Delivery status and planned work | `roadmap.md` |
| Data, projections, and search | `data-model.md` |
| i18n, errors, logs, and privacy | `cross-cutting.md` |
| Shared UI behavior and styling | `ui-common.md` |
| Pages and major workflows | `ui-*.md` |

## Workspace and Dependency Flow

```text
crates/
  oxidgene-core/     Domain types, enums, errors, and projections
  oxidgene-db/       SeaORM entities, migrations, and repositories
  oxidgene-gedcom/   GEDCOM and GeneWeb conversion into the domain
  oxidgene-geneanet/ Geneanet join, key folding, archive indexing, and hashing
  oxidgene-api/      Axum REST, GraphQL, services, media, and profiles
  oxidgene-ui/       Dioxus components and pages
apps/
  oxidgene-server/   Web server binary
  oxidgene-desktop/  Desktop binary with embedded Axum, SQLite, and WebView
```

Dependency direction:

```text
core <- db <- api <- server/desktop
core <- gedcom <- api
geneanet <- api/desktop
core <- ui
```

### Architecture invariants

- `oxidgene-ui` is platform-independent and must continue to compile to WASM.
  It must not depend on `dioxus-desktop` or `oxidgene-geneanet`. Desktop-only
  capabilities are declared as UI traits and injected by the desktop binary.
- `oxidgene-geneanet` performs no HTTP. Geneanet requests run inside the
  desktop login window because direct clients are rejected by Cloudflare.
- There is no cache tier. Durable read models live in `person_denorm` and
  `person_search_fts`; pedigrees are assembled per request from family links and
  denormalized profiles.
- All primary keys use UUID v7.
- List endpoints use cursor-based pagination.
- Domain records use soft deletion and are excluded by default.
- Persons exist independently; families connect spouses and children.
- Authentication is not part of the current MVP.
- A year displayed alone must retain its precision. Use
  `Event::qualified_year()`, not `Event::year()`. Birth may fall back to baptism
  and death to burial when the primary event has no date.

## Frontend Rules

- Reuse shared components for repeated interactions. There must be one canonical
  implementation of common controls such as context menus, dialogs, pickers,
  media controls, and date inputs.
- Reuse the design tokens and shared CSS in `components/layout.rs`. Do not
  introduce duplicate colors, spacing values, or component variants without a
  documented design requirement.
- Remove obsolete CSS selectors and translation keys when removing UI.

### Dioxus 0.7 notes

- `use_signal` returns `Copy` handles; closures capture them by copy.
- Quote camelCase SVG attributes in `rsx!`, such as `"viewBox"`,
  `"strokeWidth"`, and `"fillOpacity"`.
- Use `EventHandler<T>` for component callbacks.
- Do not use an SVG `<title>` directly in `rsx!`; Dioxus resolves it as the HTML
  element. Use escaped `dangerous_inner_html` so the browser parses it in the
  SVG namespace, following the existing pedigree card implementation.

## Backend Rules

- Keep one REST handler module per resource under `oxidgene-api/src/rest/`.
- Mirror every REST operation in `oxidgene-api/src/graphql/` and test both.
- Keep business workflows in `oxidgene-api/src/service/` and durable read-model
  logic in `oxidgene-api/src/profile/`.
- Keep storage concerns behind the existing media and repository traits.
- Refresh affected projections in the same transaction as each mutation.

## Build and Verification

```bash
just build          # Build the workspace
just test           # Run all tests
just check          # Format check, Clippy, and all tests
just fmt            # Format the workspace
just clippy         # Run Clippy
just server         # Run the development web server
just desktop        # Run the development desktop app
```

Use focused tests while iterating, then run `just check` before committing.

## Assets

The application logo is available as `docs/assets/OxidGene.png` and
`docs/assets/OxidGene.svg`.