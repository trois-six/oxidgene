---
type: "API Specification"
title: "Assistant Access (MCP)"
description: "Model Context Protocol server built into the desktop binary: read-only tools that each name their tree, stdio transport, launch, consent, and its relation to REST and GraphQL."
tags: [oxidgene, specification, api, mcp, privacy]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-24T00:00:00Z }
---

# Assistant Access (MCP)

> Part of the [OxidGene Specifications](index.md).
> See also: [API Contract](api.md) · [Architecture](architecture.md) ·
> [App Settings](ui-app-settings.md) · [Cross-cutting Rules](cross-cutting.md)
>
> The first delivery is implemented; later phases are tracked in
> [Roadmap §6](roadmap.md).

---

## 1. Purpose and scope

An AI assistant (Claude Desktop, Claude Code, or any other
[Model Context Protocol](https://modelcontextprotocol.io) client) can read the
application's genealogy trees through a set of tools the desktop binary
serves. Typical uses include answering questions about a tree, drafting a
biography from a profile, and finding inconsistencies.

The first delivery is:

- **read-only**: no tool changes stored data;
- **desktop-only**: served over stdio by `oxidgene-desktop`, with no network
  listener;
- **explicitly tree-scoped**: every tool call names the tree it reads (§3).

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

A session can read every active tree in the database. It never picks one
implicitly: each tool call names its tree.

- `list_trees` is the only tool without a tree. Every other tool requires a
  `tree_id` parameter, which its input schema marks as required. There is no
  default tree, and no state carries a tree from one call to the next.
- `tree_id` scopes the whole operation, exactly as the `{tree_id}` path
  segment does in REST. An ID that belongs to another tree returns
  `not_found`, like REST and GraphQL do, so an answer never mixes two trees.
- The tree is resolved on every call. A tree soft-deleted during a session
  disappears from `list_trees`, and later calls that name it return
  `not_found`.

---

## 4. Transport and process

### 4.1 Launch

```text
oxidgene-desktop mcp
```

The `mcp` subcommand runs a headless MCP server on standard input and output
(JSON-RPC over stdio). The process exits when its input closes. There is one
executable per platform ([Architecture §8.2](architecture.md)); the
subcommand is part of it, not a separate binary.

### 4.2 Startup

1. Resolve the platform data directory, as the desktop application does, and
   open `oxidgene.db` read-write **without creating it**. A missing database
   is a startup error.
2. Apply the same migrations as desktop startup. Applied migrations are
   recorded, so this does nothing on a database the desktop has already
   opened.
3. Build a `ProfileService` over the connection and serve.

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
`idempotentHint: true`, and `openWorldHint: false`. Every tool except
`list_trees` requires `tree_id` (§3), and every ID parameter is resolved
within that tree.

| Tool | Parameters | Product operation | Result |
|---|---|---|---|
| `list_trees` | `first`, `after` | `GET /trees` | Tree connection of the active trees, without the REST list's transient import-job fields; the IDs are what every other tool takes as `tree_id` |
| `get_tree` | `tree_id` | `GET /trees/{tree_id}` | The tree's name, description, SOSA root, and the person the user identified as themself |
| `search_persons` | `tree_id`, `q`, every `PersonSearchFilters` field, `sort`, `limit`, `offset` | `GET /trees/{tree_id}/persons/search` | `SearchResult` |
| `get_person_profile` | `tree_id`, `person_id` | `GET /trees/{tree_id}/profiles/{person_id}` | `PersonProfile` |
| `get_person_by_sosa` | `tree_id`, `number` | `GET /trees/{tree_id}/persons/sosa/{number}` | Person with `sosa_number` |
| `get_pedigree` | `tree_id`, `root_person_id`, `ancestor_depth`, `descendant_depth` | `GET /trees/{tree_id}/pedigree/{root_person_id}` | `Pedigree` |
| `get_relation_labels` | `tree_id`, `person_ids`, `family_ids` | `POST /trees/{tree_id}/relation-labels` | Names of the persons, and the spouses of the families with their names; at most 1,024 IDs |
| `list_events` | `tree_id`, `person_id`, `family_id`, `event_type`, `first`, `after` | `GET /trees/{tree_id}/events` | Event connection |
| `get_place` | `tree_id`, `place_id` | `GET /trees/{tree_id}/places/{place_id}` | Place |
| `get_source` | `tree_id`, `source_id` | `GET /trees/{tree_id}/sources/{source_id}` | Source |
| `list_citations` | `tree_id`, `person_id`, `event_id`, `family_id`, `source_id`, `first`, `after` | `GET /trees/{tree_id}/citations` | Citation connection |
| `list_notes` | `tree_id`, `person_id`, `event_id`, `family_id`, `source_id`, `first`, `after` | `GET /trees/{tree_id}/notes` | Note connection |
| `list_dictionary` | `tree_id`, `kind` (`family_names`, `occupations`, `places`, `sources`), `prefix` (`sources` only) | `GET /trees/{tree_id}/dictionary/{kind}` | Values or records with usage counts |
| `dictionary_usage` | `tree_id`, `kind`, then `value` (`family_names`, `occupations`) or `id` (`places`, `sources`) | The matching `…/usage` endpoint | `PersonUsageEntry` list |

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

- `structuredContent` is the REST JSON body of the mapped operation. MCP
  requires an object there, so a body that is a list is wrapped as
  `{ "items": [...] }`. The text content carries the same JSON serialized,
  for clients that ignore structured content. There is no other MCP-specific
  representation.
- The first delivery publishes no `outputSchema`. Output schemas would need
  JSON Schema derives on every domain and projection type the results
  contain; they are deferred to a later phase (§10).
- User and imported content (names, notes, source text) is returned verbatim
  and never translated, as on the other surfaces.
- A domain error is a tool result with `isError: true` whose content is the
  shared error envelope: its stable `code` and safe message
  ([Cross-cutting Rules §4–5](cross-cutting.md)). It carries no SQL, path,
  stack trace, or genealogy.
- Arguments that fail to deserialize against the input schema, such as a
  missing `tree_id`, are rejected by the SDK before the tool runs, as a tool
  result with `isError: true` and a text message. An unknown tool is a
  JSON-RPC error.

The server's `initialize` result carries `instructions` for the model, in
English. They say that every tool except `list_trees` needs a `tree_id`
obtained from `list_trees`, that IDs are never shared between trees, and they
describe how to read dates: a year always comes with its qualifier (`ca 1849`, `< 1917`); a
birth may fall back to the baptism and a death to the burial when the primary
event has no date; and SOSA numbers are relative to the tree's root.

Tool names, descriptions, schemas, and instructions address the model. They
are protocol text, not user-visible UI, so they are written in English and do
not go through i18n.

---

## 7. Consent and privacy

### 7.1 Consent

Nothing is exposed until the user configures an MCP client with the launch
command. The command is displayed in [App Settings §8](ui-app-settings.md)
together with a warning. Configuring it is the act of consent, and removing it
from the client revokes that consent.

The warning states that every tree in the application becomes readable by the
assistant, including living people, notes, and sources, and that what the
assistant reads is sent to the model provider the client uses. OxidGene does
not control what that provider retains.

### 7.2 Privacy values are not applied

MCP does not filter records by their `privacy` value, and it does not
withhold presumed living people:

- Privacy is defined against a viewer ([Data Model §1](data-model.md)). A
  local stdio session acts for the owner of the database, like the desktop
  UI, not for a viewer.
- A tree's default privacy is `private`. Resolving it would hide every
  unclassified record and leave every tree empty.
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

App Settings shows the command with the absolute path of the running
executable filled in. For example, for Claude Code:

```bash
claude mcp add oxidgene -- /opt/oxidgene/oxidgene-desktop mcp
```

Clients that use an `mcpServers` JSON file:

```json
{
  "mcpServers": {
    "oxidgene": {
      "command": "/opt/oxidgene/oxidgene-desktop",
      "args": ["mcp"]
    }
  }
}
```

---

## 9. Build and dependencies

- The server uses [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk),
  the official Rust SDK, behind an optional `mcp` feature of `oxidgene-api`,
  with `default-features = false` and only `server`, `macros`, and
  `transport-io`. The `schemars` derives its input schemas need sit behind the
  optional `schema` features of the crates that declare those types
  (`Sex` and `EventType` in `oxidgene-core`, `PersonSearchFilters` and
  `PersonSearchSort` in `oxidgene-db`), which `mcp` enables. Together they add
  twelve crates to the desktop build, eight of them compile-time only.
- Only `oxidgene-desktop` enables the feature. The standalone server, the
  worker, and the WASM frontend do not compile it, just as the desktop does
  not compile `graphql`.
- The MCP handlers live in `oxidgene-api/src/mcp/` and call the existing
  services and repositories. They contain no business logic.
- Logs go to standard error through `oxidgene_observability::init_to_stderr`,
  the variant of the shared initializer for processes whose standard output is
  a protocol stream.

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
  validation, and the same required `tree_id` as §3. `list_trees` then returns
only the trees the user may read.
- **Media.** Thumbnails as image content, bounded in size and count.
- **Output schemas.** An `outputSchema` per tool, once the result types carry
  JSON Schema derives.

---

## 11. Verification

- Integration tests run the server over an in-process duplex transport
  against in-memory SQLite, using fictitious data only. They require no
  external client.
- Every tool is tested, including a call without `tree_id` failing its input
  schema, an ID from another tree returning `not_found`, and a tree
  soft-deleted mid-session disappearing from `list_trees` and returning
  `not_found` afterwards.
- A test asserts that a session writes nothing but protocol messages to its
  output stream.
- A tool whose operation's behavior changes is re-tested with the REST and
  GraphQL tests of that operation.
