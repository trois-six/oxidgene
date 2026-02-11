use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{ChildType, SpouseRole};

/// A family unit linking spouses and children.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A spouse's membership in a family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilySpouse {
    pub id: Uuid,
    pub family_id: Uuid,
    pub person_id: Uuid,
    pub role: SpouseRole,
    pub sort_order: i32,
}

/// A child's membership in a family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyChild {
    pub id: Uuid,
    pub family_id: Uuid,
    pub person_id: Uuid,
    pub child_type: ChildType,
    pub sort_order: i32,
}
