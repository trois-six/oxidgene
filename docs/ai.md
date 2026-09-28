---
type: "Product Specification"
title: "AI Features (Bring Your Own LLM)"
description: "Planned AI features on media — transcript, record and photo analysis, enhancement, colorization — run with LLM providers and keys each user brings, never the host's: provider presets and protocols, where keys live, the backend relay, privacy and consent, API contract, and delivery phases."
tags: [oxidgene, specification, ai, llm, privacy, api, ui]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-28T16:00:00Z }
---

# AI Features (Bring Your Own LLM)

> Part of the [OxidGene Specifications](index.md).
> See also: [App Settings](ui-app-settings.md) · [API Contract](api.md) ·
> [Data Model](data-model.md) · [Cross-cutting Rules](cross-cutting.md) ·
> [Roadmap](roadmap.md)

**Status: planned, nothing is implemented.** This document fixes the
contract the implementation will follow; [Roadmap](roadmap.md) tracks the
phases.

---

## 1. Principles

- **The user pays, never the host.** Each user connects their own providers
  with their own keys, in the application's settings. A deployment ships no
  key, reads none from its configuration or environment, and never calls a
  provider on its own account.
- **Opt-in and invisible otherwise.** With no provider connected, no AI
  action appears anywhere. Nothing is sent to a model without an explicit
  action of the user on one media.
- **Suggestions, never writes.** A model's output is shown for review. It
  reaches the tree only when the user saves it through the ordinary forms and
  endpoints, which record history as they always do. An original is never
  overwritten: an image result becomes a *new* media.
- **Local stays local.** A provider on the user's own machine (Ollama,
  LM Studio) is called so that the data never leaves that machine (§5.3).
- **No weight.** Providers are called over the HTTP, TLS and JSON stack the
  binaries already compile, with hand-written request types. No provider SDK,
  no model runtime, no model weights ship in the binaries (§9).

---

## 2. Features

| Feature | Input → output | Where | Phase |
|---|---|---|---|
| **AI transcript** | page image → text | Media viewer, beside the page transcript field | 1 |
| **Record and photo analysis** | image → structured result | Media viewer, *Analyze* action, review panel | 2 |
| **Enhance** (restore, sharpen) | image → image | Media viewer, *Enhance* action | 3 |
| **Colorize** | image → image | Media viewer, *Colorize* action | 3 |
| **Animate** a photo or a face | image → short video | Media viewer | Deferred (§11) |

### 2.1 AI transcript

- A button **AI transcript** sits beside the page transcript (the page note of
  [Data Model](data-model.md) `Note.media_id`, edited in the media viewer's
  edit panel). It appears only when a model able to read images is chosen for
  transcripts (§4.3).
- Before sending, a small form offers a **language hint** (defaulting to
  "unknown") and a **script hint** (print, modern handwriting, old French,
  Latin, German Kurrent / Sütterlin, other). Both are optional.
- The result fills the transcript field. It stays editable and is **not
  saved**: the user saves as for a typed transcript. When the field already
  holds text, the user chooses **Replace** or **Append** first.
- The request can be cancelled; a spinner shows the provider's name.
- Prompt rules, fixed in code and versioned: a diplomatic transcription that
  keeps line breaks, original spelling, abbreviations and punctuation; an
  unreadable word is written `[illegible]`, a doubtful one is followed by
  `[?]`; the text is never translated, modernised or completed; temperature 0.
- The saved transcript carries no AI marker: the user has reviewed and owns
  it.

### 2.2 Record and photo analysis

- The *Analyze* action returns, for a document: its type (from
  `DocumentCategory`), the language, a translation into the interface
  language, a summary, and the persons (with their roles), dates (with their
  qualifiers: about, before, after…), places, events and relationships it
  mentions. For a photo: an estimated period, the clues it rests on
  (clothing, setting, photographic process) and identification hints.
- The model is asked for JSON against a schema fixed in code, with
  structured output where the protocol supports it and tolerant parsing
  otherwise; an output that does not validate is an error (`ai_output_invalid`),
  never half-applied.
- The review panel lists each extracted item with an action, and applies
  nothing by itself:
  - a person → match an existing person (the shared homonym picker) or open
    the person form prefilled;
  - an event → open the event form prefilled, date qualifiers kept;
  - a union → open the union form prefilled;
  - the media → propose its category and links to the events it documents;
  - the translation or the summary → copy into the media note on request.

### 2.3 Enhance and colorize

- The result opens **side by side** with the original. **Keep** stores it as a
  new media derived from the original (§6); **Discard** throws it away.
  Nothing is stored before *Keep*.
- The derived media is labelled *AI-generated* wherever it is shown, and
  names the operation and the model.
- The dialog states plainly that generative models may invent detail, a face
  in particular, and that the result is an illustration, not a source.

---

## 3. Providers

### 3.1 Presets

Providers are chosen from presets, grouped as users know them:

| Group | Presets |
|---|---|
| Direct | OpenAI, Anthropic, Google Gemini, Azure OpenAI, Amazon Bedrock |
| Gateways and routers | OpenRouter, LiteLLM Proxy, Portkey, Nebius, Bifrost |
| Self-hosted and custom | Ollama, LM Studio, OpenAI-compatible (any URL) |

A preset fixes the protocol, the default base URL, the authentication style
and which fields the connect dialog asks for (Azure: resource URL,
deployment, API version; Bedrock: region). The preset list lives in one
registry in code; this table describes it and is not a second source of
truth.

### 3.2 Protocols

| Protocol | Serves | Used for |
|---|---|---|
| OpenAI Chat Completions | OpenAI, Azure, every gateway, Ollama, LM Studio, OpenAI-compatible | image → text, analysis |
| OpenAI Images (edits) | OpenAI, compatible gateways | enhance, colorize |
| Anthropic Messages | Anthropic | image → text, analysis |
| Gemini `generateContent` (API key) | Google Gemini | image → text, analysis, image output |
| Bedrock Converse (API key; SigV4 later) | Amazon Bedrock | image → text, analysis |

Phase 1 implements the first and the third, which serve eleven of the
thirteen presets (twelve through Gemini's OpenAI-compatible endpoint). Streaming is not used before phase 4.

### 3.3 Models and capabilities

- The connect dialog lists the provider's models when the protocol can
  (`GET /models`), and always accepts a model name typed by hand.
- The user marks what each chosen model can do: **reads images**,
  **edits images**. A known model is pre-marked from a small table in code;
  the user can correct it.

---

## 4. Settings

### 4.1 App Settings section

A new **AI** section in [App Settings](ui-app-settings.md), listed in its
left navigation:

- **Connected providers**: one card per connection (label, preset, base URL,
  the last four characters of the key, models), with *Test*, *Edit* and
  *Remove*.
- **Add a provider**: the preset grid of §3.1, each card with *Connect*. The
  connect dialog asks for the preset's fields and the key, *Test connection*
  (§7.1) before saving, then the models.
- **Models per feature**: one choice for *transcript and analysis* (a model
  that reads images) and one for *image editing*.
- **Data notice**, always visible: what is sent (the image of the media
  acted on, and nothing else of the tree unless §2.2's context is added
  later), to which host, and that the provider's own terms and retention
  apply.
- Several connections may use the same preset (two OpenAI keys, for
  instance), each with its own label.

### 4.2 Where keys live

The key belongs to the user and is stored on the user's side, never by the
host:

| Build | Storage | Reach |
|---|---|---|
| Web | The browser's local storage for the application's origin | This browser profile only; another browser or device must be connected again |
| Desktop | The WebView's local storage in the user's application data directory | This computer's user account |

- The settings export, if any, never includes keys.
- *Remove* deletes the connection and its key at once.
- This is the only choice available before [EPIC G](general.md): the MVP has
  no accounts, so "the user" is the browser profile or the desktop account.
  With EPIC G, keys move to per-account storage on the server, encrypted at
  rest with a key the database does not hold, and are never returned by any
  endpoint once saved. The request contract of §7 does not change: the
  relay then reads the key from the account instead of the request.
- Local storage is readable by any script running on the application's
  origin. The application's content-security and markup rules
  ([Cross-cutting Rules §7.1](cross-cutting.md)) are what protect it; no AI
  change may weaken them.

### 4.3 Consent

- Connecting a provider shows its host and asks for consent before the first
  request; the consent is stored with the connection.
- Every AI action names the provider and the host it sends to, or says
  *stays on this computer* for a loopback address.
- A media, or a person it is linked to, whose resolved privacy is not public
  asks for a second confirmation before being sent to a remote provider.
  (Privacy is not enforced against viewers in the MVP, but it records the
  user's intent, which a transfer to a third party must respect.)

---

## 5. Transport

### 5.1 Backend relay (default)

The client never talks to a remote provider directly. It asks the backend,
which:

1. reads the media bytes from the media store itself (the client does not
   upload the image again), downscales them to a long edge of 2,048 px
   (transcript, analysis) or the provider's limit (image editing), and
   encodes them;
2. calls the provider with the connection and key carried **in the request
   body** (§7), holding them in memory for that request only;
3. validates the answer and returns it.

Remote providers are reached this way because most refuse browser origins
(CORS), and because the image never has to leave the backend in full size.

### 5.2 What the relay never does

- It never stores, logs, traces or echoes a key, a prompt, an image or a
  model answer: not in `tracing` fields, OpenTelemetry spans, error bodies,
  audit entries, background-job payloads or crash reports. Request bodies of
  the AI endpoints are excluded from any request logging.
- It never reuses a key for another request, and never runs an AI call
  without a user's request carrying the key.
- It never follows a redirect to another host.

### 5.3 Local providers

- **Desktop**: the backend runs on the user's computer, so the relay reaches
  `http://127.0.0.1:11434` (Ollama) or `http://127.0.0.1:1234` (LM Studio)
  like any provider. Plain HTTP is allowed for loopback only.
- **Web**: the backend cannot reach the user's own computer. For a
  connection whose base URL is a loopback address, the client calls the
  provider **directly from the browser** instead, with the image it already
  displays; the provider must allow the application's origin (Ollama's
  `OLLAMA_ORIGINS`, LM Studio's CORS option), and the connect dialog says so.
  Only local providers are called this way, and only for text output. A
  deployment whose content-security policy restricts `connect-src` must
  allow those loopback origins for this to work; the section says so when
  the call is blocked.

### 5.4 Outbound protection

A relay that calls a URL the user typed must not become a way into the
host's own network:

- On the **web server**, the relay refuses a base URL that resolves to a
  loopback, private, link-local, unique-local or cloud-metadata address,
  checks every address the name resolves to, and connects to the address it
  checked. Only `https` is accepted.
- The operator may disable the relay entirely (`ai.enabled = false` in the
  server configuration, default `true`): the section and every AI action then
  show that AI features are off on this server. The operator also sets the
  request timeout and the maximum image size. These are the only AI settings
  the operator has; none is a key.
- The desktop relay has no destination restriction: it acts for the only
  user of the machine.

### 5.5 Limits

- Timeout 180 s for image → text and analysis, 300 s for image editing;
  cancellation from the client aborts the upstream request.
- One media per request; a document's pages are sent one by one.
- Provider errors map to the codes of §7.3 with the provider's message
  stripped of anything that could echo the key.

---

## 6. Data model

Planned changes, phase 3, to be moved into [Data Model](data-model.md) when
implemented:

| Entity | Field | Type | Meaning |
|---|---|---|---|
| Media | `derived_from_id` | UUID v7? | FK → Media: the original an AI result was made from. Distinct from `parent_media_id`, which makes a media a page of a document. |
| Media | `derivation` | enum? | `enhanced`, `colorized` (later `animated`) |
| Media | `derivation_model` | string? | Provider preset and model name, for the label; never a key or a URL with credentials |

- Deleting the original does not delete derived media; `derived_from_id`
  then points to a soft-deleted record and the viewer says the original was
  deleted.
- GEDCOM export writes the derivation through the existing OxidGene media
  extensions, so a re-import keeps the label.
- The AI connections themselves are client-side settings until EPIC G and
  have no table (§4.2).

---

## 7. API

REST and GraphQL are symmetric, as everywhere; the desktop, which compiles no
GraphQL, uses REST.

### 7.1 Connection object

Every AI request carries the connection it uses:

```json
{
  "connection": {
    "preset": "openai",
    "base_url": "https://api.openai.com/v1",
    "api_key": "…",
    "model": "…",
    "options": { "azure_deployment": null, "azure_api_version": null, "bedrock_region": null }
  }
}
```

`api_key` is absent for providers without authentication. The server never
returns it.

| Method | Path | Description |
|---|---|---|
| `POST` | `/ai/test` | Check a connection: `{ connection }` → `{ ok, models: [string]? }`; lists models where the protocol can |
| `POST` | `/trees/{tree_id}/media/{media_id}/ai/transcript` | `{ connection, language_hint?, script_hint? }` → `{ text }` |
| `POST` | `/trees/{tree_id}/media/{media_id}/ai/analysis` | `{ connection, language }` → the structured result of §2.2 |
| `POST` | `/trees/{tree_id}/media/{media_id}/ai/image` | `{ connection, operation: "enhance" \| "colorize" }` → `{ mime_type, data }` (base64), not stored |

Keeping a result goes through the existing media upload with the §6 fields.
GraphQL mirrors each as a mutation: `aiTest`, `aiTranscript`, `aiAnalysis`,
`aiImage`, with the same inputs and results.

### 7.2 Server capability

`GET /ai/status` → `{ enabled, max_image_bytes, timeout_seconds }`, mirrored
by the GraphQL query `aiStatus`, so the client knows whether the operator
turned the relay off (§5.4).

### 7.3 Errors

New stable codes, added to [Cross-cutting Rules](cross-cutting.md) when
implemented:

| HTTP | Code | Meaning |
|---|---|---|
| 403 | `ai_disabled` | The operator turned the relay off |
| 403 | `ai_destination_forbidden` | The base URL resolves to a refused address (§5.4) |
| 400 | `validation_error` | Missing field, unknown preset, a model without the capability asked |
| 502 | `ai_provider_rejected` | The provider refused the key or the model (its 401/403/404) |
| 429 | `ai_provider_rate_limited` | The provider's quota or rate limit |
| 502 | `ai_provider_error` | Any other provider failure or unreachable host |
| 504 | `ai_timeout` | No answer within the limit |
| 502 | `ai_output_invalid` | The answer did not validate (§2.2) |

Messages go through i18n on the client; the provider's own message is shown
only as a detail, after redaction.

---

## 8. Privacy

- Genealogy media are sensitive data ([Cross-cutting Rules](cross-cutting.md)):
  a parish register names living people's ancestors, a photo shows living
  people. Nothing is sent without the per-provider consent and the per-action
  wording of §4.3.
- Only the acted-on media is sent: no tree data, names or notes accompany it
  in phases 1–3. Sending context (the persons already linked, to help the
  analysis) would be a later, separately consented option.
- The provider's retention and training policies apply to what it receives;
  the settings section links to them rather than paraphrasing them.
- Tests and fixtures use fictitious images and a fake provider (§10).

---

## 9. Dependencies and binary size

Measured with `cargo tree` on each binary's features, estimated without a
release build:

| Approach | New crates | Size impact |
|---|---|---|
| Hand-written OpenAI-compatible and Anthropic clients over the existing `reqwest`, `serde_json`, `base64` | none (`oxidgene-api` adds `reqwest` from the workspace) | about 60–150 KB |
| Gemini and Bedrock (API key) | none | about 40–80 KB more |
| Bedrock SigV4 | `hmac`, or a few lines over `sha2` | a few KB |
| Provider SDK crates (async-openai, genai…) | 10–60 crates | 1–3 MB — **not used** |
| Local models (ONNX Runtime, restoration or colorization weights) | `ort` and a native library | tens of MB of runtime and hundreds of MB of weights — **not used** |

No cargo feature gates the AI code: it adds no crate. Availability is decided
at run time (§1, §5.4).

---

## 10. Tests

- A fake provider (an Axum router in the test harness) speaks each protocol,
  records what it received and returns canned answers, including malformed
  ones, errors and a slow answer.
- REST and GraphQL tests cover every endpoint and error code of §7, and
  assert that no key, prompt or image appears in logs, traces or error
  bodies.
- The outbound protection of §5.4 is tested with names resolving to each
  refused range.
- Tests against real providers are `#[ignore]`d and run only by an explicit
  command; `just check` never runs them.

---

## 11. Phases

1. Provider registry, OpenAI-compatible and Anthropic protocols, the App
   Settings section with local key storage and consent, the relay with its
   outbound protection, `/ai/test`, `/ai/status`, and **AI transcript**.
   Ollama and LM Studio come with the OpenAI-compatible protocol.
2. **Record and photo analysis** with the review panel.
3. Derived media (§6), then **Enhance** and **Colorize**.
4. Gemini native, Bedrock, streaming for long answers.

Deferred, each needing its own decision:

- **Animation** of a photo or a face: video models only, vendor-specific
  job-and-poll APIs, restrictive policies on real faces; the most sensitive
  feature.
- Per-account server-side key storage, with EPIC G (§4.2).
- Local model runtimes, only behind a cargo feature, with weights downloaded
  on demand and never embedded.
- Sending tree context with an analysis.
