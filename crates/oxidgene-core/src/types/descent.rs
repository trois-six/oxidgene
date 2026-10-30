//! The descent of a family name: its bearers grouped into branches, each
//! branch a root bearer and the bearers descending from them, written as the
//! lines of an indented list.
//!
//! The lines are flat — each with its depth — rather than nested, so that the
//! two surfaces carry the same shape: a GraphQL selection cannot follow a
//! recursion of unknown depth. The rules that build them are in
//! `docs/ui-dictionary.md` §7.2.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::Sex;
use crate::types::QualifiedYear;

/// Most branches one answer holds; the next ones are asked for with an
/// offset.
pub const DESCENT_BRANCH_PAGE: usize = 50;

/// Most lines one answer unfolds below its roots. What passes it is folded
/// behind [`DescentLineKind::More`] lines, unfolded one person at a time.
pub const DESCENT_LINE_BUDGET: usize = 400;

/// The descent of one family name, or of one person when a folded part is
/// unfolded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurnameDescent {
    /// The family name, exactly as the dictionary lists it.
    pub value: String,
    /// The persons carrying it on a name other than a married name.
    pub bearer_count: u32,
    /// Every branch of the name, whatever this answer holds.
    pub branch_count: u32,
    /// How many branches come before the first one of this answer.
    pub offset: u32,
    /// The branches of this answer, in order; when a person was unfolded, the
    /// one branch made of that person's descent.
    pub branches: Vec<DescentBranch>,
    /// The persons carrying the name only as a married name who are shown
    /// beside no bearer: in the first answer only.
    pub by_marriage: Vec<DescentPerson>,
}

/// One branch: a root bearer and their descent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescentBranch {
    /// Its rank among every branch of the name, from 1.
    pub number: u32,
    /// The root has a parent in the tree, who does not bear the name.
    pub root_has_parents: bool,
    /// The bearers the branch shows in full, its root included.
    pub person_count: u32,
    /// Its lines, the root's first at depth 0.
    pub lines: Vec<DescentLine>,
}

/// What a line of a branch shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescentLineKind {
    /// A bearer, with their first union.
    Person,
    /// A bearer's next union, the bearer shown again; the union's children
    /// follow it.
    Union,
    /// The children of the union above who do not bear the name.
    Others,
    /// A bearer shown in full elsewhere: in the branch `branch`.
    Reference,
    /// The descent of the bearer above, folded: `count` bearers, unfolded by
    /// asking for `unfold_person_id`.
    More,
}

/// One line of a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescentLine {
    pub kind: DescentLineKind,
    /// Its indentation: the root's generation is 0.
    pub depth: u32,
    /// The bearer of a person, union or reference line.
    pub person: Option<DescentPerson>,
    /// The year of the union the line shows, with its precision.
    pub marriage: Option<QualifiedYear>,
    /// The other spouse of that union, when known.
    pub spouse: Option<DescentPerson>,
    /// The children of an `others` line; the folded bearers of a `more` line.
    pub count: u32,
    /// The branch a `reference` line's bearer is shown in.
    pub branch: Option<u32>,
    /// The bearer whose descent a `more` line unfolds.
    pub unfold_person_id: Option<Uuid>,
}

/// A person of a descent line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescentPerson {
    pub person_id: Uuid,
    pub sex: Sex,
    pub given_names: Option<String>,
    /// The full surname, particle included: the descent's own name for a
    /// bearer, the primary name's for a spouse.
    pub surname: Option<String>,
    /// The birth's year, or the baptism's when the birth has no date.
    pub birth: Option<QualifiedYear>,
    /// The death's year, or the burial's when the death has no date.
    pub death: Option<QualifiedYear>,
    /// A death or a burial is recorded, dated or not.
    pub deceased: bool,
    /// The person is the tree's SOSA root or one of its ancestors.
    pub sosa_ancestor: bool,
}

impl DescentLine {
    /// A line of `kind` at `depth` with nothing else set.
    pub fn new(kind: DescentLineKind, depth: u32) -> Self {
        Self {
            kind,
            depth,
            person: None,
            marriage: None,
            spouse: None,
            count: 0,
            branch: None,
            unfold_person_id: None,
        }
    }
}
