{{- define "oxidgene.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "oxidgene.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.labels" -}}
helm.sh/chart: {{ include "oxidgene.chart" . }}
app.kubernetes.io/name: {{ include "oxidgene.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "oxidgene.selectorLabels" -}}
app.kubernetes.io/name: {{ include "oxidgene.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "oxidgene.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "oxidgene.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{- define "oxidgene.cloudnativepgClusterName" -}}
{{- default (printf "%s-postgresql" (include "oxidgene.fullname" .)) .Values.database.cloudnativepg.clusterName | trunc 59 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.databaseSecretName" -}}
{{- if eq .Values.database.mode "cloudnativepg" -}}
{{- printf "%s-app" (include "oxidgene.cloudnativepgClusterName" .) -}}
{{- else -}}
{{- .Values.database.existing.secret -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.databaseUrlKey" -}}
{{- if eq .Values.database.mode "cloudnativepg" -}}uri{{- else -}}{{ .Values.database.existing.urlKey }}{{- end -}}
{{- end }}

{{- define "oxidgene.redisInstanceName" -}}
{{- default (printf "%s-redis" (include "oxidgene.fullname" .)) .Values.redis.operator.instanceName | trunc 54 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.redisSecretName" -}}
{{- if eq .Values.redis.mode "operator" -}}
{{- .Values.redis.operator.credentialsSecret -}}
{{- else -}}
{{- .Values.redis.existing.secret -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.redisUrlKey" -}}
{{- if eq .Values.redis.mode "operator" -}}{{ .Values.redis.operator.urlKey }}{{- else -}}{{ .Values.redis.existing.urlKey }}{{- end -}}
{{- end }}

{{- define "oxidgene.rustfsTenantName" -}}
{{- default (printf "%s-rustfs" (include "oxidgene.fullname" .)) .Values.s3.rustfs.tenantName | trunc 55 | trimSuffix "-" }}
{{- end }}

{{- define "oxidgene.s3Endpoint" -}}
{{- if eq .Values.s3.mode "rustfs" -}}
{{- printf "http://%s-io.%s.svc.%s:9000" (include "oxidgene.rustfsTenantName" .) .Release.Namespace .Values.clusterDomain -}}
{{- else -}}
{{- .Values.s3.existing.endpoint -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.s3Bucket" -}}
{{- if eq .Values.s3.mode "rustfs" -}}
{{- .Values.s3.rustfs.bucket -}}
{{- else -}}
{{- .Values.s3.existing.bucket -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.s3Region" -}}
{{- if eq .Values.s3.mode "rustfs" -}}
{{- .Values.s3.rustfs.region -}}
{{- else -}}
{{- .Values.s3.existing.region -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.s3CredentialsSecret" -}}
{{- if eq .Values.s3.mode "rustfs" -}}
{{- .Values.s3.rustfs.applicationCredentialsSecret -}}
{{- else -}}
{{- .Values.s3.existing.credentialsSecret -}}
{{- end -}}
{{- end }}

{{- define "oxidgene.s3AccessKeyKey" -}}
{{- if eq .Values.s3.mode "rustfs" -}}accesskey{{- else -}}{{ .Values.s3.existing.accessKeyKey }}{{- end -}}
{{- end }}

{{- define "oxidgene.s3SecretKeyKey" -}}
{{- if eq .Values.s3.mode "rustfs" -}}secretkey{{- else -}}{{ .Values.s3.existing.secretKeyKey }}{{- end -}}
{{- end }}