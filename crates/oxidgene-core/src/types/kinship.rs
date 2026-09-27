//! How two persons of a tree are related: the paths joining them, generation
//! by generation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::projection::SearchEntry;

/// Every way found to go from one person of a tree to another.
///
/// Blood relationships are reported when there is at least one: each goes up
/// from `from_person_id` to common ancestors and back down to
/// `to_person_id`. Only when the two share no ancestor are the shortest paths
/// through unions reported instead. No path at all means the two persons are
/// not connected by any recorded link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kinship {
    pub from_person_id: Uuid,
    pub to_person_id: Uuid,
    /// Closest relationship first.
    pub paths: Vec<KinshipPath>,
    /// More paths exist than were enumerated.
    pub truncated: bool,
    /// A search row for every person the paths name, so the result can be
    /// drawn without a request per person.
    pub persons: Vec<SearchEntry>,
}

/// One way to go from the first person to the second.
///
/// A blood relationship is a single segment. A relationship by marriage is
/// several, each segment starting with the spouse of the person the previous
/// one ended with.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KinshipPath {
    pub segments: Vec<KinshipSegment>,
}

/// A stretch of a path that climbs to its highest generation and comes back
/// down without passing through a union.
///
/// Both lines are listed from the generation just below the ancestors down to
/// the person they end with. An empty line means that end of the segment *is*
/// the ancestor: a direct ancestor or descendant, or a single person between
/// two unions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KinshipSegment {
    /// The highest generation of the segment: one person, or both spouses of
    /// the family the two lines descend from.
    pub ancestor_ids: Vec<Uuid>,
    /// The family both lines descend from, when they share one.
    pub family_id: Option<Uuid>,
    /// Down to the segment's first person.
    pub from_line: Vec<Uuid>,
    /// Down to the segment's last person.
    pub to_line: Vec<Uuid>,
    /// The two lines descend from the ancestor through different unions:
    /// half-siblings at the top.
    pub half: bool,
    /// The union joining the previous segment's last person to this
    /// segment's first; `None` on the first segment.
    pub union_family_id: Option<Uuid>,
}

impl KinshipSegment {
    /// The person the segment starts with.
    #[must_use]
    pub fn first_person(&self) -> Option<Uuid> {
        self.from_line
            .last()
            .or_else(|| self.ancestor_ids.first())
            .copied()
    }

    /// The person the segment ends with.
    #[must_use]
    pub fn last_person(&self) -> Option<Uuid> {
        self.to_line
            .last()
            .or_else(|| self.ancestor_ids.first())
            .copied()
    }
}
