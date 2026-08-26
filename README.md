# OxidGene

<p align="center">
  <img src="docs/assets/OxidGene.png" alt="OxidGene Logo" width="300">
</p>

A modern, high-performance genealogy platform built entirely in Rust.

## Screenshots

<table>
  <tr>
    <td rowspan="2" width="68%">
      <img src="docs/assets/screenshot2.png" alt="OxidGene interactive genealogy tree">
    </td>
    <td width="32%">
      <img src="docs/assets/screenshot1.png" alt="OxidGene tree dashboard">
    </td>
  </tr>
  <tr>
    <td width="32%">
      <img src="docs/assets/screenshot3.png" alt="OxidGene person detail page">
    </td>
  </tr>
</table>

## Features

- **Complete tree editing**: Manage multiple trees, people, families,
  relationships, events, places, sources, citations, notes, and media.
- **Interactive genealogy views**: Browse pedigrees, person profiles, timelines,
  family connections, and merge duplicate people.
- **Powerful discovery tools**: Search with genealogical filters and explore
  dictionaries of family names, occupations, places, and sources.
- **Interoperable imports and exports**: Import and export GEDCOM 5.5.1,
  GEDCOM 7.0, and GEDZIP archives; import GeneWeb `.gw` files, including the
  `gwplus` extension.
- **Geneanet media recovery**: Use the guided desktop workflow to recover a
  tree's Geneanet media, shared-photo links, portraits, documents, and event
  evidence.
- **Rich media management**: Upload images, PDFs, audio, and video; organize
  galleries and multi-page documents; generate thumbnails; identify people in
  image regions; and choose portrait crops.
- **Web and desktop from one codebase**: Run the Dioxus interface as a WASM web
  app or as a native desktop application for Linux, Windows, and macOS.
- **Offline desktop mode**: Keep genealogy and content-addressed media locally
  with an embedded Axum backend and SQLite database.
- **REST and GraphQL parity**: Access the same validated, cursor-paginated
  product operations through both API surfaces.
- **Customizable interface**: Switch between English and French and choose
  from the built-in visual themes without restarting.
- **Rust throughout**: Durable read projections and indexed ancestry traversal
  keep profiles, pedigrees, and searches responsive.

Authentication, authorization, and privacy enforcement are planned but not yet
implemented. Current deployments must keep the backend on a trusted local or
private network; see the [roadmap](docs/specifications/roadmap.md).

## Documentation

Full specifications are available in [`docs/specifications/`](docs/specifications/index.md):

- [General](docs/specifications/general.md) — Vision, users, features, MVP scope
- [Architecture](docs/specifications/architecture.md) — Tech stack, crate layout, build, deployment
- [Data Model](docs/specifications/data-model.md) — Entities, enums, ERD
- [API Contract](docs/specifications/api.md) — REST & GraphQL endpoints
- [Roadmap](docs/specifications/roadmap.md) — EPICs & sprints
- UI specs: [Homepage](docs/specifications/ui-home.md) · [Tree View](docs/specifications/ui-genealogy-tree.md) · [Person Edit](docs/specifications/ui-person-edit-modal.md) · [Settings](docs/specifications/ui-settings.md)

## Development

See [Development](docs/specifications/development.md) for prerequisites, local
workflows, and the `just` command reference.

## License

GNU Affero General Public License v3.0 - see [LICENSE](LICENSE) for details.
