---
type: "Roadmap Specification"
title: "Roadmap — Delivery Status and Milestones"
description: "Current delivery status, active priorities, and future milestones for OxidGene."
tags: [oxidgene, specification, roadmap, planning]
timestamp: 2026-08-26T00:00:00Z
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
| C | Tree browsing and editing | Complete | [Tree](ui-genealogy-tree.md), [Person](ui-person-profile.md), [Person Edit](ui-person-edit-modal.md) |
| D | Shared UX, themes, languages, runtime settings | Complete | [Common UI](ui-common.md), [Cross-cutting Rules](cross-cutting.md) |
| E | Read projections, search, dictionary | Complete except dictionary descent | [Data](data-model.md), [Search](ui-search-results.md), [Dictionary](ui-dictionary.md) |
| F | Media and Geneanet recovery | In progress | [Data](data-model.md), [API](api.md), [Import](ui-import.md), [Geneanet Pipeline](geneanet-media-import.md) |
| G | Security, privacy enforcement, deployment | Planned | [General](general.md), [Architecture](architecture.md), [Settings](ui-settings.md) |
| H | Asynchronous and large-scale processing | Post-MVP | [Architecture](architecture.md), [API](api.md) |

## 3. Active: media completion

- [ ] Add an object-storage backend while retaining `MediaStore`.
- [ ] Exercise migrations and media workflows against PostgreSQL in CI.
- [ ] Decide whether PDF page rendering justifies a native rasterizer and its
  cross-platform binary cost.
- [ ] Prefer an event-linked vignette over the whole media as its illustration.
- [ ] Add per-media progress and cancellation to the final Geneanet write pass.
- [ ] Test large media libraries and close media-specific error-state gaps.
- [ ] Run the complete Geneanet flow against an authorized test account using
  anonymized captures and committing no session or genealogy data.

## 4. Planned: dictionary descent

- [ ] Define descent grouping, including incomplete parentage and children who
  do not carry the surname.
- [ ] Add symmetric REST and GraphQL operations.
- [ ] Add the recursive view to the existing Dictionary page and specification.
- [ ] Cover SOSA badges, limits, empty states, and large surname groups.

## 5. Planned: security, release, and deployment

- [ ] Implement authentication and session management.
- [ ] Implement per-tree guest, read-only, and editor authorization.
- [ ] Enforce person, family, and media privacy according to viewer access.
- [ ] Add audit logging with anonymized operational output.
- [ ] Mirror security behavior and errors across REST and GraphQL.
- [ ] Build, smoke-test, and publish versioned desktop binaries for Linux,
  Windows, and macOS.
- [ ] Build and publish versioned OCI images for the static WASM frontend and
  the Axum backend, with immutable tags and documented configuration.
- [ ] Provide a development Docker Compose stack for the frontend, backend, and
  PostgreSQL, including health checks, persistent local volumes, and a
  documented one-command startup workflow.
- [ ] Provide production Kubernetes manifests for the frontend and backend,
  services, ingress and TLS, configuration and secret references, health
  probes, and persistent storage. Support either managed PostgreSQL or an
  explicitly optional in-cluster PostgreSQL deployment.
- [ ] Build all release artifacts in CI, publish checksums and provenance, and
  smoke-test the container and desktop deliverables before release.

Privacy fields currently record intent but do not hide data. The UI must state
this clearly until authorization is enforced.

## 6. Post-MVP: asynchronous processing

- [ ] Define queue and worker architecture without a second source of truth.
- [ ] Add chunked and resumable media uploads.
- [ ] Move large imports and processing to cancellable background jobs.
- [ ] Add processing notifications and restart recovery.
- [ ] Validate 100,000-person trees and large media libraries.

## 7. Definition of done

An item is complete only when implementation and specifications agree; i18n
keys have English/French parity; examples and artifacts are anonymized; REST
and GraphQL behavior and tests match; obsolete code, CSS, endpoints,
translations, flags, and dependencies are removed; dependency cost is
justified; and `just check` passes before a detailed Conventional Commit.
