---
type: "UI Specification"
title: "Visual & Functional Specifications — Tools"
description: "Tree tools page in tabs, one tool each: the anomalies of dates, filiations, unions, witnesses, records and places with their catalogue, the places the statistics cannot locate, the completeness of the ancestry from the SOSA root, the potential duplicates to merge or keep apart, a converter of dates between calendars, and dates written out in every language and in Latin and read back."
tags: [oxidgene, specification, ui, tools]
generated: { by: human:maintainer, at: 2026-10-30T20:30:00Z }
---

# Visual & Functional Specifications — Tools

> Part of the [OxidGene Specifications](index.md).
> See also: [Statistics](ui-statistics.md) · [Common UI](ui-common.md) · [Person Edit Modal](ui-person-edit-modal.md) (date input) · [Cross-cutting Rules](cross-cutting.md)

---

## 1. Overview

The Tools page (`/trees/{id}/tools`) gathers what the author of a tree uses
to check it and to work on it, rather than to read it.

It is reached from the **wrench icon** of the shared left icon sidebar
(`TreeIconSidebar`), right after the Statistics chart icon and before the
Gear, so it opens the same way from the pedigree canvas, a profile, and every
other tree page. Like [Statistics](ui-statistics.md), it uses the `sub-page`
layout with the shared left icon sidebar, whose wrench button shows as
current here.

## 2. Layout

```
+----------------------------------------------------------------------+
| [logo] tree / Tools                                                   |
+----------------------------------------------------------------------+
| Anomalies | Places | Ancestry | Duplicates | Converter | In words     |
+----------------------------------------------------------------------+
|  Tool title                                                           |
|  What the tool does, in a sentence or two                             |
|  [the tool]                                                           |
+----------------------------------------------------------------------+
```

The page is split into **tabs**, one per tool, so the tools are never all on
screen together:

| Tab | Tool |
|-----|------|
| Anomalies | Impossible or unlikely dates, filiations, unions, witnesses and records, by rule (§3) |
| Places not located | The places the statistics cannot locate, with their uses and a way to correct them (§4) |
| Ancestry completeness | Generation by generation from the SOSA root, who is known and which key facts are recorded (§5) |
| Potential duplicates | Pairs of records that may be one person, to merge or keep apart (§6) |
| Date converter | A date in one calendar, read in every other (§7) |
| Date in words | A date written out in every language and in Latin, and a written date read back (§8) |

The tabs are the [Statistics](ui-statistics.md) strip: at phone width
(640px and below) they scroll sideways instead of shrinking. The tab shown
is kept per viewer in local storage; the first visit opens the first tab.

Each tab is mounted only while it is shown, and asks the server for its data
when it is opened: opening one tab never loads another's data. A tool that
needs no data from the server, such as the date converter, asks for none.
While the browser has not yet said which tab the viewer left the page on,
no tab is shown.

Each tab opens with the tool's title and a sentence saying what it does.

The page prints through the print action of
[Common UI §7](ui-common.md#7-printing). A printout holds the tab shown,
under its title; the other tabs, the tool's inputs and its action buttons do
not print, and a tab that reads a date prints its results without the field.

## 3. Anomalies

Dates, filiations, unions, witnesses, records and places that are
impossible (**errors**) or unlikely (**warnings**), found by the rules of
§3.3 over the whole tree in one request (`GET /trees/{id}/anomalies`,
`treeAnomalies`, [API](api.md)), computed on each request; nothing is
stored. The rules read the person projections and, for what the projections
do not carry:

- the witness links;
- the key events a person holds more than one of, counted by the database
  (a projection keeps one birth, baptism, death and burial);
- the child links of the persons the projections show in several families
  (a projection keeps one family as a child), read only for them;
- the coordinates of the places of a person's events on one day, read only
  for those places, by the rule of §4: their own, else their label's in the
  place dictionary.

### 3.1 Display

- A row of tiles, one per category — dates, filiation, unions, witnesses and
  godparents, records, places — with how many anomalies it holds. A tile
  filters the list to its category; pressed again, it shows them all.
- Under each category, one folded block per rule that found something: its
  severity, its title and its count; opened, a sentence saying what the rule
  checks, then one row per anomaly.
- A row names the persons concerned, the subject first, each linked to their
  profile with a small link centring the pedigree on them; a union's
  anomalies also link to the couple; the places concerned are linked to
  their row in the [Dictionary](ui-dictionary.md), where a place is renamed
  or merged. Beside them, what the rule measured: an age, a gap, a distance,
  a count, the event concerned (the two events a rule compares, joined by
  ↔), the text recorded.
- A click anywhere else on a row opens the profile of its subject, where the
  anomaly is corrected — the Dictionary row of the first place of an
  anomaly naming places alone, the couple of a union's anomaly that names
  nobody. A row takes the keyboard focus, and Enter opens it the same way.
- A rule lists at most 500 anomalies and says how many more it found.
- A **tag** select above the categories limits the anomalies to those naming
  a person who carries the tag, or a place the events of such a person use
  ([Tags §2.2](tags.md#22-anomalies-limited-to-a-tag)),
  with the same choices and memory as the Statistics page's, and the same reading
  of the tag list: read the first time the select takes the focus, or at once
  when a kept tag is restored, and read again on the next focus after a failed
  read, which keeps the tag chosen; the counts follow.
- A tree with none says so, with the number of persons checked.

### 3.2 Reading dates

- The **birth** is the birth, or the baptism when the birth carries no
  date; the **death** the death, or the burial when the death carries none
  ([Architecture invariants](architecture.md)).
- A date stands for every day it may mean: a year alone its whole year, a
  month its whole month. A date about, calculated or estimated widens by the
  approximate date slack each way (§3.3, two years by default). Dates before,
  after, perhaps, or between two dates, say too
  little to compare and are not used.
- **A rule fires only when it holds for every day its dates may stand for**:
  "died before birth" needs the whole death span before the whole birth
  span, an age "over 70" needs the smallest possible age over 70, an age
  "under 11" the largest possible age under 11. A vague date therefore never
  raises an anomaly its vagueness could explain.
- The parents of a child are those of the child's family, and only for a
  biological child or one of unknown kind: adoptive, foster and step parents
  are not held to the age and death rules. A **union** is dated by its first
  marriage or civil union.
- Witnesses and godparents are the event witness links of
  [Data Model](data-model.md) (`event_witness`); a relation is a godfather's
  or a godmother's when, folded (case and accents aside), it is one of:
  *godfather, godf, parrain, pate, taufpate, patenonkel, padrino, padrinho,
  peetvader, peter, ojciec chrzestny, chrzestny* — or *godmother, godm,
  marraine, patin, taufpatin, patentante, madrina, madrinha, peetmoeder,
  meter, matka chrzestna, chrzestna*.
- The **first given name** is the first word of the primary name's given
  names, a hyphenated compound whole (*Jean-Baptiste*). Its sex is the one
  of the given-name sheet the word itself names, by its label or an alias,
  in any language ([API, Reference Content](api.md#reference-content)): each
  sheet says whether men, women or both bear it, *both* whenever the name or
  one of its aliases is given to either sex somewhere (*Camille*, *Andrea*,
  *Jean*, a woman's name in English). A compound no sheet names says nothing
  about its parts.

### 3.3 Catalogue

Thresholds are a tree setting (`tree.anomaly_thresholds`, set in
[Settings](ui-settings.md) §8b with their ranges); the defaults:

| Threshold | Default |
|---|---|
| Longest life | 105 years; 100 years for a birth before 1900 |
| Youngest parent | 11 years |
| Oldest father, oldest mother | 70 years, 55 years |
| Child after the father's death | 300 days (about ten months) |
| Twins, siblings too close | under 11 days; 11 days to 7 months (213 days) |
| Siblings too far apart | 50 years |
| Youngest and oldest spouse at the union | 12 years, 100 years |
| Spouses' birth gap | 50 years |
| Approximate date slack | 2 years each way |
| Recorded age tolerance | 2 years each way |
| Oldest person without a death | 110 years |
| Farthest places of one day | 100 km |

The rules apply the tree's thresholds, and the rules below name the
defaults: `lived_over_105`, `centenarian_before_1900` and `union_over_100`
keep their identifiers whatever the tree sets, while their titles and
sentences on the page quote the tree's values ("Died over 105 years"),
which the anomalies response carries.

The **G** column gives the number of the matching rule of Geneanet's
consistency check, whose whole list the catalogue covers.

| Rule | Category | What it checks | Severity | Status | G |
|---|---|---|---|---|---|
| `death_before_birth` | dates | The death (or burial) before the birth (or baptism) | error | implemented | 2 |
| `burial_before_death` | dates | The burial before the death | error | implemented | |
| `event_before_birth` | dates | Another own event (baptism included, the event standing for the birth excepted) before the birth | error | implemented | 21 |
| `baptism_after_death` | dates | The baptism after the death or the burial | warning | implemented | 22 |
| `event_after_death` | dates | An own event after the death, other than a burial, a cremation, a funeral, a probate, a free-form event (a succession, a mention: its text says what it is) or an LDS ordinance (performed by proxy) | warning | implemented | 24 |
| `burial_not_last` | dates | An own event after the burial, other than those `event_after_death` accepts, and not already after the death | warning | implemented | 23 |
| `lived_over_105` | dates | Died more than 105 years after the birth | warning | implemented | 5 |
| `centenarian_before_1900` | dates | Born before 1900 and died more than 100 years old (not over 105) | warning | implemented | 4 |
| `future_date` | dates | An event after today | error | implemented | |
| `recorded_age_mismatch` | dates | An age a record gives at an event (its own, or a spouse's at a family event), widened by 2 years each way, sharing no day with the age the birth and the event's dates compute; the item gives the computed age and the recorded one | warning | implemented | |
| `parent_born_after_child` | filiation | A parent born after their child | error | implemented | 15 |
| `ancestor_born_after_descendant` | filiation | An ancestor, from the grandparents up, born after a descendant; each ancestor once | error | implemented | 8 |
| `own_ancestor` | filiation | A person among their own ancestors: each loop once, with its persons | error | implemented | 14 |
| `parent_too_young` | filiation | A parent under 11 at a child's birth | warning | implemented | 17 |
| `father_too_old` | filiation | A father over 70 at a child's birth | warning | implemented | 16 |
| `mother_too_old` | filiation | A mother over 55 at a child's birth | warning | implemented | 16 |
| `born_after_mother_death` | filiation | A child born after the mother's death | error | implemented | 12 |
| `born_long_after_father_death` | filiation | A child born more than 300 days after the father's death | error | implemented | 6 |
| `siblings_too_close` | filiation | Consecutive children of a union born 11 days to 7 months apart | warning | implemented | 3 |
| `siblings_far_apart` | filiation | Consecutive children of a union born more than 50 years apart | warning | implemented | 7 |
| `union_before_birth` | unions | A union before a spouse's birth | error | implemented | 11 |
| `union_too_young` | unions | A spouse under 12 at the union | warning | implemented | 20 |
| `union_over_100` | unions | A spouse over 100 at the union | warning | implemented | 13 |
| `union_after_death` | unions | A union after a spouse's death | error | implemented | 10 |
| `spouses_age_gap` | unions | Spouses (the two parents) born more than 50 years apart | warning | implemented | 1 |
| `repeated_union` | unions | Two persons spouses of more than one union together | warning | implemented | 25 |
| `homonymous_spouses` | unions | Several spouses of one person bearing the same name (folded) | warning | implemented | 26 |
| `union_with_parent_or_child` | unions | A person the spouse of their own father or mother | error | implemented | |
| `union_many_spouses` | unions | A union with more than two spouses | warning | implemented | |
| `witness_before_birth` | witnesses | A witness or godparent born after the event | error | implemented | 19 |
| `witness_after_death` | witnesses | A witness or godparent dead before the event | error | implemented | 18 |
| `godparent_sex` | witnesses | A godfather who is a woman, a godmother who is a man (§3.2) | warning | implemented | 9 |
| `spouse_role_sex` | records | A husband (father) who is a woman, a wife (mother) who is a man, adoptive parents included | warning | implemented | 9 |
| `same_role_spouses` | records | Both spouses of a union husbands, or both wives | warning | implemented | |
| `unreadable_date` | records | A date that cannot be read as one (no sort date), shown as typed | warning | implemented | |
| `reversed_range` | records | A date between two dates whose end comes first | warning | implemented | |
| `no_name` | records | A person with neither surname nor given name | warning | implemented | |
| `unique_event_twice` | records | Two births, baptisms, deaths or burials for one person (live events counted by the database); the item gives the type and how many | warning | implemented | |
| `child_of_several_families` | filiation | A biological child (or of unknown kind) of more than one family: the child, then the spouses of each family, and how many families | warning | implemented | |
| `qualifier_contradiction` | dates | Two of the birth, baptism, death and burial out of the order of a life (in that order) for every day their dates allow, one of them before, after, between two dates or one of two: born after 1850, baptised before 1840. *Before X* allows up to the end of X, *after X* from its start; pairs of other dates are the rules above | warning | implemented | |
| `alive_too_old` | dates | No death, burial or cremation recorded (dated or not), born more than 110 years before today | warning | implemented | |
| `union_with_sibling` | unions | Spouses children of one family, or sharing a father or a mother, as biological children (or of unknown kind) | warning | implemented | |
| `given_name_sex` | records | A first given name the reference sheets give to the other sex only (§3.2) | warning | implemented | |
| `same_name_living_siblings` | filiation | Two children of a union with the same given names (folded), the elder known to be alive at the younger's birth: their death, or an event of their life other than one `event_after_death` accepts, wholly after it | warning | implemented | |
| `place_spellings` | places | Places of the events of the tree whose labels, folded (case, accents, punctuation aside), are equal, or one the other with parts missing, the first part (the locality) the same: `Lyon`, `LYON`, `Lyon, Rhône`; each group once | warning | implemented | |
| `distant_places_same_day` | places | Two events of a person (theirs or their union's) dated to one complete day, exact, in places more than 100 km apart (great-circle distance); the farthest pair of each day, the distance in km | warning | implemented | |
| `unlocated_place` | places | A used place the statistics cannot locate | warning | implemented (§4) | |
| Civil-record comparison | — | Entries compared with transcriptions of civil records | — | out of scope | 27 |

Where the defaults differ from Geneanet's, it is on purpose:

- **G6** compares years (the father's death year before the birth year less
  one); `born_long_after_father_death` counts 300 days, which gives the same
  answer for dates known to the year and a finer one for complete dates.
- **G8 and G15** are split: the parent rule for a parent, the ancestor rule
  from the grandparents up, so one wrong date is not reported once per
  generation.
- **G9** reads godparents from the witness relations (§3.2); for adoptive
  parents, whose roles are the union's husband and wife, it is
  `spouse_role_sex`, which checks every union.
- **G23 and G24** also let a cremation, a funeral and a probate follow the
  death, as they do in the records.
- **G27**, the comparison with online civil-record transcriptions, needs a
  database of those records the application does not have; it could come
  later against the user's own sources and transcripts.

Choices the rules beyond Geneanet's list make:

- `alive_too_old` stops at 110 years: a few persons live past 105, as
  `lived_over_105` allows for, almost none past 110.
- `distant_places_same_day` stops at 100 km, more than a day's travel on
  foot or on horseback before the railway; nearer events, a birth at a
  hamlet and a baptism at the parish church, are not reported. Places
  without coordinates, their own or their label's, are not compared.
- `same_name_living_siblings` needs the elder known alive: a child was
  usually named after an elder sibling who had died, and an elder of whom
  nothing later is recorded may have died before.
- `place_spellings` compares the places as the events of the tree name
  them; a place only media use is not compared.

## 4. Places not located

The places used in the tree (by an event or a media) that the
[Statistics](ui-statistics.md) map cannot locate, by the very rule of
[Statistics §4.1](ui-statistics.md): a place is located by its own
coordinates, else by its label in the [place dictionary](place-dictionary.md),
a hamlet or lieu-dit by the municipality its label names after it; the others
are listed here (`GET /trees/{id}/unlocated-places`,
`unlocatedPlaces`, [API](api.md)), most used first, with their count, the
same number the statistics report as "N places could not be located".

- **Who uses it** unfolds, on a row of its own under the place, the persons whose events or media name the place (a couple's event its spouses, a media whoever it is linked to or shows),
  as the [Dictionary](ui-dictionary.md) lists them, each opening the pedigree
  on them.
- **Correct** turns the name into the shared place field of
  [Common UI §4.4](ui-common.md), with the dictionary's suggestions. Saving
  renames the place; a dictionary label also brings its coordinates, so the
  place is located from then on and leaves the list. Its events and media
  keep it: the place is renamed, not replaced. The rename is an ordinary
  place update, with its history entry.

## 5. Ancestry completeness

Generation by generation from the tree's SOSA root (set in the
[Settings](ui-settings.md) §7), which ancestors are known and which of their
key facts are recorded. The SOSA numbers are those of the pedigree: 1 is the
root, 2n the father and 2n + 1 the mother of n.

- **Depth**: from 4 to 15 generations, the root's included; 8 by default.
  The choice is kept per viewer in local storage.
- **Summary**: one row per generation with the ancestors found against the
  ancestors expected (2^(n − 1) in generation n), the share as a bar (green
  above 70%, orange from 40%, red below), and how many of those found have a
  birth, a death (or may be alive) and a union.
- **Generations**: below the summary, each generation (open by itself only when it has something to fill: a missing ancestor, listed or only counted, or an ancestor lacking a key fact; a complete generation stays folded)
  lists by SOSA number either the ancestor, a link to their profile with
  their birth and death dates, and a mark per key fact, recorded (green) or
  missing (red, struck through), or "missing ancestor".
- **Key facts**, each recorded when its event exists with a date or a place:
  - *birth*: a birth or a baptism;
  - *death*: a death or a burial; a person with neither, born fewer than 120
    years ago, may be alive: nothing is expected, so no death mark is drawn
    for them at all, and they count as complete (the
    [Statistics](ui-statistics.md) §7 rule);
  - *union*: an event attesting the union of the ancestor with the other
    parent of their child in the line (the family that makes them a parent
    of the ancestor below): a marriage, a civil union, banns, a contract, a
    licence or a settlement, or a separation, a divorce filed, a divorce or
    an annulment. The root has no such union and shows no mark.
- **Missing ancestors**: only the missing parents of known ancestors are
  listed. The ancestors of a missing ancestor are missing too; they are
  counted under each generation ("12 more ancestors are missing because their
  children are too") instead of being listed, so an empty branch costs
  nothing whatever the depth.
- **Pedigree collapse**: an ancestor reached through two lines appears, and
  counts, at each of their SOSA numbers.
- "Show only what is missing" keeps the missing ancestors and those lacking
  a fact.
- A tree without a SOSA root shows why, with a link to the settings where
  one is chosen.

The data comes from `GET /trees/{id}/ancestry-completeness` or the
`ancestryCompleteness` query ([API](api.md)), computed from the person
projections on each request.

## 6. Potential duplicates

Pairs of records of the tree that may be one person, best first
(`GET /trees/{id}/duplicates`, `potentialDuplicates`, [API](api.md)),
computed from the person projections on each request.

### 6.1 Finding the pairs

- Only persons with both a surname and a given name are compared, as for
  [homonyms](api.md). Two records are compared when their surnames and their
  first given names **sound alike**: folded (case and accents aside), letters
  only, `y` read as `i` and `ph` as `f`, doubled letters single, and a final
  `s`, `x`, `z`, `t` or `d` dropped from a word longer than four letters —
  `Martins` meets `Martin`, `Dupond` meets `Dupont`.
- A pair is ruled out when the two are of different known sexes, when their
  births (or baptisms) or their deaths (or burials) are more than 5 years
  apart, when one died before the other's birth year, when one is the
  other's spouse, parent or child (the merge refuses them anyway), when both
  are children of one family born on known different days (the second named
  after the first), or when they have been recorded as different people.
- The **score**, out of 100, adds what the two share:

  | Clue | Points |
  |---|---|
  | The same surname and given names (folded) | 30 |
  | Only a similar name (same sound key) | 15 |
  | The same complete birth date | 30 |
  | Otherwise the same birth year | 20 |
  | Otherwise births at most 5 years apart | 5 |
  | The same birthplace (folded) | 10 |
  | The same death year | 15 |
  | Children of the same family | 20 |
  | Otherwise a father, a mother of the same name | 10 each |
  | A spouse of the same name | 10 |

- A pair is listed from **40 points**: a name alone, however common, is not
  enough; a second clue is. From 70 it reads as *very likely*, from 55
  *likely*, below *possible*. At most 500 pairs are listed, with how many
  there are.

### 6.2 Settling a pair

- Each pair shows both records as the search rows do (name, dates, relatives,
  birthplace), each a link to the profile, with its confidence, its score
  and what the two share.
- **Two different people** records that the two differ, through the same
  distinct-person confirmation as the homonym check of the
  [Person Edit Modal](ui-person-edit-modal.md) §13; the pair leaves the list
  for good.
- **Compare** opens the [merge wizard](ui-merge.md) at its Step 2 on the
  pair: the two records side by side, field by field, the record kept (the
  first by default), and the events and media to take from the other.
  Confirming **merges** them ([Person Merge](ui-merge.md) §6); **Two
  different people** is offered there too, and Cancel leaves the pair listed.
- After either answer the list is computed again.

## 7. Date converter

A date entered in any calendar the application records dates in, read in
all of them: Gregorian, Julian, Hebrew and French Republican.

- The date is entered with the shared date input of the
  [Person Edit Modal](ui-person-edit-modal.md): calendar, qualifier, day,
  month and year, and a second date for "or" and "between". Its own checks
  apply, and a date it refuses converts to nothing.
- One tile per calendar shows the date written as the application writes
  dates in that calendar, qualifier included ("about 21 ventôse 4"). The
  calendar the date was entered in is marked "as entered".
- A partial date keeps its precision: a year alone is read as the year it
  overlaps most, a month as the month, the same rule as when the calendar of
  an entered date is changed in the date input.
- A calendar that cannot express the date — the Republican calendar before
  its first year (22 September 1792), a range one of whose ends it cannot
  express — says "not expressible in this calendar" instead of a date.
- Under the tiles, a single complete date that is neither approximate nor a
  range gives its day of the week.
- Everything is computed in the browser from the calendar arithmetic of
  `oxidgene-core`; nothing is sent to the server, and nothing is stored.

## 8. Date in words

A date written out as registers and deeds wrote it, in every interface
language and in Latin, and the reverse: a written date read back into
numbers. Everything is computed in the browser (`date_words.rs` of
`oxidgene-ui`, pure Rust shared by the web and desktop builds); nothing is
sent to the server, and nothing is stored.

### 8.1 Writing

- The date is entered with the shared date input. A Gregorian or Julian date
  is written in its own calendar. A Hebrew or Republican one is written in
  its own calendar by the language rows (below), and by Latin, the Roman
  reckoning and the feast from the same day in the Gregorian calendar,
  which the page says. The qualifier and the second date of a range are
  left out; a year alone or a month and a year are written as such. Years
  run from 1 to 3999, as far as Roman numerals go (a Hebrew year to 9999).
- **Form**: *all in words* (the default) or *figures and month name*.
- **Year starts on**, for a Gregorian or Julian date, in every output:
  - *1 January* (the default);
  - *25 March*, the Annunciation style many registers kept: a date from
    1 January to 24 March bears the previous year's number;
  - *25 December*, the Nativity style: a date from 25 to 31 December bears
    the next year's number;
  - *Easter*, the French style until 1564 (*mos gallicus*): a date from
    1 January to the day before Easter bears the previous year's number.
    Easter is computed in the date's calendar: by the Julian computus for a
    Julian date, by the Gregorian one for a Gregorian date from 1583, and
    before, when the Church still followed the Julian computus, by that one
    carried into the Gregorian calendar.

  A date whose month or day is unknown where the year turns keeps its
  number.
- **Double dating**, offered with a style other than 1 January: when the
  style numbers the year otherwise than 1 January does, both years are
  given, the style's first, then the other's digits from the tens on, or
  from the hundreds when they differ (*1649/50*, *1699/1700*; *1650/49* in
  the Nativity style). The figures form writes the double year in place of
  the year; the words, Latin's forms included, write the style's year and
  add the double one after the date in parentheses (*… quadragesimo nono
  (1649/50)*); the part-by-part year shows it.
- One row per available locale document, each with the shared copy button.
  Patterns, month cases, day/year word forms and reading vocabulary come from
  that document; personal locales discovered in Language settings also appear
  here. The shipped locales are English, French,
  German, Spanish, Italian, Dutch, Polish and Portuguese, with their own
  number words, ordinals and month cases — for 2 February 1650, *the second
  of February, one thousand six hundred and fifty*; *le deux février mille
  six cent cinquante*; *am zweiten Februar sechzehnhundertfünfzig*; *dos de
  febrero de mil seiscientos cincuenta*; *due febbraio
  milleseicentocinquanta*; *de tweede februari zestienhonderdvijftig*;
  *drugiego lutego tysiąc sześćset pięćdziesiątego roku*; *dois de fevereiro
  de mil seiscentos e cinquenta*. Day 1 is an ordinal where the language
  says so (*le premier*, *primero de*, *primo*, *primeiro de*).
- A **Hebrew or Republican** date is written in each language with the
  `hebrew` and `republican` parts of its locale document: the thirteen
  month names of the calendar, its templates in both forms, the
  complementary days of the Republican year, and for Hebrew the thousands a
  year past 3999 needs. All in words, the day and the year are the
  language's words, a Hebrew year its thousands then the rest (*cinq mille
  six cent dix*); in figures, the Republican year is in Roman numerals as
  the Republic wrote it. For 25 Messidor of year II: *le vingt-cinq
  messidor de l'an deux*, *25 messidor an II*, *the twenty-fifth of
  Messidor, year two*, *am fünfundzwanzigsten Messidor des Jahres zwei*;
  for 15 Nisan 5610: *the fifteenth of Nisan, five thousand six hundred and
  ten*, *piętnastego Nisan pięć tysięcy sześćset dziesiątego roku*. Month
  names keep the spelling of the date input's in each language; the
  Republican ones are the French names, or the language's own (*mesidor*,
  *messidoro*). A locale document without these parts writes the date from
  its Gregorian day.
- **Latin**, the same whatever the interface language:
  - all in words: *die secunda mensis Februarii anno Domini millesimo
    sexcentesimo quinquagesimo* — the day a feminine ordinal, the month in
    the genitive, the year a masculine ordinal, every part of it (*bis
    millesimo* for 2000); days and years from 13 to 19 read *decima
    tertia*, *decimo octavo*;
  - with **classical Latin forms** chosen, the ordinals ending in 8 or 9
    from 18 on count down to the next ten, as classical grammar does:
    *duodevicesima* (18th), *undevicesima* (19th), *duodetricesima* (28th),
    *undequinquagesimo* (49th), *duodecentesimo* (98th) — for the day and
    the last two digits of the year, in the words form and the part-by-part
    breakdown;
  - figures and month name: *II Februarius MDCL*, day and year in Roman
    numerals;
  - the **Roman reckoning** of a complete date, counted inclusively to the
    next Kalends (the 1st), Nones (the 5th, the 7th in March, May, July and
    October) or Ides (eight days after the Nones): *ante diem IV Nonas
    Februarii*, *pridie Idus Martii*, *Kalendis Martii*; in a leap
    February the 24th is *ante diem bis VI Kalendas Martii*. The month is
    named in the genitive, as in the documents this tool was modelled on.
  - **part by part**: the day (2 = *secunda* = II), the month (2 =
    *Februarii* = II), the year (1650 = *millesimo sexcentesimo
    quinquagesimo* = MDCL) and the calendar, with the year style.
- **The feast of the day** of a complete date, in Latin as the Missal
  headed it, from the Roman calendar in the date's own calendar (Gregorian
  or Julian):
  - the movable feasts of the Easter cycle first, from Easter computed as
    above: Ash Wednesday (*Feria IV Cinerum*), Palm Sunday, Maundy Thursday,
    Good Friday, Holy Saturday, Easter Sunday (*Dominica Resurrectionis*)
    and Monday, Low Sunday, the Ascension, Pentecost Sunday and Monday,
    Trinity Sunday and Corpus Christi;
  - else the fixed feast of the day, of highest rank, from a table of 366
    days (`assets/reference/feasts.la.json`, 14 KB, embedded in the
    interface, no network): the General Roman Calendar as it stood under
    Leo XIII, about 1888 — the Tridentine calendar of 1570 with its
    additions, before the 20th-century reforms, which is the one parish
    registers of the 16th–19th centuries followed — taken from the calendar
    tables of the Divinum Officium project, its headings normalised (*S.*,
    *Ss.*, *J* spellings, no accents): *S. Martini Episcopi et Confessoris*,
    *In Nativitate Domini*. Vigils and days within an octave have no feast
    of their own unless a saint is commemorated; 110 days have none and say
    nothing.
  - In a leap year the inserted day is the 24th of February, the
    bissextile day: the feasts from the 24th on move one day later, St
    Matthias to the 25th.
  - Saints' names are data, not interface text: the row is in Latin
    whatever the interface language, its label translated.

### 8.2 Reading

A pasted text fills the date input, or says why it cannot: nothing to read,
no year found, or a day the month does not have.

- Numbers in words in any available locale's JSON vocabulary or in Latin, cardinal or
  ordinal, in any case the tables know (*zweiten*, *zweiter*; *secunda*,
  *secundo*, *quartum*), compounds included (*sechzehnhundertfünfzig*,
  *milleseicentocinquanta*, *tweeëntwintig*, *quatre-vingt-dix*); in
  figures, with *1er*, *2nd* or *2.*; in Roman numerals written in capitals,
  or anywhere in a Roman-reckoning text.
- Month names in every language, the Polish genitives and the Latin
  nominative, genitive, and the forms agreeing with Kalendas and Kalendis
  (*Februarias*, *Februariis*), and GEDCOM's abbreviations (*FEB*).
- The day before or after the month, the year after it; figures alone, day
  first (*2/2/1650*) or year first (*1650-02-02*); a year alone.
- The Roman reckoning: *ante diem IV Nonas Februarias MDCL*, *a.d. IV Non.
  Feb. 1650*, *pridie Idus Martias 1650*, *Kalendis Martii*, the doubled
  leap day; the count before the Kalends of a month is a day of the month
  before, the year being the date's own.
- The classical subtractive ordinals, in every case (*duodevicesima*,
  *undequinquagesimo*, *duodecentesimus*).
- A text read is taken as Gregorian; the date input then converts it to
  another calendar if one is chosen there.

### 8.3 Options

| Option | Status |
|---|---|
| All in words, or figures and month name | implemented |
| Year from 1 January, or from 25 March (Annunciation style) | implemented |
| Latin long and short forms, Roman reckoning, part-by-part breakdown | implemented |
| Reading text in available locale vocabularies and in Latin | implemented |
| Copy of each output | implemented (the shared copy field) |
| Double dating (*1649/50*) for the old styles | implemented |
| Other old styles (25 December, Easter) | implemented |
| The liturgical feast of the day, from a calendar of saints | implemented |
| The classical subtractive forms (*duodevicesima*) | implemented |
| Hebrew and Republican dates written out in every language | implemented |

## 9. i18n and accessibility

Every title, explanation, label and message goes through i18n in every
interface language. The dates written out (§8) are the tool's own output,
not interface text: each language's row is in that language, and Latin in
Latin, whatever the interface language. The tabs are a `tablist` whose buttons say which is
selected.
