//! Joins a media manifest onto the persons of a `.gw` export.
//!
//! The two sides meet on the GeneWeb key (see [`crate::key`]). A person in the
//! `.gw` file is identified by position: `GwDatabase::persons[i]` becomes
//! `GedcomData::individuals[i]` with xref `@I{i+1}@`, which the conversion
//! guarantees by construction.

use std::collections::HashMap;

use geneweb::database::GwDatabase;

use crate::key::geneanet_key;
use crate::model::{Manifest, ManifestDeposit, ManifestView};

/// Maps a folded GeneWeb key to the persons that carry it.
pub struct PersonIndex {
    by_key: HashMap<String, Vec<usize>>,
    person_count: usize,
}

impl PersonIndex {
    /// Indexes every person in a `.gw` database by their Geneanet-style key.
    pub fn from_database(database: &GwDatabase) -> Self {
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();

        for (index, person) in database.persons.iter().enumerate() {
            let key = geneanet_key(&person.surname, &person.first_name, person.occ);
            by_key.entry(key).or_default().push(index);
        }

        Self {
            by_key,
            person_count: database.persons.len(),
        }
    }

    pub fn person_count(&self) -> usize {
        self.person_count
    }

    fn lookup(&self, key: &str) -> &[usize] {
        self.by_key.get(key).map_or(&[], Vec::as_slice)
    }
}

/// One media file to attach to one person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Index into `GedcomData::individuals`.
    pub person: usize,
    pub deposit_id: i64,
    pub view_id: i64,
    /// Page number within a multi-page deposit, if any.
    pub page: Option<i64>,
    pub title: Option<String>,
    /// File extension taken from the rendition URLs, e.g. `jpg`.
    pub extension: String,
}

/// A reference that could not be attached, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unjoined {
    pub geneweb_ref: Option<String>,
    pub name: String,
    pub deposit_id: i64,
    pub view_id: i64,
    pub reason: UnjoinedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnjoinedReason {
    /// The reference carried no GeneWeb key at all — a person outside the tree.
    NoKey,
    /// The key is well-formed but matches nobody in the `.gw` file.
    NoSuchPerson,
    /// The key matches more than one person, so attaching would be a guess.
    Ambiguous,
}

impl UnjoinedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoKey => "no GeneWeb key (person outside the tree)",
            Self::NoSuchPerson => "no person with this key in the .gw file",
            Self::Ambiguous => "several persons share this key",
        }
    }
}

/// The outcome of joining a manifest onto a `.gw` file.
#[derive(Debug, Default)]
pub struct Join {
    pub attachments: Vec<Attachment>,
    pub unjoined: Vec<Unjoined>,
    /// Views carrying at least one reference we could attach.
    pub joined_view_count: usize,
    /// Views the manifest holds no reference for at all.
    pub unlinked_view_count: usize,
}

impl Join {
    /// Distinct persons that end up with at least one medium.
    pub fn person_count(&self) -> usize {
        self.attachments
            .iter()
            .map(|a| a.person)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }

    /// Distinct views that end up attached to somebody.
    pub fn view_count(&self) -> usize {
        self.attachments
            .iter()
            .map(|a| (a.deposit_id, a.view_id))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }
}

/// Attaches every joinable manifest reference to a person.
///
/// Views with no reference are skipped rather than reported one by one: on a
/// real tree they are the majority, and they are not failures — they are
/// documents the owner never linked to anybody.
pub fn join(manifest: &Manifest, index: &PersonIndex) -> Join {
    let mut result = Join::default();

    for deposit in &manifest.deposits {
        for view in &deposit.views {
            if view.references.is_empty() {
                result.unlinked_view_count += 1;
                continue;
            }

            let before = result.attachments.len();

            for reference in &view.references {
                let name = display_name(
                    reference.lastname.as_deref(),
                    reference.firstname.as_deref(),
                );

                let Some(key) = reference.geneweb_ref.as_deref() else {
                    result.unjoined.push(Unjoined {
                        geneweb_ref: None,
                        name,
                        deposit_id: deposit.id,
                        view_id: view.id,
                        reason: UnjoinedReason::NoKey,
                    });
                    continue;
                };

                let candidates = index.lookup(key);
                let reason = match candidates {
                    [] => UnjoinedReason::NoSuchPerson,
                    [person] => {
                        result.attachments.push(Attachment {
                            person: *person,
                            deposit_id: deposit.id,
                            view_id: view.id,
                            page: view.page,
                            title: deposit.title.clone(),
                            extension: extension(deposit, view),
                        });
                        continue;
                    }
                    _ => UnjoinedReason::Ambiguous,
                };

                result.unjoined.push(Unjoined {
                    geneweb_ref: Some(key.to_string()),
                    name,
                    deposit_id: deposit.id,
                    view_id: view.id,
                    reason,
                });
            }

            if result.attachments.len() > before {
                result.joined_view_count += 1;
            }
        }
    }

    result
}

fn display_name(lastname: Option<&str>, firstname: Option<&str>) -> String {
    match (lastname, firstname) {
        (Some(last), Some(first)) => format!("{last} {first}"),
        (Some(last), None) => last.to_string(),
        (None, Some(first)) => first.to_string(),
        (None, None) => "(unnamed)".to_string(),
    }
}

/// Derives the file extension from a rendition URL.
///
/// Geneanet names every rendition `<size>.<ext>` with the deposit's own
/// extension, so this needs no network call. Falls back to `jpg`, which is what
/// the overwhelming majority of deposits are.
fn extension(deposit: &ManifestDeposit, view: &ManifestView) -> String {
    let _ = deposit;

    for rendition in ["normal", "medium", "screen", "thumbnail"] {
        if let Some(url) = view.files.get(rendition) {
            let path = url.split('?').next().unwrap_or(url);
            if let Some(ext) = path.rsplit('.').next()
                && ext.len() <= 5
                && !ext.is_empty()
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
            {
                return ext.to_ascii_lowercase();
            }
        }
    }

    "jpg".to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{ManifestReference, ManifestView};

    fn view(id: i64, references: Vec<ManifestReference>) -> ManifestView {
        ManifestView {
            id,
            page: Some(1),
            files: BTreeMap::from([(
                "normal".to_string(),
                format!("/public/img/media/deposits/private/aa/bb/{id}/hash/normal.png?t=1"),
            )]),
            references,
        }
    }

    fn reference(key: Option<&str>) -> ManifestReference {
        ManifestReference {
            firstname: Some("person_a".to_string()),
            lastname: Some("BRANCH_A".to_string()),
            geneweb_ref: key.map(str::to_string),
        }
    }

    fn manifest(views: Vec<ManifestView>) -> Manifest {
        Manifest {
            source: "test".to_string(),
            deposit_count: 1,
            view_count: views.len(),
            linked_view_count: 0,
            person_count: 0,
            unjoinable_reference_count: 0,
            deposits: vec![ManifestDeposit {
                id: 1,
                original: "https://www.geneanet.org/media/download/?deposits[]=1".to_string(),
                title: Some("a title".to_string()),
                kind: Some("portraits".to_string()),
                private: true,
                date_create: None,
                local_file: None,
                views,
            }],
        }
    }

    /// Builds an index without going through a .gw parse.
    fn index(keys: &[(&str, usize)]) -> PersonIndex {
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (key, person) in keys {
            by_key.entry((*key).to_string()).or_default().push(*person);
        }
        let person_count = by_key.values().map(Vec::len).sum();
        PersonIndex {
            by_key,
            person_count,
        }
    }

    #[test]
    fn attaches_a_reference_to_its_person() {
        let manifest = manifest(vec![view(10, vec![reference(Some("branch_a|person_a|"))])]);
        let index = index(&[("branch_a|person_a|", 7)]);

        let join = join(&manifest, &index);

        assert_eq!(join.attachments.len(), 1);
        assert_eq!(join.attachments[0].person, 7);
        assert_eq!(join.attachments[0].deposit_id, 1);
        assert_eq!(join.attachments[0].view_id, 10);
        assert_eq!(join.attachments[0].extension, "png");
        assert!(join.unjoined.is_empty());
        assert_eq!(join.joined_view_count, 1);
    }

    #[test]
    fn a_group_photo_attaches_to_every_person_on_it() {
        // The case a GEDCOM export cannot express at all.
        let manifest = manifest(vec![view(
            10,
            vec![
                reference(Some("branch_a|person_a|")),
                reference(Some("branch_b|person_b|")),
            ],
        )]);
        let index = index(&[("branch_a|person_a|", 1), ("branch_b|person_b|", 2)]);

        let join = join(&manifest, &index);

        assert_eq!(join.attachments.len(), 2);
        assert_eq!(join.person_count(), 2);
        // Still one file, attached twice.
        assert_eq!(join.view_count(), 1);
    }

    #[test]
    fn a_reference_without_a_key_is_reported_not_guessed() {
        let manifest = manifest(vec![view(10, vec![reference(None)])]);

        let join = join(&manifest, &index(&[("branch_a|person_a|", 1)]));

        assert!(join.attachments.is_empty());
        assert_eq!(join.unjoined.len(), 1);
        assert_eq!(join.unjoined[0].reason, UnjoinedReason::NoKey);
    }

    #[test]
    fn an_unknown_key_is_reported() {
        let manifest = manifest(vec![view(10, vec![reference(Some("ghost|person|"))])]);

        let join = join(&manifest, &index(&[("branch_a|person_a|", 1)]));

        assert_eq!(join.unjoined[0].reason, UnjoinedReason::NoSuchPerson);
    }

    #[test]
    fn an_ambiguous_key_attaches_to_nobody() {
        // Two persons folding to the same key: attaching would be a coin toss,
        // so the medium is reported instead of being put on the wrong person.
        let manifest = manifest(vec![view(10, vec![reference(Some("branch_a|person_a|"))])]);
        let index = index(&[("branch_a|person_a|", 1), ("branch_a|person_a|", 2)]);

        let join = join(&manifest, &index);

        assert!(join.attachments.is_empty());
        assert_eq!(join.unjoined[0].reason, UnjoinedReason::Ambiguous);
    }

    #[test]
    fn views_linked_to_nobody_are_counted_not_reported() {
        let manifest = manifest(vec![view(10, vec![]), view(11, vec![])]);

        let join = join(&manifest, &index(&[]));

        assert_eq!(join.unlinked_view_count, 2);
        assert!(join.unjoined.is_empty());
        assert!(join.attachments.is_empty());
    }

    #[test]
    fn derives_the_extension_from_the_rendition_url() {
        let deposit = &manifest(vec![]).deposits[0];

        let png = view(1, vec![]);
        assert_eq!(extension(deposit, &png), "png");

        let mut no_files = view(2, vec![]);
        no_files.files.clear();
        assert_eq!(extension(deposit, &no_files), "jpg");

        let mut odd = view(3, vec![]);
        odd.files
            .insert("normal".to_string(), "/a/b/normal".to_string());
        assert_eq!(extension(deposit, &odd), "jpg");
    }
}
