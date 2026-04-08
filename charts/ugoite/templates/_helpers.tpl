{{- define "ugoite.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ugoite.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ugoite.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := include "ugoite.name" . -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "ugoite.labels" -}}
helm.sh/chart: {{ include "ugoite.chart" . }}
app.kubernetes.io/name: {{ include "ugoite.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "ugoite.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ugoite.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "ugoite.backendFullname" -}}
{{- printf "%s-backend" (include "ugoite.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ugoite.frontendFullname" -}}
{{- printf "%s-frontend" (include "ugoite.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ugoite.backendServiceName" -}}
{{- include "ugoite.backendFullname" . -}}
{{- end -}}

{{- define "ugoite.frontendServiceName" -}}
{{- include "ugoite.frontendFullname" . -}}
{{- end -}}

{{- define "ugoite.authSecretName" -}}
{{- printf "%s-auth" (include "ugoite.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ugoite.backendPvcName" -}}
{{- if .Values.backend.persistence.existingClaim -}}
{{- .Values.backend.persistence.existingClaim -}}
{{- else -}}
{{- printf "%s-data" (include "ugoite.backendFullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "ugoite.authBearerSecrets" -}}
{{- if .Values.auth.bearerSecrets -}}
{{- .Values.auth.bearerSecrets -}}
{{- else -}}
{{- printf "%s:%s" .Values.auth.signingKid (required "charts/ugoite values.auth.signingSecret must be set to a unique secret" .Values.auth.signingSecret) -}}
{{- end -}}
{{- end -}}

{{- define "ugoite.authBearerActiveKids" -}}
{{- if .Values.auth.bearerActiveKids -}}
{{- join "," .Values.auth.bearerActiveKids -}}
{{- else -}}
{{- .Values.auth.signingKid -}}
{{- end -}}
{{- end -}}

{{- define "ugoite.frontendBackendUrl" -}}
{{- if .Values.frontend.backendUrl -}}
{{- .Values.frontend.backendUrl -}}
{{- else -}}
{{- printf "http://%s:%v" (include "ugoite.backendServiceName" .) .Values.backend.service.port -}}
{{- end -}}
{{- end -}}
