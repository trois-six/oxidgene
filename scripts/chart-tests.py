# /// script
# requires-python = ">=3.10"
# dependencies = ["pyyaml==6.0.3"]
# ///
"""Lint and render the Helm chart, and check what it renders.

`just chart-test` runs this from the repository root after
`helm dependency build charts/oxidgene`. It covers the authentication modes
of docs/authentication.md §8 — `external`, `none` and `rauthy` — with the
values they require and refuse, the session store and the backend replica
rule, the Ingress paths, the Rauthy deployment and the setup it bootstraps;
the existing modes keep rendering as before.

- `helm lint` with each values file of charts/oxidgene/ci/.
- `helm template` with those values and variations of them, asserting on
  the parsed documents, and on the error of each configuration the chart
  must refuse.
- The Rauthy setup the chart ships (charts/oxidgene/files/rauthy/) against
  the development one (docker/rauthy/): the same groups, API scopes and
  clients, so that the development stack signs in as a deployment does. The
  development setup may differ only by its fictitious users, its secrets,
  its development API client and API key, and its back-channel logout URL
  (the server's Compose name).

It needs Helm and nothing else; a failure names the case and what differed.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import yaml

CHART = "charts/oxidgene"
NAMESPACE = "oxidgene"
RELEASE = "oxidgene"
CI = Path(CHART) / "ci"
CNPG = ["--api-versions", "postgresql.cnpg.io/v1/Cluster"]
BACKEND_PATHS = ["/api", "/graphql", "/auth", "/mcp", "/.well-known/oauth-protected-resource"]
RAUTHY = f"{RELEASE}-rauthy"
DEVELOPMENT = Path("docker/rauthy")

failures: list[str] = []


def helm(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["helm", *args], capture_output=True, text=True)


def values(name: str) -> list[str]:
    return ["-f", str(CI / f"{name}-values.yaml")]


def render(*args: str) -> list[dict]:
    result = helm("template", RELEASE, CHART, "--namespace", NAMESPACE, *args)
    if result.returncode != 0:
        raise AssertionError(f"rendering failed: {result.stderr.strip()}")
    return [doc for doc in yaml.safe_load_all(result.stdout) if doc]


def refused(*args: str) -> str:
    result = helm("template", RELEASE, CHART, "--namespace", NAMESPACE, *args)
    if result.returncode == 0:
        raise AssertionError("rendering succeeded where it must fail")
    return result.stderr


def find(docs: list[dict], kind: str, name: str | None = None) -> list[dict]:
    return [d for d in docs if d["kind"] == kind and (name is None or d["metadata"]["name"] == name)]


def env_of(container: dict) -> dict[str, object]:
    """A container's environment: a value, or (Secret, key)."""
    out: dict[str, object] = {}
    for var in container.get("env", []):
        if "value" in var:
            out[var["name"]] = var["value"]
        else:
            ref = var["valueFrom"]["secretKeyRef"]
            out[var["name"]] = (ref["name"], ref["key"])
    return out


def backend_env(docs: list[dict]) -> dict[str, object]:
    deployment = find(docs, "Deployment", f"{RELEASE}-backend")[0]
    return env_of(deployment["spec"]["template"]["spec"]["containers"][0])


def bootstrap(docs: list[dict]) -> dict[str, object]:
    """The parsed files of Rauthy's bootstrap ConfigMap."""
    data = find(docs, "ConfigMap", f"{RAUTHY}-bootstrap")[0]["data"]
    return {key: json.loads(value) for key, value in data.items()}


def check(name: str):
    def wrap(test):
        try:
            test()
        except AssertionError as error:
            failures.append(f"{name}: {error}")
            print(f"FAIL {name}: {error}")
        else:
            print(f"ok   {name}")
        return test
    return wrap


def expect(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def expect_eq(actual: object, expected: object, what: str) -> None:
    expect(actual == expected, f"{what}: expected {expected!r}, got {actual!r}")


def run() -> None:
    for values_file in sorted(CI.glob("*-values.yaml")):
        @check(f"helm lint with {values_file.name}")
        def _lint() -> None:
            result = helm("lint", CHART, "-f", str(values_file))
            expect(result.returncode == 0, result.stdout + result.stderr)

    @check("the default values name the three modes until a provider is chosen")
    def _default() -> None:
        error = refused()
        for word in ("auth.oidc.issuer", "external", "rauthy", "none"):
            expect(word in error, f"the error does not name {word}: {error.strip()}")

    @check("external: the backend signs in against the named provider")
    def _external() -> None:
        docs = render(*values("external"))
        env = backend_env(docs)
        expect_eq(env.get("OXIDGENE_AUTH_MODE"), "oidc", "OXIDGENE_AUTH_MODE")
        expect_eq(env.get("OXIDGENE_PUBLIC_URL"), "https://genealogy.example.invalid", "OXIDGENE_PUBLIC_URL")
        expect_eq(env.get("OXIDGENE_OIDC_ISSUER"), "https://idp.example.invalid/realms/genealogy", "OXIDGENE_OIDC_ISSUER")
        expect("OXIDGENE_OIDC_BACKCHANNEL_URL" not in env, "a back-channel URL nobody asked for")
        expect_eq(env.get("OXIDGENE_OIDC_CLIENT_ID"), "oxidgene-web", "OXIDGENE_OIDC_CLIENT_ID")
        expect_eq(env.get("OXIDGENE_OIDC_CLIENT_SECRET"), ("oxidgene-oidc", "client-secret"), "the client secret's Secret")
        expect_eq(env.get("OXIDGENE_SESSION_KEYS"), ("oxidgene-session", "keys"), "the session keys' Secret")
        expect_eq(env.get("OXIDGENE_SESSION_STORE"), "redis", "OXIDGENE_SESSION_STORE with Redis")
        expect_eq(env.get("OXIDGENE_REDIS_URL"), ("oxidgene-redis", "url"), "OXIDGENE_REDIS_URL")
        expect_eq(env.get("OXIDGENE_OIDC_ROLES_CLAIM"), "groups", "OXIDGENE_OIDC_ROLES_CLAIM")
        expect_eq(env.get("OXIDGENE_OIDC_ADMIN_VALUES"), "genealogy-admins", "OXIDGENE_OIDC_ADMIN_VALUES")
        expect_eq(env.get("OXIDGENE_AUTH_SIGNUP"), "invitation", "OXIDGENE_AUTH_SIGNUP")
        expect_eq(env.get("OXIDGENE_AUTH_TREE_CREATION"), "everyone", "OXIDGENE_AUTH_TREE_CREATION")
        expect_eq(env.get("OXIDGENE_AUTH_BOOTSTRAP_ADMIN_EMAIL"), "admin@example.invalid", "the bootstrap administrator")
        expect_eq(env.get("OXIDGENE_SESSION_IDLE_TIMEOUT"), "2h", "OXIDGENE_SESSION_IDLE_TIMEOUT")
        expect_eq(env.get("OXIDGENE_SESSION_ABSOLUTE_LIFETIME"), "7d", "OXIDGENE_SESSION_ABSOLUTE_LIFETIME")
        expect(not find(docs, "StatefulSet"), "external deploys a provider")
        deployment = find(docs, "Deployment", f"{RELEASE}-backend")[0]
        expect_eq(deployment["spec"]["replicas"], 2, "backend replicas with Redis")

    @check("every mode routes the backend paths through the Ingress")
    def _ingress() -> None:
        for mode in ("external", "rauthy", "none"):
            docs = render(*values(mode))
            paths = find(docs, "Ingress", RELEASE)[0]["spec"]["rules"][0]["http"]["paths"]
            routes = {p["path"]: (p["pathType"], p["backend"]["service"]["name"]) for p in paths}
            for path in BACKEND_PATHS:
                expect_eq(routes.get(path), ("Prefix", f"{RELEASE}-backend"), f"{mode}: the route of {path}")
            expect_eq(routes.get("/healthz"), ("Exact", f"{RELEASE}-backend"), f"{mode}: the route of /healthz")
            expect_eq(routes.get("/"), ("Prefix", f"{RELEASE}-frontend"), f"{mode}: the route of /")

    @check("external needs no acknowledgement of an unauthenticated API")
    def _external_ack() -> None:
        render(*values("external"), "--set", "ingress.exposeApiWithoutAuth=false")

    @check("none: no authentication, and an Ingress only once acknowledged")
    def _none() -> None:
        env = backend_env(render(*values("none")))
        expect_eq(env.get("OXIDGENE_AUTH_MODE"), "none", "OXIDGENE_AUTH_MODE")
        oidc = [k for k in env if k.startswith(("OXIDGENE_OIDC_", "OXIDGENE_SESSION_", "OXIDGENE_PUBLIC_URL"))]
        expect(not oidc, f"none mode sets {oidc}")
        error = refused(*values("none"), "--set", "ingress.exposeApiWithoutAuth=false")
        expect("exposeApiWithoutAuth" in error, error.strip())
        notes = helm("install", RELEASE, CHART, "--dry-run=client", *values("none")).stdout
        expect("readable and writable by whoever" in notes, "NOTES.txt does not warn that every tree is open")

    @check("sessions in memory allow one backend replica only")
    def _memory() -> None:
        base = [*values("external"), "--set", "redis.mode=disabled"]
        error = refused(*base)
        expect("one backend replica" in error, error.strip())
        error = refused(*base, "--set", "backend.replicaCount=1", "--set", "autoscaling.backend.enabled=true")
        expect("one backend replica" in error, error.strip())
        env = backend_env(render(*base, "--set", "backend.replicaCount=1"))
        expect_eq(env.get("OXIDGENE_SESSION_STORE"), "memory", "OXIDGENE_SESSION_STORE without Redis")
        expect("OXIDGENE_REDIS_URL" not in env, "a Redis URL without Redis")
        # Local storage already forces a single backend.
        render(*base, "--set", "database.mode=disabled")
        # Without authentication, nothing to share.
        render(*values("none"), "--set", "database.mode=existing", "--set", "s3.mode=existing",
               "--set", "backend.replicaCount=3")

    @check("the required values and their forms")
    def _required() -> None:
        cases = {
            "auth.publicUrl is required": ["--set", "auth.publicUrl="],
            "without a path": ["--set", "auth.publicUrl=https://genealogy.example.invalid/app"],
            "ingress.host": ["--set", "ingress.host=other.example.invalid"],
            "auth.signup must be": ["--set", "auth.signup=everybody"],
            "auth.treeCreation must be": ["--set", "auth.treeCreation=anyone"],
            "auth.session durations": ["--set", "auth.session.idleTimeout=2 hours"],
            "auth.idp.mode must be": ["--set", "auth.idp.mode=oauth"],
        }
        for expected, args in cases.items():
            error = refused(*values("external"), *args)
            expect(expected in error, f"{args}: {error.strip()}")
        rauthy_cases = {
            "auth.idp.rauthy.hostname": ["--set", "auth.idp.rauthy.hostname="],
            "an odd number": ["--set", "auth.idp.rauthy.replicas=2"],
            "database.mode must be": ["--set", "auth.idp.rauthy.database.mode=sqlite"],
            "smtp.from is required": ["--set", "auth.idp.rauthy.smtp.from="],
            "smtp.tlsMode must be": ["--set", "auth.idp.rauthy.smtp.tlsMode=none"],
            "postgresql.cnpg.io/v1": ["--set", "auth.idp.rauthy.database.mode=cloudnativepg"],
        }
        for expected, args in rauthy_cases.items():
            error = refused(*values("rauthy"), *args)
            expect(expected in error, f"{args}: {error.strip()}")

    @check("rauthy: the backend signs in against the Rauthy of the release")
    def _rauthy_backend() -> None:
        env = backend_env(render(*values("rauthy")))
        expect_eq(env.get("OXIDGENE_OIDC_ISSUER"), "https://auth.genealogy.example.invalid/auth/v1/", "the issuer")
        expect_eq(env.get("OXIDGENE_OIDC_BACKCHANNEL_URL"), f"http://{RAUTHY}.{NAMESPACE}.svc.cluster.local:8080",
                  "the back channel")
        expect_eq(env.get("OXIDGENE_OIDC_ROLES_CLAIM"), "groups", "Rauthy's roles claim")
        expect_eq(env.get("OXIDGENE_OIDC_ADMIN_VALUES"), "oxidgene-admins", "the administrators' group")
        expect_eq(env.get("OXIDGENE_OIDC_CREATOR_VALUES"), "oxidgene-creators", "the creators' group")
        expect_eq(env.get("OXIDGENE_OIDC_ACCOUNT_URL"), "https://auth.genealogy.example.invalid/auth/v1/account",
                  "the account page")
        expect_eq(env.get("OXIDGENE_SESSION_STORE"), "memory", "one backend without Redis")

    @check("rauthy: a Raft StatefulSet on Hiqlite, its Services, Ingress and budget")
    def _rauthy_workload() -> None:
        docs = render(*values("rauthy"))
        statefulset = find(docs, "StatefulSet", RAUTHY)[0]["spec"]
        expect_eq(statefulset["replicas"], 3, "Rauthy replicas")
        expect_eq(statefulset["serviceName"], f"{RAUTHY}-headless", "the governing Service")
        expect_eq(statefulset["podManagementPolicy"], "Parallel", "the pod management policy")
        claim = statefulset["volumeClaimTemplates"][0]
        expect_eq((claim["metadata"]["name"], claim["spec"]["resources"]["requests"]["storage"]), ("data", "1Gi"),
                  "the Hiqlite volume")
        pod = statefulset["template"]["spec"]
        rauthy = pod["containers"][0]
        env = env_of(rauthy)
        for name, key in {"ENC_KEYS": "enc-keys", "ENC_KEY_ACTIVE": "enc-key-active", "HQL_SECRET_RAFT": "raft-secret",
                          "HQL_SECRET_API": "api-secret", "BOOTSTRAP_ADMIN_PASSWORD_PLAIN": "admin-password"}.items():
            expect_eq(env.get(name), ("oxidgene-rauthy", key), name)
        expect_eq(env.get("SMTP_PASSWORD"), ("rauthy-smtp", "password"), "SMTP_PASSWORD")
        expect("PG_PASSWORD" not in env, "PostgreSQL credentials on Hiqlite")
        expect_eq(rauthy["readinessProbe"]["httpGet"], {"path": "/ready", "port": "raft-api"}, "the readiness probe")
        expect_eq(rauthy["livenessProbe"]["httpGet"], {"path": "/auth/v1/health", "port": "http"}, "the liveness probe")
        init = env_of(pod["initContainers"][0])
        expect_eq(init.get("OXIDGENE_WEB_CLIENT_SECRET"), ("oxidgene-oidc", "client-secret"), "the client secret's Secret")
        expect_eq(init.get("RAUTHY_API_KEY"), ("oxidgene-rauthy", "api-key"), "the API key's Secret")
        headless = find(docs, "Service", f"{RAUTHY}-headless")[0]["spec"]
        expect(headless["clusterIP"] == "None" and headless["publishNotReadyAddresses"], "the headless Service")
        config = find(docs, "ConfigMap", f"{RAUTHY}-config")[0]["data"]["config.toml"]
        for member in range(3):
            peer = f"{RAUTHY}-{member}.{RAUTHY}-headless.{NAMESPACE}.svc.cluster.local"
            expect(f'"{member + 1} {peer}:8100 {peer}:8200"' in config, f"Raft member {member + 1} in the configuration")
        for line in ('pub_url = "auth.genealogy.example.invalid"', "hiqlite = true", 'smtp_url = "smtp.example.invalid"',
                     'rp_origin = "https://auth.genealogy.example.invalid:443"', "proxy_mode = true"):
            expect(line in config, f"the configuration lacks {line}")
        expect_eq(find(docs, "PodDisruptionBudget", RAUTHY)[0]["spec"]["maxUnavailable"], 1, "Rauthy's budget")
        ingress = find(docs, "Ingress", RAUTHY)[0]["spec"]
        expect_eq(ingress["rules"][0]["host"], "auth.genealogy.example.invalid", "Rauthy's host")
        expect_eq(ingress["tls"], [{"secretName": "rauthy-tls", "hosts": ["auth.genealogy.example.invalid"]}], "Rauthy's TLS")
        expect(not find(docs, "Cluster"), "a CloudNativePG Cluster on Hiqlite")

    @check("rauthy: the job completes the web client with OxidGene's audience")
    def _rauthy_job() -> None:
        docs = render(*values("rauthy"))
        job = find(docs, "Job", f"{RAUTHY}-configure")[0]
        expect_eq(job["metadata"]["annotations"]["helm.sh/hook"], "post-install,post-upgrade", "the job's hook")
        args = job["spec"]["template"]["spec"]["containers"][0]["args"]
        expect("Authorization: API-Key oxidgene-chart$$$(RAUTHY_API_KEY)" in args, f"the API key header in {args}")
        expect_eq(args[-1], f"http://{RAUTHY}.{NAMESPACE}.svc.cluster.local:8080/auth/v1/clients/oxidgene-web",
                  "the client's admin URL")
        files = bootstrap(docs)
        web = files["oxidgene-web.json"]
        expect_eq(web["default_aud"], ["https://genealogy.example.invalid"], "the access tokens' extra audience")
        expect_eq(web["redirect_uris"], ["https://genealogy.example.invalid/auth/callback"], "the redirect URI")
        expect_eq(web["post_logout_redirect_uris"], ["https://genealogy.example.invalid/"], "the post-logout redirect")
        expect_eq(web["backchannel_logout_uri"], "https://genealogy.example.invalid/auth/backchannel-logout",
                  "the back-channel logout URI")
        expect_eq(web["challenges"], ["S256"], "PKCE")
        clients = {c["id"]: c for c in files["clients.json"]}
        expect_eq(sorted(clients), ["oxidgene-mcp", "oxidgene-web", "oxidgene-worker"], "the bootstrapped clients")
        expect_eq(clients["oxidgene-web"]["secret"], {"Plain": "__OXIDGENE_WEB_CLIENT_SECRET__"}, "the web secret placeholder")
        expect_eq(files["api_keys.json"][0]["secret"], {"Plain": "__RAUTHY_API_KEY__"}, "the API key placeholder")

    @check("rauthy: a CloudNativePG Cluster of its own")
    def _rauthy_cnpg() -> None:
        docs = render(*values("rauthy"), *CNPG, "--set", "auth.idp.rauthy.database.mode=cloudnativepg")
        cluster = find(docs, "Cluster", f"{RAUTHY}-db")
        expect(len(cluster) == 1, "no CloudNativePG Cluster of Rauthy's own")
        expect_eq(cluster[0]["spec"]["bootstrap"]["initdb"]["database"], "rauthy", "Rauthy's database")
        statefulset = find(docs, "StatefulSet", RAUTHY)[0]["spec"]
        expect("volumeClaimTemplates" not in statefulset, "a Hiqlite volume on PostgreSQL")
        env = env_of(statefulset["template"]["spec"]["containers"][0])
        expect_eq(env.get("PG_PASSWORD"), (f"{RAUTHY}-db-app", "password"), "PG_PASSWORD")
        config = find(docs, "ConfigMap", f"{RAUTHY}-config")[0]["data"]["config.toml"]
        for line in ("hiqlite = false", f'pg_host = "{RAUTHY}-db-rw"', 'pg_db_name = "rauthy"'):
            expect(line in config, f"the configuration lacks {line}")
        expect(not find(docs, "Cluster", f"{RELEASE}-postgresql"), "OxidGene's database became a Cluster")

    @check("mail: the backend queues and the worker sends through the relay")
    def _mail() -> None:
        relay = [
            "--set", "mail.smtp.host=smtp.example.invalid",
            "--set", "mail.from=OxidGene <no-reply@genealogy.example.invalid>",
        ]
        shared = ["--set", "database.mode=existing", "--set", "s3.mode=existing"]
        # Without a host, nothing of mail.
        env = backend_env(render(*values("external")))
        expect(not [k for k in env if k.startswith(("OXIDGENE_SMTP_", "OXIDGENE_MAIL_"))], "mail without a host")
        docs = render(*values("external"), *shared, *relay,
                      "--set", "mail.smtp.credentialsSecret.secret=oxidgene-smtp")
        env = backend_env(docs)
        expect_eq(env.get("OXIDGENE_SMTP_HOST"), "smtp.example.invalid", "OXIDGENE_SMTP_HOST")
        expect_eq(env.get("OXIDGENE_SMTP_PORT"), "587", "OXIDGENE_SMTP_PORT")
        expect_eq(env.get("OXIDGENE_SMTP_TLS"), "starttls", "OXIDGENE_SMTP_TLS")
        expect_eq(env.get("OXIDGENE_MAIL_FROM"), "OxidGene <no-reply@genealogy.example.invalid>", "OXIDGENE_MAIL_FROM")
        expect_eq(env.get("OXIDGENE_SMTP_USERNAME"), ("oxidgene-smtp", "username"), "the relay account's Secret")
        expect_eq(env.get("OXIDGENE_SMTP_PASSWORD"), ("oxidgene-smtp", "password"), "the relay password's Secret")
        worker = find(docs, "Deployment", f"{RELEASE}-worker")
        expect(len(worker) == 1, "no worker with shared database and media")
        worker_env = env_of(worker[0]["spec"]["template"]["spec"]["containers"][0])
        expect_eq(worker_env.get("OXIDGENE_SMTP_HOST"), "smtp.example.invalid", "the worker's relay")
        expect_eq(worker_env.get("OXIDGENE_PUBLIC_URL"), "https://genealogy.example.invalid", "the worker's links")
        expect_eq(worker_env.get("OXIDGENE_SESSION_KEYS"), ("oxidgene-session", "keys"), "the worker's signing keys")
        expect("OXIDGENE_OIDC_CLIENT_SECRET" not in worker_env, "the worker received the sign-in secret")
        # A relay without a sender, or with an unknown TLS mode, is refused.
        error = refused(*values("external"), "--set", "mail.smtp.host=smtp.example.invalid")
        expect("mail.from" in error, error.strip())
        error = refused(*values("external"), *relay, "--set", "mail.smtp.tls=ssl")
        expect("mail.smtp.tls" in error, error.strip())
        # The `none` mode mails nothing, whatever the values.
        env = backend_env(render(*values("none"), *relay))
        expect(not [k for k in env if k.startswith(("OXIDGENE_SMTP_", "OXIDGENE_MAIL_"))], "mail in none mode")

    @check("the shipped Rauthy setup and the development one agree")
    def _drift() -> None:
        docs = render(*values("rauthy"), "--set", "auth.publicUrl=http://localhost:8081", "--set", "ingress.enabled=false")
        compare_setups(bootstrap(docs), DEVELOPMENT)


# The development setup's own differences (see the module documentation).
DEVELOPMENT_ONLY_CLIENTS = {"oxidgene-dev-robot"}
CLIENT_DIFFERENCES = {"secret", "backchannel_logout_uri"}


def normalize(value: object) -> object:
    """Order-insensitive lists, so that a comparison sees content only."""
    if isinstance(value, dict):
        return {k: normalize(v) for k, v in value.items()}
    if isinstance(value, list):
        items = [normalize(v) for v in value]
        return sorted(items, key=lambda v: json.dumps(v, sort_keys=True))
    return value


def without(client: dict, keys: set[str]) -> dict:
    return {k: v for k, v in client.items() if k not in keys}


def compare_setups(shipped: dict[str, object], development: Path) -> None:
    for name in ("groups.json", "scopes.json"):
        dev = json.loads((development / "bootstrap" / name).read_text())
        expect(normalize(shipped[name]) == normalize(dev), f"{name} differs")
    dev_clients = {c["id"]: c for c in json.loads((development / "bootstrap" / "clients.json").read_text())
                   if c["id"] not in DEVELOPMENT_ONLY_CLIENTS}
    shipped_clients = {c["id"]: c for c in shipped["clients.json"]}
    expect_eq(sorted(shipped_clients), sorted(dev_clients), "the clients")
    for client_id, client in shipped_clients.items():
        a, b = without(client, CLIENT_DIFFERENCES), without(dev_clients[client_id], CLIENT_DIFFERENCES)
        expect(normalize(a) == normalize(b), f"client {client_id} differs: {a!r} against {b!r}")
    dev_web = json.loads((development / "oxidgene-web.json").read_text())
    a, b = without(shipped["oxidgene-web.json"], CLIENT_DIFFERENCES), without(dev_web, CLIENT_DIFFERENCES)
    expect(normalize(a) == normalize(b), f"oxidgene-web.json differs: {a!r} against {b!r}")
    # The definition the job applies is the bootstrapped client's, completed.
    bootstrapped = without(dev_clients["oxidgene-web"], CLIENT_DIFFERENCES | {"id"})
    applied = without(dev_web, CLIENT_DIFFERENCES | {"confidential", "default_aud"})
    expect(normalize(bootstrapped) == normalize(applied),
           "docker/rauthy/oxidgene-web.json is not the bootstrapped oxidgene-web with its audience")


if __name__ == "__main__":
    run()
    if failures:
        print(f"\n{len(failures)} chart check(s) failed", file=sys.stderr)
        sys.exit(1)
    print("\nevery chart check passed")
