{{- define "discord-radio.name" -}}
{{- .Chart.Name -}}
{{- end -}}

{{- define "discord-radio.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "discord-radio.selectorLabels" -}}
app.kubernetes.io/name: {{ include "discord-radio.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "discord-radio.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "discord-radio.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "discord-radio.image" -}}
{{- $repo := required "image.repository is required" .Values.image.repository -}}
{{- if .Values.image.digest -}}
{{- if .Values.image.tag -}}
{{- printf "%s:%s@%s" $repo (toString .Values.image.tag) .Values.image.digest -}}
{{- else -}}
{{- printf "%s@%s" $repo .Values.image.digest -}}
{{- end -}}
{{- else -}}
{{- printf "%s:%s" $repo (required "image.tag or image.digest is required" .Values.image.tag | toString) -}}
{{- end -}}
{{- end -}}

{{- define "discord-radio.secretEnv" -}}
- name: {{ .env }}
  valueFrom:
    secretKeyRef:
      name: {{ required "secret.name is required" .root.Values.secret.name }}
      key: {{ .key }}
      optional: {{ .optional }}
{{- end -}}
