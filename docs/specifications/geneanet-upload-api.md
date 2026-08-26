---
okf_version: "0.1"
---

# Geneanet Upload — API Specification

> **Context.** Reverse-engineering session of 2026-08-16 (OxidGene): the
> `api.geneanet.org` surface used by the official *Geneanet Upload* desktop app,
> plus live-probed facts about Cloudflare, originals vs renditions, website
> login, and HTTP clients. The decision-relevant parts are distilled into
> [Geneanet Media Import](geneanet-media-import.md) (which covers the *other*,
> richer media API on `www.geneanet.org`); this document is the full reference
> so nothing is lost.
>
> Helper scripts and artifacts produced by that session — `geneanet_token.py`,
> `geneanet_media_map.py`, `geneanet_site_session.py`, `media_map.json`,
> `zip_view_map.json`, `geneanet_cache/` — live in the reverse-engineering
> workspace, not in this repository; "same folder" below refers to it.
> The companion UI flow is [Import](ui-import.md).


Reverse-engineered from `geneanet-upload-last-linux-x64.AppImage` (**Geneanet Upload 5.4.0**).

- **Method**: AppImage extraction → `app.asar` → original sources recovered from webpack source maps, then **live probing** of the API, anonymous and **authenticated** (2026-08-16). Section 8 documents a second API surface from Geneanet's open-source repo [`geneanet/geneweb-plugin-api`](https://github.com/geneanet/geneweb-plugin-api).
- **Stack**: Electron 13.6.9 (Chromium 91.0.4472.164), AngularJS 1.8 renderer, `ng-file-upload` for multipart uploads.
- **Base URL (production)**: `https://api.geneanet.org` (behind Cloudflare; HTTP/2; JSON responses).
- Legend: **[app]** = observed in application source code · **[probe]** = verified by live request · **[auth]** = verified with a real access token · **[inferred]** = deduced, not confirmed.

---

## 1. HTTP client behavior

### User-Agent **[app]**
The app sets **no custom User-Agent** (no `setUserAgent`, `webRequest`, `extraHeaders` anywhere in the code). Requests are sent by Electron's Chromium stack with the default Electron UA:

```
Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) geneanet-upload/5.4.0 Chrome/91.0.4472.164 Electron/13.6.9 Safari/537.36
```

### Authorization interceptor **[app]**
- Every request whose URL matches the API endpoint gets the header `Authorization: Bearer <access_token>` — **except** URLs containing `/oauth/v2/`.
- On HTTP **401**, the failed request is retried **once, after a 3 s delay**.
- Tokens are stored in `sessionStorage` under key `AuthToken` as JSON:
  `{ "username": string, "accessToken": string, "expiresAt": number (epoch ms), "refreshToken": string }`.
- Expired tokens are transparently renewed via the refresh grant before the request is replayed.
- Access tokens are opaque base64 strings (hex when decoded); the server rejects malformed ones with `401 {"code":401,"message":"Invalid JWT Token"}` **[probe]**.

---

## 2. Embedded OAuth2 client **[app]** — verified valid **[probe]**

| Parameter | Value |
|---|---|
| `client_id` | `2_4pv4liwroheskk0owc4w0oos8sg8kogoww0oso0g0c0gok4gsw` |
| `client_secret` | `5449lr861fcw4skkwkgs48wo0gsskw4w4wowggowk4gsckkcgw` |

Probe evidence: real client + bad user → `invalid_grant` ("Invalid username and password combination"); fake client → `invalid_client`. The embedded client credentials are accepted by the server. A helper script `geneanet_token.py` (same folder) retrieves a token with them.

---

## 3. Endpoint reference

### 3.1 `POST /oauth/v2/token` — login & token refresh

- **Allowed methods**: `GET, POST` **[probe]** (app uses POST)
- **Auth**: none (no `Authorization` header)
- **Content-Type**: `application/x-www-form-urlencoded`

**Login request** (`password` grant) **[app]**:

| Parameter | Type | Required | Description |
|---|---|---|---|
| `grant_type` | string | yes | literal `"password"` |
| `client_id` | string | yes | embedded OAuth client id |
| `client_secret` | string | yes | embedded OAuth client secret |
| `username` | string | yes | Geneanet account username |
| `password` | string | yes | Geneanet account password |

**Refresh request** (`refresh_token` grant) **[app]**:

| Parameter | Type | Required | Description |
|---|---|---|---|
| `grant_type` | string | yes | literal `"refresh_token"` |
| `client_id` | string | yes | embedded OAuth client id |
| `client_secret` | string | yes | embedded OAuth client secret |
| `refresh_token` | string | yes | refresh token from a previous response |

**Response 200** (fields consumed by the app) **[app]**:

| Field | Type | Description |
|---|---|---|
| `access_token` | string | bearer token for all API calls |
| `expires_in` | number | token lifetime in **seconds** |
| `refresh_token` | string | used by the refresh grant |

**Errors** **[probe]**: `400 invalid_client` (bad client credentials) · `400 invalid_grant` (bad username/password or bad refresh token) · `400 invalid_request` (missing `grant_type`).

---

### 3.2 `GET /media/geneanet-upload` — startup version & rules check

- **Allowed methods**: `GET` **[probe]** (`Allow: GET` header)
- **Auth**: **not required** **[probe]** (works anonymously)

| Query parameter | Type | Required | Description |
|---|---|---|---|
| `appversion` | string (`X.Y.Z`) | yes | app version, e.g. `5.4.0` |

**Response 200** — real probed response **[probe]**:

```json
{
  "latest_available_version": "5.4.0",
  "rules": {
    "file": {
      "max_size": "50M",
      "max_pdf_pages": "30",
      "max_size_gedcom": "350M",
      "mime_types": ["image/jpeg", "image/pjpeg", "image/png", "image/gif", "application/pdf", "application/x-pdf"]
    }
  }
}
```

| Field | Type | Description |
|---|---|---|
| `latest_available_version` | string | compared against `appversion`; older app → upgrade banner |
| `rules.file.max_size` | string (`"NNM"`) or number | max media file size (app converts `"50M"` → bytes) |
| `rules.file.max_size_gedcom` | string or number | max GEDCOM zip size |
| `rules.file.max_pdf_pages` | string/number | max PDF pages |
| `rules.file.mime_types` | string[] | allowed media MIME types |

**Errors** **[probe]**: `400 {"code":"err_bad_request","message":"The \"appversion\" parameter is required."}` · app also handles `err_outdated_version` (with `data.latest_available_version`) when the app is too old **[app]**.

---

### 3.3 `GET /media/user` — user info & quota

- **Allowed methods**: `GET` **[probe]**
- **Auth**: required (`401` without token) **[probe]**

**Response 200** — real authenticated response **[auth]**:

```json
{"user": {"tree": true, "address_valid": true}, "quota": {"current": 790459109, "max": 10000000000}}
```

| Field | Type | Description |
|---|---|---|
| `user.tree` | boolean | whether the user already has a tree |
| `user.address_valid` | boolean | invalid address → warning banner in the app |
| `quota.current` | number | media quota used (bytes) |
| `quota.max` | number | media quota limit (bytes) — here 10 GB |

---

### 3.4 `POST /geneweb/tree/create` — create the user's tree

- **Allowed methods**: `POST` only **[probe]** (`GET` → `405`)
- **Auth**: required
- **Content-Type**: `application/json`

| Body field | Type | Required | Description |
|---|---|---|---|
| `privacy` | string enum | yes | `"semi_public"` ("hidden") or `"prive"` ("private") — the only two options offered by the app UI **[app]** |

Called only when `GET /media/user` reports `user.tree == false`, before the first GEDCOM upload.

---

### 3.5 `POST /geneweb/tree/upload` — upload the GEDCOM (zip)

- **Allowed methods**: `POST` only **[probe]** (`GET` → `405`)
- **Auth**: **optional in practice** **[probe]** — the app always sends `Bearer`, but an anonymous multipart POST is accepted (see below); an *invalid* bearer → `401 Invalid JWT Token`
- **Content-Type**: `multipart/form-data`

| Form field | Type | Description |
|---|---|---|
| `file` | file (binary) | ZIP archive built client-side with JSZip (DEFLATE level 9), containing the `.ged` file; archive named `upload_<username>.zip` **[app]** |

**Responses observed** **[probe]**:
- with a valid zip: `200 {"filename":"1c43fb38c71130ba9bf3dcccca82e833.ged","success":true}` — server unzips and stores the GEDCOM under an md5-style filename
- with no file: `200 []`

**Errors** **[app]**: JSON body with `error` | `message` | `msg` field → shown as "Upload failed".

---

### 3.6 `GET /geneweb/tree/lock/status` — import progress (polled)

- **Allowed methods**: `GET` **[probe]**
- **Auth**: required (`403 Forbidden` anonymously) **[probe]**

| Query parameter | Type | Description |
|---|---|---|
| `release` | string | app always sends `"update"` |

**Response 200**:
- Idle (no operation in progress): `[]` **[auth]**
- During/after an import **[app]**:

| Field | Type | Values |
|---|---|---|
| `action` | string \| absent | `"update"` → normal flow; any other value → tree locked by another operation; absent → idle |
| `status` | string | `"running"`, `"pending"`, `"done"`, `"failed"` |
| `step` | string | `"prepare"`, `"update_tree"`, `"resync_tree"`, `"verify_links"`, `"index_tree"` |

Polled every **5 s** while `status` is `running`/`pending`.

---

### 3.7 `GET /geneweb/tree/broken-links` — broken media links count

- **Allowed methods**: `GET` **[probe]**
- **Auth**: required (`403` anonymously) **[probe]**

**Response 200** — real authenticated response **[auth]**: `{"nb_broken_links": 0}` (number; the app still applies `parseInt`).

---

### 3.8 `GET /geneweb/gedcom/media-parser.json` — media referenced by the uploaded GEDCOM

- **Allowed methods**: `GET` (used by the app; OPTIONS returns no `Allow`) **[app/probe]**
- **Auth**: required (`403` anonymously) **[probe]**

**Response 200** **[auth]**:

| Field | Type | Description |
|---|---|---|
| `media` | object map | keys: media reference found in the GEDCOM (string) → values: array of GEDCOM individual ids, e.g. `["@I14@"]` |

Observed key formats **[auth]**: for media already online, keys are full CDN URLs such as
`http://gw.geneanet.org/public/img/media/deposits/private/<d1>/<d2>/<viewId>/<hash>/medium.bmp?t=<ts>`;
for new uploads the app expects local file paths. The app filters keys by allowed extensions, resolves each path on the local disk, uploads each file (3.9) and links it (3.10).

**Coverage caveat [auth]**: this endpoint only reflects media referenced by the
**last GEDCOM uploaded through the app**, not identifications made later on the
website. Authorized validation returned fewer links here than through the
deposits and references routes. Use those routes for a complete inventory.

---

### 3.9 `POST /media/deposits.json` — upload one media file · `GET /media/deposits.json` — list deposits

- **Allowed methods**: `GET, POST` **[probe]**
- **Auth**: required for both (`401` anonymously) **[probe]**

#### POST — upload **[app]**
**Content-Type**: `multipart/form-data`

| Form field | Type | Description |
|---|---|---|
| `deposit[type]` | string enum | app always sends `"autres"` |
| `no_duplicate` | number | app always sends `1` |
| `deposit[views][][uploadedFile]` | file (binary) | the media file (original filename preserved) |

**Response 200** (fields consumed by the app): `id` (number), `views[0].id` (number) — used for linking. Errors: `errors` (string[]), `message` (string); `413` → file too large.

#### GET — list the user's deposits **[auth]**

| Query parameter | Type | Description |
|---|---|---|
| `page` | number | pagination: **10 deposits per page**, `?page=1…N`; past the end → **`204 No Content`** (empty body) **[auth]** |

**Response headers** **[auth]**: `x-gnt-media-total: <count>` ·
`x-gnt-media-quota-current` · `x-gnt-media-quota-max` · `Allow: GET, POST`

**Response 200** — JSON array of deposit objects **[auth]**:

| Field | Type | Description |
|---|---|---|
| `id` | number | deposit id |
| `slug` | string | URL slug |
| `title` | string | deposit title |
| `type` | string enum | e.g. `"photo_groupe"`, `"portraits"` |
| `private` | boolean | visibility |
| `username` / `username_sender` | string | owner / uploader |
| `thumb` | string | relative CDN path of the thumbnail |
| `views` | object[] | each: `id` (number), `page` (number), `files` = map of renditions |
| `views[].files` | object | keys `"normal"`, `"medium"`, `"screen"`, `"thumbnail"` → relative CDN paths (`/public/img/media/deposits/…?t=<ts>`) |
| `date_create` | string | ISO 8601 upload timestamp |

`GET /media/deposits/{id}` returns the same object shape for a single deposit,
plus `date`, the date attributed to the media, and `location` **[auth]**. The
media date is distinct from the `date_create` upload timestamp. OxidGene
collects this detail only for linked deposits and resolves the location through
the tree's shared Place table. **No original filename is exposed anywhere**;
files are identified only by hashed CDN paths.

---

### 3.10 `POST /media/deposits/{depositId}/views/{viewId}/references` — link media to GEDCOM individuals · `GET …/references` — list links

- **Allowed methods**: `GET, POST` **[probe]**
- **Auth**: required (`401` anonymously) **[probe]**

| Path parameter | Type | Source |
|---|---|---|
| `depositId` | number | `id` from the deposit (3.9) |
| `viewId` | number | `views[0].id` from the deposit (3.9) |

#### POST — link **[app]**
**Content-Type**: `multipart/form-data`

| Form field | Type | Description |
|---|---|---|
| `id_gedcom` | string | GEDCOM individual id from `media-parser.json` (3.8), e.g. `"@I14@"` — one request per individual |

**Response 200**: `{ "reference": object }`.

#### GET — list the persons linked to a view **[auth]**
**Response 200** — JSON array of person-reference objects:

| Field | Type | Description |
|---|---|---|
| `id` | number | reference id |
| `firstname` / `lastname` | string | person name |
| `thumb` | string | relative CDN path of the face crop (`…/<referenceId>.png`) |
| `face.position` | object | `x1`, `y1`, `x2`, `y2` — face box coordinates, percent strings (0–100) |
| `reference_extra_geneweb` | object \| **absent** | `{id: number, ref: "<surname>\|<given names>\|", link_tree: "https://gw.geneanet.org/<tree>?n=<surname>&p=<given names>&oc=<occurrence>"}`; absent when the identified person is only a face tag **[auth]** |
| `event` | object \| null | `{id, type, name (e.g. "gw_event_marriage"), date: "<date>", location, spouse: <recursive person-reference>}` |

Observed behaviors **[auth]**:
- A family-event media (e.g. a marriage photo) yields **one reference entry per spouse**, each carrying the `event` with the other spouse nested in `event.spouse` — so "identified via his/her family" associations on the website materialize as individual reference entries.
- The same person can appear **multiple times for the same view** (several face tags) — deduplicate by `(deposit_id, view_id)` when building per-person media lists.
- Multi-view deposits are common, so the number of views can exceed the number
  of deposits.

---

### 3.11 Related REST resources discovered by probing **[probe]**

| Path | Allowed methods | Notes |
|---|---|---|
| `/media/deposits.json` | `GET, POST` | GET = list deposits — verified **[auth]** |
| `/media/deposits/{id}` | `GET, PUT, DELETE` | GET verified **[auth]**; PUT/DELETE not tested (write ops) |
| `/media/deposits/{id}/views/{id}` | `PUT, DELETE` | no GET; not tested |
| `/media/deposits/{id}/views/{id}/references` | `GET, POST` | GET verified **[auth]** |
| `/media/deposits/{id}/views/{id}/references/{id}` | `DELETE, PATCH` | not tested (write ops) |

---

## 4. For each POST, is there an equivalent GET? — verified answers

| POST endpoint | GET equivalent? | Evidence |
|---|---|---|
| `/oauth/v2/token` | **Yes** | `Allow: GET, POST` **[probe]** (GET works with query-string params) |
| `/geneweb/tree/create` | **No** | `Allow: POST`; `GET` → `405 Method Not Allowed` **[probe]** |
| `/geneweb/tree/upload` | **No** | `Allow: POST`; `GET` → `405` **[probe]**. The guess `GET /geneweb/tree` → **`404 Not Found`** — the route does not exist **[probe]** |
| `/media/deposits.json` | **Yes — verified working** | `Allow: GET, POST` **[probe]**; authenticated GET returns a paginated deposit list **[auth]** |
| `/media/deposits/{d}/views/{v}/references` | **Yes — verified working** | `Allow: GET, POST` **[probe]**; authenticated GET returns the linked persons **[auth]** |

Bonus read endpoints verified **[auth]**: `GET /media/deposits/{id}` (single deposit detail).

## 5. For each upload, is there an equivalent download? — verified answers

### GEDCOM (tree) upload → **No download on this API**
All candidates return `404` **[probe]**: `/geneweb/tree`, `/geneweb/tree/download`, `/geneweb/tree/export`, `/geneweb/tree/gedcom`, `/geneweb/tree/current`, `/geneweb/tree/file`, `/geneweb/tree/status`, `/geneweb/gedcom`, `/geneweb/gedcom/download`.
→ This API is **write-only for trees**. GEDCOM export/download is only available through the website (`www.geneanet.org` / `gw.geneanet.org`), not through `api.geneanet.org`. **However**, the GeneWeb plugin API on `gw.geneanet.org` (`API_ALL_PERSONS` / `API_ALL_FAMILIES`, paginated JSON) offers a full tree-data export channel — see section 8.

### Media upload → **Yes, via `GET /media/deposits.json` (+ CDN) — with a caveat**
- **Metadata read-back verified** **[auth]**: `GET /media/deposits.json` (paginated list, `?page=N`, total in `x-gnt-media-total`), `GET /media/deposits/{id}` (detail), `GET …/references` (linked persons). Each deposit exposes its file renditions (`normal`, `medium`, `screen`, `thumbnail`) as **relative CDN paths**.
- **Binary download**: the files live on `gw.geneanet.org/public/img/media/deposits/…`. For **private** deposits these URLs return **`403 Forbidden`** — even with the API `Bearer` token **[auth]**: they require a **website session cookie** (a browser logged into geneanet.org). No API route serves the binaries (`/media/deposits/{id}/download` → `404`) **[probe]**.
→ Conclusion: the API gives you the full inventory and the file locations; downloading the actual bytes of private media requires website authentication, not the API token. **All CDN files are re-encoded renditions, never the original upload — and no route on this API serves originals** (see §11). Site-free recovery of originals from the data-archive ZIPs: §12. The website's own media API (originals + bulk references, session-cookie based): §13.

---

## 6. Website URLs (opened in the system browser — **not** API calls) **[app]**

Base: `https://{sub}.geneanet.org` where `sub` = `www` for French, else the 2-letter locale (`en`, `de`, …).

| Name | URL |
|---|---|
| login page | `/connexion/` |
| password reset | `/resetting/request` |
| app download | `/product/upload?appversion={version}` |
| set address | `/profile/completion/address?cle={base64(username)}` |
| tree homepage | `/mon_compte/arbre_show.php` |
| contact | `/legal/contact` |
| tree update (gw) | `https://gw.geneanet.org/my-tree/operations/update` |
| tree repair (gw) | `https://gw.geneanet.org/my-tree/operations/repair` |

All are intercepted clicks (`js-external-link`) → `shell.openExternal()`.

---

## 7. Typical upload flow (as implemented by the app) **[app]**

1. `GET /media/geneanet-upload?appversion=…` — version/rules check (anonymous OK)
2. `POST /oauth/v2/token` (password grant) — login
3. `GET /media/user` — profile, quota, has-tree flag
4. If no tree: `POST /geneweb/tree/create` `{privacy}`
5. Zip GEDCOM locally → `POST /geneweb/tree/upload` (multipart `file`)
6. Poll `GET /geneweb/tree/lock/status?release=update` every 5 s until `done`
7. `GET /geneweb/tree/broken-links`
8. `GET /geneweb/gedcom/media-parser.json` — media paths referenced by the GEDCOM
9. For each media file: `POST /media/deposits.json` → then `POST /media/deposits/{d}/views/{v}/references` per GEDCOM id
10. Duplicates avoided client-side via md5 hashes stored locally (`images_` storage key)

---

## 8. The GeneWeb plugin API on `gw.geneanet.org` — from `geneanet/geneweb-plugin-api`

The GitHub repo [`geneanet/geneweb-plugin-api`](https://github.com/geneanet/geneweb-plugin-api) is the **server-side source of a second, different API**: an OCaml plugin (`plugin_api.ml`) running inside the GeneWeb `gwd` daemon that hosts each user's tree at `https://gw.geneanet.org/<base>` (e.g. `https://gw.geneanet.org/<username>`). It does **not** define the `api.geneanet.org` REST endpoints used by the upload app — but it reveals the full **tree-data read/write API**, complementary to it. This is where tree data can actually be **read back / exported**.

### Invocation pattern (all modes)
```
https://gw.geneanet.org/<base>?m=<MODE>&data=<payload>&input=json|pb|xml&output=json|pb|xml&filters=<payload>
```
- `m` — the API mode (see table below)
- `data` + `input` — request parameters, piqi/protobuf-serialized per `src/assets/*.proto` (proto2 syntax: `api.proto`, `api_saisie_read.proto`, `api_saisie_write.proto`, `api_stats.proto` — ~2100 lines of schemas)
- `output` — response format; **mandatory** (`json`, `pb` protobuf, or `xml`; missing → empty exit)
- `filters` — optional `Filters` message: `only_sosa` (bool), `only_recent` (bool), `sex` (male/female/unknown), `nb_results` (bool, return count only), `date_birth` / `date_death` (`FilterDateRange` with begin/end/only_exact)

### Access levels (from `plugin_api.ml`)
- **base** — any valid base (most reads); privacy depends on the base's public/private setting
- **friend** — GeneWeb "friend" or "wizard" access (`API_BASE_WARNINGS`)
- **wizard** — tree owner only (all write operations + some reads); `401`/`403` otherwise; writes take a base lock → `409 Conflict` if locked

### Modes registered by the plugin (~50)

**Read — tree data (base access):**

| Mode | Description | Key params (proto) |
|---|---|---|
| `API_INFO_BASE` | base stats: `nb_persons`, `nb_families`, sosa ref, last modified person | — |
| `API_ALL_PERSONS` | **all persons, paginated** → full tree export | `AllPersonsParams{from, limit}` (int32) |
| `API_ALL_FAMILIES` | **all families, paginated** → full tree export | `AllFamiliesParams{from, limit}` |
| `API_INFO_IND` | one person (full details) | `Index{i}` (iper id) |
| `API_SEARCH` | person search | `SearchPerson{search_type, …}` |
| `API_IMAGE` / `API_IMAGE_ALL` / `API_IMAGE_PERSON` | portrait URLs per person | `full_infos=1` for full person data |
| `API_GRAPH_ASC` / `API_GRAPH_DESC` / `API_GRAPH_REL` / `API_GRAPH_TREE` / `API_GRAPH_TREE_V2` | asc/desc/relationship/tree graphs | see protos |
| `API_PERSON_TREE` / `API_FICHE_PERSON` | person tree view / person card | — |
| `API_CPL_REL` / `API_CLOSE_PERSONS` | couple & close relations | — |
| `API_FIRST_AVAILABLE_PERSON` / `API_FIND_SOSA` / `API_REF_PERSON_FROM_ID` | navigation helpers | — |
| `API_LAST_MODIFIED_PERSONS` / `API_LAST_VISITED_PERSONS` / `API_LIST_PERSONS` | person lists | — |
| `API_LOOP_BASE` / `API_NB_ANCESTORS` / `API_NAME_FREQUENCY` / `API_PERSON_WARNINGS` / `API_STATS` / `API_SELECT_EVENTS` | integrity, counts, stats, events | — |

**Read — friend access:** `API_BASE_WARNINGS` (base consistency warnings)

**Read/write — wizard (owner) only:**
`API_MAX_ANCESTORS`, `API_AUTO_COMPLETE`, `API_GET_CONFIG`, `API_PERSON_SEARCH_LIST`, `API_GET_PERSON_SEARCH_INFO`, `API_IMAGE_UPDATE`, and the full edit suite: `API_ADD_FIRST_FAM`, `API_ADD_CHILD`, `API_ADD_FAMILY`, `API_ADD_PARENTS`, `API_ADD_SIBLING`, `API_ADD_PERSON_OK`, `API_ADD_PERSON_START_OK` (+ `…_OK` commit variants), `API_EDIT_FAMILY_REQUEST`, `API_EDIT_FAMILY`, `API_EDIT_PERSON` (+ `…_OK`), `API_DEL_FAMILY_OK`, `API_DEL_PERSON_OK` — request/response schemas in `api_saisie_write.proto`.

### Impact on the upload↔download question
- `api.geneanet.org` remains write-only for trees, **but** `API_ALL_PERSONS` + `API_ALL_FAMILIES` (paginated, `output=json`) on `gw.geneanet.org/<base>` constitute a complete **tree data export channel** — the download counterpart the upload API lacks.
- Caveats: `gw.geneanet.org` sits behind **Cloudflare bot protection** (JS challenge — curl gets "Just a moment…", verified 2026-08-16), so scripted access needs a real browser session; and on Geneanet's hosting, wizard/friend auth is tied to the website session (not the `api.geneanet.org` OAuth token — untested).

---

## 9. Caveats

- Response schemas marked **[app]** are inferred from the fields the client code consumes; the server may return additional fields.
- `401` vs `403` split: `/media/*` routes answer `401 Unauthorized` anonymously, `/geneweb/*` routes answer `403 Forbidden` anonymously — different security voters, both mean "auth required".
- Authenticated probing was **read-only** (`GET`); `PUT`/`PATCH`/`DELETE` routes (3.11) were identified via `Allow` headers but not exercised to avoid modifying account data.
- `POST /geneweb/tree/upload` accepts anonymous uploads **[probe]** — surprising, but verified; the association with the account presumably happens via the authenticated flow.
- No public API documentation exists on the host (`/doc`, `/docs`, `/openapi.json`, `/swagger.json` → `404`).

---

## 10. Building the individual → media-URLs map — verified workflow **[auth]**

Goal: for every tree individual, list **all** media it is identified in on the website (directly or via family events) — something neither the website `.ged`/`.gw` export (principal media only) nor `media-parser.json` (3.8, stale uploads only) provides.

**Approach** (implemented by `geneanet_media_map.py`, same folder; stdlib only):

1. Login: `POST /oauth/v2/token` (or paste a token from `geneanet_token.py`).
2. `GET /media/deposits.json?page=1…N` until `204` → all deposits (10/page).
3. For every view of every deposit: `GET /media/deposits/{d}/views/{v}/references`.
4. Build the person key from `reference_extra_geneweb.link_tree` query params: **`n|p|oc`** = `surname|first names|occurrence` (lowercase, GeneWeb naming — matches `.gw` `pevt SURNAME First_Name` entries and `.ged` `NAME` fields after normalization). References without `reference_extra_geneweb` (face-tagged but not tree-linked persons) are grouped by lowercased full name instead.
5. Deduplicate multiple face tags of the same person in the same view.

**Validation result:** the workflow recovered direct and family-event
associations visible in the website UI. Some views had no tree-linked person,
which confirms that unlinked media must remain explicit rather than being
discarded or guessed.

**Outputs**: `media_map.json` (person → media list with `deposit_id`, `view_id`, `title`, `event`, `face_position`, `url`, `thumb`), `media_map.csv`, `unlinked_media.csv`. API responses are cached (`geneanet_cache/`) for fast re-runs.

**Media URLs**: `https://gw.geneanet.org` + `views[].files.normal` (largest CDN rendition; recompressed, **not** the original upload). Private deposits → `403` without a website session cookie (see 5).

---

## 11. Original files vs CDN renditions — verified

**`views[].files.{normal,medium,screen,thumbnail}` are all generated renditions — re-encoded and downsized, never the original upload.** Evidence:

- **Format rewriting**: originals uploaded as uncompressed BMP (`portrait_a.bmp`, `portrait_b.bmp`, …) keep the `.bmp` extension in every rendition; PDF deposits have one `.pdf` per page in the data archive but one `normal.jpg` per page on the CDN. A `.bmp`→`.jpg` or `.pdf`→`.jpg` transformation is necessarily a re-encode.
- **Four-size ladder**: `normal` > `medium` > `screen` > `thumbnail` are derivative sizes of the same view.
- Even a 220-px-wide original gets a `normal.jpg` — "normal" means "the largest rendition", not "the file uploaded".
- The API objects carry **no byte size and no content hash**. The hexadecimal
  component of CDN paths is not the SHA-1 of the original content, so neither
  size nor hash matching against local files is possible from API data alone.

**No original download on `api.geneanet.org`** — probed 2026-08-16 **[auth]**, all `404` (SPA fallback, route absent): `/media/download`, `/media/download/`, `/media/deposits/{id}/download`, `/media/deposits/{id}/original`, `/media/deposits/{id}/file`, `/media/deposits/{id}/views/{vid}/download`, `/media/deposits/{id}/views/{vid}/original`, `/media/deposits/{id}/views/{vid}/file`.

**The only two sources of original bytes** (both website-side):
1. The **data archive** (`www.geneanet.org/my-data/dashboard`) — ZIP(s) of every original file, named after the upload, no deposit id attached (see §12 for re-linking).
2. **`GET www.geneanet.org/media/download/?deposits[]={id}`** with website session — verified byte-identical to the data-archive copy by the oxidgene spec (§13). The trailing slash matters (bare path → `301`). Multi-page deposits → a streamed ZIP without `Content-Length`; `HEAD` on a single-page deposit returns the exact `Content-Length`.

CDN rendition URLs return `403` for non-browser clients even **with** `X-Requested-With: XMLHttpRequest` and the API `Bearer` token **[probe]** — Cloudflare challenges the TLS fingerprint; a website `gntsess5` cookie is required (§13).

**Downloading renditions without the website (probed 2026-08-16)**:
- Authorized validation included both public and private deposits; private
  views were the common case.
- **Public CDN URLs can be fetched anonymously** with a TLS-fingerprint-impersonating client: `curl_cffi` (`impersonate="chrome"`) → `200 image/jpeg`, while `urllib`/plain curl → `403` (Cloudflare). No cookie, no API token needed.
- **Private CDN URLs → `404` text/html** for anonymous clients *even past the Cloudflare layer* (curl_cffi, no cookie) — Geneanet hides the asset itself; the `gntsess5` website cookie is mandatory. The API `Bearer` is not accepted on `gw.geneanet.org`.
- No binary proxy on the API either: `/media/deposits/{id}/views/{vid}/download`, `/media/deposits/{id}/download`, `/media/views/{id}/file` → all `404` **[auth probe]**.
- **Perceptual matching works**: `imagehash.phash` of a downloaded `normal.jpg` vs its ZIP-archive original → **Hamming distance 0** (tested on a recompressed+resized pair). So a downloaded rendition set can be content-matched against a local original set (pHash tolerates resize/re-encode/format change; PDF pages must be rendered first, e.g. `pdftoppm`). Ready-made tools for pair-matching compressed↔uncompressed sets: Python `imagehash`, dupeGuru (picture fuzzy mode), digiKam similarity search, `findimagedupes`, `imagededup`.

## 12. Linking data archives to deposits and views

Authorized validation found one original archive entry per media view. The ZIP
entries carry no deposit ID, but they can be re-linked without site access:

- **Join key**: the ZIP entry's DOS datetime usually equals the deposit's
  `date_create` to the minute. Exceptions exist where an original modification
  time was preserved. Archive order tracks deposit order in reverse, and pages
  of a multi-page deposit remain consecutive and ordered.
- **Method**: group entries and views by minute; equal-count groups pair by
  order; leftovers pair by elimination. The result records the ZIP filename,
  deposit and view IDs, title, matching method, and confidence.
- **Honest confidence**: singleton minutes and minute groups belonging to one
  deposit are exact. Multi-deposit batches uploaded in the same minute remain
  ambiguous where local data cannot exclude a swap. Ambiguous entries are
  flagged and never silently trusted.
- **Rejected**: filename-to-title similarity is not a robust join strategy and
  can silently misattribute media. Never attach on a probable match; detect
  clashes and leave them unresolved.
- **Deterministic upgrade** (requires a website session, §13):
  `HEAD /media/download/?deposits[]={id}` returns `Content-Length`, matched
  against ZIP entry sizes from the central directory. Authorized validation
  found mostly unique sizes, with observed collisions caused by duplicate
  uploads. This exact join is unavailable site-free because the API exposes no
  sizes (§11).

## 13. The website's own media API (`www.geneanet.org`, session cookie) — from the oxidgene spec

A separate, richer API surface than `api.geneanet.org`, documented in
[Geneanet Media Import](geneanet-media-import.md). Key differences from the app
API:

| | `api.geneanet.org` (this doc) | `www.geneanet.org/media/api` (oxidgene) |
|---|---|---|
| Auth | OAuth2 `Bearer` (§2) | `gntsess5` **or** `REMEMBERME` cookie — nothing else in the browser cookie is read |
| Extra header | none | `X-Requested-With: XMLHttpRequest` mandatory on **every** call (else `403` HTML even with valid cookie) |
| Deposits list | `GET /media/deposits.json` — 10/page | `GET /media/api/deposits?page=N&per_page=100` |
| References | one request per view | bulk `GET /media/api/references?page=N&per_page=100`; whole deposit inline; carries the `is_default` portrait flag absent from the app-API object **[verified]** |
| Originals | **none** (§11) | `GET /media/download/?deposits[]={id}` — byte-identical original; `HEAD` → `Content-Length`; multi-page → streamed ZIP |
| Bot protection | lenient with curl | Cloudflare adaptive TLS-fingerprint challenge (`cf-mitigated: challenge`); same cookie+UA: curl `200`, rustls/hyper `403` |

The bulk reference-manifest strategy requires far fewer requests than the
per-view API and is preferred when a website session is available. Its
name-folding rules join `surname|first names|occurrence` against `.gw` entries:
lowercase, accents stripped, separators normalized to spaces, stroked letters
folded explicitly, and occurrence empty when zero. The `.gw` source is required
because the occurrence belongs to the key while GEDCOM xrefs are opaque.

### Old Android-app login flow still works (verified 2026-08, anonymous probes)

The pre-mobile-app login used by [geneparse](https://github.com/trois-six/geneparse)
(`POST https://www.geneanet.org/connexion/verify.php?ctype=id`, form fields
`login` / `password` / `persistent=1`) is **still alive**: wrong credentials return
`200` with body `-1` (the old code expected body `1` on success). One change since
2019: the session cookie issued is now **`gntsess5`** (the old code looked for
`gntsess`); `autolang` and `__cf_bm` are also set. The old companion endpoints
`app/arbre/index.php?action=logged` and `?action=import` also still respond (`200`,
empty body without a session). Cloudflare on www requires a browser TLS fingerprint —
`curl_cffi` with Chrome impersonation passes; plain `curl`/`requests` are challenged.

~~A successful login therefore yields a fully scriptable website session~~ **UPDATE
2026-08-16 — password login is NOT automatable anymore:**

- `verify.php` returns `-1` even with **valid** credentials (tested) → the legacy
  endpoint is effectively dead for password auth. **Trap:** it still *sets* a
  `gntsess5` cookie (random per attempt, 1-year expiry) — but that cookie is an
  anonymous placeholder: with it, `/media/api/references` → app-level `403` (34 B,
  same as no cookie), `app/arbre/index.php?action=accountInfos` → `200` empty body,
  private CDN rendition → `404` (all verified 2026-08-16). A cookie being set is NOT
  proof of authentication — always validate against an authenticated endpoint.
- The modern login is a Symfony form: `GET /connexion/` (parse `_csrf_token`) →
  `POST /connexion/login_check` (`_username`, `_password`, `_csrf_token`,
  `_remember_me`) — but it enforces **reCAPTCHA v2 server-side on every attempt**
  (flash: *"We couldn't confirm that you are not a robot…"*; sitekey
  `6LfddkIUAAAAAL7oc3x7UINqMMMbbE-tS1V85KII`). Not scriptable without a captcha solver.

**Working path = cookie import** (yt-dlp `--cookies` pattern): log in once in a real
browser with "Remember me", then copy `gntsess5` / `REMEMBERME` from devtools into the
tool. Per the oxidgene spec, `REMEMBERME` alone is accepted by `/media/api/*`; Symfony
remember-me cookies are long-lived (~1 year). Session validator:
`geneanet_site_session.py` (checks bulk `/media/api/references`, an originals
`/media/download/` HEAD, and a private CDN rendition with the imported cookies).

### HTTP clients vs Cloudflare on www/gw — tested matrix (2026-08-16)

Empirical test against the live site (bogus-login POST, homepage, `/media/api/references`,
public gw CDN rendition). `verify.php` itself is **never challenged** (any stack passes);
all data routes are protected by an adaptive TLS/HTTP2 fingerprint check.

| Stack | verify.php | www pages | `/media/api/*` | gw CDN |
|---|---|---|---|---|
| plain libcurl / curl CLI / `requests` | ✅ | ❌ challenge | ❌ challenge | ❌ challenge |
| `__cf_bm` cookie (from verify.php) on plain curl | — | — | ❌ still challenged | — |
| [hyprcurl](https://github.com/Aditya-PS-05/hyprcurl) `ChromeLatest` (= Chrome 131) | ✅ | ❌ challenge | ❌ challenge | ❌ challenge |
| `wreq` 6.0.0-rc.29 + `wreq-util` `Emulation::Chrome131` | ✅ | ❌ challenge | ❌ challenge | ❌ challenge |
| **`wreq` + `wreq-util` `Emulation::Chrome146`** | ✅ | ✅ (302 locale redirect) | ✅ | ✅ `200` PNG, byte-size identical to curl_cffi |
| `curl_cffi` 0.16.0 `impersonate="chrome"` (= Chrome 146) | ✅ | ✅ | ✅ (403 app-level, 34 B JSON) | ✅ `200` PNG |

Conclusions:

- **hyprcurl is NOT a curl_cffi equivalent** for this site: it only sets cipher lists/curves
  through stock libcurl options (no GREASE/extension-permutation control), and its newest
  profile (Chrome 131) is already too old. Do not use it for OxidGene.
- **wreq + wreq-util works, but only with a current-Chrome emulation** (146 at time of
  writing; profiles go up to 149). The same client with Chrome 131 is challenged — the
  emulation version must track current Chrome as Cloudflare adapts.

  > **Measured, adopted, then removed (2026-08-17).** OxidGene shipped this
  > briefly and took it back out. Two reasons, and the matrix above is exactly
  > why: the working profile is a moving target with a *silent* failure mode —
  > a stale pin looks like an expired cookie — and getting BoringSSL into the
  > workspace cost a `bindgen` toolchain in CI plus a vendored, patched
  > `tungstenite` to resolve the OpenSSL symbol clash it created. Every request
  > now goes through the desktop app's Geneanet window instead, which needs no
  > emulation because it *is* a browser. The rows below stand as a record of
  > what was tested, not as a recommendation.
- `curl_cffi` (curl-impersonate patched BoringSSL) remains the reference; a Rust
  alternative with the same engine would be linking `curl-sys` against
  curl-impersonate's libcurl, but `wreq` makes that unnecessary.
- Anonymous `/media/api/references` past Cloudflare returns an app-level `403` (34-byte
  body) — i.e. reaching the app ≠ authenticated; `gntsess5` still required for data.
- www.geneanet.org issues `302` locale redirects to `en.geneanet.org` etc. — a client
  that follows redirects handles it transparently.
