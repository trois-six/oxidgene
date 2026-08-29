---
type: "Quickstart Guide"
title: "OxidGene Quickstart"
description: "Requirements and procedures for running OxidGene as a downloaded desktop application, a source build, a Compose stack, or a Kubernetes deployment."
tags: [oxidgene, quickstart, desktop, docker, kubernetes, helm]
timestamp: 2026-08-29T00:00:00Z
---

# OxidGene Quickstart

> Part of the [OxidGene Specifications](index.md).
> See also: [Development](development.md) · [Architecture](architecture.md)

OxidGene can run as a self-contained desktop application or as a web
application backed by PostgreSQL and S3-compatible object storage. Choose one
path; the desktop paths do not require the web infrastructure.

| Path | Intended use | Durable storage |
|---|---|---|
| Downloaded desktop binary | End users who do not build from source | Embedded SQLite and the local application data directory |
| Desktop source build | Contributors and local desktop testing | Embedded SQLite and the local application data directory |
| Docker Compose | Local web evaluation and integration testing | PostgreSQL, RustFS, and Redis Docker volumes |
| Kubernetes with Helm | Cluster deployment of the web application | External PostgreSQL and either external S3 or a RustFS Tenant |

The hardware figures below are operational starting points, not benchmarked
hard limits. Large trees, high-resolution media, concurrent imports, and PDF
processing require additional memory, CPU, and storage.

## 1. Download a desktop binary

GitHub Releases are the normal end-user installation path. Version tags are
configured to publish native Linux, macOS, and Windows archives automatically.
No OxidGene release is published yet, so until the first version tag use the
source-build path.

### Software requirements

- A supported 64-bit Linux, Windows, or macOS system.
- A graphical desktop session.
- A system WebView: WebKitGTK on Linux, WebView2 on Windows, or WebKit on macOS.
- No PostgreSQL, S3 service, Redis service, or container runtime.

### Hardware requirements

- 64-bit processor and 4 GiB RAM as a baseline.
- 500 MiB free for the application, plus space for the SQLite database and all
  imported media.
- An SSD and 8 GiB RAM are recommended for large trees or media libraries.

### Installation

When releases are available:

1. Open the [OxidGene releases](https://github.com/trois-six/oxidgene/releases)
   page.
2. Download the artifact matching the operating system and CPU architecture.
3. Verify the published checksum before running the artifact.
4. Follow that release's platform-specific installation notes.

Linux and macOS x86-64 downloads are `.tar.gz` archives; the Windows x86-64
download is a `.zip` archive. Every release includes `SHA256SUMS`. These are
currently unsigned portable executables rather than platform installers. Do
not download binaries presented as official OxidGene releases from another
location.

## 2. Build the desktop application

This path builds the self-contained Axum, SQLite, and Dioxus desktop binary.

### Software requirements

- Git and the stable [Rust toolchain](https://rustup.rs/).
- A native C/C++ build toolchain and `pkg-config`.
- Linux: GTK 3, WebKitGTK 4.1, JavaScriptCoreGTK 4.1, Soup 3, Xdo, OpenSSL,
  CMake, and Zstandard development packages. On Debian Trixie:

  ```bash
  sudo apt-get install build-essential cmake libgtk-3-dev \
    libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev \
    libxdo-dev libssl-dev pkg-config zstd
  ```

- Windows: the MSVC Rust toolchain, Visual Studio Build Tools with the C++
  workload, and the WebView2 runtime.
- macOS: Xcode Command Line Tools and the system WebKit framework.
- [just](https://github.com/casey/just) is recommended; the equivalent Cargo
  command is shown below.

### Hardware requirements

- 64-bit processor with 4 cores recommended.
- 8 GiB RAM and 10 GiB free disk space for dependencies and build artifacts.
- Additional local disk space for the SQLite database and imported media.

### Build and run

```bash
git clone https://github.com/trois-six/oxidgene.git
cd oxidgene
just build-desktop-release
./target/release/oxidgene-desktop
```

Without `just`:

```bash
cargo build --locked --release --package oxidgene-desktop
```

On Windows, run `target\release\oxidgene-desktop.exe`. Platform application
bundles and installers are future release deliverables; this command currently
produces the native executable.

## 3. Run the web stack with Docker Compose

The Compose stack is intended for local development and integration testing.
It starts PostgreSQL, RustFS, a one-shot bucket initializer, Redis, and the
OxidGene API and browser frontend. Redis is provisioned for future authenticated
sessions but is not used before authentication is implemented.

### Software requirements

- Git.
- Docker Engine with the Compose v2 plugin.

### Hardware requirements

- 4 CPU cores and 8 GiB RAM recommended for builds and all services.
- At least 20 GiB free Docker storage, plus capacity for PostgreSQL data and
  imported media in the RustFS volume.
- Host ports `5432`, `6379`, `8080`, `9000`, and `9001` available on loopback.
  The development browser server additionally uses `8081`.

### Start and stop

```bash
git clone https://github.com/trois-six/oxidgene.git
cd oxidgene
docker compose -f docker/docker-compose.yml up -d --build --wait --remove-orphans
docker compose -f docker/docker-compose.yml ps
```

Open `http://127.0.0.1:8081` for the application. The API is available at
`http://127.0.0.1:8080`, RustFS S3 at
`http://127.0.0.1:9000`, and the RustFS console at
`http://127.0.0.1:9001`. The `rustfs-init` service creates the
`oxidgene-media` bucket idempotently.
Stop the containers without deleting their volumes:

```bash
docker compose -f docker/docker-compose.yml down
```

Add `--volumes` only when the PostgreSQL, RustFS, and Redis development data
should be permanently deleted.

The Compose credentials are intentionally local-only. Every published port is
bound to `127.0.0.1`; do not expose this stack to an untrusted network.

## 4. Deploy to Kubernetes with Helm

The chart deploys the static frontend and Axum backend as PVC-free workloads.
PostgreSQL and object storage hold all durable web data, while import scratch
files use ephemeral `/tmp` volumes. The chart supports:

- `s3.mode=existing`: connect to an existing S3-compatible bucket.
- `s3.mode=rustfs`: create a RustFS Tenant, media policy, application user, and
  bucket through RustFS Operator 0.0.6 or newer.

The chart does not deploy PostgreSQL or Redis. Redis will become a required
external session service when authentication is implemented.

### Software requirements

- Kubernetes 1.30 or newer.
- Helm 3 and `kubectl` configured for the target cluster.
- An Ingress controller when `ingress.enabled=true`.
- A PostgreSQL database reachable from the cluster.
- Published or locally accessible OxidGene backend and frontend OCI images.
- An existing S3 bucket, or RustFS Operator 0.0.6+ and a dynamic StorageClass.

The backend is not ready for direct untrusted public exposure until
authentication and per-tree authorization are implemented. Use a private
cluster or a trusted access gateway.

### Hardware and storage requirements

With the default values, OxidGene requests two backend pods at `100m` CPU and
`128Mi` RAM each, plus two frontend pods at `25m` CPU and `32Mi` RAM each. This
is `250m` CPU and `320Mi` RAM in total before Kubernetes, Ingress, PostgreSQL,
monitoring, and object-storage overhead. The configured limits total 2.25 GiB
RAM and 2.5 CPU cores.

For `s3.mode=existing`, size cluster nodes for the OxidGene pods and keep the
database and object-store capacity outside this chart. For `s3.mode=rustfs`,
the default Tenant additionally creates four RustFS servers with one `100Gi`
ReadWriteOnce PVC each. Provide at least four suitable 100 GiB volumes, size
`s3.rustfs.pool.resources` for the expected workload, and distribute replicas
across failure domains in production.

### Obtain the OCI images

Each version tag publishes `linux/amd64` and `linux/arm64` images to
`ghcr.io/trois-six/oxidgene-server` and
`ghcr.io/trois-six/oxidgene-web`. Until the first tagged release, or when using
a development revision, build and push both images to a registry the cluster
can pull from:

```bash
docker build -f docker/Dockerfile.server \
  -t registry.example.invalid/oxidgene-server:0.1.0 .
docker build -f docker/Dockerfile.web \
  -t registry.example.invalid/oxidgene-web:0.1.0 .
docker push registry.example.invalid/oxidgene-server:0.1.0
docker push registry.example.invalid/oxidgene-web:0.1.0
```

The OxidGene runtime images use Debian Trixie. RustFS 1.0.0-beta.10 has no
official Trixie variant, so the chart uses its official Alpine image.

### Common database Secret

Create the release namespace and a Secret containing the PostgreSQL URL:

```bash
kubectl create namespace oxidgene
kubectl -n oxidgene create secret generic oxidgene-database \
  --from-literal=url='postgres://USER:PASSWORD@HOST:5432/oxidgene'
```

Manage real credentials with the cluster's secret manager rather than
committing them in a values file.

### Existing S3 bucket

Create an application credential Secret:

```bash
kubectl -n oxidgene create secret generic oxidgene-s3 \
  --from-literal=accesskey='S3_ACCESS_KEY' \
  --from-literal=secretkey='S3_SECRET_KEY'
```

Create `values-production.yaml`:

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

The bucket must already exist. Its S3 identity needs `ListBucket`, `GetObject`,
`PutObject`, and `DeleteObject` permissions for the bucket and its objects.

### Operator-managed RustFS

RustFS Operator is cluster-scoped and deliberately remains independent from
the namespaced OxidGene release. Its CRDs and controller have a separate
upgrade lifecycle. Version 0.0.6 is required for the chart's declarative
policy, user, and bucket provisioning. The official Helm repository does not
yet publish 0.0.6, so install the chart from the matching source tag:

```bash
git clone --branch 0.0.6 --depth 1 \
  https://github.com/rustfs/operator.git rustfs-operator-0.0.6
helm upgrade --install rustfs-operator \
  rustfs-operator-0.0.6/deploy/rustfs-operator \
  --namespace rustfs-system \
  --create-namespace
```

The operator enables STS TLS but does not generate its certificate by default.
For production, create `rustfs-system/sts-tls` with `tls.crt`, `tls.key`, and
`ca.crt` before installation. On an isolated development cluster only, append
`--set sts.tls.auto=true` to let the operator generate it.

Helm does not upgrade existing CRDs from a chart's `crds/` directory. Before
upgrading an existing operator, apply the new CRDs first:

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

Create distinct administrator, application, and internode RPC Secrets. RustFS
requires both credential values to contain at least eight UTF-8 bytes:

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

Select RustFS in `values-production.yaml`:

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

### Install and verify OxidGene

Install only after the database and chosen S3 backend are reachable, and after
the `Tenant` CRD exists when using RustFS mode. For a tagged release, install
the published OCI chart (replace `0.1.0` with the release version):

```bash
helm upgrade --install oxidgene \
  oci://ghcr.io/trois-six/charts/oxidgene \
  --version 0.1.0 \
  --namespace oxidgene \
  --create-namespace \
  -f values-production.yaml
```

For a source checkout, install the local chart:

```bash
helm upgrade --install oxidgene charts/oxidgene \
  --namespace oxidgene \
  --create-namespace \
  -f values-production.yaml
kubectl -n oxidgene get deploy,pod,service,ingress
```

For RustFS mode, also wait for provisioning:

```bash
kubectl -n oxidgene wait --for=condition=Ready \
  tenant/oxidgene-rustfs --timeout=10m
```

The ingress sends `/api`, `/graphql`, and `/healthz` to the backend and all
other paths to the frontend.

### Helm values reference

#### General and ServiceAccount

| Value | Default | Description |
|---|---|---|
| `nameOverride` | `""` | Override the chart application name. |
| `fullnameOverride` | `""` | Override every chart-generated base resource name. |
| `clusterDomain` | `cluster.local` | Kubernetes DNS suffix used for the RustFS Service endpoint. |
| `imagePullSecrets` | `[]` | Pod-level image pull Secret references. |
| `serviceAccount.create` | `true` | Create the shared frontend/backend ServiceAccount. |
| `serviceAccount.name` | `""` | Existing or generated ServiceAccount name. |
| `serviceAccount.annotations` | `{}` | Annotations added to a created ServiceAccount. |

#### Backend

| Value | Default | Description |
|---|---|---|
| `backend.replicaCount` | `2` | Deployment replicas when its HPA is disabled. |
| `backend.image.repository` | `ghcr.io/trois-six/oxidgene-server` | Backend image repository. |
| `backend.image.tag` | Chart `appVersion` | Backend image tag; set an explicit value to override the released chart version. |
| `backend.image.pullPolicy` | `IfNotPresent` | Kubernetes image pull policy. |
| `backend.service.type` | `ClusterIP` | Backend Service type. |
| `backend.service.port` | `8080` | Backend Service port. |
| `backend.corsOrigin` | `https://oxidgene.example.invalid` | Allowed browser origin. Set it to the public application origin. |
| `backend.logLevel` | `info` | `OXIDGENE_LOG_LEVEL` value. |
| `backend.extraEnv` | `[]` | Additional container environment entries. |
| `backend.resources` | See `values.yaml` | CPU and memory requests and limits. |
| `backend.podAnnotations` | `{}` | Additional Pod annotations. |
| `backend.podLabels` | `{}` | Additional Pod labels. |
| `backend.podSecurityContext` | Restricted defaults | Pod-level UID, GID, FSGroup, and seccomp settings. |
| `backend.securityContext` | Restricted defaults | Container privilege, capability, and read-only-root settings. |
| `backend.nodeSelector` | `{}` | Node selection constraints. |
| `backend.tolerations` | `[]` | Pod tolerations. |
| `backend.affinity` | `{}` | Pod affinity and anti-affinity. |
| `backend.topologySpreadConstraints` | `[]` | Pod topology spread rules. |

#### Frontend

| Value | Default | Description |
|---|---|---|
| `frontend.replicaCount` | `2` | Deployment replicas when its HPA is disabled. |
| `frontend.image.repository` | `ghcr.io/trois-six/oxidgene-web` | Static frontend image repository. |
| `frontend.image.tag` | Chart `appVersion` | Frontend image tag; set an explicit value to override the released chart version. |
| `frontend.image.pullPolicy` | `IfNotPresent` | Kubernetes image pull policy. |
| `frontend.service.type` | `ClusterIP` | Frontend Service type. |
| `frontend.service.port` | `80` | Frontend Service port. |
| `frontend.resources` | See `values.yaml` | CPU and memory requests and limits. |
| `frontend.podAnnotations` | `{}` | Additional Pod annotations. |
| `frontend.podLabels` | `{}` | Additional Pod labels. |
| `frontend.podSecurityContext` | Restricted defaults | Pod-level UID, GID, FSGroup, and seccomp settings. |
| `frontend.securityContext` | Restricted defaults | Container privilege, capability, and read-only-root settings. |
| `frontend.nodeSelector` | `{}` | Node selection constraints. |
| `frontend.tolerations` | `[]` | Pod tolerations. |
| `frontend.affinity` | `{}` | Pod affinity and anti-affinity. |
| `frontend.topologySpreadConstraints` | `[]` | Pod topology spread rules. |

#### Database and S3

| Value | Default | Description |
|---|---|---|
| `database.existingSecret` | `oxidgene-database` | Secret containing the PostgreSQL connection URL. |
| `database.urlKey` | `url` | Key holding the URL in the database Secret. |
| `s3.mode` | `existing` | Storage mode: `existing` or `rustfs`. |
| `s3.existing.endpoint` | Example URL | Existing S3-compatible endpoint; HTTPS is expected outside trusted local networks. |
| `s3.existing.bucket` | `oxidgene-media` | Existing bucket name. |
| `s3.existing.region` | `us-east-1` | Existing bucket region. |
| `s3.existing.credentialsSecret` | `oxidgene-s3` | Secret containing the application S3 credentials. |
| `s3.existing.accessKeyKey` | `accesskey` | Access-key field in the S3 Secret. |
| `s3.existing.secretKeyKey` | `secretkey` | Secret-key field in the S3 Secret. |

#### RustFS Tenant

| Value | Default | Description |
|---|---|---|
| `s3.rustfs.requireCrd` | `true` | Fail rendering when Helm discovery does not report the `Tenant` API. |
| `s3.rustfs.tenantName` | Generated | Optional explicit Tenant name. |
| `s3.rustfs.image` | `rustfs/rustfs:1.0.0-beta.10` | RustFS server image. |
| `s3.rustfs.adminCredentialsSecret` | `oxidgene-rustfs-admin` | Tenant administrator Secret with `accesskey` and `secretkey`. |
| `s3.rustfs.applicationCredentialsSecret` | `oxidgene-s3` | Provisioned application-user Secret with `accesskey` and `secretkey`; also consumed by the backend. |
| `s3.rustfs.rpcSecret` | `oxidgene-rustfs-rpc` | Dedicated internode RPC Secret. |
| `s3.rustfs.rpcSecretKey` | `rpc-secret` | RPC value key within that Secret. |
| `s3.rustfs.bucket` | `oxidgene-media` | Bucket created by the operator. |
| `s3.rustfs.region` | `us-east-1` | Bucket region and backend S3 region. |
| `s3.rustfs.policyName` | `oxidgene-media-rw` | Provisioned least-privilege policy name. |
| `s3.rustfs.userName` | `oxidgene` | Declarative RustFS application-user name. |
| `s3.rustfs.pool.name` | `storage` | RustFS pool name. |
| `s3.rustfs.pool.servers` | `4` | Stateful RustFS server count. |
| `s3.rustfs.pool.volumesPerServer` | `1` | PVC count per server. |
| `s3.rustfs.pool.storageClassName` | `""` | StorageClass; empty selects the cluster default. |
| `s3.rustfs.pool.size` | `100Gi` | Requested size of each PVC. |
| `s3.rustfs.pool.resources` | `{}` | RustFS container CPU and memory requests and limits. |
| `s3.rustfs.pool.nodeSelector` | `{}` | RustFS node selection constraints. |
| `s3.rustfs.pool.tolerations` | `[]` | RustFS Pod tolerations. |
| `s3.rustfs.pool.affinity` | `{}` | RustFS Pod affinity and anti-affinity. |
| `s3.rustfs.pool.topologySpreadConstraints` | `[]` | RustFS topology spread rules. |

#### Ingress, autoscaling, and availability

| Value | Default | Description |
|---|---|---|
| `ingress.enabled` | `false` | Create the application Ingress. |
| `ingress.className` | `""` | Ingress class name. |
| `ingress.annotations` | `{}` | Controller and certificate annotations. |
| `ingress.host` | `oxidgene.example.invalid` | Public application hostname. |
| `ingress.tls` | `[]` | Standard Ingress TLS entries with `secretName` and `hosts`. |
| `autoscaling.backend.enabled` | `false` | Create the backend HPA. |
| `autoscaling.backend.minReplicas` | `2` | Backend HPA minimum replicas. |
| `autoscaling.backend.maxReplicas` | `10` | Backend HPA maximum replicas. |
| `autoscaling.backend.targetCPUUtilizationPercentage` | `80` | Backend HPA CPU target. |
| `autoscaling.frontend.enabled` | `false` | Create the frontend HPA. |
| `autoscaling.frontend.minReplicas` | `2` | Frontend HPA minimum replicas. |
| `autoscaling.frontend.maxReplicas` | `10` | Frontend HPA maximum replicas. |
| `autoscaling.frontend.targetCPUUtilizationPercentage` | `80` | Frontend HPA CPU target. |
| `podDisruptionBudget.backend.enabled` | `true` | Create the backend PDB. |
| `podDisruptionBudget.backend.minAvailable` | `1` | Minimum available backend pods. |
| `podDisruptionBudget.frontend.enabled` | `true` | Create the frontend PDB. |
| `podDisruptionBudget.frontend.minAvailable` | `1` | Minimum available frontend pods. |