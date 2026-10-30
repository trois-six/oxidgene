# /// script
# requires-python = ">=3.10"
# dependencies = ["brotli==1.2.0"]
# ///
"""Build the documentation site (site/) with Zola, or serve it.

    uv run scripts/site.py [build|serve]

`just site`, `just site-serve` and .github/workflows/pages.yml run it. It
documents the library crates with rustdoc, then writes only ignored,
generated files, so the specifications stay the single source of the
technical documentation, and runs Zola on them:

- site/content/tech/: one page per specification of docs/, its OKF front
  matter turned into Zola's, the section index built from docs/index.md,
  and three reference pages: the GraphQL schema (docs/schema.graphql), the
  REST operations of the OpenAPI document the API crate's build script
  writes (so a build of oxidgene-api, such as the recipe's `cargo doc`, must
  come first), and the Rust crates, whose rustdoc the recipe publishes under
  api/. Links between
  specifications become Zola internal links, which Zola checks; links to
  other files of the repository point at GitHub; every heading gets its
  GitHub anchor as an explicit id, so the anchors the specifications link to
  (and the specs guard checks) resolve on the site too;
- site/static/: the screenshots, fonts and logo the pages and templates use,
  copied from assets/, the OpenAPI document as openapi.json, and the
  rustdoc under api/.

`build` writes the site to site/public/; `serve` previews it with Zola's
live reload.
"""

import ast
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import brotli

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
SITE = ROOT / "site"
TECH = SITE / "content" / "tech"
STATIC = SITE / "static"
REPOSITORY = "https://github.com/trois-six/oxidgene"

# The library crates whose rustdoc is published, in dependency order.
CRATES = [
    ("oxidgene_core", "Domain types, enums, errors and projections."),
    ("oxidgene_db", "SeaORM entities, migrations and repositories."),
    ("oxidgene_gedcom", "GEDCOM and GeneWeb conversion into the domain."),
    ("oxidgene_geneanet", "Geneanet join, key folding, archive indexing and hashing."),
    ("oxidgene_archives", "Archive catalogue, citation parsing and portal adapters."),
    ("oxidgene_observability", "Shared OpenTelemetry initialization."),
    ("oxidgene_api", "Axum REST, GraphQL, MCP, services, media and profiles."),
    ("oxidgene_ui", "Dioxus components and pages."),
]

FRONT_MATTER = re.compile(r"\A---\n(.*?)\n---\n", re.DOTALL)
FENCE = re.compile(r"^\s*(```|~~~)")
INLINE_LINK = re.compile(r"(!?\[[^\]]*\])\(([^)\s]+)((?:\s+\"[^\"]*\")?)\)")
REFERENCE_LINK = re.compile(r"^(\s*\[[^\]]+\]:\s+)(\S+)(.*)$")
HEADING = re.compile(r"^(#{1,6}) (.*?)\s*$")


def yaml_string(front: str, key: str) -> str:
    """A top-level scalar of the OKF front matter, quoted or not."""
    match = re.search(rf"^{key}:\s*(.+)$", front, re.MULTILINE)
    if not match:
        return ""
    value = match.group(1).strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        value = value[1:-1].replace('\\"', '"')
    return value


def toml_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def slug(heading: str) -> str:
    """GitHub's anchor of a heading, as the specs guard computes it."""
    return "".join(
        "-" if c == " " else c
        for c in heading.lower()
        if c.isalnum() or c in " -_"
    )


def target(link: str, page: str) -> str:
    """Where a link of a specification points on the site."""
    if re.match(r"^[a-z][a-z0-9+.-]*:", link) or link.startswith("#"):
        return link
    path, _, anchor = link.partition("#")
    suffix = f"#{anchor}" if anchor else ""
    if path == "index.md":
        return f"@/tech/_index.md{suffix}"
    if path.endswith(".md") and "/" not in path and (DOCS / path).exists():
        return f"@/tech/{path}{suffix}"
    if path == "schema.graphql":
        return f"@/tech/graphql-schema.md{suffix}"
    if path.startswith("../assets/screenshots/"):
        # The page is served at tech/<name>/, the screenshots at screenshots/.
        return "../../screenshots/" + path.removeprefix("../assets/screenshots/") + suffix
    resolved = (DOCS / path).resolve()
    try:
        relative = resolved.relative_to(ROOT)
    except ValueError:
        sys.exit(f"{page}: link outside the repository: {link}")
    return f"{REPOSITORY}/blob/main/{relative.as_posix()}{suffix}"


def convert(body: str, page: str) -> str:
    """Rewrite the links and pin the heading anchors of a specification."""
    lines = []
    fenced = False
    seen: dict[str, int] = {}
    for line in body.splitlines():
        if FENCE.match(line):
            fenced = not fenced
            lines.append(line)
            continue
        if fenced:
            lines.append(line)
            continue
        heading = HEADING.match(line)
        if heading:
            base = slug(heading.group(2))
            count = seen.get(base, 0)
            seen[base] = count + 1
            if heading.group(1) == "#":
                # The page template shows the title: the heading would repeat
                # it. It still counts towards the anchors, as on GitHub.
                continue
            anchor = base if count == 0 else f"{base}-{count}"
            line = f"{line.rstrip()} {{#{anchor}}}"
        line = INLINE_LINK.sub(
            lambda m: f"{m.group(1)}({target(m.group(2), page)}{m.group(3)})", line
        )
        line = REFERENCE_LINK.sub(
            lambda m: f"{m.group(1)}{target(m.group(2), page)}{m.group(3)}", line
        )
        lines.append(line)
    return "\n".join(lines) + "\n"


def page(name: str, title: str, description: str, weight: int, body: str, source: str) -> None:
    front = [
        "+++",
        f"title = {toml_string(title)}",
        f"description = {toml_string(description)}",
        f"weight = {weight}",
        "[extra]",
        f"source = {toml_string(source)}",
        "+++",
        "",
    ]
    (TECH / name).write_text("\n".join(front) + body, encoding="utf-8")


def specifications() -> list[str]:
    """The specifications in docs/index.md's order, then any it omits."""
    listed = re.findall(r"\]\(([\w-]+\.md)\)", (DOCS / "index.md").read_text(encoding="utf-8"))
    others = sorted(p.name for p in DOCS.glob("*.md") if p.name != "index.md" and p.name not in listed)
    return listed + others


def index() -> None:
    """The technical section's index, from docs/index.md's groups."""
    text = FRONT_MATTER.sub("", (DOCS / "index.md").read_text(encoding="utf-8"))
    lines = []
    for line in text.splitlines():
        if line.startswith("# "):
            line = "#" + line
        line = INLINE_LINK.sub(lambda m: f"{m.group(1)}({target(m.group(2), 'index.md')})", line)
        lines.append(line)
    body = (
        "The specifications describe the product as it is: its architecture, "
        "data model, API contract, and one specification per page and workflow. "
        "They are authoritative and written for contributors; the "
        "[user guide](@/guide/_index.md) explains the application to its users.\n\n"
        "The [REST reference](@/tech/rest-reference.md), the "
        "[GraphQL schema](@/tech/graphql-schema.md) and the "
        "[Rust crates](@/tech/rust-api.md) complete the "
        "[REST and GraphQL contract](@/tech/api.md).\n\n" + "\n".join(lines) + "\n"
    )
    front = [
        "+++",
        'title = "Technical documentation"',
        'description = "The specifications of OxidGene, the GraphQL schema and the Rust API."',
        'sort_by = "weight"',
        "+++",
        "",
    ]
    (TECH / "_index.md").write_text("\n".join(front) + body, encoding="utf-8")


def references(weight: int) -> None:
    rest_reference(weight)
    weight += 1
    schema = (DOCS / "schema.graphql").read_text(encoding="utf-8")
    page(
        "graphql-schema.md",
        "GraphQL schema",
        "The SDL of the GraphQL API, generated from the server (just graphql-schema).",
        weight,
        "The schema of the GraphQL endpoint, `/graphql`, as `just graphql-schema` "
        "writes it from the server. Its operations mirror the REST routes one for "
        "one; the [API contract](@/tech/api.md) describes both.\n\n"
        "```graphql\n" + schema.rstrip() + "\n```\n",
        "docs/schema.graphql",
    )
    rows = "\n".join(
        f"| [`{crate}`](../../api/{crate}/index.html) | {purpose} |" for crate, purpose in CRATES
    )
    page(
        "rust-api.md",
        "Rust API",
        "The rustdoc of the workspace's library crates.",
        weight + 1,
        "The documentation `cargo doc` generates from the workspace's library "
        "crates, built from `main` with the site. Their boundaries and dependency "
        "direction are in [Architecture](@/tech/architecture.md).\n\n"
        "| Crate | Purpose |\n|---|---|\n" + rows + "\n",
        "Cargo.toml",
    )


def openapi() -> dict:
    """The OpenAPI document of the latest build of the API crate."""
    built = sorted(
        (ROOT / "target").glob("*/build/oxidgene-api-*/out/openapi.json.br"),
        key=lambda path: path.stat().st_mtime,
    )
    if not built:
        sys.exit("no OpenAPI document: build oxidgene-api first (cargo doc does)")
    return json.loads(brotli.decompress(built[-1].read_bytes()))


def schema_type(schema: dict) -> str:
    """A schema in one line, a reference linked to its section."""
    if "$ref" in schema:
        name = schema["$ref"].rsplit("/", 1)[-1]
        return f"[`{name}`](#schema-{name.lower()})"
    for combinator in ("oneOf", "anyOf", "allOf"):
        if combinator in schema:
            parts = [schema_type(part) for part in schema[combinator]]
            return (" & " if combinator == "allOf" else " \\| ").join(parts)
    kind = schema.get("type", "any")
    if isinstance(kind, list):
        nullable = "null" in kind
        kind = " \\| ".join(k for k in kind if k != "null")
        text = schema_type({**schema, "type": kind})
        return f"{text} or null" if nullable else text
    if kind == "array":
        return schema_type(schema.get("items", {})) + "[]"
    if "enum" in schema:
        return " \\| ".join(f"`{value}`" for value in schema["enum"])
    if "format" in schema:
        return f"`{kind}` ({schema['format']})"
    return f"`{kind}`"


def rest_reference(weight: int) -> None:
    document = openapi()
    (STATIC / "openapi.json").write_text(json.dumps(document, indent=2), encoding="utf-8")
    tags: dict[str, list[tuple[str, str, dict]]] = {}
    for path, item in document["paths"].items():
        for method, operation in item.items():
            tag = (operation.get("tags") or ["rest"])[0]
            tags.setdefault(tag, []).append((method.upper(), path, operation))
    lines = [
        f"Every operation of the REST API version {document['info']['version']}: its path, "
        "parameters and request body, from the OpenAPI 3.1 document generated from the "
        "router at build time and served by every deployment at `/api/v1/openapi.json` "
        "([download it](../../openapi.json)). What each operation does, its responses "
        "and its errors are in the [API contract](@/tech/api.md), with the GraphQL "
        "operation mirroring each one.",
        "",
    ]
    for tag in sorted(tags):
        lines += [f"## {tag} {{#tag-{tag}}}", ""]
        for method, path, operation in sorted(tags[tag], key=lambda entry: (entry[1], entry[0])):
            anchor = operation.get("operationId", f"{method}-{path}").lower().replace("_", "-")
            lines += [f"### `{method} {path}` {{#{anchor}}}", ""]
            parameters = operation.get("parameters", [])
            if parameters:
                lines += ["| Parameter | In | Required | Type |", "|---|---|---|---|"]
                for parameter in parameters:
                    required = "yes" if parameter.get("required") else "no"
                    lines.append(
                        f"| `{parameter['name']}` | {parameter['in']} | {required} | "
                        f"{schema_type(parameter.get('schema', {}))} |"
                    )
                lines.append("")
            body = operation.get("requestBody", {}).get("content", {})
            for content_type, content in body.items():
                lines += [f"Request body (`{content_type}`): {schema_type(content.get('schema', {}))}", ""]
    lines += ["## Schemas {#schemas}", ""]
    for name, schema in sorted(document.get("components", {}).get("schemas", {}).items()):
        lines += [f"### `{name}` {{#schema-{name.lower()}}}", ""]
        properties = schema.get("properties")
        if properties:
            required = set(schema.get("required", []))
            lines += ["| Field | Type | Required |", "|---|---|---|"]
            for field, field_schema in properties.items():
                lines.append(f"| `{field}` | {schema_type(field_schema)} | {'yes' if field in required else 'no'} |")
        else:
            lines.append(schema_type(schema))
        lines.append("")
    page(
        "rest-reference.md",
        "REST reference",
        "Every REST operation with its parameters and request body, from the generated OpenAPI document.",
        weight,
        "\n".join(lines) + "\n",
        "crates/oxidgene-api/build.rs",
    )


def statics() -> None:
    for name, source in [
        ("screenshots", ROOT / "assets" / "screenshots"),
        ("fonts", ROOT / "assets" / "fonts"),
    ]:
        destination = STATIC / name
        shutil.rmtree(destination, ignore_errors=True)
        shutil.copytree(source, destination)
    shutil.copy2(ROOT / "assets" / "brand" / "OxidGene.svg", STATIC / "logo.svg")
    shutil.copy2(ROOT / "assets" / "brand" / "logo_64.png", STATIC / "favicon.png")


def tour() -> list[str]:
    """The home pages whose tour does not show the README carousel's
    screenshots (CAROUSEL in scripts/screenshots.py) in its order."""
    script = ast.parse((ROOT / "scripts" / "screenshots.py").read_text(encoding="utf-8"))
    carousel = next(
        ast.literal_eval(node.value)
        for node in script.body
        if isinstance(node, ast.Assign) and any(getattr(target, "id", None) == "CAROUSEL" for target in node.targets)
    )
    drifted = []
    for home in sorted((SITE / "content").glob("_index*.md")):
        block = re.search(r"^tour = \[\n(.*?)^\]", home.read_text(encoding="utf-8"), re.DOTALL | re.MULTILINE)
        names = re.findall(r'name = "([^"]+)"', block.group(1)) if block else []
        if names != carousel:
            drifted.append(f"site/content/{home.name}")
    return drifted


def rustdoc() -> None:
    """The rustdoc of the library crates, which also builds the API crate
    and so writes the OpenAPI document `rest_reference` reads."""
    packages = [argument for crate, _ in CRATES for argument in ("-p", crate.replace("_", "-"))]
    # rustdoc adds to target/doc and keeps what earlier runs left there,
    # dependencies' pages and search entries among them.
    shutil.rmtree(ROOT / "target" / "doc", ignore_errors=True)
    # A broken intra-doc link would publish a dead link: warnings fail.
    env = {**os.environ, "RUSTDOCFLAGS": "-D warnings"}
    subprocess.run(["cargo", "doc", "--locked", "--no-deps", *packages], cwd=ROOT, env=env, check=True)
    destination = STATIC / "api"
    shutil.rmtree(destination, ignore_errors=True)
    shutil.copytree(ROOT / "target" / "doc", destination)


def prepare() -> int:
    shutil.rmtree(TECH, ignore_errors=True)
    TECH.mkdir(parents=True)
    statics()
    drifted = tour()
    if drifted:
        print(f"{', '.join(drifted)}: `tour` differs from CAROUSEL in scripts/screenshots.py", file=sys.stderr)
        return 1
    names = specifications()
    for weight, name in enumerate(names, start=1):
        text = (DOCS / name).read_text(encoding="utf-8")
        match = FRONT_MATTER.match(text)
        if not match:
            print(f"docs/{name}: no front matter", file=sys.stderr)
            return 1
        front = match.group(1)
        page(
            name,
            yaml_string(front, "title") or name.removesuffix(".md"),
            yaml_string(front, "description"),
            weight * 10,
            convert(text[match.end():], name),
            f"docs/{name}",
        )
    references((len(names) + 1) * 10)
    index()
    print(f"site: {len(names)} specifications, {len(CRATES)} crates")
    return 0


def main() -> int:
    command = sys.argv[1] if len(sys.argv) > 1 else "build"
    if command not in ("build", "serve"):
        print(__doc__, file=sys.stderr)
        return 2
    rustdoc()
    if prepare():
        return 1
    return subprocess.run(["zola", command], cwd=SITE).returncode


if __name__ == "__main__":
    sys.exit(main())
