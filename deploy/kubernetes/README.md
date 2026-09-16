# spwn on Kubernetes

Runs `spwn serve` as a long-lived pod you open from a browser. The pod has its own
home volume: its own Claude login, its own clones of your repos, its own shells.
Nothing from your laptop is mounted.

> **spwn has no authentication.** Anyone who can reach it can run commands in the pod,
> read and change everything on its volume, and use its Claude login. Keep it on a
> network you trust, or put authentication in front of it (an auth proxy or your
> ingress controller's basic auth / forward auth).

## Layout

| Path | What it is |
|------|------------|
| `base/` | Deployment (1 replica, `Recreate`), `spwn-home` PersistentVolumeClaim, ClusterIP Service. No namespace, no ingress. |
| `examples/ingress/` | Base + `spwn` namespace + a standard `Ingress` serving spwn at the root of its own hostname. |
| `examples/traefik-path-prefix/` | Base + `spwn` namespace + a Traefik `IngressRoute` serving spwn under `/spwn` on a shared host. |
| `components/session-pods/` | Opt-in RBAC letting spwn's hooks give each session a container of its own. See below. |
| `../Dockerfile` | The image: spwn with its UI embedded, a pinned `rmux`, `claude`, `git`, `bash`. |

## Deploy

Pick an example, change the host or prefix, and apply it:

```sh
kubectl apply -k deploy/kubernetes/examples/ingress
```

Or use the base from your own overlay, pinning the image and resizing the volume:

```yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: tools
resources:
  - https://github.com/spwn-gg/spwn//deploy/kubernetes/base?ref=main
images:
  - name: ghcr.io/spwn-gg/spwn
    newTag: sha-abc1234
patches:
  - target: {kind: PersistentVolumeClaim, name: spwn-home}
    patch: |
      - op: replace
        path: /spec/resources/requests/storage
        value: 100Gi
```

The image is published to `ghcr.io/spwn-gg/spwn` from `main` (`:latest` and
`:sha-<commit>`) and from `v*` tags. It is built for `linux/amd64`. For another
architecture or a private registry, build it yourself:

```sh
docker build -f deploy/Dockerfile -t registry.example.com/spwn:dev .
```

## Giving each session its own container

A repo's `session-created` hook can create a container and report it back as an `exec`
prefix, so that session's agent and shells run *there* instead of in the spwn pod. In a
cluster that container is a pod, and creating one needs the Kubernetes API — which the
spwn pod cannot reach by default. That is deliberate: `base/` gives spwn a
ServiceAccount with **no permissions at all**, so a default install can do nothing to the
cluster.

Opt in from your overlay:

```yaml
components:
  - ../../components/session-pods
```

It grants pods, `pods/exec` and `pods/log` in **spwn's own namespace** and nothing else.
`examples/traefik-path-prefix/` has it enabled; `examples/ingress/` does not.

Note what that is worth: `pods/exec` on a pod mounting spwn's home volume is equivalent to
running code in the spwn pod. spwn already does exactly that — it opens shells and runs
agents there — so this widens nothing, but weigh it before adding spwn to a namespace that
holds anything else.

A hook writing one of these pods has three constraints, all from the home volume:

- **Mount it at `/home/spwn`, so the worktree keeps the same absolute path.** spwn locates
  a session's transcript by a slug of its working directory, and a worktree's `.git` holds
  an absolute pointer into the main repo. A different path breaks the Timeline, rewind and
  git with no error.
- **Pin the pod to spwn's node.** The claim is usually ReadWriteOnce; a pod scheduled
  elsewhere never binds it. (Two pods on one node sharing a ReadWriteOnce claim is fine.)
- **Keep it in spwn's namespace.** A PersistentVolumeClaim cannot be mounted from another.

The pod can read all three off spwn's own pod — `$HOSTNAME` is the pod name, and the
namespace is in the mounted ServiceAccount directory. A worked example, hooks and image
included, is the `threshold` repo's `.spwn/hooks/`.

## First run

Log the pod's `claude` in once. The login is stored on the volume and survives
restarts:

```sh
kubectl -n spwn exec -it deploy/spwn -- claude
```

Then open spwn, start a shell pane, and clone what you want to work on (for example
into `~/code`). For private GitHub repos, paste a personal access token in
**Settings → GitHub**: spwn's own clone/fetch/pull/push and git in its panes use it
over HTTPS, and it is kept on the volume. Other hosts, or SSH remotes, still need a
key or credential helper under `~/.ssh` / `~/.gitconfig` on the volume.

## Serving under a path prefix

spwn's server always routes at `/`. To serve it at `https://host/some/prefix/`, have
your proxy strip the prefix before forwarding. The UI needs no build flag or setting
for this: it loads assets relative to the page and derives its `/api` and `/ws` URLs
from where it was served, and a request for the prefix without its trailing slash is
redirected to add one.

## Limits

- **One replica.** Panes live in an rmux daemon inside the pod, and the home volume
  is `ReadWriteOnce`.
- **Panes don't survive a pod restart.** The rmux daemon goes with the pod. Agent
  sessions can be resumed from their transcripts; running shells and processes are
  gone.
- **The pod is the sandbox.** Agents run with the pod's permissions and network
  access. Tighten it with a NetworkPolicy and resource limits that suit your cluster.
