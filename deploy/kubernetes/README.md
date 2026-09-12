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

## First run

Log the pod's `claude` in once. The login is stored on the volume and survives
restarts:

```sh
kubectl -n spwn exec -it deploy/spwn -- claude
```

Then open spwn, start a shell pane, and clone what you want to work on (for example
into `~/code`). For private repos, add an SSH key or a credential helper under
`~/.ssh` / `~/.gitconfig` on the volume.

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
