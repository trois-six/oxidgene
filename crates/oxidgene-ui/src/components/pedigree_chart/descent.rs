//! A person's descendants as the descendant charts draw them: breadth first,
//! each with their unions, each union with its children, as deep as asked.
//! Shared by the descendant wheel and fan and the horizontal descendant
//! charts (the descendant lineage and the hourglass).

use super::*;

/// One couple of a person placed on the chart.
pub(super) struct Union {
    /// Index in [`Tree::people`] of the spouse, or `None` when the family
    /// records no other spouse.
    pub(super) spouse: Option<Uuid>,
    /// Indices in [`Tree::people`] of the couple's children.
    pub(super) children: Vec<usize>,
}

/// A person placed on the chart, with the unions drawn around them.
pub(super) struct Person {
    pub(super) id: Uuid,
    pub(super) depth: u32,
    pub(super) parent: Option<usize>,
    pub(super) unions: Vec<Union>,
}

/// The root's descendants, breadth first: every person after their parents.
pub(super) struct Tree {
    pub(super) people: Vec<Person>,
}

impl Tree {
    /// Collects the descendants of `root_id` down to `generations`. A person
    /// who turns out to be their own ancestor — a loop in bad data — is not
    /// expanded again, so the walk always ends.
    pub(super) fn collect(root_id: Uuid, data: &PedigreeData, generations: u32) -> Self {
        let mut tree = Self {
            people: vec![Person {
                id: root_id,
                depth: 0,
                parent: None,
                unions: Vec::new(),
            }],
        };
        let mut next = 0;
        while next < tree.people.len() {
            if tree.people[next].depth < generations {
                let unions = tree.unions_of(next, data);
                tree.people[next].unions = unions;
            }
            next += 1;
        }
        tree
    }

    /// The unions of `index`, in the order of the person's families, each
    /// child placed as a new person one generation further out.
    fn unions_of(&mut self, index: usize, data: &PedigreeData) -> Vec<Union> {
        let person = self.people[index].id;
        let depth = self.people[index].depth;
        let mut unions = Vec::new();
        for fid in data.families_as_spouse.get(&person).into_iter().flatten() {
            let spouse = data
                .spouses_by_family
                .get(fid)
                .and_then(|spouses| spouses.iter().find(|s| s.person_id != person))
                .map(|s| s.person_id);
            let mut children = data
                .children_by_family
                .get(fid)
                .cloned()
                .unwrap_or_default();
            children.sort_by_key(|child| child.sort_order);
            let kept: Vec<Uuid> = children
                .into_iter()
                .map(|child| child.person_id)
                .filter(|&child| !self.is_ancestor_of(child, index))
                .collect();
            let children: Vec<usize> = kept
                .into_iter()
                .map(|child| {
                    self.people.push(Person {
                        id: child,
                        depth: depth + 1,
                        parent: Some(index),
                        unions: Vec::new(),
                    });
                    self.people.len() - 1
                })
                .collect();
            if spouse.is_some() || !children.is_empty() {
                unions.push(Union { spouse, children });
            }
        }
        unions
    }

    /// Whether `id` is `index` or one of the people above it on the chart.
    fn is_ancestor_of(&self, id: Uuid, index: usize) -> bool {
        let mut at = Some(index);
        while let Some(i) = at {
            if self.people[i].id == id {
                return true;
            }
            at = self.people[i].parent;
        }
        false
    }

    /// How many leaves each person holds: 1 for a person drawn without
    /// unions, otherwise the sum over their unions, a childless union
    /// counting one. Computed from the outermost generation in.
    pub(super) fn weights(&self) -> Vec<f64> {
        let mut weights = vec![1.0; self.people.len()];
        for i in (0..self.people.len()).rev() {
            let unions = &self.people[i].unions;
            if !unions.is_empty() {
                weights[i] = unions.iter().map(|u| union_weight(u, &weights)).sum();
            }
        }
        weights
    }
}

pub(super) fn union_weight(union: &Union, weights: &[f64]) -> f64 {
    let children: f64 = union.children.iter().map(|&c| weights[c]).sum();
    children.max(1.0)
}

/// The card data of an unknown spouse: nobody to name.
pub(super) fn unknown_spouse() -> LayoutNode {
    LayoutNode {
        id: None,
        x: 0.0,
        y: 0.0,
        sex: Sex::Unknown,
        label_surname: String::new(),
        label_given: String::new(),
        birth_year: None,
        death_year: None,
        sosa_badge: SosaBadge::None,
        is_self: false,
        is_compact: false,
        child_of: None,
        is_father: false,
        is_sibling: false,
        has_more_relations: false,
    }
}
