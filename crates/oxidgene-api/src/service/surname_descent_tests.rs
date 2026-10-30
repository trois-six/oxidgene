//! The grouping rules of a family name's descent, on fictitious rows built in
//! memory: who starts a branch, what is shown under whom, and where the
//! answer folds.

use super::*;
use chrono::Utc;
use oxidgene_core::enums::{Calendar, ChildType, DateQualifier};
use oxidgene_core::types::{FamilyChild, FamilySpouse};

const NAME: &str = "Ashdown";

/// Rows built one fact at a time.
#[derive(Default)]
struct Rows(DescentRows);

fn id() -> Uuid {
    Uuid::now_v7()
}

impl Rows {
    /// A live person, carrying `surname` on a name of `name_type`.
    fn person(&mut self, given: &str, surname: &str, name_type: NameType) -> Uuid {
        let person = id();
        self.0.sexes.insert(person, Sex::Unknown);
        let name = PersonName {
            id: id(),
            person_id: person,
            name_type,
            given_names: Some(given.to_string()),
            surname: Some(surname.to_string()),
            surname_prefix: None,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        if surname == NAME {
            self.0.carrier_names.push(name.clone());
        }
        self.0.names.push(name);
        person
    }

    fn bearer(&mut self, given: &str) -> Uuid {
        self.person(given, NAME, NameType::Birth)
    }

    fn other(&mut self, given: &str) -> Uuid {
        self.person(given, "Birchley", NameType::Birth)
    }

    /// A family of `spouses` with `children`, in that record order.
    fn family(&mut self, spouses: &[Uuid], children: &[Uuid]) -> Uuid {
        let family = id();
        for (k, person) in spouses.iter().enumerate() {
            self.0.spouses.push(FamilySpouse {
                id: id(),
                family_id: family,
                person_id: *person,
                role: if k == 0 {
                    SpouseRole::Husband
                } else {
                    SpouseRole::Wife
                },
                sort_order: 0,
            });
        }
        for (k, child) in children.iter().enumerate() {
            self.0.children.push(FamilyChild {
                id: id(),
                family_id: family,
                person_id: *child,
                child_type: ChildType::Biological,
                sort_order: i32::try_from(k).unwrap(),
            });
        }
        family
    }

    fn event(&mut self, kind: EventType, owner: (Option<Uuid>, Option<Uuid>), year: Option<i32>) {
        self.0.events.push(Event {
            id: id(),
            tree_id: Uuid::nil(),
            event_type: kind,
            date_value: year.map(|y| y.to_string()),
            date_sort: year.and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1)),
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            cause: None,
            age: None,
            agency: None,
            classification: None,
            spouse_ages: Vec::new(),
            place_id: None,
            person_id: owner.0,
            family_id: owner.1,
            related_family_id: None,
            description: None,
            tags: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        });
    }

    fn born(&mut self, person: Uuid, year: i32) {
        self.event(EventType::Birth, (Some(person), None), Some(year));
    }

    fn married(&mut self, family: Uuid, year: i32) {
        self.event(EventType::Marriage, (None, Some(family)), Some(year));
    }

    fn answer(self, request: DescentRequest) -> Result<SurnameDescent, OxidGeneError> {
        let graph = Graph::new(self.0);
        let sosa = HashSet::new();
        let forest = Forest::build(&graph);
        let people = People {
            graph: &graph,
            sosa: &sosa,
        };
        forest.answer(&people, NAME, &request)
    }

    fn page(self) -> SurnameDescent {
        self.answer(DescentRequest {
            value: NAME.to_string(),
            ..DescentRequest::default()
        })
        .unwrap()
    }
}

/// A line as `depth kind given & spouse`, for comparing whole branches.
fn shape(line: &DescentLine) -> String {
    let given = |p: &Option<DescentPerson>| {
        p.as_ref()
            .and_then(|p| p.given_names.clone())
            .unwrap_or_default()
    };
    let kind = match line.kind {
        DescentLineKind::Person => "person",
        DescentLineKind::Union => "union",
        DescentLineKind::Others => "others",
        DescentLineKind::Reference => "ref",
        DescentLineKind::More => "more",
    };
    let mut text = format!("{} {kind}", line.depth);
    if line.person.is_some() {
        text += &format!(" {}", given(&line.person));
    }
    if line.spouse.is_some() {
        text += &format!(" & {}", given(&line.spouse));
    }
    if matches!(line.kind, DescentLineKind::Others | DescentLineKind::More) {
        text += &format!(" {}", line.count);
    }
    if let Some(branch) = line.branch {
        text += &format!(" @{branch}");
    }
    text
}

fn shapes(branch: &DescentBranch) -> Vec<String> {
    branch.lines.iter().map(shape).collect()
}

#[test]
fn a_root_bears_the_name_and_its_bearing_children_follow_by_birth() {
    let mut rows = Rows::default();
    let root = rows.bearer("Arthur");
    let wife = rows.other("Clara");
    let late = rows.bearer("Bernard");
    let early = rows.bearer("Cyril");
    let undated = rows.bearer("Dorian");
    rows.born(late, 1880);
    rows.born(early, 1870);
    let family = rows.family(&[root, wife], &[undated, late, early]);
    rows.married(family, 1865);

    let answer = rows.page();
    assert_eq!(answer.bearer_count, 4);
    assert_eq!(answer.branch_count, 1);
    let branch = &answer.branches[0];
    assert_eq!(
        shapes(branch),
        [
            "0 person Arthur & Clara",
            "1 person Cyril",
            "1 person Bernard",
            "1 person Dorian",
        ]
    );
    assert_eq!(branch.person_count, 4);
    assert_eq!(branch.lines[0].marriage.map(|m| m.year), Some(1865));
    assert!(!branch.root_has_parents);
}

#[test]
fn a_daughters_children_fold_into_one_line_and_a_union_repeats() {
    let mut rows = Rows::default();
    let daughter = rows.bearer("Alice");
    let first = rows.other("Bernard");
    let second = rows.other("Cyril");
    let kids = [rows.other("Delia"), rows.other("Arthur")];
    let natural = rows.bearer("Beatrice");
    let one = rows.family(&[first, daughter], &kids);
    rows.married(one, 1890);
    let two = rows.family(&[second, daughter], &[natural]);
    rows.married(two, 1900);

    let answer = rows.page();
    assert_eq!(
        shapes(&answer.branches[0]),
        [
            "0 person Alice & Bernard",
            "1 others 2",
            "0 union Alice & Cyril",
            "1 person Beatrice",
        ]
    );
}

#[test]
fn a_root_with_parents_of_another_name_says_so() {
    let mut rows = Rows::default();
    let father = rows.other("Bernard");
    let root = rows.bearer("Arthur");
    rows.family(&[father], &[root]);
    // A bearer with one known parent, who bears the name, is no root.
    let child = rows.bearer("Clara");
    rows.family(&[root], &[child]);

    let answer = rows.page();
    assert_eq!(answer.branch_count, 1);
    assert!(answer.branches[0].root_has_parents);
    assert_eq!(
        shapes(&answer.branches[0]),
        ["0 person Arthur", "1 person Clara"]
    );
}

#[test]
fn branches_are_ordered_by_their_roots_earliest_date() {
    let mut rows = Rows::default();
    rows.bearer("Arthur");
    let late = rows.bearer("Bernard");
    let early = rows.bearer("Cyril");
    rows.born(late, 1800);
    let family = rows.family(&[early], &[]);
    rows.married(family, 1750);

    let answer = rows.page();
    let roots: Vec<String> = answer.branches.iter().map(|b| shape(&b.lines[0])).collect();
    assert_eq!(
        roots,
        ["0 person Cyril", "0 person Bernard", "0 person Arthur"]
    );
    assert_eq!(
        answer.branches.iter().map(|b| b.number).collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn a_child_of_two_bearers_is_shown_once_and_referred_to_elsewhere() {
    let mut rows = Rows::default();
    let husband = rows.bearer("Arthur");
    let wife = rows.bearer("Beatrice");
    rows.born(husband, 1800);
    rows.born(wife, 1802);
    let child = rows.bearer("Cyril");
    rows.family(&[husband, wife], &[child]);

    let answer = rows.page();
    assert_eq!(answer.branch_count, 2);
    assert_eq!(
        shapes(&answer.branches[0]),
        ["0 person Arthur & Beatrice", "1 person Cyril"]
    );
    assert_eq!(
        shapes(&answer.branches[1]),
        ["0 person Beatrice & Arthur", "1 ref Cyril @1"]
    );
}

#[test]
fn a_loop_of_parentage_ends_and_still_shows_everyone() {
    let mut rows = Rows::default();
    let first = rows.bearer("Arthur");
    let second = rows.bearer("Bernard");
    rows.born(first, 1800);
    // Each is recorded as the other's child: nobody is a root.
    rows.family(&[first], &[second]);
    rows.family(&[second], &[first]);

    let answer = rows.page();
    assert_eq!(answer.branch_count, 1);
    assert_eq!(
        shapes(&answer.branches[0]),
        ["0 person Arthur", "1 person Bernard", "2 ref Arthur @1"]
    );
}

#[test]
fn a_married_name_starts_no_branch() {
    let mut rows = Rows::default();
    let husband = rows.bearer("Arthur");
    let wife = rows.person("Clara", NAME, NameType::Married);
    rows.family(&[husband, wife], &[]);
    let widow = rows.person("Delia", NAME, NameType::Married);

    let answer = rows.page();
    assert_eq!(answer.bearer_count, 1);
    assert_eq!(shapes(&answer.branches[0]), ["0 person Arthur & Clara"]);
    // Delia's husband is not in the tree: she is listed apart, Clara is not.
    let apart: Vec<Uuid> = answer.by_marriage.iter().map(|p| p.person_id).collect();
    assert_eq!(apart, [widow]);
}

#[test]
fn past_the_budget_a_branch_folds_and_unfolds_by_person() {
    let mut rows = Rows::default();
    let root = rows.bearer("Arthur");
    let sons: Vec<Uuid> = (0..DESCENT_LINE_BUDGET)
        .map(|_| rows.bearer("Bernard"))
        .collect();
    rows.family(&[root], &sons);
    let grandson = rows.bearer("Cyril");
    rows.family(&[sons[0]], &[grandson]);

    let page = rows.page();
    let lines = &page.branches[0].lines;
    // The root's children fill the budget: their own children stay folded.
    assert_eq!(lines.len(), 2 + DESCENT_LINE_BUDGET);
    assert_eq!(shape(&lines[1]), "1 person Bernard");
    assert_eq!(shape(&lines[2]), "2 more 1");
    assert_eq!(lines[2].unfold_person_id, Some(sons[0]));
}

#[test]
fn unfolding_a_person_always_shows_their_children() {
    let mut rows = Rows::default();
    let root = rows.bearer("Arthur");
    let sons: Vec<Uuid> = (0..=DESCENT_LINE_BUDGET)
        .map(|_| rows.bearer("Bernard"))
        .collect();
    rows.family(&[root], &sons);
    let request = DescentRequest {
        value: NAME.to_string(),
        offset: None,
        person_id: Some(root),
    };
    let answer = rows.answer(request).unwrap();
    assert_eq!(answer.branches.len(), 1);
    assert_eq!(answer.branches[0].lines.len(), 2 + DESCENT_LINE_BUDGET);
    assert!(answer.by_marriage.is_empty());
}

#[test]
fn unfolding_someone_outside_the_descent_is_not_found() {
    let mut rows = Rows::default();
    rows.bearer("Arthur");
    let stranger = rows.other("Bernard");
    let request = DescentRequest {
        value: NAME.to_string(),
        offset: None,
        person_id: Some(stranger),
    };
    assert!(matches!(
        rows.answer(request),
        Err(OxidGeneError::NotFound { .. })
    ));
}

#[test]
fn branches_come_a_page_at_a_time() {
    let mut rows = Rows::default();
    for _ in 0..=DESCENT_BRANCH_PAGE {
        rows.bearer("Arthur");
    }
    rows.person("Clara", NAME, NameType::Married);
    let request = DescentRequest {
        value: NAME.to_string(),
        offset: Some(u32::try_from(DESCENT_BRANCH_PAGE).unwrap()),
        person_id: None,
    };
    let answer = rows.answer(request).unwrap();
    assert_eq!(answer.branch_count as usize, DESCENT_BRANCH_PAGE + 1);
    assert_eq!(answer.branches.len(), 1);
    assert_eq!(answer.branches[0].number as usize, DESCENT_BRANCH_PAGE + 1);
    // The persons apart come with the first page only.
    assert!(answer.by_marriage.is_empty());
}

#[test]
fn a_death_without_a_date_still_marks_the_person_deceased() {
    let mut rows = Rows::default();
    let root = rows.bearer("Arthur");
    rows.event(EventType::Burial, (Some(root), None), None);
    rows.event(EventType::Baptism, (Some(root), None), Some(1701));
    let answer = rows.page();
    let person = answer.branches[0].lines[0].person.clone().unwrap();
    assert!(person.deceased);
    assert_eq!(person.death, None);
    // Birth falls back to the baptism.
    assert_eq!(person.birth.map(|b| b.year), Some(1701));
}
