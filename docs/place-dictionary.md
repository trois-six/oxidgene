---
type: "Data Specification"
title: "Place Dictionary — generated reference places"
description: "The France and United Kingdom place dictionaries: their Geneanet-compatible CSV layout, the open-data sources and licences they are generated from, and the rules that file each place under every name it has borne."
tags: [oxidgene, specification, places, reference-data, france, united-kingdom]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-27T00:00:00Z }
sources:
  - id: geneanet-dico
    title: "Geneanet geneweb-plugin-api, src/assets/dico_place_fr.csv"
    url: "https://github.com/geneanet/geneweb-plugin-api/blob/main/src/assets/dico_place_fr.csv"
  - id: insee-cog
    title: "INSEE, Code officiel géographique (data.gouv.fr)"
    url: "https://www.data.gouv.fr/datasets/code-officiel-geographique-1/"
  - id: ons-ipn
    title: "ONS, Index of Place Names in Great Britain"
    url: "https://www.ons.gov.uk/methodology/geography/geographicalproducts/otherproducts/indexofplacenamesipn"
---

# Place Dictionary — generated reference places

![OxidGene](../assets/OxidGene.png)

The place dictionary lists the places of a country the way genealogical
records name them: today's communes and towns, but also the communes merged
away since the Revolution, the names they bore before, and the départements,
regions and counties they were filed under at the time. It is the reference
data that [place autocomplete](ui-common.md) will suggest from.

It is generated, never edited by hand, and regenerated whenever a source
publishes a new edition.

Related: [Architecture](architecture.md) · [Development](development.md) ·
[Common UI §4.4](ui-common.md) · [Roadmap](roadmap.md)

---

## 1. Generating

```bash
just places                  # download every source, write the dictionary
just places --csv places.csv # also write the uncompressed CSV to read it
just places --cached         # reuse the last run's downloads (generator work)
```

`apps/oxidgene-place-dictionary` downloads its sources into
`target/place-dictionary/` and writes the dictionary Brotli-compressed to
`assets/places/places.csv.br` ([Architecture §7.1](architecture.md)). That
file is committed and embedded into the backend at build time: a build never
reaches the network, and the dictionary changes only when someone reruns
`just places` and commits the result. A run takes about four minutes, spent
waiting on Wikidata and compressing.

Every run downloads afresh, and finds the newest edition of each source by
itself: the latest COG vintage listed on data.gouv.fr and the latest Index of
Place Names on the ONS Open Geography Portal. Rerunning after a source update
needs no code change. `--cached` exists only to iterate on the generator.

The Wikidata query service answers a timeout with a success status and a
truncated body. An answer whose rows do not all match its header is rejected
and retried, never cached.

## 2. File format

One file, written in French like Geneanet's `dico_place_fr.csv`. The
application translates the country and the British nations ("Royaume-Uni",
"Angleterre") for its English interface when it loads the file; French
regions and départements are proper nouns and are not translated.

The layout extends Geneanet's `dico_place_*.csv`[^geneanet-dico]: no header,
every field quoted, and the same first five columns, so a reader of that
format reads these files unchanged.

| # | Column | France | United Kingdom |
|---|--------|--------|----------------|
| 1 | Place | Commune name | Town, village, hamlet or parish |
| 2 | Code | INSEE commune code (not a postcode) | Empty |
| 3 | Subdivision | Département | County |
| 4 | Region | Region | Nation |
| 5 | Country | France | Royaume-Uni |
| 6 | Kind | `commune`, `municipal_arrondissement`, `former_name`, `former_commune` | `settlement`, `parish` |
| 7 | Valid from | First day of this name, when known | Empty |
| 8 | Valid until | First day this name was no longer in use | Empty |
| 9 | Successor | INSEE code of the commune holding the territory today | Empty |
| 10 | Latitude | WGS 84, four decimals | WGS 84, four decimals |
| 11 | Longitude | WGS 84, four decimals | WGS 84, four decimals |
| 12 | Current | `1` when filed under today's département and region | `1` when filed under the ceremonial county |

- **Kinds.** `former_name` is a commune that still exists under another name
  or code (a rename, or the 1968 and 1976 renumberings); `former_commune` was
  merged into another or abolished, including into a commune nouvelle.
- **Dates.** An empty *valid from* means the name predates the source's
  horizon: 1943 for INSEE, and unknown for Wikidata.
- **One row per filing.** A place appears once for each département and region
  (or county) it is filed under (§3, §4). Rows repeating the first five
  columns and the kind are dropped; the ONS lists a place once per boundary it
  straddles.
- **Current.** Of the filings of one place, today's is marked, so a search
  can offer it first. Of two rows that would read the same, the current one
  is kept.
- **Order.** Country, region, subdivision, code, then name, so that two runs
  over the same sources produce the same file.

The CSV is about 18 MB, 1.7 MB compressed.

### 2.1 Use

The backend searches the dictionary for place suggestions
([API reference content](api.md)). It decompresses and indexes it on the
first search: every string in one buffer and the few hundred subdivisions,
regions and countries stored once, about 20 MB in memory, searched in
10–20 ms by a scan in a release build.

## 3. France

### 3.1 Sources

| Source | Provides | Licence |
|--------|----------|---------|
| INSEE COG[^insee-cog]: communes since 1943 and their events | Every commune and arrondissement municipal since 1943, their names, codes and dates, and what became of each | Licence Ouverte 2.0 |
| INSEE COG: départements and regions | Current names | Licence Ouverte 2.0 |
| geo.api.gouv.fr | Centre of each current commune | Licence Ouverte 2.0 |
| Wikidata | Communes dissolved before 1943, accents INSEE omits from old names, coordinates of former communes | CC0 |

Lieux-dits are not included, to keep the dictionary small enough to ship
with the application: IGN's BD TOPO would add 1.1 million of them, about
5.5 MB gzip-compressed even without coordinates.

### 3.2 Communes since 1943

Each name a code bore between two dates is one entry. Its fate follows the
COG's events from the day it ended to a commune that exists today, through
renames and renumberings (`former_name`) or through a merger (`former_commune`).
Only an event turning a commune into a commune carries it forward; the rows
turning it into a commune déléguée or associée describe what is left of it
inside its successor.

INSEE writes some old names without accents ("Eglise" for "Église"). When
Wikidata's label for the same code differs only by accents or case, the
accented form is used.

### 3.3 Communes dissolved before 1943

Wikidata items that are communes of France with a dissolution date before
1943. Their successor is the commune Wikidata says replaced them or contains
them, when that commune is known to INSEE. Otherwise the nearest current
commune in the département Wikidata places them in says which département
holds the land today, but is not claimed as their successor: a neighbour is
not always the commune that absorbed it.

An item already covered by an INSEE entry of the same name and département is
skipped.

### 3.4 Département and region names

A place is filed under:

1. today's département and today's region;
2. today's département and its region before 2016, when the place existed
   before 2016 — the regions Geneanet's dictionary still uses;
3. each former name of its département that was in use while the place
   existed, with the region before 2016.

| Code | Former name | Until |
|------|-------------|-------|
| 04 | Basses-Alpes | 1970-04-13 |
| 17 | Charente-Inférieure | 1941-09-04 |
| 20 | Corse | 1976-01-01 |
| 22 | Côtes-du-Nord | 1990-02-27 |
| 44 | Loire-Inférieure | 1957-03-09 |
| 64 | Basses-Pyrénées | 1969-10-10 |
| 75 | Seine | 1968-01-01 |
| 76 | Seine-Inférieure | 1955-01-18 |
| 78 | Seine-et-Oise | 1968-01-01 |

Seine, Seine-et-Oise and Corse changed territory with their code: their
communes were renumbered into Paris, the petite couronne, the new départements
of 1968, and Corse-du-Sud and Haute-Corse in 1976. A code from before the
renumbering is filed only under the old département, and the renumbered code
only under the new one, so no row pairs a code with a département it never
belonged to. For a commune dissolved before 1943, the département it lay in
at the time is read from its successor's code before the renumbering.

### 3.5 Not covered

- Départements before their current shape: Meurthe and Moselle before 1871,
  Mont-Blanc, Léman and the other départements of 1790–1815.
- Provinces of the Ancien Régime.
- Collectivités d'outre-mer (Saint-Pierre-et-Miquelon, Saint-Barthélemy,
  Saint-Martin, Polynesia, New Caledonia).

## 4. United Kingdom

### 4.1 Sources

| Source | Provides | Licence |
|--------|----------|---------|
| ONS Index of Place Names[^ons-ipn] | Localities, built-up areas, civil parishes and Welsh communities of Great Britain, with historic and ceremonial counties | Open Government Licence v3 |
| Wikidata | Settlements and civil parishes of Northern Ireland, with their county | CC0 |

Redistributing the dictionary requires the attribution: *Source: Office for
National Statistics licensed under the Open Government Licence v.3.0. Contains
OS data © Crown copyright and database right.*

### 4.2 Counties

A place is filed under its **historic county**, the pre-1974 county that
parish registers, census returns and civil registration refer to
(Westmorland, Cumberland, Yorkshire), and under its current **ceremonial
county** (Cumbria, North Yorkshire; in Wales, the preserved counties such as
South Glamorgan). The six counties of Northern Ireland are both.

The IPN files a locality under its main word ("Haddlesey, East", "Dell, The")
and tells homonyms apart with a parenthesis ("Aberdour (Fife)"). The
generator writes "East Haddlesey" and "The Dell", and drops the parenthesis,
which the county column already carries. Parish names keep their commas
("Cromdale, Inverallan and Advie"), and the "unparished area" entries are
skipped as they name no parish.

### 4.3 Not covered

- Northern Ireland townlands, which are its lieux-dits.
- The Isle of Man and the Channel Islands, which are not part of the United
  Kingdom and are not in the IPN.

[^geneanet-dico]: Geneanet, `dico_place_fr.csv`, five quoted columns: place, INSEE code, département, region, country.
[^insee-cog]: INSEE publishes a COG vintage each year; `v_commune_depuis_1943.csv` and `v_mvt_commune_<year>.csv` carry the history.
[^ons-ipn]: The ONS stamps the edition year into column names (`place23nm`); the generator finds columns whatever the year.
