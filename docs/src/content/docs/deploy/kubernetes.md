---
title: Kubernetes
description: Install spwn on a cluster with Helm, and open it from a browser.
---

spwn runs as a single pod with a home volume of its own: its own Claude login, its own
clones, its own shells. Nothing from your laptop is mounted.

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn --create-namespace
kubectl -n spwn port-forward svc/spwn 4317:80
```

Open <http://localhost:4317>. spwn's [setup screen](/spwn/getting-started/installation/#first-run)
handles the rest in the browser — sign in to Claude, add a GitHub token if you want
private repos, clone your first repository. There is nothing to `kubectl exec` into and
nothing to put on the volume by hand.

:::danger
**spwn has no authentication.** Anyone who can reach it can run commands in the pod, read
and change everything on its volume, and use its Claude login. Keep it on a network you
trust, or put authentication in front of it — an auth proxy, or your ingress controller's
basic auth or forward auth.
:::

## With an Ingress

```sh
helm install spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn --create-namespace \
  --set ingress.enabled=true \
  --set ingress.host=spwn.example.com \
  --set ingress.className=nginx
```

Every terminal streams over a WebSocket at `/ws`. Most controllers pass upgrades through
untouched; nginx closes idle upgrades after 60 seconds unless you raise its read timeout:

```sh
  --set ingress.annotations."nginx\.ingress\.kubernetes\.io/proxy-read-timeout"=3600
```

## Storage

The chart provisions one ReadWriteOnce claim from the cluster's default StorageClass and
mounts it at `/home/spwn`. It references nothing that has to exist beforehand: no host
paths, no pre-made PersistentVolume, no Secret of its own. Delete the volume, install
again, and setup starts over from scratch.

```sh
  --set persistence.size=100Gi --set persistence.storageClass=fast-ssd
```

What a fresh volume actually costs is worth knowing before you treat it as disposable:
session branches you never pushed live in the clone's `.git`, checkpoints back the
Timeline's rewind, and the transcripts the Timeline reads are under `~/.claude`. The
claim is kept on `helm uninstall` for that reason.

## Signing in without the browser

An unattended pod can be handed a key from a Secret **you** manage; spwn then reports
itself signed in and skips that setup step. The chart never creates or holds a Secret.

```sh
kubectl -n spwn create secret generic spwn-env --from-literal=ANTHROPIC_API_KEY=sk-ant-…
helm upgrade spwn oci://ghcr.io/spwn-gg/charts/spwn -n spwn \
  --set 'envFrom[0].secretRef.name=spwn-env'
```

## Giving each session its own pod

A repo's `session-created` [hook](/spwn/reference/hooks/) can create a container and
report it back as an `exec` prefix, so that session's agent and shells run *there*
instead of in the spwn pod. Creating one needs the Kubernetes API, which spwn's
ServiceAccount cannot reach by default — a default install can do nothing at all to the
cluster.

```sh
  --set sessionPods.enabled=true
```

That grants pods, `pods/exec` and `pods/log` in spwn's own namespace and nothing else.
Note what it is worth: `pods/exec` on a pod mounting spwn's home volume is equivalent to
running code in the spwn pod, which spwn already does — so it widens nothing, but weigh
it before adding spwn to a namespace holding anything else.

## Limits

- **One replica.** Panes live in an rmux daemon inside the pod and the volume is
  ReadWriteOnce. The chart rejects `replicaCount` rather than letting you corrupt the
  volume.
- **Panes don't survive a pod restart.** Agent sessions can be resumed from their
  transcripts; running shells and processes are gone.
- **The pod is the sandbox.** Agents run with its permissions and network access.

Full reference, including path-prefix ingress, a Traefik recipe and migrating from the
old kustomize deploy:
[`deploy/charts/spwn`](https://github.com/spwn-gg/spwn/tree/main/deploy/charts/spwn).
