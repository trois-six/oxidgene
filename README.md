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

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust (stable) |
| Frontend | Dioxus 0.7+ (Web + Desktop) |
| Backend | Axum 0.8+ |
| GraphQL | async-graphql 7.2+ |
| ORM | SeaORM 1.1+ |
| Web DB | PostgreSQL 16+ |
| Desktop DB | SQLite 3.35+ |
| GEDCOM | ged_io 0.12+ |

## Project Structure

```
oxidgene/
├── crates/
│   ├── oxidgene-core/      # Domain types, enums, error types
│   ├── oxidgene-db/        # SeaORM entities, migrations, repositories
│   ├── oxidgene-api/       # Axum REST handlers + GraphQL resolvers
│   ├── oxidgene-gedcom/    # GEDCOM import/export (wraps ged_io)
│   └── oxidgene-ui/        # Dioxus frontend components
├── apps/
│   ├── oxidgene-server/    # Web backend binary
│   ├── oxidgene-desktop/   # Desktop binary (Axum + SQLite + WebView)
│   └── oxidgene-cli/       # CLI tool
└── docker/                 # Container build files
```

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
