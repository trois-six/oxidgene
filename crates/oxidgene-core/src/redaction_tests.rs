//! The policy of docs/privacy.md §3, §5, §6 as table tests: who each
//! viewing hides, how each display mode shows them, and what a profile
//! and a pedigree keep.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::enums::{ChildType, Sex, SpouseRole};
use crate::projection::{PedigreeEdge, PedigreeFamily};

const OWNER: Viewing = Viewing {
    read_private: true,
    read_confidential: true,
    read_tags: true,
    exclusions_apply: false,
};
const EDITOR: Viewing = Viewing {
    exclusions_apply: true,
    ..OWNER
};
const GUEST: Viewing = Viewing {
    read_confidential: false,
    ..EDITOR
};
const VISITOR: Viewing = Viewing {
    read_private: false,
    read_confidential: false,
    read_tags: false,
    exclusions_apply: true,
};

fn row(privacy: Privacy, deceased: bool, latest: Option<i32>) -> PersonVisibility {
    PersonVisibility {
        privacy,
        deceased,
        latest_birth_year: latest,
        ..PersonVisibility::default()
    }
}

struct Tree {
    excluded: Uuid,
    private: Uuid,
    living: Uuid,
    public: Uuid,
    excluded_set: HashSet<Uuid>,
    visibility: HashMap<Uuid, PersonVisibility>,
    families: Vec<(Uuid, Privacy, Vec<Uuid>)>,
}

fn tree() -> Tree {
    let [excluded, private, living, public] = [(); 4].map(|()| Uuid::now_v7());
    let visibility = HashMap::from([
        (excluded, row(Privacy::Public, true, Some(1800))),
        (private, row(Privacy::Private, true, Some(1800))),
        (living, row(Privacy::Public, false, Some(1990))),
        (public, row(Privacy::Default, true, Some(1800))),
    ]);
    let families = vec![
        (Uuid::now_v7(), Privacy::Default, vec![public, living]),
        (Uuid::now_v7(), Privacy::Private, vec![public]),
    ];
    Tree {
        excluded,
        private,
        living,
        public,
        excluded_set: HashSet::from([excluded]),
        visibility,
        families,
    }
}

fn redaction(viewing: Viewing, display: HiddenDisplay, t: &Tree) -> Redaction {
    let settings = PrivacySettings {
        hidden_display: display,
        ..PrivacySettings::default()
    };
    Redaction::build(
        viewing,
        TreeDefaultPrivacy::Public,
        &settings,
        &RedactionInputs {
            excluded: Some(&t.excluded_set),
            visibility: Some(&t.visibility),
            families: &t.families,
        },
        2026,
    )
}

#[test]
fn the_owner_sees_everything() {
    let t = tree();
    let r = redaction(OWNER, HiddenDisplay::Stub, &t);
    assert!(r.is_unrestricted());
    for id in [t.excluded, t.private, t.living, t.public] {
        assert_eq!(r.person(id), Appearance::Shown);
    }
}

#[test]
fn editors_and_guests_lose_the_excluded_persons_only() {
    let t = tree();
    for viewing in [EDITOR, GUEST] {
        let r = redaction(viewing, HiddenDisplay::Stub, &t);
        assert_eq!(r.person(t.excluded), Appearance::Absent);
        assert_eq!(r.public_id(t.excluded), None);
        for id in [t.private, t.living, t.public] {
            assert!(r.shows(id));
        }
        assert!(!r.private(Privacy::Private));
    }
    assert!(!redaction(EDITOR, HiddenDisplay::Stub, &t).withholds_confidential());
    let guest = redaction(GUEST, HiddenDisplay::Stub, &t);
    assert!(guest.withholds_confidential());
    assert!(guest.note_withheld(true, false));
    assert!(!guest.note_withheld(false, false));
}

#[test]
fn visitors_see_hidden_persons_as_the_display_says() {
    let t = tree();
    for (display, appearance) in [
        (HiddenDisplay::Stub, Appearance::Stub),
        (HiddenDisplay::Initials, Appearance::Initials),
        (HiddenDisplay::Hidden, Appearance::Absent),
    ] {
        let r = redaction(VISITOR, display, &t);
        assert_eq!(r.person(t.private), appearance, "{display}");
        assert_eq!(r.person(t.living), appearance, "{display}");
        // Excluded persons are absent whatever the mode.
        assert_eq!(r.person(t.excluded), Appearance::Absent);
        assert!(r.shows(t.public));
        assert!(r.withholds_tags());
        // A couple with a hidden spouse, and a private one, are withheld.
        assert!(t.families.iter().all(|(f, ..)| r.family_withheld(*f)));
    }
}

#[test]
fn a_placeholder_that_may_not_be_navigated_has_a_pseudonym() {
    let t = tree();
    let settings = PrivacySettings {
        hidden_navigation: false,
        ..PrivacySettings::default()
    };
    let r = Redaction::build(
        VISITOR,
        TreeDefaultPrivacy::Public,
        &settings,
        &RedactionInputs {
            excluded: Some(&t.excluded_set),
            visibility: Some(&t.visibility),
            families: &t.families,
        },
        2026,
    );
    let shown = r.public_id(t.living).unwrap();
    assert_ne!(shown, t.living);
    assert_eq!(
        r.public_id(t.living),
        Some(shown),
        "stable within a request"
    );
    assert_eq!(r.public_id(t.public), Some(t.public));
}

#[test]
fn a_contemporarys_photo_shows_only_when_the_tree_says_so() {
    let t = tree();
    let r = redaction(VISITOR, HiddenDisplay::Stub, &t);
    assert!(r.medium_withheld(Privacy::Default, [t.living], []));
    assert!(r.medium_withheld(Privacy::Private, [t.public], []));
    assert!(!r.medium_withheld(Privacy::Default, [t.public], []));
    let settings = PrivacySettings {
        contemporary_photos: true,
        ..PrivacySettings::default()
    };
    let r = Redaction::build(
        VISITOR,
        TreeDefaultPrivacy::Public,
        &settings,
        &RedactionInputs {
            excluded: Some(&t.excluded_set),
            visibility: Some(&t.visibility),
            families: &[],
        },
        2026,
    );
    assert!(!r.medium_withheld(Privacy::Default, [t.living], []));
    assert!(r.medium_withheld(Privacy::Default, [t.private], []));
    assert!(r.medium_withheld(Privacy::Default, [t.excluded], []));
}

fn profile(id: Uuid, spouse: Uuid, child: Uuid) -> PersonProfile {
    let name = |given: &str| ProfileName {
        name_id: Uuid::now_v7(),
        name_type: crate::enums::NameType::Birth,
        display_name: format!("{given} Wrenfield"),
        given_names: Some(given.to_string()),
        surname: Some("Wrenfield".to_string()),
    };
    PersonProfile {
        person_id: id,
        tree_id: Uuid::nil(),
        sex: Sex::Male,
        primary_name: Some(name("Ashby")),
        other_names: Vec::new(),
        birth: None,
        death: None,
        baptism: None,
        burial: None,
        other_events: Vec::new(),
        families_as_spouse: vec![ProfileFamilyLink {
            family_id: Uuid::now_v7(),
            role: SpouseRole::Husband,
            spouse_id: Some(spouse),
            spouse_display_name: Some("Thistle Canaryexcluded".to_string()),
            spouse_surname: Some("Canaryexcluded".to_string()),
            spouse_given_names: Some("Thistle".to_string()),
            spouse_sex: Some(Sex::Female),
            events: Vec::new(),
            children_ids: vec![child],
        }],
        family_as_child: None,
        primary_media: None,
        media_count: 0,
        citation_count: 0,
        note_count: 0,
        tags: Vec::new(),
        updated_at: Utc::now(),
        built_at: Utc::now(),
    }
}

#[test]
fn a_profile_names_no_hidden_relative() {
    let t = tree();
    let r = redaction(EDITOR, HiddenDisplay::Stub, &t);
    let shown = r
        .profile(&profile(t.public, t.excluded, t.excluded))
        .unwrap();
    let text = serde_json::to_string(&shown).unwrap();
    assert!(!text.contains("Canaryexcluded"), "{text}");
    assert!(!text.contains(&t.excluded.to_string()), "{text}");
    assert!(
        r.profile(&profile(t.excluded, t.public, t.public))
            .is_none()
    );

    let r = redaction(VISITOR, HiddenDisplay::Initials, &t);
    let stub = r.profile(&profile(t.living, t.public, t.public)).unwrap();
    assert_eq!(stub.primary_name.unwrap().display_name, "A. W.");
    assert!(stub.birth.is_none() && stub.other_events.is_empty());
}

#[test]
fn a_pedigree_loses_absent_persons_and_their_links() {
    let t = tree();
    let node = |id: Uuid| PedigreeNode {
        person_id: id,
        sex: Sex::Female,
        display_name: "Thistle Canaryexcluded".to_string(),
        given_names: Some("Thistle".to_string()),
        surname: Some("Canaryexcluded".to_string()),
        birth: None,
        death: None,
        occupation: Some("Lamplighter".to_string()),
        primary_media_path: None,
        generation: 0,
        sosa_number: Some(3),
        portrait: None,
        sosa_ancestor: true,
        tags: Vec::new(),
    };
    let family = Uuid::now_v7();
    let mut pedigree = Pedigree {
        tree_id: Uuid::nil(),
        root_person_id: t.public,
        persons: HashMap::from([(t.public, node(t.public)), (t.excluded, node(t.excluded))]),
        edges: vec![PedigreeEdge {
            parent_id: t.excluded,
            child_id: t.public,
            family_id: family,
            edge_type: ChildType::Biological,
        }],
        family_events: HashMap::new(),
        families: HashMap::from([(
            family,
            PedigreeFamily {
                family_id: family,
                spouse_ids: vec![t.excluded],
                children_ids: vec![t.public],
                members: Vec::new(),
            },
        )]),
        ancestor_depth_loaded: 1,
        descendant_depth_loaded: 0,
        built_at: Utc::now(),
    };
    redaction(GUEST, HiddenDisplay::Stub, &t).pedigree(&mut pedigree);
    assert_eq!(pedigree.persons.len(), 1);
    assert!(pedigree.edges.is_empty());
    assert!(pedigree.families[&family].spouse_ids.is_empty());
    let text = serde_json::to_string(&pedigree).unwrap();
    assert!(!text.contains(&t.excluded.to_string()), "{text}");
}
