---
type: "Cross-cutting Specification"
title: "Cross-cutting Rules — Language, Errors, Logging, and Privacy"
description: "Rules shared by all OxidGene frontends, backends, APIs, tests, and documentation."
tags: [oxidgene, specification, i18n, errors, logging, privacy, documentation]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-05T14:00:00Z }
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

- The language catalogue is discovered from `assets/i18n/*.json` at build
  time, with no Rust enumeration or registration list. It currently includes
  English, French, German, Spanish, Italian, Dutch, Polish and Portuguese.
- Desktop also discovers `<config directory>/languages/*.json` when the
  Language settings section is opened. Startup reads only the explicitly
  selected custom file, not the directory. The browser offers embedded locales.
- An explicit stored choice wins.
- On first use, the client walks `navigator.languages` in preference order and
  chooses the first supported locale, trying the complete normalized code
  and then progressively removing trailing subtags.
- English is the fallback when detection or storage is unavailable.
- Switching language updates mounted UI without a page reload and persists
  across sessions.
- A custom selection is stored as `custom:<code>`. If its file disappears or
  becomes invalid, the selection and stored preference reset to English.

### 3.3 Translation structure

Locale documents live under `assets/i18n/`. Keys use a stable,
hierarchical `surface.section.element` form. Every language's table must
contain exactly the same keys and interpolation placeholders; tests enforce
both properties.

Source and personal locale documents remain readable JSON. Desktop compresses
only embedded documents with Brotli at build time and decompresses them once
when initializing the catalogue. Web keeps ordinary embedded JSON and no
locale-compression decoder in WASM. See [Architecture §7.1](architecture.md#71-embedded-data).

Dynamic values use named placeholders. Plural forms use `_one` and `_other`
keys; `_one` covers 1, and 0 as well in French. Polish, whose plurals have
three forms, adds a `_few` (2–4, 22–24…, not 12–14) and a `_many` form to
every such pair, and keeps `_other` for parity. Missing keys may fall back to English at runtime, but a parity test must
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

One JSON document defines a language: its metadata, plural rules, translations,
date patterns, number words and reading vocabulary. No Rust edit or manual
selector registration is required. Add it to `assets/i18n/` for an embedded
locale, or save it as `<code>.json` in the desktop languages directory and
open Language settings. Embedded locales must have complete key and placeholder
parity; custom documents may omit strings, which fall back to English.
Review layout expansion and use logical CSS properties so future RTL support
remains possible.

#### Locale document contract

Use an existing embedded document as the complete template. Documents have
the same schema whether embedded or personal; they do not inherit another
UI language.

| Field | Contract |
|---|---|
| `code` | Normalized lowercase locale identifier, 2–35 ASCII letters/digits/hyphens; nonempty subtags of at most 8 characters. Custom files cannot claim an embedded code. |
| `name`, `flag` | Native name and flag displayed verbatim; the name must be nonempty. |
| `reference_language` | Reference-dictionary locale, default `en`. It is separate from the UI locale because a new translation does not create a professions, names or places dataset. |
| `translations` | String key/value map with named interpolation placeholders. Unknown keys and mismatched placeholders are rejected in personal files. |
| `plurals.rules` | Ordered rules, each with a category `suffix` and an AND-list of `conditions`. First matching rule wins. |
| `plurals.default` | Category when no rule matches. Categories are `_zero`, `_one`, `_two`, `_few`, `_many`, `_other`; embedded parity checks derive their additional keys from these declarations. |
| Plural condition | Optional nonzero `modulo`, inclusive `min`/`max`, and `exclude` pairs of inclusive ranges, evaluated after modulo. |
| `dates.months`, `dates.months_with_day` | Twelve full month names, standalone and when accompanied by a day. |
| `dates.display`, `dates.numeric` | Ordinary UI patterns for year only, month/year, and full date. The tree chooses precision and named/numeric presentation; the locale controls order and separators. Numeric day and month are zero-padded. Abbreviated and non-Gregorian names remain translation keys. |
| `dates.long`, `dates.short` | The written-date tool's three corresponding patterns, with word numbers or numeric day/year. |
| Date placeholders | `{year}`, then `{month}` when present, then `{day}` for a full date; each pattern must declare exactly its applicable placeholders. |
| `dates.years`, `dates.days` | Explicit word forms indexed by value: 4000 and 32 entries, with index zero unused. They preserve historical year pronunciation and inflected date ordinals without language-specific Rust rules. The tool's supported year range remains 1–3999. |
| `dates.reading` | Word-to-number-piece map: `{ "kind": "add", "value": 2 }`, or `hundred`, `thousand`, `and`. The shared reader folds and segments compounds and merges vocabularies from the available locales. |

Malformed files are excluded and reported by filename with a localized error
in settings. Files are sorted by filename. Reopening the section refreshes
edited definitions as well as additions/removals; stable locale handles do not
retarget when the file order changes. Standard GEDCOM values and the Latin
historical-date engine remain protocol/domain logic, not UI locale dispatch.

### 3.6 Comparing words

Wherever a typed word meets a written one — search, homonyms and duplicates,
media tags, entry suggestions, the reference dictionaries, the place field,
the written-date reader — both are compared in one folded form, produced by
one function, `oxidgene_core::search::fold_words`, on the server and in the
interface alike: lowercase; every letter decomposed and stripped of its
accents, in any script (`é`, `ễ`, `ά`); the Latin letters that do not
decompose spelled out (`ł` as `l`, `ø` as `o`, `æ` as `ae`, `œ` as `oe`, `ß` as
`ss`, `þ` as `th`); every character other than a letter or a digit a word
break. No component folds text its own way. The one variant, `fold_text`,
lets a key another system defines keep its own separators — Geneanet's person
references ([Geneanet media import](geneanet-media-import.md) §6).

What is stored folded — the search rows, media tag keys — must be rebuilt
when the folding changes. The search rows are, through a
`PROJECTION_SCHEMA_VERSION` bump. Stored keys are not converted while the
product is unreleased: its databases are recreated and their genealogy
reimported ([Architecture §9](architecture.md)); once it is released, a
migration re-keys them in place.

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
  "request_id": "optional-correlation-id"
}
```

`error` is the stable code and `message` the safe English message its code
always carries — never the internal cause, which may hold SQL, filesystem
paths, secrets, or genealogy. `request_id` is present only on an unexpected
error (a `500`), and names the server log line that records it.

| Status | Code | Meaning |
|---|---|---|
| 400 | `validation_error` | Invalid field, format, or business input. |
| 400 | `gedcom_error` | Invalid or unsupported genealogy input. |
| 401 | `unauthenticated` | The desktop's embedded server received a request without its launch token (§7.1); later, any missing authentication. |
| 403 | `forbidden` | A browser page on another origin attempted a write, or a request named a host the server does not answer under (§7.1); later, a viewer lacking access. |
| 404 | `not_found` | Missing or soft-deleted resource. |
| 409 | `conflict` | State conflicts with an invariant or concurrent change, such as starting an import or export while another runs on the tree. |
| 413 | `payload_too_large` | Request body exceeds the route's limit. |
| 415 | `unsupported_media_type` | Payload format is unsupported, such as a body without a JSON content type on a JSON route. |
| 422 | `not_an_archive_citation`, `no_adapter` | A source asked for its archive target is not an archive citation, or cites an act no catalogued collection holds ([API Contract](api.md#sources)). |
| 503 | `timeout` | The standalone server's time limit for the request ran out before a response (§7.1); a `504 timeout` is an archive portal's. |
| 502 | `unexpected_response`, `challenged`, `unreachable` | An archive portal answered not as its adapter expects, answered with an anti-bot challenge, or could not be reached. |
| 504 | `timeout` | An archive portal did not answer in time. |
| 500 | `database_error` | Persistence failed unexpectedly. |
| 500 | `io_error` | Storage or transport I/O failed unexpectedly. |
| 500 | `internal_error` | Unclassified server failure. |

Every client error carries this envelope, including a request the framework
refuses before a handler runs (a malformed identifier or body, an unsupported
content type, an unknown route). A body that is valid JSON but does not match
the operation's shape is a `400 validation_error` like any other invalid
input.

The [API Contract](api.md) identifies which codes each operation can return and
documents deviations that still exist in the implementation.

### 4.3 GraphQL mapping

GraphQL uses the standard `errors` array. Each error carries the equivalent
uppercase code (`VALIDATION_ERROR`, `GEDCOM_ERROR`, `NOT_FOUND`, `CONFLICT`,
`DATABASE_ERROR`, `IO_ERROR`, `INTERNAL_ERROR`, and the archive codes
`NOT_AN_ARCHIVE_CITATION`, `NO_ADAPTER`, `UNEXPECTED_RESPONSE`, `TIMEOUT`,
`UNREACHABLE`) and optional request ID in `extensions`. Mutation payloads do not invent a second error model.

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

A `validation_error` names neither the field nor the value it refused: its
message is the generic one of its code on both surfaces. Forms therefore
validate their fields before submitting, retain submitted values, and focus
the first invalid field; a `validation_error` the server still returns appears
as a summary, localized from its code.

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

Native console logs are `text` by default and `json` when
`OXIDGENE_LOG_FORMAT=json` (desktop: also `--log-format json`). A JSON line is
one flat object per event — `timestamp`, `level`, `target`, `message`, and the
event's own fields — with no span context, and neither format writes ANSI
colour codes unless the destination is a terminal. An invalid format fails
startup with a stable category and does not echo the value. The console layer
receives events only, never spans, so it never enables a span callsite on its
own; an event that needs request or job context records it as fields of its
own (route template and method, job kind and format, error category).

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

HTTP request spans and logs record the method, the route template and the
aggregate response outcome, never the raw URI or query string. Search terms and
resource identifiers may be carried by either and are therefore treated as
genealogy rather than routing metadata: a route template replaces every
identifier with `{id}`. A failed request is logged by its error category and
status, never by the error's message, which can repeat the URL or the server's
answer. Configuration failures likewise log a stable category without echoing
the rejected value.

### 5.4 Operational behavior

- Panic and unexpected error boundaries attach a correlation ID and preserve
  the source chain internally without returning it to clients. The server and
  desktop routers catch a panicking handler and answer `500 internal_error`
  with a fresh `request_id`, logged as `request panicked`; the panic payload
  is free text and is neither logged nor returned.
- An unexpected-error event (`request failed`, `GraphQL request failed`,
  `request panicked`, `background job failed`) records its correlation ID, its
  public code, and `error.kind`: a category from a fixed list — the database
  failure class (`busy`, `connection_acquire`, `unique_violation`, …), the I/O
  error kind, or `panic`/`cancelled` for a blocking task — never the error
  message, which may carry SQL values, paths, or genealogy. Request events add
  the route template and method; job events add the job kind and format.
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
  headers. The OTLP log bridge takes events only, at the process's log filter,
  and never those of the exporter's own transport (`h2`, `hyper`, `tonic`,
  `tower`, `reqwest`) or of the OpenTelemetry SDK, which would otherwise feed
  the export from itself. Because every console event is also an OTLP log
  record, a pipeline collecting both ships each event twice.
- Every native runtime reports a distinct `service.name` and its package
  version. Incoming HTTP `traceparent` headers are extracted with W3C Trace
  Context so calls remain connected across trusted gateways and services.
- A browser bundle built with a non-empty `OTEL_EXPORTER_OTLP_ENDPOINT` exports
  client spans over OTLP/HTTP and injects W3C Trace Context into every request
  the typed API client sends to the backend (never into a remote download).
  Like the native runtimes, it exports only the application's own spans
  (`oxidgene_*` targets, `INFO` and above), never those of its dependencies,
  and sends the spans that end within a second of each other in one request.
  The API request span parents SeaORM spans and
  persists its context with a queued background job; the worker restores that
  context before executing the job, so durable work remains in the originating
  trace after process and time boundaries.
- `OXIDGENE_LOG_LEVEL` independently configures each process using
  `EnvFilter` syntax. `OTEL_EXPORTER_OTLP_ENDPOINT` is likewise process-local,
  so server, worker, and desktop may use different collectors or disable
  export independently. `RUST_LOG` is not part of the configuration contract.
- HTTP spans use the Axum route template, method, status, and duration. They do
  not record raw URIs, query strings, request or response bodies, headers,
  resource IDs, or unmatched paths. The `http.server.request.duration`
  histogram carries the same bounded labels: `http.request.method` (`_OTHER`
  for a non-standard method), `http.route` (`unmatched` when no route
  answered), and `http.response.status_code`.
- `/healthz` is served outside the trace layer on the server and the desktop:
  probes call it every few seconds and produce neither spans nor metric
  points.
- The final Axum router owns one server span for every matched REST and GraphQL
  request; route modules do not duplicate that span in each handler. GraphQL
  adds a `graphql.execute` span recording `graphql.operation.type` (`query`,
  `mutation`, or `subscription`; never the client-chosen operation name), and
  one `graphql.resolve` child per non-introspection root field, recording only
  parent type and field name. Nested fields are not spanned: a span per field
  of every row of a list buries the operation's boundaries — its root fields
  and their database calls — under volume. Long-running import, export,
  projection, and media workflows add `skip_all` service spans. SeaORM spans
  remain the database leaves below those boundaries. Trivial glue functions
  are not individually spanned because that would add volume without a useful
  operational boundary.
- GEDCOM generation divides its `export.serialize` service span into
  `export.build_model`, `export.write`, and `export.inject_extensions`. These
  spans record aggregate entity counts and
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
- The typed API client opens one `http.client.request` span per request to
  the backend, displayed as the method and route template (`GET
  /api/v1/trees/{id}/persons/{id}`) and recording `http.route`; a remote
  download is named by its method only. A response served from the client's
  cache is a `ui.response.cached` span, marked when the request waited for an
  identical one already in flight, and that wait is a `ui.request.wait` span
  of its own, so neither looks like data arriving from nowhere.
- Client response processing separates `ui.response.read` from
  `ui.response.deserialize`. Expensive synchronous transformations use
  `ui.compute`: pedigree-data construction, the layout of each chart view
  (`pedigree_layout.<view>`, and `pedigree_layout.mini` for a fragment), a
  person's profile, the statistics charts, map and basemap, and a gallery's
  assembly. These spans use their stable compute label as the OpenTelemetry
  display name. A computation runs under the load in progress, else under the
  operation running, if any; with neither it opens no span, rather than an
  orphan root. Response
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
- Tree-wide analyses own a service span: `statistics.load`, `anomalies.load`,
  `anomalies.unlocated_places`, `duplicates.load`, `ancestry.load`, and
  `kinship.find`. Their CPU-bound work runs on the blocking pool, never on an
  async worker, under a child span created before the hand-off and entered on
  the blocking thread, since the pool does not carry the caller's span:
  `profile.decode` for the projection payloads, `statistics.compute`,
  `anomalies.compute`, `anomalies.locate`, `duplicates.compute`,
  `ancestry.compute`, `kinship.walk`, and `reference.places.locate` inside
  them. Place suggestions run under `reference.places.search`. These spans
  record aggregate counts (persons, places, links, the suggestion limit)
  only, never a query, name, or identifier.
- The person page's reads are spanned as `person_detail.load` and
  `gallery.load` (media and vignette counts). Inline pictures for the web
  build load under `images.load` (image count), each base64 encoding under
  `image.encode_base64` (byte length). A vignette crop runs on the blocking
  pool under `media.crop`, split into `media.decode` and `media.encode`, with
  input and output byte lengths.
- SeaORM spans include the time spent waiting for a pooled connection without
  separating it, and a transaction waits before its `begin` span opens. The
  pool reports each acquisition's wait as a `sqlx::pool::acquire` event at
  `trace` level, which native export records in the
  `db.client.connection.wait_time` histogram (seconds) and nowhere else. The
  desktop's SQLite writer is one connection, so this is where concurrent
  writes queue; reads have a pool of their own
  ([Architecture §4](architecture.md)).
- Every user-initiated import, Geneanet import, and export owns a root span of
  its own: `ui.import`, `ui.geneanet_import`, and `ui.export`. So does every
  other write a single button starts that would otherwise reach the collector
  as bare HTTP spans: deleting a tree (`ui.delete_tree`), duplicating one
  (`ui.duplicate_tree`), merging two persons (`ui.merge`), recording two
  persons as distinct (`ui.mark_distinct`), restoring an earlier version
  (`ui.restore`), attaching a media to a person, couple or event or detaching
  it (`ui.media_link`), and printing (`ui.print`). These operations outlive the
  render that started them, so they never attach to the load span of whichever
  screen was active. An import or export root records the requested format
  only; the others record nothing.
- One run of the Geneanet assistant is one trace. Its root is opened by the
  first step the user drives, held for as long as the assistant is on screen,
  and closed when the import lands or the assistant is abandoned — so a reader
  looking for an import finds it as a single trace rather than one per button.
  The root therefore reaches the collector only when the run ends; every step
  is exported as it completes and already carries the trace identifier, so a
  waterfall fills in as the work happens and only the outermost span arrives
  last.
- Each action root owns bounded phase children named after it:
  `ui.import.upload` and `ui.import.poll`; `ui.geneanet_import.read`, `.write`,
  `.inspect`, `.index`, `.connect`, `.preview`, `.collect`, `.upload`, `.poll`,
  `.session_encode`, `.session_decode`, and `.homonyms` (the namesakes of the
  persons it created, looked up once it has landed, so in a short
  `ui.geneanet_import` trace of their own); `ui.export.request`,
  `.queue`, `.poll`, and `.save`; `ui.print.measure`, `.prepare`, and
  `.dialog`. A browser download the page hands to the user agent has no phase
  span because the transfer is outside the application.
- Import traces continue on the server with a format-specific root and bounded
  phase children for parsing, upload or collection, media preparation,
  persistence, projections, and job polling where applicable. Export traces
  likewise expose `export.load`, `export.serialize`, `export.media`,
  `export.package`, `export.publish`, and `export.job` for queued archives. They
  record format, depth, aggregate size, count, phase, and outcome dimensions
  only. User filenames, filesystem paths, external account details, genealogy,
  media metadata, SQL, and payloads are never span attributes.
- Native span export is parent-based: a span follows its parent's sampling
  decision, and a root span is kept unless it is a SeaORM call. A database
  call made outside any operation — the job worker polling for work every
  second, a startup query — would otherwise be a trace by itself. Work that
  runs outside a request owns a root span instead: `startup.migrate`,
  `reference.preheat`, `purge.sweep` (the startup purge
  of trees a previous run left), and `mcp.tool` (with the tool name, one of
  a fixed set).
- One page load is one trace, end to end: `ui.<page>.load` →
  `ui.resource.load` → `http.client.request`, whose W3C `traceparent` the
  server extracts → `http.server.request` → service spans → compute spans on
  the blocking pool → `sea_orm.*`. A resource future is polled inside its
  resource span (re-entered on every poll, including on the browser's
  single-threaded executor), so the client span is that span's child. On the
  server, work that leaves the request's task stays in its trace: every
  blocking hand-off enters a span created by the caller before the hand-off
  (`service::blocking`), and work queued for later carries the W3C context
  with it — a background job in its `trace_parent`/`trace_state` columns,
  restored by `background_job.process`, and a tree purge to its `purge.tree`
  span. Only genuinely detached loops start traces of their own, under the
  named roots above. `oxidgene-api/tests/trace_continuity_test.rs` sends a
  `traceparent` to a pedigree, a person detail bundle, statistics,
  anomalies, a media upload, a vignette crop, a GraphQL query and an import
  job, and asserts that every exported span belongs to the incoming trace
  with an unbroken parent chain and that no span opened a trace of its own;
  a UI test asserts the client span's parent and the header it sends.
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
- Copies of genealogy the application makes for its own work do not outlive
  that work: export artifacts, job inputs and payloads, and the photos a
  loaded Geneanet session stages are deleted when used or after a bounded
  time, and a tree's purge deletes what belongs to it ([Architecture §6](architecture.md)).
- Showing a tree makes no request the user did not ask for. Note bodies keep
  no image: a remote one would be fetched by every reader's browser — an
  imported file can carry a tracking pixel — and a relative one would resolve
  against the application's own origin. Links stay, minus any relative
  target, since nothing is fetched until somebody follows one. The rule is
  enforced when a note is written ([Data Model §Note](data-model.md#note)).
  The web fonts ship with the application rather than coming from a font
  service, and GraphiQL, whose page loads from a CDN, is served only where a
  deployment enables it.

### 7.1 Backend exposure before authentication

Until authentication and per-tree authorization are implemented and enforced
equally by REST, GraphQL, exports, search, and direct media reads, the backend
must never be exposed directly to an untrusted network:

- standalone and development servers bind to loopback by default;
- host-published container ports bind to loopback; a container may listen on
  its private network only when a trusted same-origin gateway is its sole
  ingress;
- CORS uses an explicit trusted frontend origin and never `*`, and the
  standalone server refuses any state-changing request whose `Origin` is
  present and different (`403 forbidden`) — CORS withholds responses from
  other origins but lets their form and multipart posts through unasked;
- the standalone server bounds what one client can hold. A request must
  answer within five minutes, an upload (`POST …/import-jobs`,
  `POST …/media/upload`) within an hour, and a request body that sends
  nothing for a minute is cut off; past its limit a request answers
  `503 timeout`, while a response already streaming is never cut. Every file
  intake — an import job's upload, a media upload on either surface, a
  Geneanet session decode or import staging — takes one of two process-wide
  slots before reading its body and keeps it until the bytes are stored;
  further intakes wait. The desktop's embedded server has the slots but no
  time limits;
- both servers answer only under a host name they are known by: loopback
  names, plus the CORS origin's host and `OXIDGENE_ALLOWED_HOSTS` on the
  standalone server. Any other `Host` answers `403 forbidden`. A page that
  rebinds its own DNS name to the server's address is same-origin with it
  and passes CORS and the origin check, but its browser still names the
  page's domain as the host. Health probes are answered whatever the host;
- the desktop's embedded server answers only requests that carry a bearer
  token generated at each launch and handed to the application's own client
  (`401 unauthenticated`). A loopback port is reachable by every local account,
  process and browser page — including a page that rebinds its DNS name to
  `127.0.0.1` to read responses as its own. Only the API description
  (`/api/v1/openapi.json`) is exempt. The client sends the token, and its trace
  context, to the backend alone, never to a remote address a download names. App Settings shows the token so the user
  can hand it to an external client of their choosing, behind a warning that
  it grants read and write access to every tree until the application quits;
- every response says it is not to be sniffed, framed or followed with a
  referrer. The web frontend's nginx sends a Content-Security-Policy that
  allows scripts from its own origin only (plus `'wasm-unsafe-eval'` for the
  WebAssembly and `'unsafe-eval'` for Dioxus' `document::eval`, never an
  inline script), connections to its own origin and the API's and OTLP
  endpoint's, no plugin, no `<base>`, no form submission and no framing
  (`frame-ancestors 'none'`, `X-Frame-Options: DENY`), with
  `Referrer-Policy: no-referrer` and `X-Content-Type-Options: nosniff`. The
  API adds the last three to every response, and
  `default-src 'none'; frame-ancestors 'none'` to JSON. The desktop window
  loads no plugin and honours no `<base>`;
- stored files are served with `X-Content-Type-Options: nosniff` and, except
  for PDFs, `Content-Security-Policy: sandbox`, so a file cannot run as a page
  of the origin that serves it; the type of a held file is the one sniffed from
  its bytes and cannot be relabelled;
- UI markup never places a backend URL in `href`, `src`, `action`, redirects,
  new-window navigation, or other user-visible navigation targets, except the
  App Settings API section's links to the API description and, in the web
  build, the GraphiQL page, neither of which holds tree data;
- media, thumbnails, crops, archives, and exports are fetched through the
  typed client, then exposed to the rendering engine as local `data:` or
  `blob:` resources or written through a platform save dialog;
- a shell that answers on an origin of its own may serve them from it instead —
  the desktop build registers a handler under `/oxidgene-media/…` and proxies
  it onto the embedded server. That is the application's own address, not the
  backend's, so nothing about this rule is relaxed: the markup still carries no
  backend URL, and the picture cannot be reached by anyone the shell has not
  already let in. The handler forwards, with the launch token, only the paths
  its image host builds — a medium's thumbnail or file, a vignette's image —
  and answers `404` to anything else, and note bodies keep no image and no
  relative URL that could point at it (§7). It is preferred where available, because the engine can then
  cache, lazily load and decode pictures the way it does for any other image —
  see [API §Image sources](api.md).

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

## 9. Specification format

`docs/` is a conformant
[Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/open-knowledge-format/blob/main/SPEC.md)
bundle. Every change to it keeps it conformant.

### 9.1 Organization

- Each routed page has exactly one UI specification. Shared UI behavior lives
  only in [Common UI](ui-common.md); a modal or workflow has one canonical
  specification and no versioned or per-tab companion file.
- Specifications link to each other with standard relative markdown links.

### 9.2 Frontmatter

Every specification except `index.md` starts with a YAML frontmatter block:

| Key | Content |
|-----|---------|
| `type` | Required. A descriptive kind, such as `UI Specification` or `API Reference`; reuse an existing value when one fits. |
| `title` | The document's H1 heading. |
| `description` | One sentence summarizing the document. |
| `tags` | A YAML list starting with `oxidgene`. |
| `generated` | `{ by: <actor>, at: <ISO 8601 UTC datetime> }` for the last meaningful content change; updated whenever the body changes in substance. |

- Actors follow OKF §7: `human:maintainer` for human-written content and
  `<tool>/<model>` for agent-written content, for example
  `claude-code/claude-opus-5-5`. An actor never contains a personal or account
  name.
- The retired v0.1 forms are not used: no `timestamp` key and no body
  `# Citations` list.
- A specification derived from external material, such as a reverse-engineered
  API or a third-party format, records it in `sources` entries with an `id`,
  and attributes individual claims with markdown footnotes keyed by that `id`.
- The optional `verified`, `status`, and `stale_after` keys are used only as
  OKF §5 defines them. A `verified` entry is never added for a review that did
  not happen.

### 9.3 Reserved files

- `index.md` and `log.md` are reserved and never hold a specification.
- `index.md` carries only `okf_version: "0.2"` as frontmatter. Its body
  contains only `#` section headings followed by
  `* [Title](file.md) - description` entries. Each entry's description matches
  the linked document's frontmatter `description` word for word, and every
  specification is listed exactly once.
