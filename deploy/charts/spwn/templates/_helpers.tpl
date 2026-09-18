{{/*
Standard name helpers. `fullname` collapses `<release>-<chart>` to `<release>` when the
release name already contains the chart name, so `helm install spwn` yields the bare
names `spwn` and `spwn-home` -- the same names the kustomize deploy this chart replaces
used, which is what lets an existing volume be adopted instead of orphaned.
*/}}
{{- define "spwn.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "spwn.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "spwn.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "spwn.labels" -}}
helm.sh/chart: {{ include "spwn.chart" . }}
{{ include "spwn.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "spwn.selectorLabels" -}}
app.kubernetes.io/name: {{ include "spwn.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "spwn.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "spwn.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/*
The home claim. `<fullname>-home` -- i.e. `spwn-home` for the documented install.
Changing this strands every existing install's Claude login and clones.
*/}}
{{- define "spwn.claimName" -}}
{{- .Values.persistence.existingClaim | default (printf "%s-home" (include "spwn.fullname" .)) -}}
{{- end -}}

{{/*
Guards for the mistakes that actually bite. The messages are the documentation: a user
who reaches for one of these gets the reason, not a shrug.
*/}}
{{- define "spwn.guards" -}}
{{- if hasKey .Values "replicaCount" -}}
{{- fail "spwn runs exactly one replica: one rmux daemon, one ReadWriteOnce home volume. `replicaCount` is not supported -- a second pod would corrupt the volume, not scale anything." -}}
{{- end -}}
{{- if and .Values.ingress.enabled (not .Values.ingress.host) -}}
{{- fail "ingress.enabled is true but ingress.host is empty: set the hostname spwn should answer on." -}}
{{- end -}}
{{- if and .Values.persistence.existingClaim (not .Values.persistence.enabled) -}}
{{- fail "persistence.existingClaim is set but persistence.enabled is false: the claim would be ignored and spwn would run on an emptyDir, losing its Claude login on every restart." -}}
{{- end -}}
{{- end -}}
