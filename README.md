# OxidGene

<p align="center">
  <img src="docs/assets/OxidGene.png" alt="OxidGene Logo" width="300">
</p>

A modern, high-performance genealogy platform built entirely in Rust.

## Overview

OxidGene is a multiplatform genealogy application featuring:

- **Dual API**: REST + GraphQL with full feature parity
- **Cross-platform**: Web (WASM) and Desktop from a single Dioxus codebase
- **GEDCOM support**: Import/export GEDCOM 5.5.1 and 7.0 files
- **Offline-capable**: Desktop app with embedded SQLite database
- **Performant**: Rust from top to bottom, closure table for fast tree traversal

## Documentation

Full specifications are available in [`docs/specifications/`](docs/specifications/README.md):

- [General](docs/specifications/general.md) — Vision, users, features, MVP scope
- [Architecture](docs/specifications/architecture.md) — Tech stack, crate layout, build, deployment
- [Data Model](docs/specifications/data-model.md) — Entities, enums, ERD
- [API Contract](docs/specifications/api.md) — REST & GraphQL endpoints
- [Roadmap](docs/specifications/roadmap.md) — EPICs & sprints
- UI specs: [Homepage](docs/specifications/ui-home.md) · [Tree View](docs/specifications/ui-genealogy-tree.md) · [Person Edit](docs/specifications/ui-person-edit-modal.md) · [Settings](docs/specifications/ui-settings.md)

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [just](https://github.com/casey/just) (task runner)
- PostgreSQL 16+ (for web deployment)

## Quick Start

```bash
# Build all crates
just build

# Run all tests
just test

# Run all checks (format, lint, test)
just check

# See all available commands
just
```

## Development

```bash
# Format code
just fmt

# Run clippy
just clippy

# Run the web server (dev)
just server

# Run the desktop app (dev)
just desktop
```

## License

MIT License - see [LICENSE](LICENSE) for details.
