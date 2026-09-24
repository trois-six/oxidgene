---
type: "API Specification"
title: "Assistant Access (MCP)"
description: "Model Context Protocol server built into the desktop binary: one tree per session, read-only tools, stdio transport, launch, consent, and its relation to REST and GraphQL."
tags: [oxidgene, specification, api, mcp, privacy]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-24T00:00:00Z }
---

# Assistant Access (MCP)

> Part of the [OxidGene Specifications](index.md).
> See also: [API Contract](api.md) · [Architecture](architecture.md) ·
> [Tree Settings](ui-settings.md) · [Cross-cutting Rules](cross-cutting.md)
>
> **Status: planned.** This document is the contract the first delivery
> implements. Delivery is tracked in [Roadmap §6](roadmap.md).

---

## 1. Purpose and scope

An AI assistant (Claude Desktop, Claude Code, or any other
[Model Context Protocol](https://modelcontextprotocol.io) client) can read a
genealogy tree through a set of tools the desktop binary serves. Typical uses
include answering questions about the tree, drafting a biography from a
profile, and finding inconsistencies.

The first delivery is:

- **read-only**: no tool changes stored data;
- **desktop-only**: served over stdio by `oxidgene-desktop`, with no network
  listener;
- **scoped to one tree** per session (§3).

Mutating tools and a network transport are later phases (§10).

---

## 2. Relation to REST and GraphQL

MCP is an adapter over the product operations the [API Contract](api.md)
defines. It is not a third mirror of that contract.

- Every tool maps to one existing product operation and calls the same
  service or repository code as its REST and GraphQL mappings. It has the
  same validation, domain errors, and read-after-write guarantees.
- MCP never offers a capability that REST and GraphQL lack. A new
  capability needed by an assistant is added to REST and GraphQL first,
  with their tests, and only then exposed as a tool.
- Parameters keep the REST names and the REST value spellings (`snake_case`,
  stable English enum values). Input schemas are derived from the same Rust
  types the REST surface deserializes, such as
  `oxidgene_db::repo::PersonSearchFilters`, so each parameter list is declared
  once.
- A tool schema may narrow a parameter's range so a result fits in a model's
  context (§5). It never widens a range or adds a behavior.
- Only a curated subset of operations is exposed. Operations about files,
  media bytes, imports, exports, Geneanet, projection maintenance, and the
  local-file capability are not tools.

---

## 3. Tree scope

One MCP session serves exactly one tree. The tree is fixed when the process
starts (§4) and cannot be changed during the session.

- No tool takes a `tree_id` parameter, and no tool lists, names, or reaches
  another tree. The bound tree's ID is applied to every operation.
- An ID that belongs to another tree returns `not_found`, exactly as REST and
  GraphQL do for cross-tree IDs, so the session cannot reveal that another
  tree exists.
- The tree is resolved on every call, not only at startup. If the tree is
  soft-deleted while a session runs, every later tool call returns
  `not_found`.
- A client that needs several trees configures one server entry per tree.

---

## 4. Transport and process

### 4.1 Launch

```text
oxidgene-desktop mcp --tree <tree-uuid>
```

The `mcp` subcommand runs a headless MCP server on standard input and output
(JSON-RPC over stdio). The process exits when its input closes. There is one
executable per platform ([Architecture §8.2](architecture.md)); the
subcommand is part of it, not a separate binary.

### 4.2 Startup

1. Resolve the platform data directory, as the desktop application does, and
   open `oxidgene.db` read-write **without creating it**. A missing database
   is a startup error.
2. Apply the same migrations as desktop startup. With the single consolidated
   migration ([Architecture §9.2](architecture.md)), this does nothing on a
   database the desktop has already opened.
3. Check that `--tree` names an active tree. A malformed ID or a missing or
   soft-deleted tree is a startup error.
4. Build a `ProfileService` over the connection and serve.

A startup error is reported on standard error with a generic message (no
path, ID, or genealogy) and a non-zero exit code.

### 4.3 What the process does not do

The MCP process does not duplicate work the desktop process owns:

- It opens no HTTP listener and no WebView.
- It starts no background job worker and never calls
  `BackgroundJobRepo::requeue_running`, which would requeue a job the open
  desktop application is still running.
- It starts no purge worker, so it does not build an `AppState`, whose
  constructor spawns one.
- It opens no media store.

### 4.4 Output discipline

Standard output carries protocol messages only. Logs and traces go to
standard error or, when configured, to OTLP
([Cross-cutting Rules](cross-cutting.md)). Anything printed to standard
output breaks the session.

### 4.5 Running beside the desktop application

The desktop application may be open on the same database at the same time.
SQLite runs in WAL mode, so readers in either process do not block each other.
The only writes the MCP process performs are lazy rebuilds of stale person
projections ([Data Model §4](data-model.md)). They contend for the writer lock
like any other write and wait for SQLite's busy timeout. A read-only session
never makes the desktop's caches stale.

---

## 5. Tools

Every tool in the first delivery is annotated `readOnlyHint: true`,
`idempotentHint: true`, and `openWorldHint: false`.

| Tool | Parameters | Product operation | Result |
|---|---|---|---|
| `get_tree` | — | `GET /trees/{tree_id}` | The bound tree: name, description, SOSA root, and the person the user identified as themself |
| `search_persons` | `q`, every `PersonSearchFilters` field, `sort`, `limit`, `offset` | `GET /trees/{tree_id}/persons/search` | `SearchResult` |
| `get_person_profile` | `person_id` | `GET /trees/{tree_id}/profiles/{person_id}` | `PersonProfile` |
| `get_person_by_sosa` | `number` | `GET /trees/{tree_id}/persons/sosa/{number}` | Person with `sosa_number` |
| `get_pedigree` | `root_person_id`, `ancestor_depth`, `descendant_depth` | `GET /trees/{tree_id}/pedigree/{root_person_id}` | `Pedigree` |
| `get_family` | `family_id` | `GET /trees/{tree_id}/families/{family_id}` | Family with spouses, children, and events |
| `list_events` | `person_id`, `family_id`, `event_type`, `first`, `after` | `GET /trees/{tree_id}/events` | Event connection |
| `get_place` | `place_id` | `GET /trees/{tree_id}/places/{place_id}` | Place |
| `get_source` | `source_id` | `GET /trees/{tree_id}/sources/{source_id}` | Source |
| `list_citations` | `person_id`, `event_id`, `family_id`, `source_id`, `first`, `after` | `GET /trees/{tree_id}/citations` | Citation connection |
| `list_notes` | `person_id`, `event_id`, `family_id`, `source_id`, `first`, `after` | `GET /trees/{tree_id}/notes` | Note connection |
| `list_dictionary` | `kind` (`family_names`, `occupations`, `places`, `sources`) | `GET /trees/{tree_id}/dictionary/{kind}` | Values or records with usage counts |
| `dictionary_usage` | `kind`, then `value` (`family_names`, `occupations`) or `id` (`places`, `sources`) | The matching `…/usage` endpoint | `PersonUsageEntry` list |

Bounds:

- Search pages default to `SEARCH_DEFAULT_LIMIT` (25). `ProfileService`
  caps them at `SEARCH_MAX_LIMIT` (100) for every surface.
- Connection pages follow the shared pagination rules (`first` defaults to
  25, maximum 100).
- `get_pedigree` narrows each depth to 0–10 generations, the same range the
  pedigree view offers ([Tree Settings §19](ui-settings.md)).

The first delivery offers no MCP resources and no prompts.

---

## 6. Results and errors

- `structuredContent` is the REST JSON body of the mapped operation, and each
  tool publishes an `outputSchema`. The text content carries the same JSON
  serialized, for clients that ignore structured content. There is no
  MCP-specific representation.
- User and imported content (names, notes, source text) is returned verbatim
  and never translated, as on the other surfaces.
- A domain error is a tool result with `isError: true` whose content is the
  shared error envelope: its stable `code` and safe message
  ([Cross-cutting Rules §4–5](cross-cutting.md)). It carries no SQL, path,
  stack trace, or genealogy.
- Protocol errors, such as an unknown tool or parameters that fail the input
  schema, are JSON-RPC errors.

The server's `initialize` result carries `instructions` for the model, in
English. They say that the session covers one tree, and they describe how to
read dates: a year always comes with its qualifier (`ca 1849`, `< 1917`); a
birth may fall back to the baptism and a death to the burial when the primary
event has no date; and SOSA numbers are relative to the tree's root.

Tool names, descriptions, schemas, and instructions address the model. They
are protocol text, not user-visible UI, so they are written in English and do
not go through i18n.

---

## 7. Consent and privacy

### 7.1 Consent

Nothing is exposed until the user configures an MCP client with the launch
command for a tree. The command is displayed in
[Tree Settings §17](ui-settings.md) together with a warning. Configuring it is
the act of consent, and removing it from the client revokes that consent.

The warning states that everything in the tree becomes readable by the
assistant, including living people, notes, and sources, and is sent to the
model provider the client uses. OxidGene does not control what that provider
retains.

### 7.2 Privacy values are not applied

MCP does not filter records by their `privacy` value, and it does not
withhold presumed living people:

- Privacy is defined against a viewer ([Data Model §1](data-model.md)). A
  local stdio session acts for the owner of the database, like the desktop
  UI, not for a viewer.
- A tree's default privacy is `private`. Resolving it would hide every
  unclassified record and leave the session empty.
- Withholding living people would break the features an assistant is used
  for: relatives, siblings, and date consistency.

When authorization ships, filtering by viewer access happens once, in the
read path that REST, GraphQL, exports, and MCP share. There is no MCP-specific
filter. A future "never send to an assistant" choice, if one is wanted, is a
separate setting, not an overloaded `privacy` value.

### 7.3 Untrusted content

Notes, sources, and names come from imports and are untrusted text for a
model: they may contain instructions (prompt injection). The first delivery
is read-only, so such text cannot trigger a write through OxidGene.

### 7.4 Logging

A tool call logs its tool name, duration, and outcome code. It never logs
parameters or results, because they contain names, places, and dates.

---

## 8. Client configuration

Tree Settings shows the command with the absolute path of the running
executable and the tree's ID filled in. For example, for Claude Code:

```bash
claude mcp add oxidgene-sample -- /opt/oxidgene/oxidgene-desktop mcp --tree 01900000-0000-7000-8000-000000000000
```

Clients that use an `mcpServers` JSON file:

```json
{
  "mcpServers": {
    "oxidgene-sample": {
      "command": "/opt/oxidgene/oxidgene-desktop",
      "args": ["mcp", "--tree", "01900000-0000-7000-8000-000000000000"]
    }
  }
}
```

---

## 9. Build and dependencies

- The server uses [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk),
  the official Rust SDK, behind an optional `mcp` feature of `oxidgene-api`,
  with `default-features = false` and only `server`, `macros`, and
  `transport-io`. The `schemars` derives it needs for input and output
  schemas sit behind optional features of the crates that declare those types
  (`oxidgene-core`, `oxidgene-db`), which `mcp` enables.
- Only `oxidgene-desktop` enables the feature. The standalone server, the
  worker, and the WASM frontend do not compile it, just as the desktop does
  not compile `graphql`.
- The MCP handlers live in `oxidgene-api/src/mcp/` and call the existing
  services and repositories. They contain no business logic.

---

## 10. Later phases

- **Mutating tools.** Each maps to an existing mutation, runs in the same
  transaction, and refreshes projections the same way. It is annotated
  `destructiveHint` so clients ask for confirmation. Deletions come last.
  Writes made by an MCP process appear in an open desktop window only after
  its caches refresh.
- **Network transport.** Streamable HTTP on `/mcp`, merged into the Axum
  router. This is blocked by the rule that the backend is never exposed before
  authentication
  ([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)).
  It requires MCP authorization mapped to the user's per-tree access, `Origin`
  validation, and the same tree scope as §3.
- **Media.** Thumbnails as image content, bounded in size and count.

---

## 11. Verification

- Integration tests run the server over an in-process duplex transport
  against in-memory SQLite, using fictitious data only. They require no
  external client.
- Every tool is tested, including a cross-tree ID returning `not_found` and a
  call made after the bound tree was soft-deleted.
- A test asserts that a session writes nothing but protocol messages to its
  output stream.
- A tool whose operation's behavior changes is re-tested with the REST and
  GraphQL tests of that operation.
