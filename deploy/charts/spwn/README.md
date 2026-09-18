# spwn on Kubernetes

Runs `spwn serve` as a long-lived pod you open from a browser. The pod has its own home
volume: its own Claude login, its own clones of your repos, its own shells. Nothing from
your laptop is mounted.

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn --create-namespace
kubectl -n spwn port-forward svc/spwn 4317:80
```

Open <http://localhost:4317> and spwn walks you through the rest: sign in to Claude in
the terminal it gives you, add a GitHub token if you want private repos, clone your first
repository. **There is nothing to `kubectl exec` into and nothing to put on the volume by
hand.** Delete the volume, install again, and you get the same working spwn.

> **spwn has no authentication.** Anyone who can reach it can run commands in the pod,
> read and change everything on its volume, and use its Claude login. Keep it on a
> network you trust, or put authentication in front of it (an auth proxy, or your ingress
> controller's basic auth / forward auth).

## What the chart creates

A ServiceAccount with no permissions, a PersistentVolumeClaim, a single-replica
Deployment and a ClusterIP Service. Storage is provisioned from the cluster's default
StorageClass — the chart references nothing that has to exist beforehand: no hostPath, no
pre-made PersistentVolume, no Secret of its own.

With an Ingress:

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn --create-namespace \
  --set ingress.enabled=true \
  --set ingress.host=spwn.example.com \
  --set ingress.className=nginx
```

Every value is documented in [`values.yaml`](values.yaml). The image is published to
`ghcr.io/spwn-gg/spwn` from `main` (`:latest` and `:sha-<commit>`) and from `v*` tags, for
`linux/amd64`. For another architecture or a private registry, build it yourself:

```sh
docker build -f deploy/Dockerfile -t registry.example.com/spwn:dev .
```

## The home volume

One claim, mounted at `/home/spwn`, holding everything: the repos you clone, the Claude
login (`~/.claude`, `~/.claude.json`), agent and hook definitions (`~/.spwn`), and the
project store (`~/.local/share/com.markbarta.spwn` — settings, projects, the GitHub token
at mode 0600, checkpoints, workflow state).

Treat it as a cache you would rather not lose, not as something precious you must back
up separately — but know what a fresh volume actually costs:

- **Unmerged `spwn/<id>` session branches live in the clone's `.git`.** A fresh volume
  re-clones from the remote, so session work you never pushed or merged is gone.
- **Checkpoints** back the Timeline's rewind and undo.
- **Claude transcripts** under `~/.claude/projects` are what the Timeline reads.
- **Context blocks and scheduled tasks** live on the project record.

That is why `persistence.retain` defaults to `true`: `helm uninstall` leaves the claim
behind. Self-contained means reinstallable, not disposable.

Check your StorageClass's reclaim policy too — with `Delete`, removing the claim removes
the workspace and the login with it.

### Uninstall and reinstall

Because the claim is kept, it is no longer owned by a release, and a plain reinstall
fails with *"PersistentVolumeClaim spwn-home exists and cannot be imported into the
current release"*. Adopt it explicitly:

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn \
  --set persistence.existingClaim=spwn-home
```

## Giving each session its own container

A repo's `session-created` hook can create a container and report it back as an `exec`
prefix, so that session's agent and shells run *there* instead of in the spwn pod. In a
cluster that container is a pod, and creating one needs the Kubernetes API — which the
spwn pod cannot reach by default. That is deliberate: the chart gives spwn a
ServiceAccount with **no permissions at all**, so a default install can do nothing to the
cluster.

```sh
helm upgrade spwn … --set sessionPods.enabled=true
```

It grants pods, `pods/exec` and `pods/log` in **spwn's own namespace** and nothing else.

Note what that is worth: `pods/exec` on a pod mounting spwn's home volume is equivalent
to running code in the spwn pod. spwn already does exactly that — it opens shells and
runs agents there — so this widens nothing, but weigh it before adding spwn to a
namespace that holds anything else.

A hook writing one of these pods has three constraints, all from the home volume:

- **Mount it at `/home/spwn`, so the worktree keeps the same absolute path.** spwn
  locates a session's transcript by a slug of its working directory, and a worktree's
  `.git` holds an absolute pointer into the main repo. A different path breaks the
  Timeline, rewind and git with no error.
- **Pin the pod to spwn's node.** The claim is usually ReadWriteOnce; a pod scheduled
  elsewhere never binds it. (Two pods on one node sharing a ReadWriteOnce claim is fine.)
- **Keep it in spwn's namespace.** A PersistentVolumeClaim cannot be mounted from another.

The pod can read all three off spwn's own pod — `$HOSTNAME` is the pod name, and the
namespace is in the mounted ServiceAccount directory.

## Signing in without the browser

The first-run screen is the documented path and needs nothing from the chart. An
unattended pod can instead be handed a key from a Secret **you** manage; spwn then
reports itself signed in and skips that step. The chart never creates or holds a Secret.

```sh
kubectl -n spwn create secret generic spwn-env --from-literal=ANTHROPIC_API_KEY=sk-ant-…
helm upgrade spwn … --set 'envFrom[0].secretRef.name=spwn-env'
```

Editing the Secret does not restart the pod — `kubectl -n spwn rollout restart deploy/spwn`.

## Serving under a path prefix

spwn's server always routes at `/`. To serve it at `https://host/some/prefix/`, have your
proxy strip the prefix before forwarding. The UI needs no build flag or setting for this:
it loads assets relative to the page and derives its `/api` and `/ws` URLs from where it
was served.

**nginx** — the capture group is what makes the rewrite work, and `pathType` must be
`ImplementationSpecific` for a regex path:

```yaml
ingress:
  enabled: true
  className: nginx
  host: tools.example.com
  path: /spwn(/|$)(.*)
  pathType: ImplementationSpecific
  annotations:
    nginx.ingress.kubernetes.io/rewrite-target: /$2
    # Terminals stream over /ws; nginx closes idle upgrades at 60s without this.
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
```

**Traefik** — the chart emits no CRDs, so the `Middleware` and `IngressRoute` go through
`extraObjects`. Note the match: a bare ``PathPrefix(`/spwn`)`` is a string prefix and
would also catch `/spwn-anything`.

```yaml
ingress:
  enabled: false # an IngressRoute replaces the Ingress entirely
extraObjects:
  - apiVersion: traefik.io/v1alpha1
    kind: Middleware
    metadata:
      name: strip-spwn-prefix
    spec:
      stripPrefix:
        prefixes: ["/spwn"]
  - apiVersion: traefik.io/v1alpha1
    kind: IngressRoute
    metadata:
      name: spwn
    spec:
      entryPoints: [web]
      routes:
        - match: Path(`/spwn`) || PathPrefix(`/spwn/`)
          kind: Rule
          middlewares:
            - name: strip-spwn-prefix
          services:
            - name: '{{ include "spwn.fullname" . }}'
              port: 80
```

`extraObjects` is the chart's general escape hatch, and the reason it has no
`networkPolicy` or `podDisruptionBudget` values of its own. (Don't reach for a PDB here:
`minAvailable: 1` on a single-replica workload permanently blocks node drains.)

## Migrating from the kustomize deploy

Earlier versions shipped a kustomize base under `deploy/kubernetes/`. The chart renders
the **same object names** — `spwn` and `spwn-home` — when the release is named `spwn`, so
`helm install spwn -n <your namespace>` adopts your existing volume rather than orphaning
it. Helm 3.17+ needs `--take-ownership`; older versions need the objects annotated with
`meta.helm.sh/release-name` and labelled `app.kubernetes.io/managed-by=Helm` first.

Two things to know before you run it. Pass `--set persistence.size=` the size you
actually have — a claim can grow but never shrink, and the chart's 20Gi default over a
larger claim fails on an immutable field. And delete the Deployment first: its pod
selector is immutable and the chart's adds `app.kubernetes.io/instance`. That is safe —
every byte of state is on the volume, and panes never survive a restart anyway.

## Limits

- **One replica.** Panes live in an rmux daemon inside the pod, and the home volume is
  `ReadWriteOnce`. The chart rejects `replicaCount` rather than letting you corrupt the
  volume.
- **Panes don't survive a pod restart.** The rmux daemon goes with the pod. Agent
  sessions can be resumed from their transcripts; running shells and processes are gone.
- **The pod is the sandbox.** Agents run with the pod's permissions and network access.
  Tighten it with a NetworkPolicy (via `extraObjects`) and resource limits that suit your
  cluster.

## Developing the chart

```sh
helm lint deploy/charts/spwn --strict
helm template spwn deploy/charts/spwn -n spwn -f deploy/charts/spwn/ci/full-values.yaml
helm template spwn deploy/charts/spwn -n spwn | kubectl apply --dry-run=server -f -
```

`ci/*-values.yaml` exist to force every template branch; CI renders each of them and
validates the output against the Kubernetes API schemas.
