{{/*
Authentication (docs/authentication.md §8). `oxidgene.authValidate` fails
rendering on a configuration the backend would refuse or a deployment should
not run; the backend and Rauthy templates include it.
*/}}
{{- define "oxidgene.authEnabled" -}}
{{- if ne .Values.auth.idp.mode "none" }}true{{ end -}}
{{- end }}

{{- define "oxidgene.rauthyName" -}}
{{- default (printf "%s-rauthy" (include "oxidgene.fullname" .)) .Values.auth.idp.rauthy.name | trunc 50 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.rauthyDatabaseClusterName" -}}
{{- default (printf "%s-db" (include "oxidgene.rauthyName" .)) .Values.auth.idp.rauthy.database.cloudnativepg.clusterName | trunc 59 | trimSuffix "-" }}
{{- end }}

{{/* Rauthy's issuer ends with a slash, which the comparison keeps. */}}
{{- define "oxidgene.oidcIssuer" -}}
{{- if eq .Values.auth.idp.mode "rauthy" -}}
{{- printf "https://%s/auth/v1/" .Values.auth.idp.rauthy.hostname -}}
{{- else -}}
{{- .Values.auth.oidc.issuer -}}
{{- end -}}
{{- end }}

{{/* The Rauthy Service, reached without the Ingress (docs/authentication.md §3.4). */}}
{{- define "oxidgene.oidcBackchannelUrl" -}}
{{- if and (eq .Values.auth.idp.mode "rauthy") (not .Values.auth.oidc.backchannelUrl) -}}
{{- printf "http://%s.%s.svc.%s:8080" (include "oxidgene.rauthyName" .) .Release.Namespace .Values.clusterDomain -}}
{{- else -}}
{{- .Values.auth.oidc.backchannelUrl -}}
{{- end -}}
{{- end }}

{{/* The roles claim and its values: Rauthy's `groups` with the groups it is
bootstrapped with, unless the values name another claim. */}}
{{- define "oxidgene.oidcRoles" -}}
{{- $oidc := .Values.auth.oidc -}}
{{- if and (eq .Values.auth.idp.mode "rauthy") (not $oidc.rolesClaim) -}}
{{- dict "claim" "groups" "admin" (list "oxidgene-admins") "creator" (list "oxidgene-creators") | toJson -}}
{{- else -}}
{{- dict "claim" $oidc.rolesClaim "admin" $oidc.adminValues "creator" $oidc.creatorValues | toJson -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.authValidate" -}}
{{- $auth := .Values.auth -}}
{{- if not (has $auth.idp.mode (list "external" "rauthy" "none")) -}}
{{- fail "auth.idp.mode must be external (any OpenID Connect provider, named by auth.oidc.issuer), rauthy (a Rauthy this release deploys), or none (no authentication, for a single trusted user)" -}}
{{- end -}}
{{- if include "oxidgene.authEnabled" . -}}
{{- if and (eq $auth.idp.mode "external") (not $auth.oidc.issuer) -}}
{{- fail "auth.idp.mode=external needs auth.oidc.issuer, the issuer of an OpenID Connect provider. Choose the provider explicitly: external with auth.oidc.issuer for one you run, rauthy for a Rauthy this release deploys, or none for no authentication (a single trusted user only)" -}}
{{- end -}}
{{- if not $auth.publicUrl -}}
{{- fail "auth.publicUrl is required unless auth.idp.mode=none: the application's external origin, normally https:// plus ingress.host" -}}
{{- end -}}
{{- if not (regexMatch "^https?://[^/?#]+$" $auth.publicUrl) -}}
{{- fail "auth.publicUrl must be an origin, such as https://genealogy.example.invalid, without a path or a trailing slash" -}}
{{- end -}}
{{- if not (has $auth.signup (list "invitation" "claim" "open")) -}}
{{- fail "auth.signup must be invitation, claim, or open" -}}
{{- end -}}
{{- if not (has $auth.treeCreation (list "everyone" "claim")) -}}
{{- fail "auth.treeCreation must be everyone or claim" -}}
{{- end -}}
{{- $_ := include "oxidgene.durationSeconds" $auth.session.idleTimeout -}}
{{- $_ = include "oxidgene.durationSeconds" $auth.session.absoluteLifetime -}}
{{- if eq .Values.redis.mode "disabled" -}}
{{- $usesLocalState := or (eq .Values.database.mode "disabled") (eq .Values.s3.mode "disabled") -}}
{{- if or .Values.autoscaling.backend.enabled (and (gt (int .Values.backend.replicaCount) 1) (not $usesLocalState)) -}}
{{- fail "with redis.mode=disabled the sessions live in the backend process, which allows one backend replica only: enable Redis (redis.mode=existing or operator), or set backend.replicaCount=1 and disable backend autoscaling" -}}
{{- end -}}
{{- end -}}
{{- if and .Values.ingress.enabled (ne (regexReplaceAll "^https?://" $auth.publicUrl "") .Values.ingress.host) -}}
{{- fail "auth.publicUrl must be the origin the Ingress serves, its scheme followed by ingress.host: the redirect URI and the session cookie are bound to it" -}}
{{- end -}}
{{- if eq $auth.idp.mode "rauthy" -}}
{{- $rauthy := $auth.idp.rauthy -}}
{{- if not $rauthy.hostname -}}
{{- fail "auth.idp.mode=rauthy needs auth.idp.rauthy.hostname, Rauthy's own host such as auth.genealogy.example.invalid" -}}
{{- end -}}
{{- if not (has $rauthy.database.mode (list "hiqlite" "cloudnativepg")) -}}
{{- fail "auth.idp.rauthy.database.mode must be hiqlite or cloudnativepg" -}}
{{- end -}}
{{- $replicas := int $rauthy.replicas -}}
{{- if or (lt $replicas 1) (and (gt $replicas 1) (eq (mod $replicas 2) 0)) -}}
{{- fail "auth.idp.rauthy.replicas must be 1, or an odd number for its Raft cluster to keep a majority" -}}
{{- end -}}
{{- if not $rauthy.secret -}}
{{- fail "auth.idp.rauthy.secret is required: the existing Secret holding Rauthy's encryption keys, Hiqlite secrets and API key" -}}
{{- end -}}
{{- if and $rauthy.smtp.host (not $rauthy.smtp.from) -}}
{{- fail "auth.idp.rauthy.smtp.from is required with an SMTP host" -}}
{{- end -}}
{{- if not (has $rauthy.smtp.tlsMode (list "tls" "starttls")) -}}
{{- fail "auth.idp.rauthy.smtp.tlsMode must be tls or starttls" -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end }}

{{/* The backend's authentication environment (docs/authentication.md §3.2). */}}
{{- define "oxidgene.authEnv" -}}
{{- $auth := .Values.auth -}}
{{- if not (include "oxidgene.authEnabled" .) -}}
- name: OXIDGENE_AUTH_MODE
  value: none
{{- else -}}
{{- $roles := include "oxidgene.oidcRoles" . | fromJson -}}
- name: OXIDGENE_AUTH_MODE
  value: oidc
- name: OXIDGENE_PUBLIC_URL
  value: {{ $auth.publicUrl | quote }}
- name: OXIDGENE_OIDC_ISSUER
  value: {{ include "oxidgene.oidcIssuer" . | quote }}
{{- with include "oxidgene.oidcBackchannelUrl" . }}
- name: OXIDGENE_OIDC_BACKCHANNEL_URL
  value: {{ . | quote }}
{{- end }}
- name: OXIDGENE_OIDC_CLIENT_ID
  value: {{ required "auth.oidc.clientId is required" $auth.oidc.clientId | quote }}
- name: OXIDGENE_OIDC_CLIENT_SECRET
  valueFrom:
    secretKeyRef:
      name: {{ required "auth.oidc.clientSecret.secret is required" $auth.oidc.clientSecret.secret | quote }}
      key: {{ required "auth.oidc.clientSecret.key is required" $auth.oidc.clientSecret.key | quote }}
{{- with $roles.claim }}
- name: OXIDGENE_OIDC_ROLES_CLAIM
  value: {{ . | quote }}
{{- end }}
{{- with $roles.admin }}
- name: OXIDGENE_OIDC_ADMIN_VALUES
  value: {{ join "," . | quote }}
{{- end }}
{{- with $roles.creator }}
- name: OXIDGENE_OIDC_CREATOR_VALUES
  value: {{ join "," . | quote }}
{{- end }}
{{- with $auth.oidc.allowedValues }}
- name: OXIDGENE_OIDC_ALLOWED_VALUES
  value: {{ join "," . | quote }}
{{- end }}
{{- if $auth.oidc.trustEmail }}
- name: OXIDGENE_OIDC_TRUST_EMAIL
  value: "true"
{{- end }}
{{- with $auth.oidc.trustedClients }}
- name: OXIDGENE_OIDC_TRUSTED_CLIENTS
  value: {{ join "," . | quote }}
{{- end }}
{{- $accountUrl := $auth.oidc.accountUrl }}
{{- if and (not $accountUrl) (eq $auth.idp.mode "rauthy") }}
{{- $accountUrl = printf "%saccount" (include "oxidgene.oidcIssuer" .) }}
{{- end }}
{{- with $accountUrl }}
- name: OXIDGENE_OIDC_ACCOUNT_URL
  value: {{ . | quote }}
{{- end }}
- name: OXIDGENE_AUTH_SIGNUP
  value: {{ $auth.signup | quote }}
- name: OXIDGENE_AUTH_TREE_CREATION
  value: {{ $auth.treeCreation | quote }}
{{- with $auth.bootstrapAdminEmail }}
- name: OXIDGENE_AUTH_BOOTSTRAP_ADMIN_EMAIL
  value: {{ . | quote }}
{{- end }}
- name: OXIDGENE_SESSION_STORE
  value: {{ ternary "memory" "redis" (eq .Values.redis.mode "disabled") }}
- name: OXIDGENE_SESSION_KEYS
  valueFrom:
    secretKeyRef:
      name: {{ required "auth.session.keysSecret.secret is required" $auth.session.keysSecret.secret | quote }}
      key: {{ required "auth.session.keysSecret.key is required" $auth.session.keysSecret.key | quote }}
- name: OXIDGENE_SESSION_IDLE_TIMEOUT
  value: {{ $auth.session.idleTimeout | quote }}
- name: OXIDGENE_SESSION_ABSOLUTE_LIFETIME
  value: {{ $auth.session.absoluteLifetime | quote }}
{{- end -}}
{{- end }}

{{/* A session duration of the values (`45s`, `30m`, `2h`, `7d`) in seconds:
it fails rendering on any other form. */}}
{{- define "oxidgene.durationSeconds" -}}
{{- $units := dict "s" 1 "m" 60 "h" 3600 "d" 86400 -}}
{{- $amount := regexFind "^[0-9]+" (toString .) -}}
{{- $unit := trimPrefix $amount (toString .) -}}
{{- if or (not $amount) (not (hasKey $units $unit)) -}}
{{- fail (printf "auth.session durations are a number and one unit among s, m, h and d, such as 2h or 7d, not %q" (toString .)) -}}
{{- end -}}
{{- mul (atoi $amount) (get $units $unit) -}}
{{- end }}