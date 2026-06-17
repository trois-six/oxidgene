---
type: "Error Handling Specification"
title: "Error Handling & Loading States Specification"
description: "Error, loading, and empty-state behavior across OxidGene API responses and UI flows."
tags: [oxidgene, specification, error-handling, ux]
timestamp: 2026-06-17T00:00:00Z
---


# Error Handling & Loading States Specification

> Part of the [OxidGene Specifications](/index.md).
> See also: [API Contract](/api.md) · [Architecture](/architecture.md) · [i18n](/i18n.md) (error message translations)

---

## 1. Overview

This document specifies how errors, loading states, and empty states are presented to the user across the application. It covers both API error responses and frontend UI feedback patterns.

---

## 2. API Error Responses

### REST error format

All REST endpoints return errors in a consistent JSON envelope:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "Person not found",
    "details": null
  }
}
```

### Error codes

| HTTP Status | Code | When |
|---|---|---|
| 400 | `VALIDATION_ERROR` | Invalid input (missing required field, wrong format) |
| 400 | `INVALID_DATE` | Unparseable date value |
| 400 | `INVALID_GEDCOM` | GEDCOM file is malformed or unsupported |
| 404 | `NOT_FOUND` | Resource does not exist or is soft-deleted |
| 409 | `CONFLICT` | Duplicate entry, circular reference, or merge conflict |
| 413 | `FILE_TOO_LARGE` | Upload exceeds size limit |
| 415 | `UNSUPPORTED_FORMAT` | Unsupported file type for upload |
| 422 | `BUSINESS_RULE` | Domain logic violation (e.g. person cannot be their own parent) |
| 500 | `INTERNAL_ERROR` | Unexpected server error |

### GraphQL errors

GraphQL errors follow the standard `errors` array in the response, with the `code` in the `extensions` field:

```json
{
  "data": null,
  "errors": [{
    "message": "Person not found",
    "extensions": {
      "code": "NOT_FOUND"
    }
  }]
}
```

### Validation errors

For `VALIDATION_ERROR`, the `details` field contains field-level errors:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Validation failed",
    "details": {
      "fields": {
        "name": "Name is required",
        "date_value": "Invalid date format"
      }
    }
  }
}
```

---

## 3. Frontend Error Display

### Toast Notifications

Transient, non-blocking messages shown in the **bottom-right** corner of the viewport. Auto-dismiss after 5 seconds, or on click.

```
┌────────────────────────────────────┐
│  ✓  Changes saved successfully     │
└────────────────────────────────────┘

┌────────────────────────────────────┐
│  ⚠  Could not delete tree         │
│     Please try again.             │
└────────────────────────────────────┘

┌────────────────────────────────────┐
│  ✗  Connection lost               │
│     Retrying…                     │
└────────────────────────────────────┘
```

| Type | Icon | Border color | Usage |
|---|---|---|---|
| Success | ✓ | `var(--green)` | Save, create, delete completed |
| Warning | ⚠ | `var(--orange)` | Partial failure, degraded functionality |
| Error | ✗ | `var(--color-danger)` | Operation failed, connection issues |
| Info | ℹ | `var(--blue)` | Neutral notifications |

Toast styling: `var(--bg-card)` background, `var(--shadow-md)` shadow, `var(--radius)` border radius, left-side colored border (4px).

### Inline Field Errors

For form validation, errors appear **below the invalid field**:

```
[Surname _______________]
  ⚠ Surname is required

[Date 32/13/1842________]
  ⚠ Invalid date format (expected dd/mm/yyyy)
```

- Error text: `var(--color-danger)`, 0.72rem, Lato
- Field border: `var(--color-danger)`
- Errors appear on blur or on save attempt

### Full-page Errors

For unrecoverable errors (API unreachable, 500 errors on page load):

```
┌──────────────────────────────────────┐
│                                      │
│            ⚠                         │
│                                      │
│   Something went wrong               │
│   We couldn't load this page.        │
│                                      │
│   [Try again]    [Go to homepage]    │
│                                      │
└──────────────────────────────────────┘
```

Centered in the content area. "Try again" retries the failed request. "Go to homepage" navigates to `/`.

---

## 4. Loading States

### Page-level loading

When a page is loading data (tree list, person details, search results):

- A **skeleton screen** replaces the content area with animated placeholder shapes matching the expected layout
- Skeleton shapes use `var(--bg-card)` background with a shimmer animation (light gradient sweep)
- The topbar and sidebar remain fully rendered

### Component-level loading

For inline data fetching (e.g. search autocomplete, person picker):

- A small **spinner** (16px, `var(--orange)` color, CSS animation) appears next to or inside the component
- Existing content remains visible (no layout shift)

### Button loading

When a button triggers an async operation (save, delete, import):

- Button text is replaced with a spinner + "Saving…" / "Deleting…" / "Importing…"
- Button is disabled (no double-click)
- Other form fields remain interactive (user can continue filling while save is in progress — but this is rare)

### Tree canvas loading

When the pedigree tree is loading or recalculating:

- A centered spinner overlay appears on the canvas area
- Existing cards fade to 50% opacity
- New layout fades in once ready

---

## 5. Empty States

Empty states are shown when a list or area has no content. See [EmptyState component](/ui-shared-components.md) §8.

| Context | Icon | Title | Subtitle | Action |
|---|---|---|---|---|
| No trees (first use) | 🌳 | Welcome to OxidGene | Create your first genealogy tree to get started. | [+ Create a tree] |
| No trees (search) | 🔍 | No tree found | Try a different search term. | — |
| No persons (search) | 🔍 | No person found | Try adjusting your search terms or clearing some filters. | [Clear filters] |
| No events (profile) | 📅 | No events | This person has no recorded events. | [Add an event] |
| No media (profile) | 🖼 | No media | No photos or documents attached to this person. | [Upload] |
| No children (union) | 👶 | No children | No children linked to this union. | — |
| No anomalies (settings) | ✓ | No anomalies detected | Your tree data looks consistent. | — |
| No duplicates (settings) | ✓ | No duplicates found | No potential duplicates were detected. | — |

---

## 6. Offline & Connection Errors

### Desktop mode

The desktop app runs a local Axum server. Connection errors should not occur under normal circumstances. If the embedded server fails to start:

- A full-screen error is shown: "Failed to start the local server. Please restart the application."
- No retry (requires app restart)

### Web mode

| Scenario | Behavior |
|---|---|
| **API unreachable** | Toast error: "Cannot reach the server. Retrying…" + automatic retry with exponential backoff (1s, 2s, 4s, max 30s) |
| **Timeout** (> 10s) | Toast error: "Request timed out. Please try again." + retry button |
| **Token expired** (post-MVP) | Redirect to login page |
| **Network restored** | Toast info: "Connection restored" + automatic refresh of stale data |

---

## 7. Optimistic Updates

For fast-feeling interactions, certain operations use optimistic updates:

| Operation | Optimistic behavior | On failure |
|---|---|---|
| Save person | Modal closes immediately, card updates | Toast error + modal reopens with previous data |
| Delete tree | Card removed from grid immediately | Toast error + card reappears |
| Reorder events | New order applied immediately | Toast error + order reverted |

The rule: use optimistic updates only for operations that rarely fail and where reverting is simple.
