---
type: "Cross-cutting Specification"
title: "Cross-cutting Rules — Language, Errors, Logging, and Privacy"
description: "Rules shared by all OxidGene frontends, backends, APIs, tests, and documentation."
tags: [oxidgene, specification, i18n, errors, logging, privacy]
timestamp: 2026-08-26T00:00:00Z
---

# Cross-cutting Rules — Language, Errors, Logging, and Privacy

> Part of the [OxidGene Specifications](index.md).
> See also: [API Contract](api.md) · [Common UI](ui-common.md) ·
> [Architecture](architecture.md)

---

## 1. Scope

These rules apply to every crate, binary, API surface, UI page, background
workflow, test, fixture, screenshot, log, and specification. A feature is not
complete when only one layer follows them.

## 2. Technical language

- Git commit subjects and bodies, code comments, identifiers, logs, API field
  names, error codes, and technical documentation are written in English.
- Imported content and user-entered genealogy remain in their source language.
- Protocol constants, GEDCOM tags, URLs, CSS classes, and persisted enum values
  are technical identifiers and are not localized.

## 3. Internationalization

### 3.1 Coverage

Every user-visible project string uses the i18n mechanism, including:

- page titles, labels, buttons, menus, placeholders, and tooltips;
- validation, confirmation, warning, loading, empty, and error states;
- accessibility names, descriptions, live-region text, and image alt text;
- enum display values, event names, date qualifiers, and formatting labels;
- backend workflow messages intended for display by a client.

User-provided names, places, notes, sources, media metadata, and imported data
are never translated.

### 3.2 Locale selection

- English and French are supported at runtime.
- An explicit stored choice wins.
- On first use, the client walks `navigator.languages` in preference order and
  chooses the first supported primary language subtag.
- English is the fallback when detection or storage is unavailable.
- Switching language updates mounted UI without a page reload and persists
  across sessions.

### 3.3 Translation structure

Translation maps live under `crates/oxidgene-ui/src/i18n/`. Keys use a stable,
hierarchical `surface.section.element` form. English and French tables must
contain exactly the same keys and interpolation placeholders; tests enforce
both properties.

Dynamic values use named placeholders. Plural forms use `_one` and `_other`
keys. Missing keys may fall back to English at runtime, but a parity test must
prevent shipping a known omission.

French labels for a field that can contain one or several given names use
**Prénom(s)**. **Prénom** remains the label for a singular name type or category,
while ordinary prose uses the grammatically appropriate singular or plural
instead of the parenthesized form.

### 3.4 Dates and numbers

- Locale and the tree's date-display preference are independent.
- Full surfaces use localized qualifier text and calendar month names.
- Space-constrained pedigree cards use the documented GeneWeb-compatible short
  precision marks; these protocol-like marks are not translated.
- Gendered French strings have unknown, male, and female keys. English keeps
  the same keys even when values are identical.
- Number formatting follows locale conventions.

### 3.5 Adding a language

Adding a language requires its complete translation map, registration in the
language selector, placeholder and parity tests, date and number formatting,
and review of layout expansion. Use logical CSS properties so future RTL
support remains possible.

## 4. Error contract

### 4.1 Principles

- Errors expose a stable machine-readable code and a safe human-readable
  message without internal details or personal data.
- Expected domain and validation errors are not logged as server failures.
- Unexpected errors receive a correlation ID; clients may display that ID in a
  localized support message.
- REST and GraphQL map the same domain error to equivalent codes, semantics,
  and tests.
- UI code translates known error codes. It may display a sanitized server
  message only as a fallback.

### 4.2 REST envelope

```json
{
  "error": "validation_error",
  "message": "The request is invalid",
  "request_id": "optional-correlation-id",
  "details": {
    "field": "stable_field_code"
  }
}
```

`request_id` and `details` are optional. Details contain machine-readable
values, never stack traces, SQL, filesystem paths, secrets, or genealogy.

| Status | Code | Meaning |
|---|---|---|
| 400 | `validation_error` | Invalid field, format, or business input. |
| 400 | `gedcom_error` | Invalid or unsupported genealogy input. |
| 401 | `unauthenticated` | Authentication is required, once security ships. |
| 403 | `forbidden` | The viewer lacks access, once security ships. |
| 404 | `not_found` | Missing or soft-deleted resource. |
| 409 | `conflict` | State conflicts with an invariant or concurrent change. |
| 413 | `payload_too_large` | Request exceeds the documented endpoint limit. |
| 415 | `unsupported_media_type` | Payload or media format is unsupported. |
| 500 | `database_error` | Persistence failed unexpectedly. |
| 500 | `io_error` | Storage or transport I/O failed unexpectedly. |
| 500 | `internal_error` | Unclassified server failure. |

The [API Contract](api.md) identifies which codes each operation can return and
documents deviations that still exist in the implementation.

### 4.3 GraphQL mapping

GraphQL uses the standard `errors` array. Each error carries the equivalent
uppercase code and optional request ID in `extensions`. Mutation payloads do
not invent a second error model.

```json
{
  "data": null,
  "errors": [{
    "message": "The requested resource was not found",
    "extensions": {
      "code": "NOT_FOUND",
      "requestId": "optional-correlation-id"
    }
  }]
}
```

### 4.4 Validation

Field validation returns stable field identifiers suitable for localized
inline errors. Clients retain submitted values and focus the first invalid
field. Cross-field and domain validation appears in a summary and links to the
relevant control where possible.

## 5. Logging and observability

### 5.1 Structured logs

Backends and desktop infrastructure use `tracing` structured fields. Log text
is English and describes the operation, not the user's data.

Recommended fields:

- request or correlation ID;
- route or GraphQL operation name;
- HTTP method and status;
- duration and aggregate counts;
- error code and error category;
- tree or resource IDs only when needed for operation, preferably hashed or
  omitted from persistent production logs.

### 5.2 Levels

| Level | Use |
|---|---|
| `trace` | Local diagnostic detail disabled in normal builds. |
| `debug` | Developer-oriented control flow without user data. |
| `info` | Startup, shutdown, migrations, completed jobs, aggregate outcomes. |
| `warn` | Recoverable degradation, skipped import item, retry, stale external dependency. |
| `error` | Operation failed and requires investigation. |

Expected `404`, validation failures, and user cancellation are not `error`
events unless they reveal an infrastructure defect.

### 5.3 Sensitive data

Never log or commit:

- names, relationships, dates, places, notes, source text, or media metadata;
- account names, email addresses, cookies, tokens, passwords, or headers carrying
  credentials;
- imported payloads, filenames that reveal identity, archive contents, SQL
  values, or filesystem paths below a user data directory;
- screenshots or serialized sessions from external services.

Use aggregate counts, stable error categories, sanitized extensions, and
fictitious fixtures. Debug logging does not weaken this rule.

HTTP request spans record the method and aggregate response outcome, never the
raw URI or query string. Search terms and resource identifiers may be carried
by either and are therefore treated as genealogy rather than routing metadata.
Configuration failures likewise log a stable category without echoing the
rejected value.

### 5.4 Operational behavior

- Panic and unexpected error boundaries attach a correlation ID and preserve
  the source chain internally without returning it to clients.
- Retried operations log the attempt and final outcome without duplicating a
  full error at every layer.
- Metrics use aggregate dimensions with bounded cardinality; personal data and
  raw UUIDs are not metric labels.

### 5.5 OpenTelemetry export

- Native server, worker, and desktop runtimes always emit structured `tracing`
  logs, including optimized desktop release builds. Setting a non-empty
  `OTEL_EXPORTER_OTLP_ENDPOINT` additionally exports logs, traces, and metrics
  through OTLP/gRPC to an OpenTelemetry Collector; leaving it unset performs no
  network export and keeps span callsites disabled. Console log events remain
  enabled independently of span collection. Successful exporter initialization
  emits an informational startup event without recording the endpoint or export
  headers.
- Every native runtime reports a distinct `service.name` and its package
  version. Incoming HTTP `traceparent` headers are extracted with W3C Trace
  Context so calls remain connected across trusted gateways and services.
- A browser bundle built with a non-empty `OTEL_EXPORTER_OTLP_ENDPOINT` exports
  client spans over OTLP/HTTP and injects W3C Trace Context into every request
  sent by the typed API client. The API request span parents SeaORM spans and
  persists its context with a queued background job; the worker restores that
  context before executing the job, so durable work remains in the originating
  trace after process and time boundaries.
- `OXIDGENE_LOG_LEVEL` independently configures each process using
  `EnvFilter` syntax. `OTEL_EXPORTER_OTLP_ENDPOINT` is likewise process-local,
  so server, worker, and desktop may use different collectors or disable
  export independently. `RUST_LOG` is not part of the configuration contract.
- HTTP spans use the Axum route template, method, status, and duration. They do
  not record raw URIs, query strings, request or response bodies, headers,
  resource IDs, or unmatched paths.
- The final Axum router owns one server span for every matched REST and GraphQL
  request; route modules do not duplicate that span in each handler. GraphQL
  adds an execution span and one child span per non-introspection resolver,
  recording only parent type and field name. Long-running import, export,
  projection, and media workflows add `skip_all` service spans. SeaORM spans
  remain the database leaves below those boundaries. Trivial glue functions
  are not individually spanned because that would add volume without a useful
  operational boundary.
- GEDCOM generation divides its `export.serialize` service span into
  `export.build_model`, `export.write`, `export.inject_event_media`, and
  `export.inject_extensions`. These spans record aggregate entity counts and
  input/output byte lengths only; genealogical values and identifiers remain
  excluded.
- Every routed UI screen owns one root load span named `ui.<page>.load`. Each
  Dioxus resource started by the screen or one of its nested components is a
  `ui.resource.load` child with a bounded, stable `ui.resource.name`. Resources
  remain separate siblings so their overlap exposes actual client-side
  parallelism. A resource that Dioxus cancels or replaces releases its place in
  the active-resource count just like a completed resource. After the last
  resource settles, `ui.render.stabilize` waits for two browser animation
  frames before closing the load cycle. The stable resource name is also the
  OpenTelemetry display name so trace waterfalls identify each load directly.
- Client response processing separates `ui.response.read` from
  `ui.response.deserialize`. Expensive synchronous transformations use
  `ui.compute`, including pedigree-data construction and layout. These spans
  use their stable compute label as the OpenTelemetry display name. Response
  spans use explicit display names and record HTTP status, expected and actual
  body sizes, and serialization format. `ui.render.stabilize` records that it
  waits for two animation frames after the resource cycle completes. Response
  content, search values, filenames, identifiers, and raw endpoints remain
  excluded. Events and links remain empty unless a real point-in-time event or
  non-parent causal relationship exists; operational dimensions belong in the
  span attributes.
- Pedigree service work exposes `pedigree.build`, `pedigree.projections`,
  `pedigree.ancestors`, and `pedigree.descendants`. SeaORM contributes one
  child span per query, execution, and transaction operation. Query spans
  include the parameterized SQL statement as `db.statement`; bound values are
  never included. Native exporters include `INFO`-or-higher OxidGene and SeaORM
  span targets only; Tokio, HTTP transport, SQLx runtime internals, and SeaORM's
  `TRACE` wrappers are excluded so routed UI load spans and UI action spans
  remain the operational roots.
- Every user-initiated import, Geneanet import, and export owns a root span of
  its own: `ui.import`, `ui.geneanet_import`, and `ui.export`. These operations
  outlive the render that started them, so they never attach to the load span of
  whichever screen was active. Each Geneanet wizard step the user drives is a
  root of its own, because the steps are separated by however long the person
  spends on the screen between them. The root records the requested format only.
- Each action root owns bounded phase children named after it:
  `ui.import.upload` and `ui.import.poll`; `ui.geneanet_import.read`, `.write`,
  `.inspect`, `.index`, `.connect`, `.preview`, `.collect`, `.upload`, `.poll`,
  `.session_encode`, and `.session_decode`; `ui.export.request`, `.queue`,
  `.poll`, and `.save`. A browser download the page hands to the user agent has
  no phase span because the transfer is outside the application.
- Import traces continue on the server with a format-specific root and bounded
  phase children for parsing, upload or collection, media preparation,
  persistence, projections, and job polling where applicable. Export traces
  likewise expose `export.load`, `export.serialize`, `export.media`,
  `export.package`, `export.publish`, and `export.job` for queued archives. They
  record format, depth, aggregate size, count, phase, and outcome dimensions
  only. User filenames, filesystem paths, external account details, genealogy,
  media metadata, SQL, and payloads are never span attributes.
- Background job spans record only bounded technical dimensions such as job
  kind and import/export format. Job IDs, tree IDs, filenames, source keys,
  user content, and media metadata are excluded.
- Export failures must not stop application work. Providers flush during
  graceful native runtime shutdown; abrupt process termination can still lose
  buffered signals.
- Optimized desktop releases retain OpenTelemetry and client propagation
  dependencies. Export remains a runtime choice controlled by
  `OTEL_EXPORTER_OTLP_ENDPOINT`, with no network telemetry when it is absent or
  empty.

## 6. UI feedback states

### 6.1 Errors

- Inline errors belong below the affected control and are linked through
  accessibility attributes.
- Toasts are for transient operation outcomes, not field validation or content
  required to complete a task.
- Full-page errors provide localized retry and safe-navigation actions.
- A failed mutation retains or restores the user's staged input.

### 6.2 Loading

- Page loads use layout-matched skeletons while shared navigation remains
  stable.
- Component loads preserve existing content and avoid layout shift.
- Async buttons are disabled and show a localized busy label or accessible
  spinner to prevent duplicate submission.
- Progress reports determinate values when known and an honest indeterminate
  state otherwise.

### 6.3 Empty states

The shared `EmptyState` is used only when content is genuinely empty. Loading,
permission denial, filtering with no match, and server failure are distinct
states with different localized text and actions.

### 6.4 Connectivity

- Desktop startup failure is a blocking local-server error with a restart
  action.
- Web network failures use bounded exponential retry where the operation is
  idempotent.
- Non-idempotent mutations are never retried automatically unless an
  idempotency contract exists.
- Recovery refreshes stale reads and announces restored connectivity.

Optimistic updates are allowed only when rollback is deterministic and the
user cannot lose entered data.

## 7. Privacy and anonymization

- Treat every genealogy, media file, import archive, export, session, and
  support capture as sensitive.
- All repository artifacts use fictitious neutral data: tests, fixtures,
  examples, screenshots, docs, logs, sample commands, and commit messages.
- Real data used for authorized local validation stays outside the repository
  and is never copied into issue text or CI artifacts.
- Sanitization removes indirect identifiers as well as obvious names, including
  locations, archive references, usernames, filenames, dates, and relationship
  combinations.
- Privacy settings must not claim to enforce protection before authorization
  is implemented; the UI states the current limitation.

### 7.1 Backend exposure before authentication

Until authentication and per-tree authorization are implemented and enforced
equally by REST, GraphQL, exports, search, and direct media reads, the backend
must never be exposed directly to an untrusted network:

- standalone and development servers bind to loopback by default;
- host-published container ports bind to loopback; a container may listen on
  its private network only when a trusted same-origin gateway is its sole
  ingress;
- CORS uses an explicit trusted frontend origin and never `*`;
- UI markup never places a backend URL in `href`, `src`, `action`, redirects,
  new-window navigation, or other user-visible navigation targets;
- media, thumbnails, crops, archives, and exports are fetched through the
  typed client, then exposed to the rendering engine as local `data:` or
  `blob:` resources or written through a platform save dialog.

Public backend deployment is blocked until the authentication flow and all
authorization checks are complete. Privacy flags do not relax this rule.

## 8. Verification

Every feature verifies, as applicable:

- English/French key and placeholder parity;
- REST/GraphQL behavior, validation, and error-code symmetry;
- logs contain no sensitive values in success and failure paths;
- UI loading, empty, error, offline, and accessibility states;
- fixtures and documentation are anonymized;
- deployment defaults and UI navigation expose no unauthenticated backend URL;
- focused tests followed by `just check` before committing code changes.

`just check` is not required when a change is limited to documentation, the
repository `README`, Dockerfiles, Docker Compose files, or GitHub Actions
workflows. Such changes use focused validation appropriate to their file type,
such as Markdown diagnostics, link checks, configuration validation, or a
workflow syntax check.

### 8.1 Optional performance and load tests

- Benchmarks, performance tests, load tests, soak tests, and tests requiring
  external infrastructure are opt-in and must not run as part of `just check`.
- Test-harness cases use `#[ignore]`; suites that do not use the Rust test
  harness use an equivalent dedicated target or explicit opt-in flag.
- Run these tests only with an explicit command that selects the relevant test,
  ignored-test set, benchmark, or load-test target.
- Keep deterministic, fast correctness assertions derived from these scenarios
  in the normal suite when they provide useful regression coverage.
- Document prerequisites, expected duration, resource requirements, and the
  invocation command next to each optional suite.
