# OxidGene

<p align="center">
	<img src="docs/assets/OxidGene.png" alt="OxidGene Logo" width="300">
</p>

A modern, high-performance genealogy platform built entirely in Rust.

Start with the [OxidGene Quickstart](docs/specifications/quickstart.md) to run
the desktop application, the Docker Compose stack, or a Kubernetes deployment.

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

## Overview

OxidGene is a multiplatform genealogy application featuring:

- **Bring your family history with you**: Import and export GEDCOM 5.5.1,
	GEDCOM 7.0, and GEDZIP archives, including their media
- **Reconnect your Geneanet archives**: Import GeneWeb `.gw` and `gwplus`
	trees, then use the guided desktop workflow to recover linked photos and
	documents without storing shared media twice
- **Explore and edit complete family trees**: Navigate interactive pedigrees,
	manage people, families, events, sources, places, notes, and media, and find
	records through fast search and dictionary views
- **Enjoy genealogy on every screen**: Use the responsive WebAssembly frontend
	on desktop or mobile browsers, or run the native desktop application built
	from the same Dioxus codebase
- **Keep working offline**: The desktop application embeds SQLite and stores
	your genealogy and media locally, with no server required
- **Make the workspace your own**: Switch themes and change between English
	and French without restarting the application
- **Integrate without compromise**: Build on REST and GraphQL APIs with full
	feature parity
- **Stay fast as trees grow**: Rust powers the complete stack, backed by
	durable read projections and efficient family traversal

## Documentation

Full specifications are available in
[`docs/specifications/`](docs/specifications/index.md):

- [Quickstart](docs/specifications/quickstart.md) - installation and deployment
	paths.
- [General](docs/specifications/general.md) - vision, users, features, and MVP
	scope.
- [Architecture](docs/specifications/architecture.md) - technology stack,
	crate layout, build, and deployment.
- [Data Model](docs/specifications/data-model.md) - entities, enums, and ERD.
- [API Contract](docs/specifications/api.md) - REST and GraphQL endpoints.
- [Roadmap](docs/specifications/roadmap.md) - delivery status and milestones.
- UI specifications: [Homepage](docs/specifications/ui-home.md),
	[Tree View](docs/specifications/ui-genealogy-tree.md),
	[Person Edit](docs/specifications/ui-person-edit-modal.md), and
	[Settings](docs/specifications/ui-settings.md).

## Development

The development environment, prerequisites, and `just` command reference are
documented in [Development](docs/specifications/development.md).

## License

GNU Affero General Public License v3.0 - see [LICENSE](LICENSE) for details.
