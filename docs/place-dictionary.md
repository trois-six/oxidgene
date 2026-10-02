---
type: "Data Specification"
title: "Place Dictionary — generated reference places"
description: "The place dictionary of France, the United Kingdom, Germany, Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium, Luxembourg and the Netherlands: its Geneanet-compatible CSV layout, the open-data sources and licences it is generated from, and the rules that file each place under every name it has borne."
tags: [oxidgene, specification, places, reference-data, france, united-kingdom, germany, italy, spain, switzerland, poland, united-states, portugal, belgium, luxembourg, netherlands]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-02T00:46:32Z }
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
  - id: destatis-gv100
    title: "Destatis, Gemeindeverzeichnis GV100AD"
    url: "https://www.destatis.de/DE/Themen/Laender-Regionen/Regionales/Gemeindeverzeichnis/_inhalt.html"
  - id: istat-comuni
    title: "ISTAT, Codici statistici delle unità amministrative territoriali"
    url: "https://www.istat.it/classificazione/codici-dei-comuni-delle-province-e-delle-regioni/"
  - id: ine-codmun
    title: "INE, Relación de municipios, provincias, comunidades autónomas y sus códigos"
    url: "https://www.ine.es/daco/daco42/codmun/codmun_anual.htm"
  - id: bfs-agv
    title: "BFS, Amtliches Gemeindeverzeichnis der Schweiz"
    url: "https://www.agvchapp.bfs.admin.ch/"
  - id: gus-teryt
    title: "GUS, Krajowy Rejestr Urzędowy Podziału Terytorialnego Kraju (TERYT)"
    url: "https://eteryt.stat.gov.pl/"
  - id: caop
    title: "DGT, Carta Administrativa Oficial de Portugal (OGC API)"
    url: "https://ogcapi.dgterritorio.gov.pt/"
  - id: cbs-gemeenten
    title: "CBS, Gemeentelijke indeling"
    url: "https://www.cbs.nl/nl-nl/onze-diensten/methoden/classificaties/overig/gemeentelijke-indelingen-per-jaar"
  - id: census-gazetteer
    title: "U.S. Census Bureau, Gazetteer Files"
    url: "https://www.census.gov/geographies/reference-files/time-series/geo/gazetteer-files.html"
---

# Place Dictionary — generated reference places

![OxidGene](../assets/brand/OxidGene.png)

The place dictionary lists the places of a country the way genealogical
records name them: today's municipalities, but also those merged away, the
names and codes they bore before, and the subdivisions and regions they were
filed under at the time, as far as each country's sources go. It covers
France, the United Kingdom, Germany, Italy, Spain, Switzerland, Poland, the United States, Portugal, Belgium, Luxembourg and the Netherlands, and is what [place fields](ui-common.md) suggest from.

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
`assets/places/places.csv.br` ([Architecture §7.1](architecture.md)). The
same run writes the basemap of the [statistics](ui-statistics.md) heat map,
`assets/basemap/countries.json.br`: every country's outline from Natural
Earth's 1:50m admin-0 layer (public domain), snapped to a tenth of a degree,
with the country's populated places from Natural Earth's 1:10m layer (their
names in the interface languages, position, labelling zoom and population),
joined by the country's `ADM0_A3` code. That
file is committed and embedded into the backend at build time: a build never
reaches the network, and the dictionary changes only when someone reruns
`just places` and commits the result. A run takes about four minutes, spent
waiting on Wikidata and compressing.

Every run downloads afresh, and finds the newest edition of each source by
itself: the latest COG vintage listed on data.gouv.fr, the latest Index of
Place Names on the ONS Open Geography Portal, every GV100AD edition the
Destatis page links, the newest INE, CBS and Census lists published, the
current CAOP, and the Swiss register up to the day of the run. Rerunning after a source update
needs no code change. `--cached` exists only to iterate on the generator.

The Wikidata query service answers a timeout with a success status and a
truncated body. An answer whose rows do not all match its header is rejected
and retried, never cached.

## 2. File format

One file, written in French like Geneanet's `dico_place_fr.csv`: the
countries and the British nations carry their French names ("Royaume-Uni",
"Angleterre", "Etats-Unis d'Amérique" spelled as Geneanet does), which the
application translates for its English interface when it loads the file.
Subdivisions and regions keep their official local names (Bayern, Toscana,
Cataluña/Catalunya; a Swiss canton under the first of its official names) and
are never translated.

The layout extends Geneanet's `dico_place_*.csv`[^geneanet-dico]: no header,
every field quoted, and the same first five columns, so a reader of that
format reads these files unchanged.

| # | Column | Content |
|---|--------|---------|
| 1 | Place | Municipality, town, village or parish name |
| 2 | Code | The country's official municipality code (below), empty for the United Kingdom |
| 3 | Subdivision | Département, county, Kreis, province, powiat; empty where the country has none that records use |
| 4 | Region | Region, nation, Land, autonomous community, canton, voivodeship, state |
| 5 | Country | The country, in French |
| 6 | Kind | `commune`, `municipal_arrondissement`, `former_name`, `former_commune`, `settlement`, `parish` |
| 7 | Valid from | First day of this name, when known |
| 8 | Valid until | First day this name was no longer in use, when known |
| 9 | Successor | Code of the municipality holding the territory today, when known |
| 10 | Latitude | WGS 84, four decimals |
| 11 | Longitude | WGS 84, four decimals |
| 12 | Current | `1` for the place's filing of today |

| Country | Code | Subdivision | Region |
|---------|------|-------------|--------|
| France | INSEE commune code (not a postcode) | Département | Region |
| United Kingdom | Empty | County | Nation |
| Germany | Amtlicher Gemeindeschlüssel (8 digits) | Kreis, empty for a kreisfreie Stadt | Land |
| Italy | ISTAT commune code (6 digits) | Province, metropolitan city or free consortium | Region |
| Spain | INE municipality code (5 digits) | Province | Autonomous community |
| Switzerland | BFS commune number | Empty | Canton |
| Poland | TERYT gmina code (7 digits) | Powiat, empty for a city with powiat rights | Voivodeship |
| United States | Census GEOID (state and place, or county subdivision) | County | State |
| Portugal | INE code: DICO (4 digits) for a municipality, DICOFRE (6) for a parish | Municipality, for a parish | District or autonomous region |
| Belgium | NIS code (5 digits; a section adds a letter) | Province, empty in Brussels | Region |
| Luxembourg | LAU code | Empty | Canton |
| Netherlands | CBS municipality code (4 digits) | Empty | Province |

- **Kinds.** `commune` is a municipality of any country; `former_name` is one
  that still exists under another name or code (a rename, a renumbering, a
  new province or Kreis); `former_commune` was merged into another or
  abolished. `settlement` is a British locality or an American
  census-designated place, `parish` a British civil parish or a Portuguese
  freguesia.
- **Dates.** An empty *valid from* means the name predates the source's
  horizon (1943 for INSEE, 1848 for the BFS) or is unknown.
- **One row per filing.** A place appears once for each département and region
  (or county) it is filed under (§3, §4). A row reading like another in the
  first five columns once case, accents and punctuation are set aside, with
  the same kind, dates and successor, is dropped: the ONS lists a place once
  per boundary it straddles and spells some names two ways ("St George",
  "St. George"). Rows that differ in their dates or successor are two eras
  of a name, or two homonyms absorbed by different communes, and are both
  kept. The generator reports how many rows it dropped, and a backend test
  checks the embedded file holds no repeated row.
- **Current.** Of the filings of one place, today's is marked, so a search
  can offer it first. Of two rows that would read the same, the current one
  is kept, then the first spelling.
- **Order.** Country, region, subdivision, code, then name, so that two runs
  over the same sources produce the same file.

The CSV is about 28 MB and 284,000 rows, 2.9 MB compressed.

### 2.1 Use

The backend searches the dictionary for place suggestions
([API reference content](api.md)). It decompresses and indexes it on the
first search: every string in one buffer and the few hundred subdivisions,
regions and countries stored once, a few tens of megabytes in memory, loaded
in about 0.4 s and searched in under 10 ms by a scan in a release build.

The [statistics](ui-statistics.md) locate a tree's places with that index
when a search has built it. Otherwise they do not build it: they read the
decompressed file once for the rows named like a part of one of the tree's
place labels, or bearing one of those parts as a code (a few thousand), so a
hamlet can be located by its municipality; they locate the places among them
exactly as the whole index would,
and drop them, in about a fifth of a second in a release build, so a
session that only opens the statistics keeps no dictionary in memory.

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

## 5. Germany

| Source | Provides | Licence |
|--------|----------|---------|
| Destatis GV100AD[^destatis-gv100], every annual edition since 1993-12-31 and the latest monthly one | Every municipality listed since 1993, with its Kreis and Land in each edition | Free reuse with source attribution (Datenlizenz Deutschland – Namensnennung) |
| Wikidata | Municipalities dissolved before 1994, and the successor of many dissolved since | CC0 |

- A municipality is one `(key, name)` pair. It is filed under every Kreis it
  was listed in, so Klein Bünzow appears under Anklam, Ostvorpommern and
  Vorpommern-Greifswald, today's being current.
- A pair missing from the latest edition is a `former_name` when its key or
  its name (within the Land) lives on, the latter being a Kreis reform that
  renumbered it, and a `former_commune` otherwise, with the successor
  Wikidata gives for its key, when it gives one.
- *Valid until* is the first edition that no longer lists it: the change
  happened in that year at the latest. *Valid from* is not recorded, the
  editions being yearly.
- "Flensburg, Stadt" is filed as Flensburg: what follows the comma is the kind
  of municipality. Unincorporated areas (forests, lakes) are left out. Some
  editions are in the DOS code page 850, which the generator recognises.
- Not covered: the municipalities of the reforms before 1993 that Wikidata
  does not know, the Prussian provinces and the territories lost in 1945.

## 6. Italy

| Source | Provides | Licence |
|--------|----------|---------|
| ISTAT[^istat-comuni]: current communes | Communes, provinces and regions, with the Italian and other official names ("Bolzano/Bozen") | CC BY 4.0 |
| ISTAT: suppressed communes | Every commune suppressed since 1861 and the commune it went to | CC BY 4.0 |
| ISTAT: variations since 1991 | Renames (CD) and changes of province (AP), with the old code | CC BY 4.0 |
| Wikidata | Coordinates | CC0 |

- A suppressed commune is followed through later suppressions and changes of
  province to the commune holding its land today, and filed under that
  commune's province and region. The communes ceded to Yugoslavia in 1947
  have no Italian successor and are left out.
- A commune that changed province keeps its old code as a `former_name`,
  filed under the old province when that province still exists.

## 7. Spain

| Source | Provides | Licence |
|--------|----------|---------|
| INE[^ine-codmun], newest yearly list | Municipalities with their province and autonomous community | Free reuse with attribution |
| Wikidata | Dissolved municipalities | CC0 |

- INE sorts names by their main word ("Iglesuela del Cid, La"); the generator
  writes them as said ("La Iglesuela del Cid"), in each language of a
  bilingual name.
- Provinces are named as INE names them today, and filed also under their
  names from before a law made the co-official form official:

| Code | Former name | Until |
|------|-------------|-------|
| 01 | Álava | 2011-07-06 |
| 07 | Baleares | 1997-04-26 |
| 15 | La Coruña | 1998-03-05 |
| 17 | Gerona | 1992-03-01 |
| 20 | Guipúzcoa | 2011-07-06 |
| 25 | Lérida | 1992-03-01 |
| 32 | Orense | 1998-03-05 |
| 48 | Vizcaya | 2011-07-06 |

- Not covered: the municipalities merged since 1842 that Wikidata does not
  know (INE's history of them is published only as a book).

## 8. Switzerland

| Source | Provides | Licence |
|--------|----------|---------|
| BFS commune register[^bfs-agv] | Every commune since 1960, with its canton, its number, its dates, and the mutation that ended it | Open use with source attribution |
| Wikidata | Former communes dissolved before 1960, mostly undated | CC0 |

- The register's snapshot on 1960-01-01 and on every mutation day since hold
  every record. A record that only moved district, keeping its name and
  number, repeats the live one and is left out; a rename or a renumbering
  (the Jura communes in 1979) is a `former_name`, a merger a
  `former_commune`, even when the merged commune kept one of the old names.
- Districts are not filed: records cite the commune and its canton.

## 9. Poland

| Source | Provides | Licence |
|--------|----------|---------|
| TERYT TERC[^gus-teryt], official file | Gminy with their powiat and voivodeship | Free reuse with attribution |
| Wikidata | Coordinates | CC0 |

- The towns are the urban gminy and the towns of urban-rural gminy. A rural
  gmina is named after its seat, filed as that place unless a town of that
  name already stands in the powiat. City districts are left out.
- The register's download page is a form: the generator reads its view state
  and posts it back, as its button would.
- Not covered: villages (TERYT SIMC), and places under their German or
  historical names.

## 10. United States

| Source | Provides | Licence |
|--------|----------|---------|
| Census gazetteer[^census-gazetteer], newest year | Incorporated places, census-designated places, county subdivisions, counties, with coordinates | Public domain |
| Census national place-by-county file (2020) | The counties each place lies in | Public domain |

- A place spanning several counties is filed under each of them.
- New England governs by town, and its towns are county subdivisions, not
  places: they are included, and a census-designated place of the same name
  in the same county is left out as the town's centre.
- The legal description ends the Census name ("Abbeville city") and is
  dropped.
- Not covered: townships outside New England, and places that no longer
  exist.

## 11. Portugal

| Source | Provides | Licence |
|--------|----------|---------|
| DGT, CAOP OGC API[^caop] | Mainland municipalities and parishes (freguesias) with their district | Licence of the CAOP (open, attribution) |
| Wikidata | Municipalities and parishes of the Azores and Madeira, the parishes merged in 2013, coordinates | CC0 |

- A parish is filed under its municipality and district, a municipality
  under its district; the autonomous regions stand in for districts.
- The parishes merged in 2013 are filed under the municipality their code
  names, pointing at the union parish that holds them when Wikidata says
  which, at the municipality otherwise.

## 12. Belgium

| Source | Provides | Licence |
|--------|----------|---------|
| Wikidata | Municipalities with their NIS code, those merged in 2019 and 2025, and the sections: the communes of before the fusions of 1977 | CC0 |

- Statbel's code list sits behind a bot challenge a generator cannot pass;
  Wikidata carries the same NIS codes, from which province and region
  follow.
- Names are in the municipality's language: Dutch in Flanders, French in
  Wallonia, German in the nine German-speaking municipalities, both in
  Brussels ("Ixelles/Elsene").
- The municipalities of Brussels, Flemish Brabant and Walloon Brabant are
  also filed under the province of Brabant, split in 1995.

## 13. Luxembourg

| Source | Provides | Licence |
|--------|----------|---------|
| Wikidata | Communes with their LAU code and canton, and the communes merged since 1978 | CC0 |

- Communes are filed under their canton, "Canton de Redange" as Redange.

## 14. Netherlands

| Source | Provides | Licence |
|--------|----------|---------|
| CBS[^cbs-gemeenten], newest yearly list | Municipalities with their code and province | CC BY 4.0 |
| Wikidata | Municipalities merged since 1812 | CC0 |

- A former municipality is filed under the province of the municipality
  that absorbed it.

[^geneanet-dico]: Geneanet, `dico_place_fr.csv`, five quoted columns: place, INSEE code, département, region, country.
[^insee-cog]: INSEE publishes a COG vintage each year; `v_commune_depuis_1943.csv` and `v_mvt_commune_<year>.csv` carry the history.
[^ons-ipn]: The ONS stamps the edition year into column names (`place23nm`); the generator finds columns whatever the year.
[^destatis-gv100]: Fixed-width records; the eight digits of the Gemeindeschlüssel come first, followed in later editions by the four of the Gemeindeverband.
[^istat-comuni]: `Elenco-comuni-italiani.csv`, `Elenco-comuni-soppressi.zip` and `Variazioni-amministrative-e-territoriali-dal-1991.zip`.
[^ine-codmun]: `diccionarioYY.xlsx`, one per year.
[^bfs-agv]: The register's API: `/api/communes/snapshot` and `/api/communes/mutations`.
[^gus-teryt]: The TERC file, in its official version.
[^census-gazetteer]: `YYYY_Gaz_place_national.zip`, `_cousubs_` and `_counties_`; before 2025 tab-separated, since then bar-separated.
[^caop]: `/collections/municipios` and `/collections/freguesias`, paged without geometry.
[^cbs-gemeenten]: `gemeenten-alfabetisch-YYYY.xlsx`, the sheet headed "Gemeentecode".
