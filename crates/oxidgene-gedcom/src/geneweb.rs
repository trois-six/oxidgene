//! GeneWeb `.gw` → OxidGene domain model import.
//!
//! `.gw` is the textual interchange format of the [GeneWeb] genealogy software
//! — what `gwu` writes and `gwc` reads. The [`geneweb`] crate reads it into a
//! lossless syntax tree and converts that tree into `ged_io`'s GEDCOM model, so
//! the whole GEDCOM → domain mapping in [`crate::import`] is reused as-is here.
//!
//! Two things are worth knowing about the format:
//!
//! - A `.gw` file is ISO-8859-1 unless it opts into UTF-8 with an `encoding:`
//!   directive, and the switch takes effect mid-file. The reader therefore
//!   takes raw bytes, never a `String` — decoding upstream would mangle
//!   accented names in every Latin-1 file.
//! - GeneWeb records concepts GEDCOM has no room for (the exact access right,
//!   wizard notes, wiki pages). Those survive the conversion as user-defined
//!   `_GW…` tags, which OxidGene's GEDCOM importer does not model, so they are
//!   dropped here. What GEDCOM can say of them is kept: a person whose access
//!   is restricted (`#apriv`, `#semipub`) is also `RESN confidential`, which
//!   imports as a private person.
//!
//! [GeneWeb]: https://geneweb.tuxfamily.org
//! [`geneweb`]: https://github.com/trois-six/rust-geneweb

use geneweb::database::GwDatabase;
use uuid::Uuid;

use crate::ImportResult;
use crate::import::import_gedcom_data;

/// Import a GeneWeb `.gw` file into OxidGene domain model entities.
///
/// `input` is the raw file content — see the module docs on why this is bytes
/// and not a string. `origin_file` is the file's name, which GeneWeb records on
/// every family and which is echoed back in parse errors.
///
/// Reading is lenient: a malformed block is skipped and reported in
/// [`ImportResult::warnings`] rather than aborting the whole file, so a partly
/// broken export still yields everything it can.
///
/// # Errors
///
/// Returns `Err` if the file yielded no persons at all while reporting parse
/// errors — that is a file that failed to parse, not an empty genealogy.
pub fn import_geneweb(
    input: &[u8],
    origin_file: &str,
    tree_id: Uuid,
) -> Result<ImportResult, String> {
    let (db, errors) = GwDatabase::read_lenient(input, origin_file);

    if db.persons.is_empty() && !errors.is_empty() {
        let detail = errors
            .iter()
            .take(3)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "GeneWeb parse error: no person could be read from {origin_file} ({} error(s)): {detail}",
            errors.len()
        ));
    }

    let mut result = import_gedcom_data(&db.to_gedcom(), tree_id)?;

    // Prepend the reader's own warnings: they point at source lines of the .gw
    // file, which the GEDCOM-level warnings that follow cannot do.
    let mut warnings: Vec<String> = errors
        .iter()
        .map(|e| format!("GeneWeb: {}", e.to_string().replace('\n', " ")))
        .collect();
    warnings.append(&mut result.warnings);
    result.warnings = warnings;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use oxidgene_core::types::Event;
    use oxidgene_core::{ChildType, DateQualifier, EventType, NameType, Privacy};

    use super::*;

    const SAMPLE: &str = "\
encoding: utf-8

fam Doe Jean.0 +1900 #mp Paris Roe Marie.0
beg
- h Pierre.0 1925
end
";

    /// `.gw` goes through `to_gedcom()` and then the shared GEDCOM importer,
    /// so surname particles get split there too — this pins that the GeneWeb
    /// path is not a separate code path that could drift.
    #[test]
    fn splits_surname_particles_via_the_shared_gedcom_path() {
        const WITH_PARTICLE: &str = "\
encoding: utf-8

fam de_la_Cruz Jean.0 +1900 Roe Marie.0
beg
end
";
        let result =
            import_geneweb(WITH_PARTICLE.as_bytes(), "particle.gw", Uuid::now_v7()).unwrap();

        // GeneWeb spells a multi-word surname with underscores; the importer
        // sees "de la Cruz" and files it under its root.
        let cruz = result
            .person_names
            .iter()
            .find(|n| n.surname.as_deref() == Some("Cruz"))
            .unwrap_or_else(|| {
                panic!(
                    "expected a surname root of Cruz, got {:?}",
                    result
                        .person_names
                        .iter()
                        .map(|n| (n.surname.clone(), n.surname_prefix.clone()))
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(cruz.surname_prefix.as_deref(), Some("de la"));
        assert_eq!(cruz.full_surname().as_deref(), Some("de la Cruz"));
    }

    #[test]
    fn imports_a_minimal_family() {
        let tree_id = Uuid::now_v7();
        let result = import_geneweb(SAMPLE.as_bytes(), "sample.gw", tree_id).unwrap();

        assert_eq!(result.persons.len(), 3);
        assert_eq!(result.families.len(), 1);
        assert_eq!(result.family_spouses.len(), 2);
        assert_eq!(result.family_children.len(), 1);
        assert!(result.persons.iter().all(|p| p.tree_id == tree_id));

        let surnames: Vec<_> = result
            .person_names
            .iter()
            .filter_map(|n| n.surname.clone())
            .collect();
        assert!(surnames.iter().all(|s| s == "Doe" || s == "Roe"));
    }

    #[test]
    fn decodes_iso_8859_1_by_default() {
        // "Émile" as ISO-8859-1: É is 0xC9. Without a `encoding:` directive the
        // reader must treat the byte as Latin-1, not as invalid UTF-8.
        let mut input = Vec::new();
        input.extend_from_slice(b"fam Doe \xC9mile.0 + Roe Marie.0\n");

        let result = import_geneweb(&input, "latin1.gw", Uuid::now_v7()).unwrap();
        assert!(
            result
                .person_names
                .iter()
                .any(|n| n.given_names.as_deref() == Some("Émile")),
            "expected the Latin-1 É to decode, got {:?}",
            result.person_names
        );
    }

    fn import(gw: &str) -> ImportResult {
        import_geneweb(gw.as_bytes(), "t.gw", Uuid::now_v7()).expect("the .gw imports")
    }

    /// The person whose primary name has these given names.
    fn person(result: &ImportResult, given: &str) -> Uuid {
        result
            .person_names
            .iter()
            .find(|n| n.is_primary && n.given_names.as_deref() == Some(given))
            .unwrap_or_else(|| panic!("no person named {given}"))
            .person_id
    }

    fn event_of(result: &ImportResult, owner: Uuid, event_type: EventType) -> &Event {
        result
            .events
            .iter()
            .find(|e| {
                e.event_type == event_type
                    && (e.person_id == Some(owner) || e.family_id == Some(owner))
            })
            .unwrap_or_else(|| panic!("no {event_type:?} event"))
    }

    /// The witnesses of an event, in order, as (person, relation).
    fn witnesses_of(result: &ImportResult, event_id: Uuid) -> Vec<(Uuid, Option<&str>)> {
        let mut rows: Vec<_> = result
            .event_witnesses
            .iter()
            .filter(|w| w.event_id == event_id)
            .collect();
        rows.sort_by_key(|w| w.sort_order);
        rows.iter()
            .map(|w| (w.person_id, w.relation.as_deref()))
            .collect()
    }

    #[test]
    fn a_pevt_witness_is_recorded_once_on_its_own_event() {
        let result = import(concat!(
            "fam Doe John 1900 + Roe Jane\n",
            "pevt Doe John\n#deat 1970\nwit m: Poe Paul\nend pevt\n",
            "fam Poe Paul + Moe Mary\n",
        ));
        let john = person(&result, "John");
        let death = event_of(&result, john, EventType::Death);

        // On the death it attended, and on nothing else: not on the birth an
        // individual-level copy of the witness used to be attached to.
        assert_eq!(result.event_witnesses.len(), 1);
        assert_eq!(
            witnesses_of(&result, death.id),
            [(person(&result, "Paul"), Some("Witness"))]
        );
    }

    #[test]
    fn witnesses_mentioned_only_on_their_event_are_imported() {
        let result = import(concat!(
            "fam Doe John + Roe Jane\n",
            "fevt\n#marr 1925\nwit m: Poe Paul\nend fevt\n",
            "pevt Doe John\n#deat 1970\nwit m: Moe Mark 1850\nend pevt\n",
        ));
        // Paul and Mark appear nowhere but as witnesses.
        assert_eq!(result.persons.len(), 4);

        let family = result.families[0].id;
        let marriage = event_of(&result, family, EventType::Marriage);
        assert_eq!(
            witnesses_of(&result, marriage.id),
            [(person(&result, "Paul"), Some("Witness"))]
        );

        let mark = person(&result, "Mark");
        let death = event_of(&result, person(&result, "John"), EventType::Death);
        assert_eq!(witnesses_of(&result, death.id), [(mark, Some("Witness"))]);
        // The witness defined inline keeps what the definition said.
        let birth = event_of(&result, mark, EventType::Birth);
        assert_eq!(birth.date_value.as_deref(), Some("1850"));
    }

    #[test]
    fn witnesses_of_a_union_without_details_keep_their_marriage() {
        let result = import(concat!(
            "fam Doe John + Roe Jane\nwit m: Poe Paul\n",
            "fam Poe Paul + Moe Mary\n",
        ));
        let family = result.families[0].id;
        let marriage = event_of(&result, family, EventType::Marriage);
        assert_eq!(marriage.date_value, None);
        assert_eq!(
            witnesses_of(&result, marriage.id),
            [(person(&result, "Paul"), Some("Witness"))]
        );
    }

    #[test]
    fn the_death_reason_stays_on_the_death_event() {
        let result = import(concat!(
            "fam Doe John k1900 + Roe Jane\n",
            "pevt Doe John\n#deat 1900\nend pevt\n",
        ));
        let john = person(&result, "John");
        let deaths: Vec<_> = result
            .events
            .iter()
            .filter(|e| e.person_id == Some(john) && e.event_type == EventType::Death)
            .collect();
        // The `pevt` death replaces the line's, and keeps how the person died.
        assert_eq!(deaths.len(), 1);
        let note = result
            .notes
            .iter()
            .find(|n| n.event_id == Some(deaths[0].id))
            .expect("the death reason");
        assert_eq!(note.text, "_GWDEATH killed");
    }

    #[test]
    fn a_title_keeps_its_domain_rank_period_and_holder() {
        let result = import(concat!(
            "fam Doe John [Samplename:Count:Sampleshire:1800:1810:2] ",
            "[:Baron:Sampleton:::] + Roe Jane\n",
        ));
        let mut titles: Vec<&Event> = result
            .events
            .iter()
            .filter(|e| e.event_type == EventType::NobilityTitle)
            .collect();
        titles.sort_by_key(|e| e.description.clone());
        assert_eq!(titles.len(), 2);
        // The domain is part of the title, never a place.
        assert!(result.places.is_empty());

        let baron = titles[0];
        assert_eq!(baron.description.as_deref(), Some("Baron, Sampleton"));
        assert_eq!(baron.date_value, None);

        let count = titles[1];
        assert_eq!(count.description.as_deref(), Some("Count, Sampleshire, 2"));
        assert_eq!(count.place_id, None);
        // Both ends of the period.
        assert_eq!(count.date_qualifier, DateQualifier::Between);
        assert_eq!(count.date_value.as_deref(), Some("1800"));
        assert_eq!(count.date_value2.as_deref(), Some("1810"));
        // The name the title is held under.
        let holder = result.notes.iter().find(|n| n.event_id == Some(count.id));
        assert_eq!(holder.map(|n| n.text.as_str()), Some("Samplename"));
    }

    /// How a child belongs to the family a spouse heads.
    fn child_type_in(result: &ImportResult, child: Uuid, spouse: Uuid) -> ChildType {
        let family = result
            .family_spouses
            .iter()
            .find(|s| s.person_id == spouse)
            .expect("a family of that spouse")
            .family_id;
        result
            .family_children
            .iter()
            .find(|c| c.family_id == family && c.person_id == child)
            .expect("the child in that family")
            .child_type
    }

    #[test]
    fn adoptive_parents_become_a_family_adopting_the_child() {
        let result = import(concat!(
            "fam Doe John + Roe Jane\nbeg\n- h Paul 1930\nend\n",
            "rel Doe Paul\nbeg\n- adop fath: Poe Peter\nend\n",
        ));
        assert_eq!(result.families.len(), 2);
        let paul = person(&result, "Paul");
        let (john, peter) = (person(&result, "John"), person(&result, "Peter"));
        assert_eq!(child_type_in(&result, paul, john), ChildType::Biological);
        assert_eq!(child_type_in(&result, paul, peter), ChildType::Adopted);
        // The adoption is an event of the child's, and no witness of anything.
        event_of(&result, paul, EventType::Adoption);
        assert!(result.event_witnesses.is_empty());
    }

    #[test]
    fn foster_parents_become_a_family_fostering_the_child() {
        let result = import(concat!(
            "fam Doe John + Roe Jane\nbeg\n- h Paul 1930\nend\n",
            "rel Doe Paul\nbeg\n- fost: Poe Peter + Poe Mary\nend\n",
        ));
        let paul = person(&result, "Paul");
        assert_eq!(
            child_type_in(&result, paul, person(&result, "Mary")),
            ChildType::Foster
        );
        assert!(
            !result
                .events
                .iter()
                .any(|e| e.event_type == EventType::Adoption)
        );
        assert!(result.event_witnesses.is_empty());
    }

    #[test]
    fn godparents_are_witnesses_of_the_birth_as_godfather_and_godmother() {
        let result = import(concat!(
            "fam Doe John + Roe Jane\nbeg\n- h Paul 1930\nend\n",
            "rel Doe Paul\nbeg\n- godp: Poe Peter + Poe Mary\nend\n",
        ));
        let birth = event_of(&result, person(&result, "Paul"), EventType::Birth);
        assert_eq!(
            witnesses_of(&result, birth.id),
            [
                (person(&result, "Peter"), Some("GODF")),
                (person(&result, "Mary"), Some("GODM")),
            ]
        );
    }

    #[test]
    fn every_nickname_is_kept_the_extra_ones_as_bynames() {
        let result = import("fam Doe John #nick Johnny #nick Jacko + Roe Jane\n");
        let john = person(&result, "John");
        let names: Vec<_> = result
            .person_names
            .iter()
            .filter(|n| n.person_id == john)
            .collect();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0].nickname.as_deref(), Some("Johnny"));
        // The second restated John's name only to carry a nickname: it is a
        // byname, written as the person form writes one.
        let byname = names[1];
        assert_eq!(byname.name_type, NameType::Byname);
        assert_eq!(byname.nickname.as_deref(), Some("Jacko"));
        assert_eq!(
            (&byname.given_names, &byname.surname, &byname.surname_prefix),
            (&None, &None, &None)
        );
    }

    #[test]
    fn a_malformed_block_is_one_warning_and_its_body_no_records() {
        let result = import(concat!(
            "notes Doe John extra\nbeg\n",
            "fam Fake Line + Should_Not Exist\n",
            "end notes\n",
            "fam Doe John + Roe Jane\n",
        ));
        assert_eq!(result.families.len(), 1);
        assert_eq!(result.persons.len(), 2);
        let reader_warnings = result
            .warnings
            .iter()
            .filter(|w| w.starts_with("GeneWeb: "))
            .count();
        assert_eq!(reader_warnings, 1, "{:?}", result.warnings);
    }

    #[test]
    fn restricted_access_makes_a_person_private() {
        let result = import(concat!(
            "fam Doe John #apriv + Roe Jane #semipub\n",
            "fam Poe Paul #apubl + Moe Mary\n",
        ));
        let privacy = |given| {
            let id = person(&result, given);
            result.persons.iter().find(|p| p.id == id).unwrap().privacy
        };
        assert_eq!(privacy("John"), Privacy::Private);
        assert_eq!(privacy("Jane"), Privacy::Private);
        assert_eq!(privacy("Paul"), Privacy::Default);
        assert_eq!(privacy("Mary"), Privacy::Default);
    }

    #[test]
    fn a_text_date_is_kept_verbatim() {
        let result = import("fam Doe John 0(vers_la_Saint-Jean) + Roe Jane\n");
        let birth = event_of(&result, person(&result, "John"), EventType::Birth);
        assert_eq!(birth.date_value.as_deref(), Some("(vers la Saint-Jean)"));
        assert_eq!(birth.date_sort, None);
    }

    #[test]
    fn rejects_a_file_that_yields_nothing() {
        let err = import_geneweb(b"this is not a gw file at all\n", "junk.gw", Uuid::now_v7())
            .unwrap_err();
        assert!(err.contains("GeneWeb parse error"), "got: {err}");
    }
}
