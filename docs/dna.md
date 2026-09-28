---
type: "Product Specification"
title: "DNA Kits"
description: "Planned import of consumer DNA raw data attached to persons, processed locally only: supported kit formats, haplogroups, matching between the kits of one tree cross-checked with its kinship paths, continental ancestry estimates, storage and encryption, consent and deletion, reference data and licences, API, and delivery phases."
tags: [oxidgene, specification, dna, genetics, privacy, api, ui]
generated: { by: claude-code/claude-opus-5-5, at: 2026-09-28T20:16:00Z }
sources:
  - id: snps
    title: "snps — Python library reading every consumer raw-data format (BSD-3-Clause)"
    url: "https://github.com/apriha/snps"
  - id: ftdna-download
    title: "FamilyTreeDNA help — Downloading your Family Finder data"
    url: "https://help.familytreedna.com/hc/en-us/articles/14860944283407-Downloading-Your-Family-Finder-Data"
  - id: ftdna-ystr
    title: "FamilyTreeDNA help — Downloading your Y-DNA STR results"
    url: "https://help.familytreedna.com/hc/en-us/articles/4476943795599-Downloading-Your-Y-DNA-STR-Results"
  - id: myheritage-raw
    title: "MyHeritage help — How should I interpret my raw DNA data"
    url: "https://www.myheritage.com/help/en/articles/12852246-how-should-i-interpret-my-raw-dna-data"
  - id: kessler-2018
    title: "L. Kessler, Behold Genealogy blog, SNP counts and overlap of five vendors' raw data (2018-08-31)"
    url: "https://www.beholdgenealogy.com/blog/?p=2700"
  - id: isogg-chips
    title: "ISOGG Wiki — Autosomal SNP comparison chart"
    url: "https://isogg.org/wiki/Autosomal_SNP_comparison_chart"
  - id: noodles
    title: "noodles — Rust bioinformatics I/O (VCF, BAM, CRAM), MIT"
    url: "https://github.com/zaeleus/noodles"
  - id: beagle-maps
    title: "Beagle genetic maps (HapMap, PLINK format)"
    url: "https://bochet.gcc.biostat.washington.edu/beagle/genetic_maps/"
  - id: shared-cm
    title: "The Shared cM Project, version 4.0 (March 2020), CC BY 4.0"
    url: "https://thegeneticgenealogist.com/2020/03/27/version-4-0-march-2020-update-to-the-shared-cm-project/"
  - id: yfull-ytree
    title: "YFull YTree (JSON), CC BY 4.0"
    url: "https://github.com/YFullTeam/YTree"
  - id: phylotree
    title: "PhyloTree mtDNA tree, Build 17 (2016)"
    url: "https://www.phylotree.org/"
  - id: haplogrep3
    title: "haplogrep3, MIT"
    url: "https://github.com/genepi/haplogrep3"
  - id: haplogrep-trees
    title: "haplogrep trees and their licences"
    url: "https://genepi.github.io/haplogrep-trees/"
  - id: hgdp-1kg
    title: "gnomAD HGDP + 1000 Genomes callset, CC0"
    url: "https://github.com/atgu/hgdp_tgp"
  - id: admixture
    title: "ADMIXTURE download page (closed source)"
    url: "https://dalexander.github.io/admixture/download.html"
  - id: isogg-admixture
    title: "ISOGG Wiki — Admixture analyses (calculator terms)"
    url: "https://isogg.org/wiki/Admixture_analyses"
  - id: gdpr-art9
    title: "GDPR Article 9 — special categories of personal data"
    url: "https://gdpr-info.eu/art-9-gdpr/"
  - id: gdpr-art2
    title: "GDPR Article 2 — material scope (household exemption)"
    url: "https://gdpr-info.eu/art-2-gdpr/"
  - id: fr-226-28-1
    title: "French Penal Code, article 226-28-1"
    url: "https://www.legifrance.gouv.fr/codes/article_lc/LEGIARTI000024324189"
  - id: rfg-2026
    title: "Revue française de généalogie — genealogical DNA still forbidden to the French (2026-07-16)"
    url: "https://www.rfgenealogie.com/infos/la-france-ouvre-la-porte-a-l-adn-genealogique-interdit-aux-francais"
  - id: opensnp
    title: "TechCrunch — openSNP shuts down and deletes its data (2025-04-01)"
    url: "https://techcrunch.com/2025/04/01/genetic-sharing-site-opensnp-to-shut-down-citing-concerns-of-data-privacy-and-rise-in-authoritarian-governments/"
---

# DNA Kits

> Part of the [OxidGene Specifications](index.md).
> See also: [Data Model](data-model.md) · [API Contract](api.md) ·
> [Kinship](ui-kinship.md) · [Cross-cutting Rules](cross-cutting.md) ·
> [Roadmap](roadmap.md)

**Status: planned, nothing is implemented.** This document fixes the
contract an implementation will follow; [Roadmap](roadmap.md) tracks it.

---

## 1. Principles

- **Local only.** Genetic data is read, stored and analysed by the backend the
  user runs. It is never sent to a third party, never to a model provider
  ([AI Features](ai.md)), never to an MCP client ([Assistant Access](mcp.md)).
  The only network use is downloading public reference data.
- **The tested person consents.** A kit belongs to a living relative more
  often than to the tree's owner; importing it records that person's consent
  (§6).
- **Deletion is real.** Deleting a kit erases it, unlike every other record,
  which is soft-deleted (§5.3).
- **Within the tree only.** OxidGene compares the kits the user imports with
  each other. It has no database of other testers and will never offer
  matches with strangers (§8).
- **Honest estimates.** Every inferred result (relationship, haplogroup,
  ancestry) states its uncertainty and its method.
- **Desktop first.** The feature is available in the desktop build. A web
  deployment enables it only through an operator setting, off by default
  (§6.3).

---

## 2. Kits and formats

### 2.1 What each test gives

| Test | What it tells a genealogist | Supported |
|---|---|---|
| Autosomal chip, ~600–700k SNPs | Relatives on every line, to about 4th–6th cousins; coarse ancestry | Phase 1 import, phase 3 matching, phase 4 ancestry |
| X-DNA (on the same chip) | Excludes lines: a father passes no X to his son | Phase 3, from the chip data |
| Y-STR (37 / 111 markers) | Direct paternal line; distance between men of one surname | Phases 1–2 |
| Y-SNP (chip, Big Y) | Paternal haplogroup | Phase 2 |
| mtDNA (chip, HVR, full sequence) | Maternal haplogroup | Phase 2 |
| Whole genome, VCF | All of the above, reduced to the chip positions | Later |
| Whole genome, BAM / CRAM (30–100 GB) | — | Out of scope |

### 2.2 Raw-data formats

Every chip vendor exports text on the GRCh37 build[^snps]:

| Vendor | Layout | No-call |
|---|---|---|
| 23andMe, LivingDNA | Tab-separated `rsid chromosome position genotype`, `#` comment header | `--` |
| AncestryDNA | Tab-separated `rsid chromosome position allele1 allele2`; chromosomes 23 = X, 24 = Y, 25 = PAR, 26 = MT | `0` |
| FamilyTreeDNA, MyHeritage | Quoted CSV `RSID,CHROMOSOME,POSITION,RESULT`, delivered `.csv.gz` or `.zip`[^ftdna-download][^myheritage-raw] | `--` |
| FamilyTreeDNA Y-STR | CSV of DYS marker values[^ftdna-ystr] | empty |

- A file is 15–25 MB of text, 5–7 MB compressed. The importer accepts the
  file itself, `.gz` and `.zip`, and detects the vendor, the chip version
  and the build from the header and the columns; an unknown layout is
  refused with `dna_format_unsupported`, never guessed.
- Vendors test different SNPs. In 2018, five vendors' files shared only
  110,231 positions, while 23andMe and LivingDNA shared over 90% of
  theirs[^kessler-2018]; vendors since moved to the Illumina GSA chip overlap
  much more, AncestryDNA remaining apart[^isogg-chips]. Comparisons use only
  the positions both kits hold (§4.2).
- A whole-genome VCF (phase later) is read with a pure-Rust reader such as
  `noodles` (MIT)[^noodles]; a gVCF is required to tell "matches the
  reference" from "not called".

---

## 3. Data model

Planned, to move into [Data Model](data-model.md) when implemented.

### 3.1 `dna_kit`

| Field | Type | Meaning |
|---|---|---|
| `id` | UUID v7 | |
| `tree_id` | UUID v7 | FK → Tree |
| `person_id` | UUID v7 | FK → Person: the tested person |
| `kind` | enum | `autosomal`, `y_str`, `y_snp`, `mtdna`, `wgs_vcf` |
| `vendor` | enum | `ancestry`, `twenty_three_and_me`, `myheritage`, `ftdna`, `living_dna`, `other` |
| `chip` | string? | Detected chip or file version |
| `build` | enum | `grch37`, `grch38` |
| `snp_count` | integer | Called positions |
| `sha256` | string | Of the original file, to refuse the same file twice for one person |
| `storage_key` | string | The encrypted blob in the media store (§5.1) |
| `consent` | JSON | §6.1 |
| `imported_at` | timestamp | |

No `deleted_at`: a kit is erased, not soft-deleted (§5.3).

### 3.2 `dna_match`

One row per pair of autosomal kits of the same tree, recomputed when either
kit changes:

| Field | Type | Meaning |
|---|---|---|
| `kit_a`, `kit_b` | UUID v7 | Ordered pair |
| `shared_cm` | float | Total of the kept segments |
| `longest_cm` | float | |
| `segments` | JSON | Chromosome, start and end positions, cM, SNPs, half or full identity |
| `x_cm` | float? | Shared X, when both kits carry X data |
| `overlap_snps` | integer | Positions both kits hold |
| `algorithm` | string | Algorithm and parameter version, so an upgrade recomputes |

### 3.3 Haplogroups

`dna_haplogroup(kit_id, lineage: y | mt, haplogroup, tree_version,
confidence)`. A haplogroup label may later appear in the person profile; that
would change `PersonProfile` and bump `PROJECTION_SCHEMA_VERSION`.

---

## 4. Analyses

### 4.1 Haplogroups (phase 2)

- Walk the reference tree from its root, scoring each branch's defining
  SNPs as derived, ancestral or missing, and stop where the evidence ends.
  The result states its depth and how many defining SNPs were tested.
- Y tree: YFull YTree, CC BY 4.0[^yfull-ytree]. mtDNA tree: PhyloTree
  Build 17[^phylotree], whose licence must be confirmed (§9); haplogrep3
  (MIT) is the model to follow for the mtDNA scoring[^haplogrep3]. FTDNA's
  Mitotree and 23andMe's yhaplo are non-commercial and cannot be
  used[^haplogrep-trees].
- Chip data gives an intermediate resolution at best, and none for vendors
  that report few Y or mt positions; Big Y and full mtDNA files give full
  resolution. The UI says which.
- Y-STR: genetic distance between two men's marker sets, per marker, with
  the multi-step rule for multi-copy markers documented in code.

### 4.2 Matching between the tree's kits (phase 3)

- **Algorithm.** A half-identical-region scan over the positions both kits
  hold: a segment runs until the two kits are opposite homozygotes (AA
  against BB); regions where they share both alleles are fully identical
  and reveal full siblings. Pure Rust, linear per pair.
- **Thresholds.** A segment is kept at 7 cM and 500 SNPs or more by
  default, raised when the overlap is small (cross-vendor pairs); the
  thresholds are named constants documented here when implemented.
- **Genetic map.** Centimorgans come from a HapMap genetic map on
  GRCh37[^beagle-maps], interpolated to the chip positions; its licence must
  be confirmed before it is embedded (§9).
- **Relationship ranges.** The Shared cM Project v4 (CC BY 4.0, attributed
  in the UI) gives the relationships compatible with a total[^shared-cm].
- **Cross-check with the tree.** For the two tested persons, the
  [Kinship](ui-kinship.md) paths give an expected total: the sum over the
  paths of 2^-(steps up + steps down) × about 6,800 cM (first cousins:
  2 × 1/16 → about 850 cM). The match shows *the tree predicts X cM, the DNA
  shows Y cM* and flags a total outside the range of every relationship the
  tree allows. Several paths (implex) raise the expectation and are shown.
- **X-DNA.** Shared X is reported with the tree paths that can carry X; a
  shared X segment on a path that cannot carry one is flagged.
- **Chromosome browser.** An SVG drawing of the shared segments, chromosome
  by chromosome, for one pair, or for one kit against several to show
  triangulation (A–B, A–C and B–C on the same segment).
- Imputation is out of scope.

### 4.3 Continental ancestry (phase 4)

- A frequency matrix (populations × chip positions) is built offline from
  the gnomAD HGDP + 1000 Genomes callset (CC0)[^hgdp-1kg] by a `just` recipe,
  like the place dictionary, and downloaded on demand rather than embedded.
- The proportions are estimated by the projection EM of the ADMIXTURE model,
  reimplemented; ADMIXTURE itself is closed source[^admixture], and the
  community calculators' files are for non-commercial use only and cannot be
  bundled[^isogg-admixture].
- Results are given at continental level and labelled an *estimate*, with
  the method and the reference panel named. Sub-continental splits
  (neighbouring European countries) are not offered: open panels cannot
  support them.

---

## 5. Storage

### 5.1 Encrypted blobs

- The original file and a compact genotype blob (2 bits per genotype and the
  positions, about 1–1.5 MB before compression) are stored as encrypted
  objects in the media store, not in the database, which holds only the
  metadata and the matches.
- Encryption is authenticated (AEAD) with a RustCrypto crate (MIT/Apache),
  a new dependency justified by the data's category. The key is kept in the
  operating system's keyring on desktop and in a server secret on the web,
  never next to the data.

### 5.2 Out of every other flow

Genotypes and segments never enter: GEDCOM / GEDZIP exports, the MCP tools,
the AI relay, `person_denorm`, `person_search_fts`, logs and traces, and the
change history, which records only that a kit was imported or deleted, for
whom, never its content.

### 5.3 Deletion

- Deleting a kit removes its row, its blobs, its matches and haplogroups,
  in one operation. This is an explicit exception to the soft-deletion
  invariant ([Architecture](architecture.md)).
- Deleting the tested person, or the tree, deletes their kits the same way.
- Revoking consent (§6.1) deletes the kit.
- The user can export a kit's original file before deleting it.

---

## 6. Consent, privacy and law

### 6.1 Consent record

Importing a kit asks who the tested person is (the linked person) and
records, in `consent`: that this person, or their legal representative,
agreed to the import; the date; the scope (storage, matching within this
tree, haplogroups, ancestry); and who entered it. Each scope can be
withdrawn; withdrawing the storage scope deletes the kit.

### 6.2 Wording

- The import dialog explains in plain language that DNA identifies the
  tested person and reveals facts about their relatives, and that the data
  stays on this computer (or this server).
- Results never touch health traits.

### 6.3 Law

- Genetic data is special-category data under GDPR Article 9[^gdpr-art9].
  A family's own use on its desktop falls under the household
  exemption[^gdpr-art2]; an operator running a multi-user server becomes a
  controller of such data. Hence §1: desktop first, and on a server an
  operator switch `dna.enabled`, off by default, whose documentation states
  those obligations.
- In France, soliciting a genetic test outside the legal framework is an
  offence[^fr-226-28-1], still in force in 2026[^rfg-2026]. Importing a file
  is not the offence, but the application never promotes, links to or
  recommends buying a test.
- Public genomic sharing has proven fragile: openSNP deleted all its data in
  2025 over privacy concerns[^opensnp]. OxidGene shares nothing.

---

## 7. API and UI

### 7.1 API

REST and GraphQL are symmetric; the desktop uses REST.

| Method | Path | Description |
|---|---|---|
| `POST` | `/trees/{tree_id}/persons/{person_id}/dna-kits` | Import a raw file (raw body, as the GeneWeb import) with the consent as query or multipart fields; a background job, polled like the file imports |
| `GET` | `/trees/{tree_id}/dna-kits` | Kits of the tree, metadata only |
| `DELETE` | `/trees/{tree_id}/dna-kits/{kit_id}` | Erase (§5.3) |
| `GET` | `/trees/{tree_id}/dna-kits/{kit_id}/file` | Export the original file |
| `GET` | `/trees/{tree_id}/dna-kits/{kit_id}/haplogroups` | §4.1 |
| `GET` | `/trees/{tree_id}/dna-matches?kit_id=` | Matches, with the tree's expectation (§4.2) |
| `GET` | `/trees/{tree_id}/dna-matches/{kit_a}/{kit_b}/segments` | Segments for the chromosome browser |
| `GET` | `/trees/{tree_id}/dna-kits/{kit_id}/ancestry` | §4.3 |

GraphQL takes the file base64-encoded, as its other imports do. Errors add
`dna_format_unsupported`, `dna_build_unsupported`, `dna_duplicate_kit` and
`dna_disabled` to [Cross-cutting Rules](cross-cutting.md) when implemented.

### 7.2 UI

- **Person profile**: a *DNA* section listing the person's kits (vendor,
  kind, date, haplogroups), with *Import*, *Export* and *Delete*.
- **A DNA page** per tree, in the sidebar, in tabs like Statistics: *Kits*,
  *Matches* (pairs with shared cM, the tree's expectation and the flag),
  *Chromosome browser*, *Haplogroups*, *Ancestry*.
- Every label in every interface language; vendor names, rsids and
  haplogroup names are not translated.

---

## 8. Out of scope

- Matching against strangers: it needs a database of millions of testers,
  which only vendors and services such as GEDmatch hold.
- Imputation, BAM/CRAM analysis, health traits.
- Any upload of genetic data anywhere.

---

## 9. Open questions

- The licence of the HapMap genetic map distributed by Beagle: none is
  stated.
- The licence of PhyloTree Build 17: only a citation request is stated.
- Whether a web deployment should offer the feature at all, beyond the
  off-by-default switch.

---

## 10. Dependencies and size

| Part | New crates | Size |
|---|---|---|
| Parsers, blobs, half-identical scan, haplogroup walk, EM | none | small |
| Encryption at rest | one RustCrypto AEAD crate | small |
| Genetic map and Y / mt trees, embedded compressed | none (Brotli exists) | a few MB |
| Ancestry frequency matrix | none; downloaded on demand | 0 in the binary, a few MB on disk |
| VCF reading (later) | `noodles` VCF only | to measure then |

The genotype code lives in a new `oxidgene-dna` crate depending only on
`oxidgene-core`; everything runs in the backend, and `oxidgene-ui` only draws
the results, so the WASM build is unaffected.

---

## 11. Tests

- No real DNA in any test or fixture, not even public genomes. Fixtures are
  synthetic: founders with random genotypes, children produced by
  recombination over the genetic map, which gives each pair's exact shared
  segments and relationship as ground truth.
- Parsers are tested on synthetic files in every vendor layout, compressed
  and not, including malformed ones.
- REST and GraphQL tests cover import, listing, deletion (the blob is gone),
  matches and the flags, and the disabled switch.

---

## 12. Phases

| Phase | Content | Effort |
|---|---|---|
| 1 | Import and attach kits: five chip layouts and Y-STR, detection, encrypted storage, consent, real deletion, export, the person-profile section | 1–2 weeks |
| 2 | Haplogroups (YFull, PhyloTree after its licence check) and Y-STR distance | 1–2 weeks |
| 3 | Matching within the tree: scan, cM, Shared cM ranges, cross-check with kinship, X-DNA, chromosome browser, triangulation | 2–4 weeks |
| 4 | Continental ancestry estimate | 3–6 weeks |

[^snps]: snps, the vendor format readers.
[^ftdna-download]: FamilyTreeDNA help, Family Finder download.
[^myheritage-raw]: MyHeritage help, raw DNA data.
[^ftdna-ystr]: FamilyTreeDNA help, Y-DNA STR download.
[^kessler-2018]: Kessler 2018, five vendors' SNP counts and overlap.
[^isogg-chips]: ISOGG, autosomal SNP comparison chart.
[^noodles]: noodles.
[^beagle-maps]: Beagle genetic maps.
[^shared-cm]: The Shared cM Project v4.0.
[^yfull-ytree]: YFull YTree.
[^phylotree]: PhyloTree Build 17.
[^haplogrep3]: haplogrep3.
[^haplogrep-trees]: haplogrep trees and licences.
[^hgdp-1kg]: gnomAD HGDP + 1KG.
[^admixture]: ADMIXTURE download page.
[^isogg-admixture]: ISOGG, admixture analyses.
[^gdpr-art9]: GDPR Article 9.
[^gdpr-art2]: GDPR Article 2(2)(c).
[^fr-226-28-1]: Code pénal, article 226-28-1.
[^rfg-2026]: Revue française de généalogie, 2026-07-16.
[^opensnp]: TechCrunch, 2025-04-01.
