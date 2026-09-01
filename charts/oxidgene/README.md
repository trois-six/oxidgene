# OxidGene Helm Chart

The canonical installation guide and complete values reference are in the
[Kubernetes section of the OxidGene Quickstart](../../docs/specifications/quickstart.md#4-deploy-to-kubernetes-with-helm).

This chart deploys the OxidGene web frontend and backend. For durable stateless
deployments, it can consume an existing PostgreSQL database or create one
through CloudNativePG, and use an existing S3 bucket or operator-managed RustFS.
For evaluation, disabling those integrations selects ephemeral SQLite and
filesystem storage in a single backend pod. Redis can remain disabled, use an
existing service, or be provisioned through OpsTree Redis Operator. It remains
reserved for user sessions once authentication is implemented.

## Prerequisites

- Kubernetes 1.30 or newer and Helm 3.
- For durable database storage, an existing PostgreSQL database or a dynamic
  StorageClass for a CloudNativePG cluster.
- When Redis is enabled, an existing Redis service or a dynamic StorageClass
  for an operator-managed instance.
- For durable media storage, either an existing S3-compatible bucket or RustFS
  Operator 0.0.6 and a StorageClass able to provision the RustFS Tenant PVCs.
- Published or locally accessible OxidGene backend and frontend images.

All referenced Secrets must be in the OxidGene release namespace. Manage them
with the cluster's secret manager rather than committing credential values.

## Ephemeral SQLite and filesystem mode

For evaluation without PostgreSQL or S3, disable both integrations:

```yaml
database:
  mode: disabled

s3:
  mode: disabled
```

The backend then uses `/data/oxidgene.db` for SQLite and `/media` for uploaded
media, each backed by an `emptyDir`. The chart forces one backend replica and
rejects backend autoscaling whenever either local backend is selected. All
database and media data is lost when that pod is deleted, rescheduled, or
recreated; these modes are not suitable for production or a stateless service.

## Existing PostgreSQL database

Create the database credential Secret:

```bash
kubectl create namespace oxidgene
kubectl -n oxidgene create secret generic oxidgene-database \
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/oxidgene'
```

Select it in the values file:

```yaml
database:
  mode: existing
  existing:
    secret: oxidgene-database
    urlKey: url
```

## Operator-managed PostgreSQL

Set `database.mode=cloudnativepg` to create a `Cluster`, its PostgreSQL
instances, PVCs, services, and application credentials. To install the official
CloudNativePG 1.30.0 operator as part of this Helm release, enable its subchart:

```yaml
database:
  mode: cloudnativepg
  cloudnativepg:
    instances: 3
    imageName: ghcr.io/cloudnative-pg/postgresql:18.6-system-trixie
    storage:
      storageClass: fast-rwo
      size: 100Gi

cloudnative-pg:
  enabled: true
```

The backend automatically consumes the generated `<cluster>-app` Secret and
its `uri` key. For an operator already managed by the cluster administrator,
leave `cloudnative-pg.enabled=false`; the chart verifies that the `Cluster` CRD
exists. Only one release should own the cluster-wide operator and its CRDs.
Production clusters should normally manage that operator independently so its
upgrade and removal lifecycle is not tied to one OxidGene release.

When installing from a source checkout, fetch the optional chart dependencies
before running Helm:

```bash
helm dependency build charts/oxidgene
```

## Redis for future sessions

Redis is disabled by default because authentication and user sessions are not
implemented yet. To prepare an external Redis service, create a Secret with its
connection URL and select existing mode:

```bash
kubectl -n oxidgene create secret generic oxidgene-redis \
  --from-literal=url='redis://:PASSWORD@REDIS_HOST:6379/0'
```

```yaml
redis:
  mode: existing
  existing:
    secret: oxidgene-redis
    urlKey: url
```

For a managed instance, create a URL-safe password and store both the password
required by Redis Operator and the complete URL reserved for the backend. The
default Redis Service name is `oxidgene-redis`:

```bash
REDIS_PASSWORD="$(openssl rand -hex 32)"
kubectl -n oxidgene create secret generic oxidgene-redis \
  --from-literal=password="$REDIS_PASSWORD" \
  --from-literal=url="redis://:${REDIS_PASSWORD}@oxidgene-redis:6379/0"
unset REDIS_PASSWORD
```

```yaml
redis:
  mode: operator
  operator:
    storage:
      storageClass: fast-rwo
      size: 20Gi

redis-operator:
  enabled: true
```

This creates a password-protected, persistent Redis 8.2.1 instance and injects
`OXIDGENE_REDIS_URL` into the backend. That variable is reserved for EPIC G and
does not activate sessions before authentication is implemented. If OpsTree
Redis Operator 0.26.0 is already managed by the cluster administrator, leave
`redis-operator.enabled=false`; the chart requires its `Redis` CRD. As with
CloudNativePG, production clusters should normally manage the cluster-wide
operator separately.

## Existing S3 bucket

Create the S3 credential Secret:

```bash
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
  otlpEndpoint: https://telemetry.example.invalid

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
| `database.mode` | `disabled`, `existing`, or `cloudnativepg`. |
| `database.sqlite.*` | SQLite directory and file name used by the ephemeral disabled mode. |
| `database.existing.*` | Existing Secret name and PostgreSQL URL key. |
| `database.cloudnativepg.*` | Managed cluster name, image, instances, database owner, storage, resources, and PostgreSQL parameters. |
| `cloudnative-pg.enabled` | Install the official cluster-wide operator dependency; disabled by default. |
| `redis.mode` | `disabled`, `existing`, or `operator`. |
| `redis.existing.*` | Existing Secret name and Redis URL key. |
| `redis.operator.*` | Managed instance name, image, credentials, storage, resources, scheduling, and configuration. |
| `redis-operator.enabled` | Install the cluster-wide OpsTree operator dependency; disabled by default. |
| `s3.mode` | `disabled`, `existing`, or `rustfs`. |
| `s3.filesystem.root` | Media directory used by the ephemeral disabled mode. |
| `s3.existing.*` | Existing endpoint, bucket, region, and credential Secret. |
| `s3.rustfs.*` | Tenant image, Secrets, bucket, policy, user, and pool. |
| `ingress.*` | Optional same-origin routing and TLS configuration. |
| `frontend.otlpEndpoint` | Public OTLP/HTTP base URL injected into the browser runtime; empty disables browser trace export. |
| `autoscaling.*` | Optional backend and frontend HPAs. |
| `podDisruptionBudget.*` | Backend and frontend disruption budgets. |

The ingress sends `/api`, `/graphql`, and `/healthz` to the backend and `/` to
the frontend. Keep the backend private until authentication and per-tree
authorization are implemented.