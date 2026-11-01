{{/* OxidGene's own mail (docs/messaging.md §3): whether this release mails
at all — an SMTP host and authentication on. */}}
{{- define "oxidgene.mailEnabled" -}}
{{- if and (include "oxidgene.authEnabled" .) .Values.mail.smtp.host -}}
true
{{- end -}}
{{- end }}

{{/* Fail rendering on mail values that cannot work. */}}
{{- define "oxidgene.mailValidate" -}}
{{- if include "oxidgene.mailEnabled" . -}}
{{- if not .Values.mail.from -}}
{{- fail "mail.from is required with mail.smtp.host: the sender address, such as \"OxidGene <no-reply@genealogy.example.invalid>\"" -}}
{{- end -}}
{{- if not (has .Values.mail.smtp.tls (list "starttls" "implicit" "none" "danger-insecure")) -}}
{{- fail "mail.smtp.tls must be starttls, implicit, none or danger-insecure" -}}
{{- end -}}
{{- end -}}
{{- end }}

{{/* The relay's environment, for the backend, which queues the mail, and
the worker, which sends it (docs/messaging.md §3.2). Nothing without a host
or in the `none` mode. */}}
{{- define "oxidgene.mailEnv" -}}
{{- if include "oxidgene.mailEnabled" . -}}
{{- $smtp := .Values.mail.smtp -}}
- name: OXIDGENE_SMTP_HOST
  value: {{ $smtp.host | quote }}
- name: OXIDGENE_SMTP_PORT
  value: {{ $smtp.port | quote }}
- name: OXIDGENE_SMTP_TLS
  value: {{ $smtp.tls | quote }}
- name: OXIDGENE_MAIL_FROM
  value: {{ .Values.mail.from | quote }}
{{- with $smtp.credentialsSecret.secret }}
- name: OXIDGENE_SMTP_USERNAME
  valueFrom:
    secretKeyRef:
      name: {{ . | quote }}
      key: {{ required "mail.smtp.credentialsSecret.usernameKey is required" $smtp.credentialsSecret.usernameKey | quote }}
- name: OXIDGENE_SMTP_PASSWORD
  valueFrom:
    secretKeyRef:
      name: {{ . | quote }}
      key: {{ required "mail.smtp.credentialsSecret.passwordKey is required" $smtp.credentialsSecret.passwordKey | quote }}
{{- end }}
{{- end -}}
{{- end }}

{{/* What the worker needs besides the relay to send: the public URL its
links start with and the session keys its unsubscribe tokens are signed
with. None of the sign-in settings. */}}
{{- define "oxidgene.workerMailEnv" -}}
{{- if include "oxidgene.mailEnabled" . -}}
{{- $auth := .Values.auth -}}
- name: OXIDGENE_AUTH_MODE
  value: oidc
- name: OXIDGENE_PUBLIC_URL
  value: {{ $auth.publicUrl | quote }}
- name: OXIDGENE_SESSION_KEYS
  valueFrom:
    secretKeyRef:
      name: {{ required "auth.session.keysSecret.secret is required" $auth.session.keysSecret.secret | quote }}
      key: {{ required "auth.session.keysSecret.key is required" $auth.session.keysSecret.key | quote }}
{{ include "oxidgene.mailEnv" . }}
{{- end -}}
{{- end }}
