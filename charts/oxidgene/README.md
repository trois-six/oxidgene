# OxidGene Helm Chart

The canonical installation guide and complete values reference are in the
[Kubernetes section of the OxidGene Quickstart](../../docs/specifications/quickstart.md#4-deploy-to-kubernetes-with-helm).

This chart deploys the stateless OxidGene web frontend and backend. PostgreSQL
and S3-compatible object storage hold durable data; the OxidGene pods only use
an ephemeral `/tmp` volume. The chart does not deploy PostgreSQL or Redis.
Redis is reserved for user sessions once authentication is implemented.

## Prerequisites

- Kubernetes 1.30 or newer and Helm 3.
- A PostgreSQL database reachable from the cluster.
- Either an existing S3-compatible bucket or RustFS Operator 0.0.6 and a
  StorageClass able to provision the RustFS Tenant PVCs.
- Published or locally accessible OxidGene backend and frontend images.

All referenced Secrets must be in the OxidGene release namespace. Manage them
with the cluster's secret manager rather than committing credential values.

## Existing S3 bucket

Create the database and S3 credential Secrets:

```bash
kubectl create namespace oxidgene
kubectl -n oxidgene create secret generic oxidgene-database \
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/oxidgene'
kubectl -n oxidgene create secret generic oxidgene-s3 \
  --from-literal=accesskey='S3_ACCESS_KEY' \
  --from-literal=secretkey='S3_SECRET_KEY'
```

Create a values file containing non-secret configuration:

```yaml
backend:
  image:
    repository: registry.example.invalid/oxidgene-server
    tag: "0.1.0"
  corsOrigin: https://genealogy.example.invalid

frontend:
  image:
    repository: registry.example.invalid/oxidgene-web
    tag: "0.1.0"

database:
  existingSecret: oxidgene-database

s3:
  mode: existing
  existing:
    endpoint: https://s3.example.invalid
    bucket: oxidgene-media
    region: us-east-1
    credentialsSecret: oxidgene-s3

ingress:
  enabled: true
  className: nginx
  host: genealogy.example.invalid
  tls:
    - secretName: oxidgene-tls
      hosts:
        - genealogy.example.invalid
```

Install the release:

```bash
helm upgrade --install oxidgene \
  oci://ghcr.io/trois-six/charts/oxidgene \
  --version 0.1.0 \
  --namespace oxidgene \
  --create-namespace \
  -f values-production.yaml
```

For a source checkout, replace the OCI reference and `--version` with the
local `charts/oxidgene` path. If an image tag is omitted, the chart uses its
`appVersion`, which matches the images published with the same release.

The existing bucket must already exist. The configured S3 identity needs
`ListBucket`, `GetObject`, `PutObject`, and `DeleteObject` permissions on that
bucket and its objects.

## Operator-managed RustFS

RustFS Operator is cluster-scoped and has its own CRDs and upgrade lifecycle,
so it is deliberately not a dependency of this application chart. Version
0.0.6 is required for declarative policy, user, and bucket provisioning. The
official Helm repository does not yet publish 0.0.6; install the chart from the
matching Git tag:

```bash
git clone --branch 0.0.6 --depth 1 https://github.com/rustfs/operator.git rustfs-operator-0.0.6
helm upgrade --install rustfs-operator \
  rustfs-operator-0.0.6/deploy/rustfs-operator \
  --namespace rustfs-system \
  --create-namespace
```

The operator enables STS TLS but does not generate its certificate by default.
For production, create the `sts-tls` Secret in `rustfs-system` with `tls.crt`,
`tls.key`, and `ca.crt` before installation. For an isolated development
cluster only, append `--set sts.tls.auto=true` to let the operator generate it.

When upgrading an existing operator, apply both cluster-scoped CRDs from the
new checkout before upgrading Helm, because Helm does not upgrade files from a
chart's `crds/` directory:

```bash
kubectl apply --server-side --force-conflicts \
  --field-manager=rustfs-operator-crd-upgrade \
  -f rustfs-operator-0.0.6/deploy/rustfs-operator/crds/tenant-crd.yaml
kubectl apply --server-side --force-conflicts \
  --field-manager=rustfs-operator-crd-upgrade \
  -f rustfs-operator-0.0.6/deploy/rustfs-operator/crds/policybinding-crd.yaml
helm upgrade rustfs-operator \
  rustfs-operator-0.0.6/deploy/rustfs-operator \
  --namespace rustfs-system
```

Create the database Secret as above, then create three RustFS Secrets. Both
credential keys must be at least eight UTF-8 bytes. Use distinct administrator,
application, and internode RPC values:

```bash
kubectl -n oxidgene create secret generic oxidgene-rustfs-admin \
  --from-literal=accesskey='ADMIN_ACCESS_KEY' \
  --from-literal=secretkey='ADMIN_SECRET_KEY'
kubectl -n oxidgene create secret generic oxidgene-s3 \
  --from-literal=accesskey='APP_ACCESS_KEY' \
  --from-literal=secretkey='APP_SECRET_KEY'
kubectl -n oxidgene create secret generic oxidgene-rustfs-rpc \
  --from-literal=rpc-secret='DEDICATED_RPC_SECRET'
```

Select RustFS mode and size the durable pool for the cluster:

```yaml
s3:
  mode: rustfs
  rustfs:
    adminCredentialsSecret: oxidgene-rustfs-admin
    applicationCredentialsSecret: oxidgene-s3
    rpcSecret: oxidgene-rustfs-rpc
    bucket: oxidgene-media
    pool:
      servers: 4
      volumesPerServer: 1
      storageClassName: fast-rwo
      size: 100Gi
```

Install OxidGene only after the `Tenant` CRD is available. The chart creates a
RustFS `Tenant`, least-privilege media policy, application user, and bucket.
RustFS owns PVCs; the OxidGene backend and frontend remain PVC-free.

```bash
helm upgrade --install oxidgene charts/oxidgene \
  --namespace oxidgene \
  --create-namespace \
  -f values-production.yaml
kubectl -n oxidgene wait --for=condition=Ready \
  tenant/oxidgene-rustfs --timeout=10m
```

The default RustFS image is `rustfs/rustfs:1.0.0-beta.10`. RustFS does not
publish a Debian Trixie variant for this release; the official image is Alpine.
The OxidGene backend and frontend runtime images use Debian Trixie variants.

## Important values

| Value | Purpose |
|---|---|
| `clusterDomain` | Kubernetes DNS suffix, default `cluster.local`. |
| `database.existingSecret` | Secret containing the PostgreSQL URL. |
| `database.urlKey` | PostgreSQL URL key, default `url`. |
| `s3.mode` | `existing` or `rustfs`. |
| `s3.existing.*` | Existing endpoint, bucket, region, and credential Secret. |
| `s3.rustfs.*` | Tenant image, Secrets, bucket, policy, user, and pool. |
| `ingress.*` | Optional same-origin routing and TLS configuration. |
| `autoscaling.*` | Optional backend and frontend HPAs. |
| `podDisruptionBudget.*` | Backend and frontend disruption budgets. |

The ingress sends `/api`, `/graphql`, and `/healthz` to the backend and `/` to
the frontend. Keep the backend private until authentication and per-tree
authorization are implemented.