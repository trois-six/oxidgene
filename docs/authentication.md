---
type: "Security Specification"
title: "Authentication and Sessions"
description: "Authentication of the web deployment against any OpenID Connect provider, OxidGene never holding a password: the Backend-for-Frontend sign-in with server-side tokens and a session cookie, bearer access tokens validated on every request, the client application named on every request and job, users provisioned from claims, how the web-only code stays out of the unchanged desktop, the Helm chart's external and Rauthy modes with the setup Rauthy is bootstrapped with, the development identity provider, security baselines, threat model, and tests."
tags: [oxidgene, specification, security, authentication, oidc, oauth, sessions, rauthy, deployment]
generated: { by: human:maintainer, at: 2026-11-02T16:00:00Z }
---

# Authentication and Sessions

> Part of the [OxidGene Specifications](index.md).
> See also: [Authorization](authorization.md) · [Privacy](privacy.md) ·
> [Messaging and Notifications](messaging.md) · [Architecture](architecture.md) ·
> [API Contract](api.md) · [Data Model](data-model.md) ·
> [Cross-cutting Rules](cross-cutting.md) · [Roadmap](roadmap.md)

**Status: in progress.** The deployment of §8 — the development stack, the
Helm chart's modes and the Rauthy they bundle — the end-to-end sign-in suite
of §12, phase 1 — identity, sessions, accounts and the profile, on the
server and in the web frontend (§10) — and the per-tree roles of phase 2
([Authorization](authorization.md)) are in place. This document
fixes the contract EPIC G follows;
[Roadmap §8](roadmap.md#8-planned-security-release-and-deployment) tracks its
phases. Until privacy is enforced for other viewers (phase 3), the backend
is never exposed to an untrusted network
([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)).

---

## 1. Principles

- **OxidGene is not an identity provider.** It stores no password, sends no
  verification or reset mail, keeps no second factor and locks no account.
  Local accounts, multi-factor authentication, password reset, lockout, SAML
  federation and social sign-in (Google, Microsoft, Apple, GitHub…) are the
  identity provider's job, brokered by it into OpenID Connect — Rauthy's
  upstream providers, Keycloak's identity brokering, or the equivalent of
  another provider.
- **One protocol.** OxidGene speaks OpenID Connect and OAuth 2.x only, against
  any standards-compliant provider: discovery, JWKS, `iss` and `aud` checks.
  It never implements a provider-specific login.
- **The provider authenticates, OxidGene authorizes.** Who may read or change
  a tree lives in OxidGene's database ([Authorization](authorization.md)); the
  provider holds no per-tree permission.
- **Tokens stay on the server.** The browser holds a session cookie and
  nothing else (§4). Every request is nevertheless authorized from a validated
  access token, never from the cookie alone (§5).
- **Every request names its user and its client.** The `ViewerContext` of a
  request carries the issuer and subject of the user, the client application
  the user signed in through, the granted scopes and the session (§5.4). Jobs
  carry the same actor (§5.5).
- **Web only.** Authentication, and everything that rests on it — users,
  roles, sharing, exclusions, privacy for viewers, messaging, email, API
  clients — is compiled and active only in the web build: the standalone
  server, its worker and the web frontend. The desktop changes nothing: one
  local user with full access, no sign-in, today's loopback guards (§7).

---

## 2. Deployment modes

| Mode | Where | Who is the viewer | Use |
|---|---|---|---|
| Desktop | Desktop's embedded server | Its one local user, with full access, as today | The desktop application, unchanged (§7) |
| `none` | Standalone server, `OXIDGENE_AUTH_MODE=none` | A local owner of every tree (issuer `local`, subject `local`), for every request | A personal web install equivalent to the desktop, and development and tests. Startup logs a `warn` (`auth_disabled`) at each start, `/auth/session` answers `mode: "none"`, and the Helm chart keeps refusing an Ingress without `ingress.exposeApiWithoutAuth` (§8.1) |
| `oidc` | Standalone server, `OXIDGENE_AUTH_MODE=oidc` | The signed-in user, an API client, a share-link holder or an anonymous visitor ([Authorization §4](authorization.md#4-viewercontext)) | Every shared deployment. The only mode that may face an untrusted network |

`none` is a supported mode, not a development shortcut, and stays so: the
web build run without authentication does everything the desktop does, and
looks as it does — no sign-in, account menu, banner or profile. What needs
identities is absent there, as on the desktop: accounts, roles and shares,
redaction for other viewers, messaging and API clients.

`oidc` is the default of the standalone server once EPIC G ships: a server
started without an issuer refuses to start (`auth_config_missing`) unless
`none` is chosen explicitly. A request in `none` mode records author `local`
in the history ([Authorization §9.1](authorization.md#91-authors-in-the-change-history)).

---

## 3. Identity provider

### 3.1 What OxidGene requires of it

- OpenID Connect Discovery (`/.well-known/openid-configuration`) and a JWKS.
- The Authorization Code flow with PKCE `S256`, for a confidential client.
- JWT access tokens whose `aud` can carry OxidGene's resource identifier
  (§5.2), or token introspection (RFC 7662) for opaque ones.
- Refresh tokens for the web client, rotated on use.
- `email` and `email_verified`, or an operator's statement that the provider's
  emails are verified (`OXIDGENE_OIDC_TRUST_EMAIL`, §3.2).

Optional, used when present: RP-initiated logout (`end_session_endpoint`),
back-channel logout, the `iss` authorization-response parameter (RFC 9207),
a groups or roles claim, the client credentials grant and client
registration (RFC 7591/7592) for API clients
([Authorization §7](authorization.md#7-api-clients)), and OAuth 2.0 Token
Exchange (RFC 8693) for delegated job work (§5.5).

### 3.2 Configuration

The server reads `oxidgene.toml` and the environment, as for its other
settings; every secret also has a `_FILE` variant naming a mounted file.

| Variable | Default | Description |
|---|---|---|
| `OXIDGENE_AUTH_MODE` | `oidc` | `oidc` or `none` (§2) |
| `OXIDGENE_PUBLIC_URL` | Required in `oidc` | The application's external origin, such as `https://genealogy.example.invalid`: the redirect URI's base, the cookie's host, the origin same-origin checks compare with, and the default resource identifier |
| `OXIDGENE_OIDC_ISSUER` | Required in `oidc` | The issuer, compared exactly with discovery's `issuer` and every token's `iss`, a trailing slash included (Rauthy's ends with one); discovery is read at the issuer without its trailing slash, followed by `/.well-known/openid-configuration` |
| `OXIDGENE_OIDC_BACKCHANNEL_URL` | Unset | Scheme, host and port for the server-to-server requests (discovery, JWKS, token, introspection, registration), when the issuer's public host is not reachable from the backend, such as `http://rauthy:8080`; the issuer check is unchanged (§3.4) |
| `OXIDGENE_OIDC_CLIENT_ID` | `oxidgene-web` | The web client's identifier |
| `OXIDGENE_OIDC_CLIENT_AUTH` | `client_secret_basic` | `client_secret_basic`, `client_secret_post` (the secret in the form body, for a provider that refuses HTTP Basic) or `private_key_jwt` (RFC 7523) |
| `OXIDGENE_OIDC_CLIENT_SECRET` | — | The web client's secret, for `client_secret_basic` and `client_secret_post` |
| `OXIDGENE_OIDC_PRIVATE_KEY_FILE` | — | The PEM key signing client assertions, for `private_key_jwt`: an RSA (`RS256`), P-256 (`ES256`) or Ed25519 (`EdDSA`) key |
| `OXIDGENE_OIDC_SCOPES` | `openid profile email` | Scopes asked at sign-in, plus the API scopes of [Authorization §7.2](authorization.md#72-scopes) the web client needs |
| `OXIDGENE_OIDC_AUDIENCE` | `OXIDGENE_PUBLIC_URL` | The resource identifier every access token's `aud` must contain (§5.2); never the client id, which is refused |
| `OXIDGENE_OIDC_ALGORITHMS` | `RS256,PS256,ES256,EdDSA` | The signature allow-list (§5.2) |
| `OXIDGENE_OIDC_TRUSTED_CLIENTS` | Unset | Client ids, other than the web client, whose user tokens are accepted as bearer tokens: the assistant clients' registration (`oxidgene-mcp`) and the operator's own interactive applications (§5.1) |
| `OXIDGENE_OIDC_INTROSPECTION` | `false` | Validate opaque access tokens through the introspection endpoint (§5.3) |
| `OXIDGENE_OIDC_ROLES_CLAIM` | Unset | Dotted path of a claim holding a list of strings, such as `groups`, `roles` or `realm_access.roles` (§6.3) |
| `OXIDGENE_OIDC_ADMIN_VALUES` | Unset | Values of that claim that make a user an instance administrator |
| `OXIDGENE_OIDC_CREATOR_VALUES` | Unset | Values that let a user create trees, with `OXIDGENE_AUTH_TREE_CREATION=claim` |
| `OXIDGENE_OIDC_ALLOWED_VALUES` | Unset | With `OXIDGENE_AUTH_SIGNUP=claim`, values one of which a user must carry to sign in |
| `OXIDGENE_OIDC_TRUST_EMAIL` | `false` | Treat `email` as verified when the provider sends no `email_verified`, for a provider that controls its addresses itself |
| `OXIDGENE_OIDC_ACCOUNT_URL` | Unset | The provider's account page, linked from the Account section (§10) |
| `OXIDGENE_AUTH_SIGNUP` | `invitation` | Who gets an account at first sign-in: `invitation`, `claim` or `open` (§6.2) |
| `OXIDGENE_AUTH_TREE_CREATION` | `everyone` | `everyone` or `claim` (§6.3) |
| `OXIDGENE_AUTH_BOOTSTRAP_ADMIN_EMAIL` | Unset | A verified email whose first sign-in becomes instance administrator while none exists (§6.2) |
| `OXIDGENE_SESSION_STORE` | `memory` | `memory` (one backend process) or `redis` (§4.5) |
| `OXIDGENE_REDIS_URL` | Unset | Redis for sessions and rate-limit counters (§4.5, §9.2) |
| `OXIDGENE_SESSION_KEYS` | Required in `oidc` | Comma-separated base64 32-byte keys; the first encrypts the tokens a session holds, every one decrypts, so keys rotate without signing anyone out |
| `OXIDGENE_SESSION_IDLE_TIMEOUT` | `2h` | A session unused this long ends |
| `OXIDGENE_SESSION_ABSOLUTE_LIFETIME` | `7d` | A session ends this long after sign-in, whatever its use |
| `OXIDGENE_ACCESS_LOG_RETENTION` | `180d` | How long access events are kept ([Authorization §9.2](authorization.md#92-access-log)) |

An invalid value fails startup with a stable category and never echoes the
value; no secret reaches a log.

### 3.3 Compatible providers

`external` mode (§8.1) works with any provider meeting §3.1. The table records
what each documented at the time of writing; the operator checks the
provider's current documentation.

| Provider | Groups or roles claim | Back-channel logout | Client credentials | Client registration (RFC 7591) | Notes |
|---|---|---|---|---|---|
| Rauthy | `groups`, `roles` | Yes | Yes | Yes, when enabled (`dynamic_clients`) | Deployed by the chart (§8.2) and the development stack (§8.5). No SAML. Its issuer ends with `/`; access tokens carry `email`, `email_verified`, `groups` and `roles`, the ID token the names too; an extra audience is a per-client setting (`default_aud`); a refresh token becomes valid only in its access token's last minute |
| Keycloak | `groups` mapper, `realm_access.roles` | Yes | Yes (service accounts) | Yes, and an admin REST API | Also brokers SAML: an institution whose sign-in is SAML connects through it, or another provider that has SAML, in `external` mode |
| Zitadel | Project roles claim | Yes | Yes (machine users) | No | Roles are an object keyed by role name: point `OXIDGENE_OIDC_ROLES_CLAIM` at it |
| Authentik | `groups` through a scope mapping | Recent releases | Yes | No | |
| Kanidm | `groups` | No | Check the release | No | PKCE is mandatory, which OxidGene always sends |
| Microsoft Entra ID | `groups` (object ids) or app `roles` | No (front-channel only) | Yes | No | No `email_verified`: set `OXIDGENE_OIDC_TRUST_EMAIL` only for a tenant that controls its addresses; scopes arrive as `scp` |
| Google | None | No | No | No | Refresh tokens only with Google's own parameters, so sessions end at their absolute lifetime; API clients need another provider |
| Auth0, Okta | Custom claims | Yes | Yes | Yes | |

Where a provider has no back-channel logout, a session it ended stays usable
in OxidGene until its access token can no longer be refreshed (§4.6).

### 3.4 Back-channel requests

The backend must reach the provider's token endpoint and JWKS while the
browser reaches its authorization endpoint, often under another name (an
in-cluster Service, a Compose service name). With `OXIDGENE_OIDC_BACKCHANNEL_URL`
set, OxidGene fetches discovery from the issuer's discovery address with its
scheme, host and port replaced by the back channel's and its path kept —
`http://rauthy:8080` turns the issuer `http://localhost:8180/auth/v1/` into
`http://rauthy:8080/auth/v1/.well-known/openid-configuration` — and, for its
own requests, uses the endpoints it returns with the same replacement where
they are on the issuer's origin: a provider such as Rauthy answers with its
public addresses whatever host it was asked on, while one such as Keycloak
with a dynamic back-channel hostname already answers with the host the
request used. The authorization and end-session endpoints, which the
browser follows, stay as discovery gives them. Discovery's `issuer`, and
every token's `iss`, must still equal `OXIDGENE_OIDC_ISSUER`. Discovery and JWKS
are refreshed as §5.2 says; a provider that cannot be reached at startup does
not stop the server, which answers `503 auth_unavailable` to what needs it
until it can.

---

## 4. Browser sessions (Backend-for-Frontend)

### 4.1 Why a Backend-for-Frontend

The web frontend is a public WebAssembly application. Holding access and
refresh tokens in it would expose them to any script running in the page —
an injected script, a compromised dependency — and to the browser's
storage, which the IETF's *OAuth 2.0 for Browser-Based Applications* best
current practice ranks as the weakest architecture. The Backend-for-Frontend
pattern it recommends keeps them out: the Axum server is the OAuth client — a
confidential one, authenticated to the provider — runs the flow, keeps the
tokens, and gives the browser an `httpOnly` cookie no script can read. The
frontend and the API share one origin (§9.1), so no CORS is needed and the
cookie is first-party.

### 4.2 Endpoints

These routes sit outside `/api/v1`, at the application's origin, and are
answered by the backend. They are a transport, not product operations, and
have no GraphQL twin.

| Method | Path | Description |
|---|---|---|
| `GET` | `/auth/login?return_to=/path&lang=fr` | Starts a sign-in (§4.3) and redirects to the provider. `return_to` must be a path of the application (§4.3); otherwise `/`. `lang`, optional, is the interface language, sent as `ui_locales` when it is one of the eight |
| `GET` | `/auth/callback` | The redirect URI. Completes the sign-in, rotates the session (§4.4), redirects to `return_to`, or to `/?auth_error=<code>` |
| `GET` | `/auth/session` | Always `200`, `Cache-Control: no-store`: `{ mode, authenticated, user?, client_id, csrf_token?, expires_at?, idle_expires_at?, account_url? }`. `mode` is `oidc` or `none`; `user` is `{ id, display_name, email, email_verified, locale, instance_role, can_create_trees }` |
| `POST` | `/auth/logout` | Ends the session (§4.7). Answers `{ logout_url }`, the provider's end-session address, or `null` when it has none |
| `POST` | `/auth/logout-everywhere` | Ends every session of the user, then as `/auth/logout` |
| `POST` | `/auth/backchannel-logout` | The provider's back-channel logout (§4.7). Takes no cookie and no CSRF token |
| `GET` | `/auth/share/{token}` | Redeems a share link and redirects to its tree ([Authorization §6.3](authorization.md#63-share-links)) |

`auth_error` codes, translated by the frontend: `access_denied` (the user
refused or the provider refused them), `login_failed` (any validation
failure), `signup_closed` (§6.2), `account_deactivated` (§6.5),
`auth_unavailable` (the provider could not be reached).

### 4.3 Sign-in

1. `/auth/login` creates a sign-in transaction: a `state` and a `nonce` of
   256 random bits each, a PKCE verifier, the validated `return_to`, the
   UI language. It stores it in the session store for ten minutes, keyed by
   the hash of `state`, with the hash of a fresh binding value, which it
   sets in the `__Host-oxidgene-login` cookie (`Secure`, `HttpOnly`,
   `SameSite=Lax`, `Path=/`, ten minutes). A sign-in started in another tab
   replaces it.
2. It redirects to the provider's authorization endpoint with
   `response_type=code`, `client_id`, `redirect_uri`
   (`{OXIDGENE_PUBLIC_URL}/auth/callback`), `scope`, `state`, `nonce`,
   `code_challenge`, `code_challenge_method=S256`, `ui_locales`, and
   `login_hint` when an invitation names the address.
3. `/auth/callback` requires the `state` of a live transaction **and** the
   login cookie whose hash it stored — a code planted in a victim's browser
   by another page finds neither — and consumes the transaction: a replay
   fails. It checks `iss` against the issuer when the response carries it
   (RFC 9207), then exchanges the code with the verifier and the client's
   authentication.
4. It validates the ID token (§5.2): signature, `iss`, `aud` containing the
   client id, `azp` equal to it when present, `exp`, `iat`, and `nonce`. It
   validates the access token as every request will (§5.2).
5. It provisions or updates the user (§6), refuses a deactivated one, and
   creates a session (§4.4).
6. It redirects to `return_to`. A `return_to` is accepted only as a path
   that starts with a single `/`, has no scheme, no authority, no backslash
   and no control character, at most 2 KiB: no open redirect.

### 4.4 Session cookie and CSRF

- The session cookie is `__Host-oxidgene`: `Secure`, `HttpOnly`,
  `SameSite=Strict`, `Path=/`, no `Domain`, no `Max-Age` beyond the
  absolute lifetime. Its value is 256 random bits; the store keeps only its
  SHA-256, so a read of the store yields no usable cookie. `Strict` costs
  nothing to a single-page application: the first navigation from another
  site loads static files, and every API call the page then makes is
  same-site.
- Signing in always creates a new session id, and any session the browser
  held is destroyed — no session fixation. So does a change of the user's
  instance role.
- A state-changing request (any method but `GET`, `HEAD`, `OPTIONS`)
  authenticated by the cookie must carry `X-OxidGene-CSRF` equal to the
  session's CSRF token, which `/auth/session` returns, compared in constant
  time; and an `Origin`, when present, must be `OXIDGENE_PUBLIC_URL`'s.
  Either failure is `403 forbidden`. `SameSite=Strict`, the header — which
  a cross-site form cannot set — and the origin check are three independent
  defences. A request carrying `Authorization: Bearer` is not
  cookie-authenticated: the cookie is ignored and no CSRF token is needed.
  The CSRF token is derived from the session id (the SHA-256 of a fixed
  label and the cookie value), so `/auth/session` recomputes it from the
  cookie the request carries while the store keeps only its hash; a page
  that cannot read the cookie cannot compute it. `/auth/logout` and
  `/auth/logout-everywhere` take the same checks.
- `localhost` is a secure context for browsers, so the development stack's
  `http://localhost` keeps the `Secure` and `__Host-` cookies (§8.5).

### 4.5 Session store

A session holds: the hashes of its id and CSRF token; the user's id, issuer
and subject; the client id; the provider's `sid`; the access token, its
expiry and its scopes; the refresh token; the ID token (the logout hint);
the share-link grants it redeemed; creation, last use, idle and absolute
expiries. The three tokens are encrypted with AES-256-GCM under the first of
`OXIDGENE_SESSION_KEYS`, through the `aws-lc-rs` the workspace already
builds.

| Store | When | Behaviour |
|---|---|---|
| `memory` | One backend process | A map in the process. A restart ends every session; users sign in again, usually without typing anything while their provider session lives |
| `redis` | Several backend replicas | `oxidgene:session:{hash}` with the idle expiry as TTL, plus the indexes `oxidgene:user-sessions:{user_id}` and `oxidgene:sid:{sid}` for ending a user's or a provider session's sessions. Redis is the one place the replicas share a session, a sign-in transaction and the rate-limit counters (§9.2) |

The Helm chart selects `redis` whenever Redis is configured and refuses to
render more than one backend replica, or autoscaling, with `memory` (§8.1).
Redis holds no genealogy and is not a cache of tree data
([Architecture](architecture.md)).

### 4.6 Lifetimes, refresh and revalidation

- A session ends after `OXIDGENE_SESSION_IDLE_TIMEOUT` without a request, or
  `OXIDGENE_SESSION_ABSOLUTE_LIFETIME` after sign-in.
- The access token is short-lived (five minutes for the clients the chart
  and the development stack bootstrap). When a request finds it expired or
  within a minute of expiring — not earlier: a Rauthy refresh token becomes
  valid only then — the server refreshes it before validating, once per
  session at a time: a lock in the
  store (`SET NX`, ten seconds) makes concurrent requests wait for that one
  refresh, because a refresh token rotated twice is a reuse that the
  provider answers by revoking the whole family. The new refresh token
  replaces the old one.
- A refused refresh (`invalid_grant`: the user was disabled, signed out, or
  their provider session ended) ends the session: `401 unauthenticated`.
  A provider that cannot be reached leaves the token unrefreshed; once it has
  expired, the request answers `503 auth_unavailable` — a request is never
  authorized from the cookie alone (§5.4).
- The idle and absolute limits bound a session even where a provider issues
  no refresh token, and the refresh bounds the time a session outlives its
  user being disabled at the provider to one access-token lifetime.

### 4.7 Sign-out

- `/auth/logout` destroys the session, clears the cookie, and returns the
  provider's end-session URL with `id_token_hint`, `client_id` and
  `post_logout_redirect_uri` (`{OXIDGENE_PUBLIC_URL}/`); the frontend
  navigates to it, ending the provider session too (RP-initiated logout).
- `/auth/backchannel-logout` takes the provider's `logout_token` (form
  field). It is validated like an ID token (signature, `iss`, `aud`, `iat`)
  and must carry the back-channel logout event, a `sid` or a `sub`, a `jti`
  not seen within its lifetime, and no `nonce`. Every session of that `sid`
  — or of that user — ends; a token naming both a `sid` no session holds
  and a user ends that user's sessions signed in without a `sid`, which no
  `sid` names. It answers `200`, or `400` for a token that fails
  validation.
- Deactivating or erasing a user, and `/auth/logout-everywhere`, end every
  session of the user at once, through the store's user index.

### 4.8 One session for REST, GraphQL and media

One authentication layer runs before the merged REST and GraphQL routers and
the direct media reads, so the three see the same principal, and GraphQL
resolvers receive the request's `ViewerContext` in their context. A GraphQL
request with an invalid or expired credential is refused at the transport
with `401`, exactly as REST; one with no credential executes as an anonymous
viewer, and a field it may not read answers `UNAUTHENTICATED`
([Authorization §4.3](authorization.md#43-errors-401-403-and-404)).
`/mcp` takes bearer tokens only, never the cookie
([Authorization §8](authorization.md#8-assistant-access-over-http-mcp)).

Each operation's requirement decides what a request without a credential
may do ([Authorization §3.2](authorization.md#32-permission-matrix)): the
open operations — the API description, the reference content, the export
destinations — answer it, every other answers `401 unauthenticated`, REST
before its handler and GraphQL per root field. A signed-in user has the
access their memberships give. A session records its last use at most once
a minute, so the idle limit holds to the minute without a store write per
request.

---

## 5. Access tokens and the identity of a request

### 5.1 Accepted credentials

| Credential | Principal | Client recorded |
|---|---|---|
| Session cookie of a signed-in user | That user | The web client (`OXIDGENE_OIDC_CLIENT_ID`), from the session's access token |
| Session cookie of a share-link holder | Anonymous visitor with the link's grant | The web client, with no token |
| No credential | Anonymous visitor | None |
| Bearer token whose `client_id` (or `azp`) is an OxidGene API client | That API client ([Authorization §7](authorization.md#7-api-clients)) | That client id |
| Bearer token whose `client_id` (or `azp`) is in `OXIDGENE_OIDC_TRUSTED_CLIENTS` | The user its `sub` names | That client id (an assistant's, an interactive application's) |
| Any other bearer token | Refused: `401 unauthenticated` | — |

A token issued to another application of the same provider — even one whose
audience happens to name OxidGene — is refused, because its client is
neither an API client nor trusted.

### 5.2 Validation

Every access token — the session's, at each request, and every bearer token —
and every ID token is validated as follows; a failure is
`401 unauthenticated`, with `WWW-Authenticate: Bearer error="invalid_token"`
for a bearer token.

- **Signature.** JWS with an algorithm of `OXIDGENE_OIDC_ALGORITHMS`; `none`
  and every HMAC algorithm are refused, whatever the header says, and the key
  must be the JWKS key whose `kid` the header names, of the matching type.
- **Claims.** `iss` equal to the issuer; `exp` in the future and `nbf`, `iat`
  not in the future, with 60 seconds of leeway; an access token's `aud`
  containing `OXIDGENE_OIDC_AUDIENCE` (the resource identifier — never a
  client id, so an ID token presented as an access token fails); an ID
  token's `aud` containing the client id. `typ` `at+jwt` (RFC 9068) and
  `JWT` are accepted for access tokens.
- **Scopes** come from `scope` (space-separated) or `scp` (a list) and
  bound what the request may do ([Authorization §7.2](authorization.md#72-scopes)).
- **JWKS.** Fetched at startup and cached for its `Cache-Control` lifetime,
  between five minutes and a day. An unknown `kid` refetches it at most once
  a minute, so a rotated key is picked up at once and a flood of forged
  `kid`s cannot hammer the provider. Keys that left the set stop validating.
- **Size.** A token over 16 KiB is refused before parsing.

JWT parsing and signature checks use the `jsonwebtoken` crate on the
`aws-lc-rs` backend the workspace already builds; discovery, JWKS and the
token endpoint use the existing `reqwest`. No OpenID Connect client crate is
added (§11).

### 5.3 Opaque tokens

With `OXIDGENE_OIDC_INTROSPECTION`, a token that is not a JWT is introspected
(RFC 7662) with the web client's credentials; `active`, `iss`, `aud`, `exp`,
`client_id` and `scope` are checked as in §5.2. The answer is kept for the
token's lifetime, at most 60 seconds, keyed by the token's SHA-256: one
token, one viewer, never shared.

### 5.4 The identity a request carries

Every request reaching a service carries a `ViewerContext`
([Authorization §4](authorization.md#4-viewercontext)) whose identity part
comes from a token validated for that request:

| Field | From |
|---|---|
| `issuer`, `subject` | The access token's `iss` and `sub` (`local` and `local` in `none` mode) |
| `user_id` | The user row keyed by them (§6) |
| `client_id` | The token's `client_id`, else `azp` |
| `scopes` | The token's scopes |
| `session_id` | The hash of the session id, for a cookie session |
| `api_client_id` | The API client the `client_id` names, if any |

The authentication layer gives every request this identity as an
`oxidgene_core::access::Principal` — `Local` in `none` mode, `Anonymous`, or
`User` with the fields above, the user's instance role and tree creation
right read with the user row — in the request's extensions; the
authorization builds the request's `ViewerContext` on it, which REST
handlers and GraphQL resolvers receive. `api_client_id` stays empty until
API clients ship.

The client id is also recorded in the change history's author
([Authorization §9.1](authorization.md#91-authors-in-the-change-history)),
in the access log, and as the `oxidgene.client_id` span attribute; client ids
are configuration, not personal data. The subject and the email never reach
a log or a span.

### 5.5 Background jobs

A job outlives the request that queued it, and the worker holds no session.
So a job row records, at submission, the actor of the request: issuer,
subject, user id, client id, API client id, scopes and session id hash, beside
the trace context it already keeps ([Data Model §6.10](data-model.md#610-changes-to-existing-tables)).
It never records an access or refresh token.

- Before it starts and again before each commit, the worker re-checks that
  the actor still holds the permission the job needs on its tree
  ([Authorization §3.2](authorization.md#32-permission-matrix)) — `import`
  or `export` — reading the user row and the membership afresh, and
  attributes every write and its audit entry to that actor and client. The
  re-check before the commit runs on the job's own transaction. A job the
  local owner submitted acts for the local owner.
- A worker that must call OxidGene's API or another service on the user's
  behalf obtains a short-lived delegated token through OAuth 2.0 Token
  Exchange (RFC 8693): it authenticates as its own confidential client
  (`oxidgene-worker`), presents its client credentials token as the actor
  token, names the recorded subject, and receives a token whose `act` claim
  names the worker, whose audience is the one service called and whose
  scopes are at most the recorded ones. Today's workers write through the
  services directly and need no token; the exchange is the contract for the
  first one that does. A provider without token exchange cannot run such
  jobs, and they fail with `delegation_unavailable`.
- When the actor's session is revoked while their job runs, the job goes on:
  authority comes from the actor's membership, re-checked before each commit,
  not from the browser session. A deactivated or erased user, or one who
  lost the permission, fails the job before its next commit
  (`actor_revoked`), leaving nothing half-written (open decision 1).

---

## 6. Users

### 6.1 Claims

| Claim | Use |
|---|---|
| `iss`, `sub` | The user's key. A user is never keyed or matched by email |
| `email`, `email_verified` | Matching invitations and shares by address ([Authorization §6](authorization.md#6-sharing)); shown to the user in their Account section. An unverified address matches nothing |
| `name`, else `preferred_username` | The display name until the user sets their own (§6.7) |
| `preferred_username` | The username another user may type to share a tree with them ([Authorization §6.1](authorization.md#61-sharing-with-a-user-of-the-instance)) |
| `picture` | Not used: an avatar it names would be fetched from a third party by every reader's browser ([Cross-cutting Rules §7](cross-cutting.md#7-privacy-and-anonymization)). A user's avatar is the image they upload, or generated from their initials (§6.7) |
| `locale` | The interface language when the device has none chosen yet |
| The roles claim | Instance roles (§6.3) |

Claims are refreshed at every sign-in and with every new ID token a refresh
returns; a display name the user set themselves is never overwritten.

### 6.2 Provisioning

A user row ([Data Model §6.1](data-model.md#61-app_user)) is created at the
first successful sign-in that passes the signup gate:

| `OXIDGENE_AUTH_SIGNUP` | Who gets an account |
|---|---|
| `invitation` (default) | An identity whose verified email has a pending invitation or share ([Authorization §6.2](authorization.md#62-invitations-by-email)), and the bootstrap administrator |
| `claim` | An identity carrying one of `OXIDGENE_OIDC_ALLOWED_VALUES` |
| `open` | Every identity the provider authenticates |

An identity refused by the gate gets no row and lands on `/?auth_error=signup_closed`;
nothing about it is stored. While no instance administrator exists, the first
identity whose verified email equals `OXIDGENE_AUTH_BOOTSTRAP_ADMIN_EMAIL`
becomes one; the variable is ignored afterwards. With `invitation`, the
address must be verified and the invitation pending — not expired, answered
or revoked — on a live tree.
With `claim`, an existing user who no longer carries an allowed value is
refused at sign-in with `access_denied`.

### 6.3 Instance roles from claims

With `OXIDGENE_OIDC_ROLES_CLAIM` set, the claim is authoritative at each
sign-in: a value of `OXIDGENE_OIDC_ADMIN_VALUES` makes the user an instance
administrator and its absence removes the role; with
`OXIDGENE_AUTH_TREE_CREATION=claim`, a value of `OXIDGENE_OIDC_CREATOR_VALUES`
grants tree creation the same way. Without the claim, administrators grant
both from the Instance section ([Authorization §10](authorization.md#10-api)).
The provider never holds per-tree roles.

### 6.4 Account linking

One identity, one user. OxidGene never links two identities, and never by
email: a provider that lets a user type an unverified address would otherwise
hand them someone else's trees. Linking a person's social, enterprise and
local sign-ins is the provider's job — Rauthy's upstream providers or
Keycloak's first-broker-login flow link them to one user there, so the `sub`
OxidGene sees stays the same.
Moving to another provider rebinds each user explicitly: an administrator sets
the new issuer and subject of an existing user
(`PUT /api/v1/admin/users/{user_id}/identity`), whose memberships and history
stay theirs.

### 6.5 Deactivation

An administrator deactivates a user: their sessions end at once (§4.7), their
API clients stop answering ([Authorization §7.4](authorization.md#74-credentials-rotation-and-revocation)),
their running jobs fail before the next commit (§5.5), and signing in answers
`account_deactivated`. Memberships are kept, so reactivating restores them. A
user disabled at the provider loses their sessions within one access-token
lifetime (§4.6) and keeps their row.

### 6.6 Erasure and account data

- **Erasure** (GDPR Art. 17), by the user from their Account section or by an
  administrator. It is refused (`409 conflict`) while the user owns a tree:
  they transfer or delete it first. Then, in one transaction: sessions end;
  memberships, pending invitations to their address, notifications,
  preferences and API clients are deleted (API clients deprovisioned at the
  provider by the worker); their messages are erased as
  [Messaging §9](messaging.md#9-retention-and-erasure) says; the user row
  becomes a tombstone — issuer, subject, email, names and locale cleared,
  status `erased`. History entries and access events keep pointing at the
  tombstone, shown as *Former member* with a short code derived from its id:
  readers of a tree's history still see that several changes share an
  author, and nothing identifies them (open decision 2). The genealogy they
  entered belongs to the tree and stays; a record describing them is the
  tree owner's to correct.
- **Account data** (GDPR Art. 15 and 20): `GET /api/v1/me/export` returns, as
  JSON, the user row and profile, memberships, invitations sent, API clients (no secret),
  notification preferences, the list of their conversations and their access
  events. Messages are end-to-end encrypted: the server holds no plaintext to
  export, and a conversation's text is exported from one of the user's devices
  ([Messaging §5](messaging.md#5-end-to-end-encryption)). Each section joins
  the export with the phase that stores it; today it holds the user row and
  profile, the memberships, the invitations to their address, the
  notification preferences and the access events
  ([API Contract §2](api.md#accounts)).

### 6.7 Profile and avatar

Every user has a profile (§10) holding their display name and avatar, beside
their language, notification preferences, sessions, API clients, blocked users
and account deletion, which the other sections and specifications define.

- **Display name.** 1 to 80 characters, trimmed, without control
  characters. Set by the user, it replaces the provider's name everywhere in
  OxidGene; cleared, the provider's name returns. It is not sent back to the
  provider.
- **Who sees a user.** A user's display name and avatar are visible to the
  user, to everyone who shares a tree with them (as owner or member), to the
  members of their conversations and message requests
  ([Messaging §4](messaging.md#4-conversations)), and to administrators. To
  anyone else the user does not exist (`404 not_found`): there is no user
  directory.
- **Uploaded avatar.** A JPEG, PNG or WebP image, its type sniffed from its
  bytes as for media, at most 5 MiB and 40 megapixels decoded, refused
  beyond either before decoding. The page lets the user choose a square in
  the image, centred by default, and sends its rectangle with the upload; the
  server applies the EXIF orientation, crops that square, scales it to 256 and
  64 pixels, flattens any transparency onto white and re-encodes both as JPEG
  with the `image` crate the workspace already builds. Re-encoding drops
  every metadata block — EXIF, GPS, XMP, ICC — so nothing of the original file
  survives but its pixels. Both sizes are stored through the existing
  `MediaStore` under the user-scoped key `users/{user_id}/avatar/{sha256}-{size}.jpg`,
  outside every tree, so a tree's purge never touches them; a replaced or
  removed avatar's files are deleted once the change commits, and erasure
  deletes them.
- **Serving.** `GET /api/v1/users/{user_id}/avatar?size=256|64` answers the
  JPEG to whoever may see the user, with `ETag` (its SHA-256),
  `Cache-Control: private, max-age=86400`, `Vary: Cookie, Authorization`,
  `X-Content-Type-Options: nosniff` and `Content-Security-Policy: sandbox`,
  and `If-None-Match` answered `304`. The frontend fetches it through its
  client and draws it from memory, as it does every media
  ([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)):
  the address of the backend never reaches the markup.
- **Generated avatar.** Without an upload, the avatar is the user's initials —
  the first letter of the first two words of the display name, upper-cased,
  `?` when there is none — on a background chosen from the user's id: the
  first byte of the SHA-256 of the id's sixteen bytes, modulo the palette's
  size, indexes a fixed palette of eight avatar colours
  defined among the design tokens of `components/layout.rs` for every theme,
  each paired with a text colour that keeps a contrast of at least 4.5:1. The
  frontend draws it itself; no request, no external service, the same colour
  for a user on every device.

| Method | Path | GraphQL | Description |
|---|---|---|---|
| `PUT` | `/me` | `updateMe` | `{ display_name?, locale? }`; `display_name: null` returns to the provider's name |
| `PUT` | `/me/avatar` | `uploadMyAvatar` | Multipart: the image (`file`) and the crop square (`x`, `y`, `size`, in pixels of the upright image). GraphQL takes the image base64-encoded, as it does media uploads, within its body limit |
| `DELETE` | `/me/avatar` | `deleteMyAvatar` | Back to the generated avatar |
| `GET` | `/users/{user_id}` | `userProfile` | `{ id, display_name, avatar }`, `avatar` being `{ etag, updated_at }` or `null` |
| `GET` | `/users/{user_id}/avatar?size=` | — | The image; a direct read, REST only like media files. GraphQL's `UserProfile.avatarUrl` gives its URL, with the avatar's `etag` as `v` so that a replaced avatar is a new address |
| `GET` | `/me/sessions` | `mySessions` | The user's sessions: an opaque handle, creation, last use, client, browser family read from the user agent, and which one is the current |
| `DELETE` | `/me/sessions/{session_id}` | `endMySession` | Ends one session; the current one signs out |

These paths take the `/api/v1` prefix and are part of the API contract
([API Contract §2](api.md#accounts)). `GET /me` returns the profile with
`provider_name`, `email`, `locale`, `instance_role` and `can_create_trees`.
In `none` mode, whose local owner has no account, they and the
administration operations are absent, as on the desktop: `404 not_found`.

---

## 7. Web build only; the desktop is unchanged

Everything this specification and [Authorization](authorization.md),
[Privacy](privacy.md) and [Messaging and Notifications](messaging.md)
describe is compiled and active only in the web build — `oxidgene-server`,
`oxidgene-worker` and `oxidgene-web` — the way GraphQL already is:

- **Backend.** The code lives behind an optional `auth` feature of
  `oxidgene-api` (sessions, token validation, users, memberships, sharing,
  exclusions, viewer redaction, notifications, mail, messaging, API clients),
  enabled by `oxidgene-server` and `oxidgene-worker` only. Without it the
  services receive the full-access viewer the desktop has today, the routes
  of these specifications are not mounted (`404 not_found`), and none of
  their dependencies is linked. The tables exist in the one schema both
  backends share, and stay empty on the desktop. The authorization itself
  (`oxidgene-api`'s `authz`) is compiled everywhere: without the feature the
  viewer is always the local owner, who passes every requirement on every
  live tree.
- **Frontend.** `oxidgene-ui` stays platform-independent: the sign-in,
  account, members, sharing, exclusion, notification and messaging surfaces
  appear only when the web entry point injects their capability, as desktop-
  only capabilities are injected today in the other direction; the desktop
  injects none, so it renders exactly what it renders now.
- **A guard** keeps the `auth` feature out of `oxidgene-desktop`'s
  dependency graph, as one does for `graphql`.

The desktop keeps its one local user with full access to every tree, no
sign-in and no session, and its embedded server keeps the guards of
`oxidgene-api::access` as they are: the launch token required as
`Authorization: Bearer` on every request but the API description, the
loopback host check, and the security headers
([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication)).
Its MCP stdio server acts for that same user ([Assistant Access](mcp.md)).
The export options that already exist keep working there as they do today
([Privacy §8](privacy.md#8-exports)).

---

## 8. Deployment

### 8.1 Helm chart

The chart has an `auth` block, following its existing pattern for
CloudNativePG, Redis and RustFS: modes `disabled`/`existing`/managed, the
chart never installing an operator or its CRDs.

| Value | Default | Description |
|---|---|---|
| `auth.idp.mode` | `external` | `external`: any provider (§3.3); `rauthy`: a Rauthy the chart deploys (§8.2); `none`: no authentication (§2) |
| `auth.publicUrl` | Required unless `none` | `OXIDGENE_PUBLIC_URL`, an origin without a path; with the chart's Ingress, its scheme followed by `ingress.host` |
| `auth.oidc.issuer` | Required with `external` | The issuer; `https://<auth.idp.rauthy.hostname>/auth/v1/` with `rauthy` |
| `auth.oidc.backchannelUrl` | `""` | §3.4; the Rauthy Service with `rauthy` |
| `auth.oidc.clientId` | `oxidgene-web` | |
| `auth.oidc.clientSecret.secret`, `.key` | `oxidgene-oidc`, `client-secret` | Existing Secret holding the web client's secret |
| `auth.oidc.rolesClaim`, `adminValues`, `creatorValues`, `allowedValues`, `trustEmail`, `trustedClients`, `accountUrl` | Unset | §3.2; lists for the values. With `rauthy`, an empty `rolesClaim` takes Rauthy's `groups` with the groups of §8.3, and an empty `accountUrl` Rauthy's account page |
| `auth.signup`, `auth.treeCreation`, `auth.bootstrapAdminEmail` | `invitation`, `everyone`, `""` | §6.2, §6.3 |
| `auth.session.keysSecret.secret`, `.key` | `oxidgene-session`, `keys` | Existing Secret holding `OXIDGENE_SESSION_KEYS` |
| `auth.session.idleTimeout`, `absoluteLifetime` | `2h`, `7d` | §4.6; a number and one unit among `s`, `m`, `h` and `d` |
| `auth.idp.rauthy.*` | §8.2 | The Rauthy mode |

- `external` renders nothing for the provider; rendering fails until
  `auth.oidc.issuer` is set, naming the three modes, so a deployment chooses
  its provider explicitly (open decision 3).
- The backend receives the variables of §3.2, its secrets from the Secrets
  above through `secretKeyRef`; `none` sets `OXIDGENE_AUTH_MODE=none` and
  nothing else of this section.
- The session store is `redis` whenever `redis.mode` is not `disabled`;
  with `redis.mode=disabled` and authentication on, rendering fails for
  `backend.replicaCount` above 1 or backend autoscaling.
- The Ingress gains the backend paths `/auth`, `/mcp` and
  `/.well-known/oauth-protected-resource`, so the frontend and the API share
  one origin. With a mode other than `none`, `ingress.exposeApiWithoutAuth` is
  no longer required, and `auth.publicUrl` must name the Ingress's host; with
  `none` it still is, and `NOTES.txt` prints that every tree is readable and
  writable by whoever reaches the backend.
- The frontend's Content-Security-Policy is unchanged: the browser talks to
  its own origin only, and the provider's pages are navigated to, not
  fetched.

### 8.2 Rauthy mode

With `auth.idp.mode=rauthy` the chart deploys
[Rauthy](https://github.com/sebadob/rauthy), an OpenID Connect provider
written in Rust, itself: a StatefulSet and its Services, no operator and no
custom resource of its own. Rauthy holds the passwords, passkeys and second
factors, its users' account page, the sign-in through upstream providers
(any OpenID Connect or OAuth provider, configured in its admin UI) and its
own mail; OxidGene still speaks only OpenID Connect to it (§1).

Rauthy has **no SAML**. An institution whose sign-in is SAML connects through
`external` mode to a provider that has it, such as Keycloak or Zitadel, or
brokers its SAML into OpenID Connect elsewhere.

| Value | Default | Description |
|---|---|---|
| `auth.idp.rauthy.name` | `<release>-rauthy` | The StatefulSet, its Service and `<name>-headless` |
| `auth.idp.rauthy.image` | `ghcr.io/sebadob/rauthy`, pinned | Read Rauthy's changelog before moving it: its configuration changes between releases |
| `auth.idp.rauthy.replicas` | `1` | 1, or an odd number — 3 for high availability — for its Raft cluster to keep a majority; rendering refuses an even one |
| `auth.idp.rauthy.hostname` | Required | Its own host, such as `auth.genealogy.example.invalid`: the issuer becomes `https://<hostname>/auth/v1/` |
| `auth.idp.rauthy.ingress.enabled`, `className`, `annotations`, `tlsSecret` | `true`, `""`, `{}`, `""` | An Ingress for that host; TLS terminates there |
| `auth.idp.rauthy.trustedProxies` | The private ranges | Proxies whose forwarded headers Rauthy trusts: the Ingress controller's pods |
| `auth.idp.rauthy.secret` | `oxidgene-rauthy` | Existing Secret: `enc-keys` (`id/base64-32-bytes`, one per line), `enc-key-active`, `raft-secret` and `api-secret` (Hiqlite's, 16 characters at least), `api-key` (64 alphanumeric characters at least, the job's). The web client's secret must be 64 alphanumeric characters too, as Rauthy requires |
| `auth.idp.rauthy.admin.email`, `passwordKey` | `""`, `""` | Rauthy's administrator; without a password key in the Secret, Rauthy prints a first password once in its log |
| `auth.idp.rauthy.database.mode` | `hiqlite` | `hiqlite`: Rauthy's embedded SQLite, replicated by Raft, on a volume per pod; `cloudnativepg`: a CloudNativePG `Cluster` of its own, `<name>-db` (open decision 4) |
| `auth.idp.rauthy.database.hiqlite.storage.size`, `storageClass` | `1Gi`, `""` | |
| `auth.idp.rauthy.database.cloudnativepg.*` | `2` instances, `5Gi`, database and owner `rauthy` | Also `requireCrd`, `clusterName`, `imageName` and `resources`, as for OxidGene's own `Cluster` |
| `auth.idp.rauthy.smtp.host`, `port`, `tlsMode`, `from`, `user`, `passwordSecret` | `""` (none), `587`, `starttls`, `""`, `""`, `rauthy-smtp`/`password` | Rauthy's own mail (address verification, password reset, new-login notices) — separate from OxidGene's ([Messaging §3](messaging.md#3-email)) |
| `auth.idp.rauthy.initImage` | `curlimages/curl`, pinned | The init container and the job below |
| `auth.idp.rauthy.resources` | requests `100m`/`128Mi`, limit `512Mi` | Indicative |

- **Workload.** A StatefulSet whose pods start together
  (`podManagementPolicy: Parallel`), each taking its Raft id from its
  ordinal, peers listed in the configuration through the headless Service
  (ports 8100 and 8200, published before readiness); readiness on Hiqlite's
  `/ready`, liveness on `/auth/v1/health`; a disruption budget of one pod
  above one replica. Its configuration is a ConfigMap mounted at
  `/app/config.toml`; its secrets reach it through the environment, which
  overrides the file. Rauthy runs read-only as UID 10001.
- **Hostname and back channel.** Rauthy serves plain HTTP behind the Ingress
  (`proxy_mode` with `trustedProxies`), its public URL the hostname, so its
  discovery answers with public addresses whatever host it is asked on.
  OxidGene's `backchannelUrl` is the Rauthy Service,
  `http://<name>.<namespace>.svc.<clusterDomain>:8080`, and the backend
  rewrites those addresses to it (§3.4), never hairpinning through the
  Ingress.
- **Storage.** Hiqlite by default (open decision 4): no second PostgreSQL to
  run, back up and upgrade, three pods for high availability, each pod's
  volume kept by Kubernetes when the release is uninstalled. With
  `cloudnativepg`, Hiqlite only caches and the pods keep no volume.
- **Cost.** One Rust process per pod, a fraction of a JVM provider: the
  defaults request 100m CPU and 128 MiB per pod.
- **Administrator.** Rauthy's admin UI, at `https://<hostname>/auth/v1/admin`,
  requires its administrator to register a passkey first (Rauthy's
  `admin_force_mfa`, on). There the administrator creates users, adds
  instance administrators to `oxidgene-admins`, and connects upstream
  providers.

### 8.3 The bootstrapped setup

Rauthy reads a bootstrap directory at its first start, on an empty
database. An init container of the StatefulSet writes it into a memory
volume of the pod from `charts/oxidgene/files/rauthy/`, filling in the web
client's secret from `auth.oidc.clientSecret` and the API key from
`auth.idp.rauthy.secret`; no secret is in a ConfigMap or in values.

| Part | Content |
|---|---|
| Client `oxidgene-web` | Confidential, its secret the Secret's; authorization code and refresh token only; PKCE `S256` required; redirect URI `{publicUrl}/auth/callback`; post-logout redirect `{publicUrl}/`; back-channel logout URI `{publicUrl}/auth/backchannel-logout`; no allowed origins (no CORS); access tokens of five minutes signed with `EdDSA`; scopes and default scopes `openid`, `profile`, `email`, `groups` and the API scopes; extra audience `{publicUrl}` (`default_aud`) |
| Client `oxidgene-mcp` | Public; authorization code with PKCE `S256`; loopback redirects, the operator adding a hosted assistant's redirect URI ([Authorization §8](authorization.md#8-assistant-access-over-http-mcp)); the API scopes but `instance:admin` |
| Client `oxidgene-worker` | Confidential, its secret generated by Rauthy; client credentials and token exchange (§5.5) |
| Groups | `oxidgene-admins`, `oxidgene-creators`; the chart sets `rolesClaim: groups`, `adminValues: [oxidgene-admins]`, `creatorValues: [oxidgene-creators]` |
| Scopes | `tree:read`, `tree:write`, `media:read`, `media:write`, `import`, `export`, `history:read`, `tree:admin`, `instance:admin` ([Authorization §7.2](authorization.md#72-scopes)), in the token's `scope` |
| API key `oxidgene-chart` | Reads and updates clients, nothing else: the job's |
| Users | None but Rauthy's administrator |

Rauthy's bootstrap files cannot set a client's extra audience. After each
install and upgrade, a Helm hook job (`<name>-configure`) applies
`oxidgene-web`'s whole definition through Rauthy's admin API
(`PUT /auth/v1/clients/oxidgene-web`, authenticated by the API key), retrying
until Rauthy answers: the chart owns that client, and a change made to it in
the admin UI lasts until the next upgrade.

The tokens Rauthy issues for `oxidgene-web` carry, measured on the
development stack:

| Token | Claims |
|---|---|
| Access (`typ` header `at+jwt`) | `iss`, `sub` (the user's id), `aud` (`oxidgene-web` and `{publicUrl}`), `azp`, `scope`, `email`, `email_verified`, `groups`, `roles`, `iat`, `nbf`, `exp`, `jti`; no `client_id` (`azp` names the client) and no `sid` |
| ID (`JWT`) | The same identity, `aud` `oxidgene-web`, plus `sid`, `nonce`, `auth_time`, `amr`, `at_hash`, `preferred_username` (the email), `name`, `given_name`, `family_name`, `locale` |

A user without a group or role may get `[""]`: an empty value matches no
configured value.

### 8.4 Limits of the bootstrap

- Rauthy applies its bootstrap directory only to an empty database. Changing
  the chart's values afterwards changes nothing in Rauthy but
  `oxidgene-web`, which the job applies again: groups, scopes and the other
  clients change through the admin UI or the admin API.
- Rotating the web client's secret means generating a new one in Rauthy's
  admin UI (Rauthy 0.37 generates a secret; it does not take a chosen one),
  writing it into the `auth.oidc.clientSecret` Secret, then restarting the
  backend.
- Uninstalling the release keeps the Hiqlite volumes, as Kubernetes keeps a
  StatefulSet's claims, and deletes a `cloudnativepg` database: back it up,
  or keep the `Cluster` out of the release's lifecycle, as for OxidGene's own
  database.

### 8.5 Development stack

The development Compose stack (`docker/docker-compose.yml`) runs the
provider, its mail relay and the session store beside the application:

- A `rauthy` service, `ghcr.io/sebadob/rauthy` pinned to a release, with
  `docker/rauthy/config.toml`: plain HTTP published on `127.0.0.1:8180`,
  public URL `localhost:8180`, its embedded Hiqlite database in the
  container — a recreated container starts empty and bootstraps afresh —
  and Rauthy's administrator `idp-admin@example.invalid` /
  `idp-admin-development`, without the passkey requirement. The server sets
  `OXIDGENE_OIDC_ISSUER=http://localhost:8180/auth/v1/` and
  `OXIDGENE_OIDC_BACKCHANNEL_URL=http://rauthy:8080`.
- `docker/rauthy/bootstrap/`, the setup of §8.3 for the public URL
  `http://localhost:8081` with development secrets, and fictitious users
  only, each named after a role and with its role as password:
  `admin@example.invalid` (group `oxidgene-admins`), `owner@example.invalid`,
  `editor@example.invalid`, `guest@example.invalid`,
  `visitor@example.invalid` and `outsider@example.invalid`, all with verified
  addresses; a confidential client `oxidgene-dev-robot` with the client
  credentials grant (its secret sent in the form) and `tree:read`, for
  API-client tests; and the API key `oxidgene-dev`, which also reads users
  and ends their sessions for the end-to-end suite. It differs from the
  chart's only there, and by calling the back-channel logout at
  `http://server:8080/auth/backchannel-logout`, the server's Compose name,
  which the server accepts through `OXIDGENE_ALLOWED_HOSTS=server`.
  `just chart-test` fails when the two setups drift apart.
- A one-shot `rauthy-init` service applies `docker/rauthy/oxidgene-web.json`
  through the admin API once Rauthy answers, as the chart's job does; the
  server waits for it. Rauthy's image has no shell or HTTP client for a
  health check.
- `docker/rauthy/oxidgene-dev-auth.env`, the server's settings for it:
  `oidc` mode, the public URL, issuer, client and its secret, the `groups`
  roles claim, `OXIDGENE_AUTH_SIGNUP=open` (nobody can be invited yet),
  `admin@example.invalid` as bootstrap administrator, the account page and
  a development session key. Compose adds the back channel and
  `OXIDGENE_SESSION_STORE=redis` on the stack's `redis`.
- A `mailpit` service keeps every mail — Rauthy's own, without which Rauthy
  stops at start, and OxidGene's — for inspection on `http://127.0.0.1:8025`
  (SMTP on `127.0.0.1:1025`) and delivers none
  ([Messaging §3.5](messaging.md#35-without-smtp)). The settings file above
  names it as `localhost:1025` with `OXIDGENE_SMTP_TLS=none` and
  `OXIDGENE_MAIL_FROM` `OxidGene development <oxidgene@example.invalid>`,
  for `just dev-web`, whose embedded worker sends; the stack's server, which
  queues OxidGene's mail, and its worker, which reads the same file and
  sends it, use `mailpit:1025` with `danger-insecure` instead.
- The `web` container proxies `/api`, `/graphql`, `/auth`, `/mcp` and
  `/.well-known/oauth-protected-resource` to the server
  (`docker/nginx-dev-proxy.conf`, mounted into the image's
  `/etc/nginx/locations.d/`, which the published image leaves empty), with
  the browser's `Host` and the forwarded headers; its bundle calls the API on
  its own origin. The stack is opened at `http://localhost:8081`, so the
  browser sees one origin and keeps its `Secure` cookies.
- `just dev-web` keeps `OXIDGENE_AUTH_MODE=none`; with
  `OXIDGENE_AUTH_MODE=oidc` it reads the settings file above, builds the
  bundle for its own origin, and serves it on `http://localhost:8081`
  through the Dioxus development server, whose proxy (`[[web.proxy]]` of
  `apps/oxidgene-web/Dioxus.toml`) forwards the same paths to the backend
  on port 8080, against the Compose Rauthy, started alone — with Mailpit and
  `rauthy-init` — by `just dev-idp-up` and removed by `just dev-idp-down`.
  Sessions stay in memory, and the provider's back-channel logout cannot
  reach a backend outside Compose.

---

## 9. Security baselines

### 9.1 Origin, headers and caching

- **One origin.** In `oidc` mode the frontend and the API are served under
  `OXIDGENE_PUBLIC_URL`; the server sends no CORS headers, and a browser
  application on another origin cannot use the API — non-browser clients use
  bearer tokens, which need no CORS. The `none` mode keeps today's single
  trusted CORS origin.
- **Headers.** The existing ones stay ([Cross-cutting Rules §7.1](cross-cutting.md#71-backend-exposure-before-authentication));
  the API and nginx add `Strict-Transport-Security: max-age=63072000;
  includeSubDomains` when the public URL is `https`.
- **Caching.** `/auth/*` and every JSON response holding tree data answer
  `Cache-Control: no-store`; media answer `Cache-Control: private` with
  `Vary: Cookie, Authorization`, keeping their `ETag`. No response is cached
  across viewers, by OxidGene or by an intermediary
  ([Privacy §7](privacy.md#7-enforcement-points)). The frontend's in-memory
  caches are dropped when the session's user changes or ends. Built: the web
  build marks every JSON answer of the API, REST's and GraphQL's, that sets
  no caching of its own `no-store`, and sends `Vary: Cookie, Authorization`
  beside the `private` caching and the `ETag` of a medium's bytes and crops.
  The desktop keeps its headers: it has one viewer, and its per-launch token
  would make every launch miss the pictures the previous one cached.

### 9.2 Rate limits and brute force

Password guessing, credential stuffing and lockout are the provider's.
OxidGene limits what it answers itself, per address for anonymous requests
and per principal otherwise, with counters in Redis when several replicas run
and in the process otherwise; past a limit, `429 rate_limited` with
`Retry-After` (GraphQL `RATE_LIMITED`).

| What | Default limit |
|---|---|
| `/auth/login`, `/auth/callback` | 30 per minute per address |
| `/auth/share/{token}` | 30 per minute per address |
| Share and invitation lookups by email or username, and the invitation mails resent | 30 per hour per user ([Authorization §6.1](authorization.md#61-sharing-with-a-user-of-the-instance)) |
| An API client | 600 requests per minute, bursts of 100 ([Authorization §7.5](authorization.md#75-rate-limits-and-usage)) |
| Messages | [Messaging §8](messaging.md#8-rate-limits) |

Addresses are held in counters for the window only and never stored.

### 9.3 Logs

Authentication logs carry stable categories — `login_succeeded`,
`login_failed` with a reason code, `session_ended` with its cause,
`token_invalid` with the failed check (`signature`, `issuer`, `audience`,
`expired`, `algorithm`), `refresh_failed`, `idp_unreachable`,
`backchannel_logout` — with the client id, never a token, a cookie, a code,
an email, a subject, a name or an address
([Cross-cutting Rules §5.3](cross-cutting.md#53-sensitive-data)). What an
administrator may need to trace is in the access log, a table with its
retention ([Authorization §9.2](authorization.md#92-access-log)).

### 9.4 Threat model

| Threat (STRIDE) | Example | Mitigation |
|---|---|---|
| Spoofing | Stolen session cookie | `httpOnly`, `Secure`, `SameSite=Strict`, `__Host-`; idle and absolute limits; ended by back-channel logout, deactivation, sign-out everywhere |
| Spoofing | Forged or substituted token | Signature with an allow-listed algorithm and a JWKS key; exact `iss`; `aud` as the resource identifier; client must be trusted or an API client (§5.1) |
| Spoofing | Authorization code injection, provider mix-up | PKCE `S256`; `state` bound to a login cookie; `nonce`; one issuer, RFC 9207 `iss` check |
| Tampering | Cross-site request forgery | `SameSite=Strict`; CSRF header; `Origin` check (§4.4) |
| Tampering | Open redirect after sign-in | `return_to` restricted to application paths (§4.3) |
| Repudiation | A write nobody owns | Every history entry names its user and client ([Authorization §9.1](authorization.md#91-authors-in-the-change-history)); jobs carry their actor (§5.5); access log |
| Information disclosure | Tokens readable by page scripts | Backend-for-Frontend: no token in the browser (§4.1) |
| Information disclosure | Enumerating trees or users | `404` for what the viewer may not see ([Authorization §4.3](authorization.md#43-errors-401-403-and-404)); no user directory; rate-limited exact lookups |
| Information disclosure | Living persons leaking through search, statistics, exports, caches or logs | Server-side redaction in one policy ([Privacy](privacy.md)); `no-store`; anonymized logs |
| Information disclosure | Share token in a URL | Redeemed then redirected away; `Referrer-Policy: no-referrer`; route templates in logs |
| Denial of service | Flooding sign-in or forged `kid`s | Rate limits (§9.2); JWKS refetch at most once a minute (§5.2) |
| Denial of service | Expensive scopes or exclusions | Materialized, rebuilt once per change ([Authorization §6.5](authorization.md#65-access-scopes)) |
| Elevation of privilege | An editor granting themselves more | Only the owner manages members, shares and exclusions ([Authorization §3.2](authorization.md#32-permission-matrix)) |
| Elevation of privilege | An API client wider than its owner | Effective rights are the owner's ∩ the client's ∩ the token's scopes ([Authorization §7.1](authorization.md#71-design)) |
| Elevation of privilege | Cross-tree identifiers, GraphQL nesting | Tree-scoped checks per record; nested resolvers go through the same services ([Authorization §4.2](authorization.md#42-propagation)) |

---

## 10. User interface

The account UI is one capability of `oxidgene-ui` (`account.rs`), which the
web entry point injects and the desktop does not (§7). With it, the
application asks `/auth/session` as it starts, without making the page wait
for the answer, so that the `none` mode loads as fast as the desktop. A
`none` answer, or none at all, leaves the interface exactly the desktop's,
and no other account route is ever called.

- **Navbar.** In `oidc` mode the right zone shows **Sign in** to an anonymous
  visitor, or the user's avatar opening a menu: their name and email,
  **Profile**, **Sign out**. `none` mode shows neither, nor any banner: the interface is the
  desktop's (§2). The avatar is the shared trigger of a menu
  ([Common UI §4.8](ui-common.md#48-contextmenu)) and the one avatar component
  ([Common UI §4.19](ui-common.md#419-useravatar)).
- **Sign in** navigates to `/auth/login?return_to=` the current route, with
  `lang` the interface language. A failed sign-in shows the translated
  `auth_error` message in a dialog on the page it returns to, with **Try
  again**, and the code leaves the address so a reload does not show it again.
- **A visitor.** Until phase 2 there is nothing an anonymous visitor may read
  (§4.8): once the answer names nobody, the home page, every page of a tree
  and the profile show, instead of their content and its refused requests,
  *Sign in to continue* with **Sign in**, which returns to that page. The
  application settings stay open: they are the device's.
- **Signed in.** Every state-changing request carries the session's CSRF token
  (§4.4). The account's language becomes the interface's on a device where
  none was chosen, without being stored as the device's choice.
- **Session end.** A `401` on a request the page made opens a dialog —
  *Your session has ended* — whose **Sign in again** keeps the current route,
  and whose **Close** leaves the page as it was: an open form keeps its
  entries in memory, for the user to copy before signing in again.
- **Sign out** posts `/auth/logout` and goes to the provider's end-session
  address it answers, or to the home page; either way the page loads afresh,
  with nothing of the account left in memory, nor in the tab's saved page
  history ([Common UI](ui-common.md)), which names the persons visited.
  **Sign in again** after a session ended forgets that history too: whoever
  signs in next may be someone else.
- **Profile page** (`/profile`, web only, signed in), in the `sub-page`
  layout with a left navigation like App Settings:
  - *Profile*: the avatar with **Upload** (file picker, then a square crop
    selector with zoom) and **Remove**, the display name with **Save** and
    *Use my provider's name*, the email read-only as the provider gives it;
  - *Language*: the interface language, saved for the account and used as the
    device's choice where none is stored;
  - *Notifications*: the preferences of [Messaging §2.3](messaging.md#23-preferences);
  - *Sessions*: the list of §6.7 with **End** per session and **Sign out
    everywhere**;
  - *API clients*: the user's own clients ([Authorization §7](authorization.md#7-api-clients));
  - *Messaging security* and *Blocked users*
    ([Messaging §12](messaging.md#12-user-interface));
  - *Account*: a link to the provider's account page (`account_url`) for the
    password and second factor, **Download my account data**, and **Delete
    my account**, which lists the trees to transfer or delete first.

  Phase 1 ships *Profile*, *Language*, *Sessions* and *Account*, phase 4
  *Notifications* — a switch *Send me emails* and a choice per kind, *Not by
  email*, *By email, at once* or *In a daily email summary*, or, on a
  server without a relay, only that notifications appear in the
  application; the other sections join the page with their phases. Until memberships exist (phase 2)
  no user owns a tree, so deletion has nothing to list; a refusal (`409`)
  says to transfer or delete the trees first. On the desktop and in `none`
  mode, `/profile` is not found.
- **Instance.** An administrator finds an *Instance* group in App Settings
  ([App Settings §12](ui-app-settings.md#11-section-instance-web-only)) whose *Users*
  section lists every user by cursor — avatar, name, username and email, role,
  tree creation, status, last sign-in — with a menu per user: the
  administrator role and tree creation granted or withdrawn, deactivation and
  reactivation, **Change identity…** (issuer and subject, §6.4) and **Erase**
  behind a confirmation. An erased user is listed as *Former member*, with no
  menu; the administrator's own row has none either, their account being
  their profile's. A change the roles claim decides (§6.3) is refused by the
  server, and the section says so.

  Another user's name or avatar, wherever it appears, opens a small card
  with both, for whoever may see that user. The card arrives with the first
  surface that shows other users to those who are not administrators
  (members, phase 5); the administrator's list already shows what it would.
- Every label, message, dialog and accessibility text goes through i18n in
  every interface language.

---

## 11. Dependencies

| Need | Choice | Cost |
|---|---|---|
| HTTP to the provider | `reqwest`, already in the workspace | None |
| JWS verification | `jsonwebtoken` on the `aws-lc-rs` backend | One crate and a few small ones; the backend is already built |
| Token encryption at rest | `aws-lc-rs` AES-256-GCM, already built | None |
| Session store | The `redis` crate with its Tokio and `rustls` features | A handful of crates, server only |
| OxidGene's mail | `lettre`, in the same `auth` feature ([Messaging §13](messaging.md#13-dependencies-and-size)) | A few small crates, server and worker only |
| OpenID Connect client crates (`openidconnect`, `oauth2`) | **Not used**: discovery, the code flow and refresh are a few hundred lines over `reqwest`, and the crates bring their own HTTP and crypto stacks | — |
| The bundled identity provider | Rauthy's image (`ghcr.io/sebadob/rauthy`, pinned), run by the chart's `rauthy` mode and the development stack, never linked | No crate; one container of about 95 MB |
| Completing Rauthy's web client | `curlimages/curl` (pinned), the chart's init container and job and the development stack's `rauthy-init` | No crate; one container of about 11 MB |

Each addition is measured with `cargo tree`, the dependency budget and the
binary-size budget when it lands ([Development §2.8](development.md#28-guards)).
The WASM frontend gains no dependency: it calls `/auth/session` and
navigates. A guard keeps `jsonwebtoken`, `redis`, `lettre` and
`async-graphql` out of the desktop's dependency graph.

---

## 12. Tests

- **Unit.** Token validation against keys generated in the test: each claim
  failing alone, `alg: none`, an HMAC algorithm with the public key as
  secret, an unknown `kid` and the refetch limit, leeway bounds, an ID token
  presented as an access token. `return_to` validation. Cookie attributes.
  Claim mapping and the signup gate.
- **Integration with a mock provider.** An Axum router in the test harness
  serving discovery, JWKS, authorization, token, end-session, introspection,
  registration and token-exchange endpoints with test keys — no external
  service. Over a cookie-keeping client: sign-in, a replayed or unbound
  callback, CSRF refusals, refresh with rotation under concurrent requests,
  refresh refused, provider unreachable, logout and back-channel logout,
  deactivation, erasure. Every REST and GraphQL operation answers the same
  `401`, `403` or `404` for the same viewer, generated from the parity
  table ([Authorization §12](authorization.md#12-tests)). Logs of every
  scenario hold no token, email or subject.
- **End-to-end.** A Playwright project (`e2e/playwright.auth.config.ts`,
  `e2e/auth/`) against the Compose stack with its Rauthy, signing in with
  the fictitious users of §8.5 on Rauthy's own page: sign-in and the cookie
  no script can read, `return_to` kept within the application, a replayed
  callback, the CSRF refusals and RP-initiated sign-out, a session the
  provider ends (back-channel logout), sign-out everywhere, the bearer
  tokens refused, and the instance role each user's groups give; the
  journeys of the tree roles join it with them. It needs external services,
  so it is opt-in (`just e2e-auth`, which starts the stack) and never part
  of `just check`; every test skips itself while the server answers
  `/auth/session` without `mode: "oidc"`.
- **Chart.** `just chart-test` (`scripts/chart-tests.py`): `helm lint` with
  each values file of `charts/oxidgene/ci/`, and rendering tests for every
  `auth.idp.mode`, the required values, the Redis and replica rule, the
  Ingress paths, the Rauthy workload on Hiqlite and on CloudNativePG, the
  job completing the web client, and the setup the chart bootstraps against
  the development one.

---

## 13. Open decisions

1. **A job whose actor's session is revoked.** Recommended: the job continues,
   authority being the actor's membership re-checked before each commit, and
   fails only on deactivation, erasure or a lost permission (§5.5).
   Alternative: cancel every job of a session that ends, which makes signing
   out on one device stop an import started on another.
2. **Erasure of a history author.** Recommended: a pseudonymous tombstone,
   keeping "same author" readable. Alternative: clear the author of every
   entry, which loses that and gains nothing a tombstone with no identity
   keeps.
3. **The chart's default mode.** Recommended: `external`, failing to render
   until a provider is named. Alternative: `none`, today's behaviour,
   relying on the Ingress guard alone.
4. **Rauthy's storage.** Recommended, and the chart's default: Hiqlite,
   Rauthy's embedded SQLite replicated by Raft across its pods, each on a
   volume of its own — Rauthy's own default, with no second PostgreSQL to
   run, back up and upgrade beside OxidGene's, and highly available with
   three pods. Alternative: its own CloudNativePG `Cluster`
   (`database.mode=cloudnativepg`), for an operator who wants PostgreSQL's
   backups and point-in-time recovery for the identities too.
5. **The web client's authentication.** Recommended: `client_secret_basic`
   by default, `private_key_jwt` available. Alternative: `private_key_jwt`
   only, which removes a shared secret from the provider's database at the
   cost of a key to manage.
