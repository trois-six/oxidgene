# OxidGene

<p align="center">
	<img src="assets/OxidGene.png" alt="OxidGene Logo" width="300">
</p>

A modern, high-performance genealogy platform built entirely in Rust.

Start with the [OxidGene Quickstart](docs/quickstart.md) to run
the desktop application, the Docker Compose stack, or a Kubernetes deployment.

## Screenshots

<table>
	<tr>
		<td rowspan="2" width="68%">
			<img src="assets/screenshot2.png" alt="OxidGene interactive genealogy tree">
		</td>
		<td width="32%">
			<img src="assets/screenshot1.png" alt="OxidGene tree dashboard">
		</td>
	</tr>
	<tr>
		<td width="32%">
			<img src="assets/screenshot3.png" alt="OxidGene person detail page">
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
- **Type places the way the records name them**: Every place field suggests
	from a built-in place dictionary of France, the United Kingdom, Germany,
	Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium,
	Luxembourg and the Netherlands, with the
	municipalities merged away, their former names, and the subdivisions and
	regions they were filed under at the time, while still accepting any free
	text
- **Understand what the records say**: Explanatory sheets for more than 3,000
	historical occupations, including trades and offices particular to each
	covered country, and meanings of given names, in all eight interface
	languages and recognised whatever the language of the record, shown right
	beside the person
- **Ask your AI assistant about your tree**: The desktop application serves
	your trees read-only to Claude and any other Model Context Protocol client
- **Make the workspace your own**: Switch themes and change between English,
	French, German, Spanish, Italian, Dutch, Polish and Portuguese without
	restarting the application
- **Integrate without compromise**: Build on REST and GraphQL APIs with full
	feature parity. Full OpenTelemetry instrumentation.
- **Stay fast as trees grow**: Rust powers the complete stack, backed by
	durable read projections and efficient family traversal

## Documentation

Full specifications are available in
[`docs/`](docs/index.md):

- [Quickstart](docs/quickstart.md) - installation and deployment
	paths.
- [General](docs/general.md) - vision, users, features, and MVP
	scope.
- [Architecture](docs/architecture.md) - technology stack,
	crate layout, build, and deployment.
- [Data Model](docs/data-model.md) - entities, enums, and ERD.
- [API Contract](docs/api.md) - REST and GraphQL endpoints.
- [Assistant Access (MCP)](docs/mcp.md) - the read-only Model Context
	Protocol server of the desktop application.
- [Place Dictionary](docs/place-dictionary.md) - the built-in places, their
	sources and how they are generated.
- [Roadmap](docs/roadmap.md) - delivery status and milestones.
- UI specifications: [Homepage](docs/ui-home.md),
	[Tree View](docs/ui-genealogy-tree.md),
	[Person Edit](docs/ui-person-edit-modal.md), and
	[Settings](docs/ui-settings.md).

## Development

The development environment, prerequisites, and `just` command reference are
documented in [Development](docs/development.md).

## License

GNU Affero General Public License v3.0 - see [LICENSE](LICENSE) for details.
