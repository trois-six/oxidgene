//! GraphQL object types for OxidGene.
//!
//! Each domain type is wrapped in a GraphQL object with resolvers for
//! nested relationships (e.g., Person -> names, events, families).

use async_graphql::{ComplexObject, Context, Enum, ID, Result, SimpleObject};
use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use oxidgene_db::repo::{
    CitationRepo, EventFilter, EventRepo, FamilyChildRepo, FamilySpouseRepo, MediaLinkRepo,
    MediaRepo, NoteRepo, PaginationParams, PersonNameRepo, PersonRepo, PlaceRepo,
};

// ── GraphQL Enums ────────────────────────────────────────────────────

/// Biological sex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSex {
    Male,
    Female,
    Unknown,
}

impl From<oxidgene_core::Sex> for GqlSex {
    fn from(s: oxidgene_core::Sex) -> Self {
        match s {
            oxidgene_core::Sex::Male => Self::Male,
            oxidgene_core::Sex::Female => Self::Female,
            oxidgene_core::Sex::Unknown => Self::Unknown,
        }
    }
}

impl From<GqlSex> for oxidgene_core::Sex {
    fn from(s: GqlSex) -> Self {
        match s {
            GqlSex::Male => Self::Male,
            GqlSex::Female => Self::Female,
            GqlSex::Unknown => Self::Unknown,
        }
    }
}

/// Name type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlNameType {
    Birth,
    Married,
    AlsoKnownAs,
    Maiden,
    Religious,
    Other,
}

impl From<oxidgene_core::NameType> for GqlNameType {
    fn from(n: oxidgene_core::NameType) -> Self {
        match n {
            oxidgene_core::NameType::Birth => Self::Birth,
            oxidgene_core::NameType::Married => Self::Married,
            oxidgene_core::NameType::AlsoKnownAs => Self::AlsoKnownAs,
            oxidgene_core::NameType::Maiden => Self::Maiden,
            oxidgene_core::NameType::Religious => Self::Religious,
            oxidgene_core::NameType::Other => Self::Other,
        }
    }
}

impl From<GqlNameType> for oxidgene_core::NameType {
    fn from(n: GqlNameType) -> Self {
        match n {
            GqlNameType::Birth => Self::Birth,
            GqlNameType::Married => Self::Married,
            GqlNameType::AlsoKnownAs => Self::AlsoKnownAs,
            GqlNameType::Maiden => Self::Maiden,
            GqlNameType::Religious => Self::Religious,
            GqlNameType::Other => Self::Other,
        }
    }
}

/// Spouse role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSpouseRole {
    Husband,
    Wife,
    Partner,
}

impl From<oxidgene_core::SpouseRole> for GqlSpouseRole {
    fn from(r: oxidgene_core::SpouseRole) -> Self {
        match r {
            oxidgene_core::SpouseRole::Husband => Self::Husband,
            oxidgene_core::SpouseRole::Wife => Self::Wife,
            oxidgene_core::SpouseRole::Partner => Self::Partner,
        }
    }
}

impl From<GqlSpouseRole> for oxidgene_core::SpouseRole {
    fn from(r: GqlSpouseRole) -> Self {
        match r {
            GqlSpouseRole::Husband => Self::Husband,
            GqlSpouseRole::Wife => Self::Wife,
            GqlSpouseRole::Partner => Self::Partner,
        }
    }
}

/// Child type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlChildType {
    Biological,
    Adopted,
    Foster,
    Step,
    Unknown,
}

impl From<oxidgene_core::ChildType> for GqlChildType {
    fn from(c: oxidgene_core::ChildType) -> Self {
        match c {
            oxidgene_core::ChildType::Biological => Self::Biological,
            oxidgene_core::ChildType::Adopted => Self::Adopted,
            oxidgene_core::ChildType::Foster => Self::Foster,
            oxidgene_core::ChildType::Step => Self::Step,
            oxidgene_core::ChildType::Unknown => Self::Unknown,
        }
    }
}

impl From<GqlChildType> for oxidgene_core::ChildType {
    fn from(c: GqlChildType) -> Self {
        match c {
            GqlChildType::Biological => Self::Biological,
            GqlChildType::Adopted => Self::Adopted,
            GqlChildType::Foster => Self::Foster,
            GqlChildType::Step => Self::Step,
            GqlChildType::Unknown => Self::Unknown,
        }
    }
}

/// Event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlEventType {
    Birth,
    Death,
    Baptism,
    Burial,
    Cremation,
    Graduation,
    Immigration,
    Emigration,
    Naturalization,
    Census,
    Occupation,
    Residence,
    Retirement,
    Will,
    Probate,
    Marriage,
    Divorce,
    Annulment,
    Engagement,
    MarriageBann,
    MarriageContract,
    MarriageLicense,
    MarriageSettlement,
    Other,
}

impl From<oxidgene_core::EventType> for GqlEventType {
    fn from(e: oxidgene_core::EventType) -> Self {
        match e {
            oxidgene_core::EventType::Birth => Self::Birth,
            oxidgene_core::EventType::Death => Self::Death,
            oxidgene_core::EventType::Baptism => Self::Baptism,
            oxidgene_core::EventType::Burial => Self::Burial,
            oxidgene_core::EventType::Cremation => Self::Cremation,
            oxidgene_core::EventType::Graduation => Self::Graduation,
            oxidgene_core::EventType::Immigration => Self::Immigration,
            oxidgene_core::EventType::Emigration => Self::Emigration,
            oxidgene_core::EventType::Naturalization => Self::Naturalization,
            oxidgene_core::EventType::Census => Self::Census,
            oxidgene_core::EventType::Occupation => Self::Occupation,
            oxidgene_core::EventType::Residence => Self::Residence,
            oxidgene_core::EventType::Retirement => Self::Retirement,
            oxidgene_core::EventType::Will => Self::Will,
            oxidgene_core::EventType::Probate => Self::Probate,
            oxidgene_core::EventType::Marriage => Self::Marriage,
            oxidgene_core::EventType::Divorce => Self::Divorce,
            oxidgene_core::EventType::Annulment => Self::Annulment,
            oxidgene_core::EventType::Engagement => Self::Engagement,
            oxidgene_core::EventType::MarriageBann => Self::MarriageBann,
            oxidgene_core::EventType::MarriageContract => Self::MarriageContract,
            oxidgene_core::EventType::MarriageLicense => Self::MarriageLicense,
            oxidgene_core::EventType::MarriageSettlement => Self::MarriageSettlement,
            oxidgene_core::EventType::Other => Self::Other,
        }
    }
}

impl From<GqlEventType> for oxidgene_core::EventType {
    fn from(e: GqlEventType) -> Self {
        match e {
            GqlEventType::Birth => Self::Birth,
            GqlEventType::Death => Self::Death,
            GqlEventType::Baptism => Self::Baptism,
            GqlEventType::Burial => Self::Burial,
            GqlEventType::Cremation => Self::Cremation,
            GqlEventType::Graduation => Self::Graduation,
            GqlEventType::Immigration => Self::Immigration,
            GqlEventType::Emigration => Self::Emigration,
            GqlEventType::Naturalization => Self::Naturalization,
            GqlEventType::Census => Self::Census,
            GqlEventType::Occupation => Self::Occupation,
            GqlEventType::Residence => Self::Residence,
            GqlEventType::Retirement => Self::Retirement,
            GqlEventType::Will => Self::Will,
            GqlEventType::Probate => Self::Probate,
            GqlEventType::Marriage => Self::Marriage,
            GqlEventType::Divorce => Self::Divorce,
            GqlEventType::Annulment => Self::Annulment,
            GqlEventType::Engagement => Self::Engagement,
            GqlEventType::MarriageBann => Self::MarriageBann,
            GqlEventType::MarriageContract => Self::MarriageContract,
            GqlEventType::MarriageLicense => Self::MarriageLicense,
            GqlEventType::MarriageSettlement => Self::MarriageSettlement,
            GqlEventType::Other => Self::Other,
        }
    }
}

/// Confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlConfidence {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl From<oxidgene_core::Confidence> for GqlConfidence {
    fn from(c: oxidgene_core::Confidence) -> Self {
        match c {
            oxidgene_core::Confidence::VeryLow => Self::VeryLow,
            oxidgene_core::Confidence::Low => Self::Low,
            oxidgene_core::Confidence::Medium => Self::Medium,
            oxidgene_core::Confidence::High => Self::High,
            oxidgene_core::Confidence::VeryHigh => Self::VeryHigh,
        }
    }
}

impl From<GqlConfidence> for oxidgene_core::Confidence {
    fn from(c: GqlConfidence) -> Self {
        match c {
            GqlConfidence::VeryLow => Self::VeryLow,
            GqlConfidence::Low => Self::Low,
            GqlConfidence::Medium => Self::Medium,
            GqlConfidence::High => Self::High,
            GqlConfidence::VeryHigh => Self::VeryHigh,
        }
    }
}

// ── Helper ───────────────────────────────────────────────────────────

pub(crate) fn db_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a DatabaseConnection {
    ctx.data_unchecked::<DatabaseConnection>()
}

// ── PageInfo ─────────────────────────────────────────────────────────

/// Relay-style pagination info.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

// ── Tree ─────────────────────────────────────────────────────────────

/// A genealogical tree.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlTree {
    pub id: ID,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlTree {
    /// Count of persons in this tree.
    async fn person_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 0,
            after: None,
        };
        let conn = PersonRepo::list(db, tree_id, &params).await?;
        Ok(conn.total_count)
    }

    /// Count of families in this tree.
    async fn family_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 0,
            after: None,
        };
        let conn = oxidgene_db::repo::FamilyRepo::list(db, tree_id, &params).await?;
        Ok(conn.total_count)
    }
}

impl From<oxidgene_core::types::Tree> for GqlTree {
    fn from(t: oxidgene_core::types::Tree) -> Self {
        Self {
            id: ID(t.id.to_string()),
            name: t.name,
            description: t.description,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

// ── Tree Connection ──────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeEdge {
    pub cursor: String,
    pub node: GqlTree,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeConnection {
    pub edges: Vec<GqlTreeEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Tree>> for GqlTreeConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Tree>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlTreeEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Person ───────────────────────────────────────────────────────────

/// A person in a genealogical tree.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlPerson {
    pub id: ID,
    pub tree_id: ID,
    pub sex: GqlSex,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlPerson {
    /// All names for this person.
    async fn names(&self, ctx: &Context<'_>) -> Result<Vec<GqlPersonName>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names.into_iter().map(GqlPersonName::from).collect())
    }

    /// Primary name of this person.
    async fn primary_name(&self, ctx: &Context<'_>) -> Result<Option<GqlPersonName>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names
            .into_iter()
            .find(|n| n.is_primary)
            .map(GqlPersonName::from))
    }

    /// Events associated with this person.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let filter = EventFilter {
            event_type: None,
            person_id: Some(person_id),
            family_id: None,
        };
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let conn = EventRepo::list(db, tree_id, &filter, &params).await?;
        Ok(conn
            .edges
            .into_iter()
            .map(|e| GqlEvent::from(e.node))
            .collect())
    }

    /// Families this person belongs to (as spouse).
    async fn families(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamily>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        // Get all families for this tree and filter where person is a spouse
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let families = oxidgene_db::repo::FamilyRepo::list(db, tree_id, &params).await?;
        let mut result = Vec::new();
        for edge in families.edges {
            let spouses = FamilySpouseRepo::list_by_family(db, edge.node.id).await?;
            if spouses.iter().any(|s| s.person_id == person_id) {
                result.push(GqlFamily::from(edge.node));
            }
        }
        Ok(result)
    }

    /// Citations referencing this person.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        // Use note repo pattern — list all sources and filter citations by person_id
        // For now, iterate sources. This is acceptable for MVP.
        let source_params = PaginationParams {
            first: 100,
            after: None,
        };
        let sources = oxidgene_db::repo::SourceRepo::list(db, tree_id, &source_params).await?;
        let mut citations = Vec::new();
        for se in sources.edges {
            let cits = CitationRepo::list_by_source(db, se.node.id).await?;
            for c in cits {
                if c.person_id == Some(person_id) {
                    citations.push(GqlCitation::from(c));
                }
            }
        }
        Ok(citations)
    }

    /// Media linked to this person.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let media_params = PaginationParams {
            first: 100,
            after: None,
        };
        let media_list = MediaRepo::list(db, tree_id, &media_params).await?;
        let mut result = Vec::new();
        for me in media_list.edges {
            let links = MediaLinkRepo::list_by_media(db, me.node.id).await?;
            if links.iter().any(|l| l.person_id == Some(person_id)) {
                result.push(GqlMedia::from(me.node));
            }
        }
        Ok(result)
    }

    /// Notes attached to this person.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let notes =
            NoteRepo::list_by_entity(db, tree_id, Some(person_id), None, None, None).await?;
        Ok(notes.into_iter().map(GqlNote::from).collect())
    }
}

impl From<oxidgene_core::types::Person> for GqlPerson {
    fn from(p: oxidgene_core::types::Person) -> Self {
        Self {
            id: ID(p.id.to_string()),
            tree_id: ID(p.tree_id.to_string()),
            sex: p.sex.into(),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ── Person Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonEdge {
    pub cursor: String,
    pub node: GqlPerson,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonConnection {
    pub edges: Vec<GqlPersonEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Person>> for GqlPersonConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Person>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlPersonEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── PersonWithDepth ──────────────────────────────────────────────────

/// A person with ancestry depth info.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonWithDepth {
    pub person: GqlPerson,
    pub depth: i32,
}

// ── PersonName ───────────────────────────────────────────────────────

/// A person name.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonName {
    pub id: ID,
    pub person_id: ID,
    pub name_type: GqlNameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::PersonName> for GqlPersonName {
    fn from(n: oxidgene_core::types::PersonName) -> Self {
        Self {
            id: ID(n.id.to_string()),
            person_id: ID(n.person_id.to_string()),
            name_type: n.name_type.into(),
            given_names: n.given_names,
            surname: n.surname,
            prefix: n.prefix,
            suffix: n.suffix,
            nickname: n.nickname,
            is_primary: n.is_primary,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

// ── Family ───────────────────────────────────────────────────────────

/// A family unit.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlFamily {
    pub id: ID,
    pub tree_id: ID,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlFamily {
    /// Spouses in this family.
    async fn spouses(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamilySpouseDetail>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let spouses = FamilySpouseRepo::list_by_family(db, family_id).await?;
        let mut result = Vec::new();
        for s in spouses {
            let person = PersonRepo::get(db, s.person_id).await?;
            result.push(GqlFamilySpouseDetail {
                id: ID(s.id.to_string()),
                person: GqlPerson::from(person),
                role: s.role.into(),
                sort_order: s.sort_order,
            });
        }
        Ok(result)
    }

    /// Children in this family.
    async fn children(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamilyChildDetail>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let children = FamilyChildRepo::list_by_family(db, family_id).await?;
        let mut result = Vec::new();
        for c in children {
            let person = PersonRepo::get(db, c.person_id).await?;
            result.push(GqlFamilyChildDetail {
                id: ID(c.id.to_string()),
                person: GqlPerson::from(person),
                child_type: c.child_type.into(),
                sort_order: c.sort_order,
            });
        }
        Ok(result)
    }

    /// Events associated with this family.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let filter = EventFilter {
            event_type: None,
            person_id: None,
            family_id: Some(family_id),
        };
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let conn = EventRepo::list(db, tree_id, &filter, &params).await?;
        Ok(conn
            .edges
            .into_iter()
            .map(|e| GqlEvent::from(e.node))
            .collect())
    }
}

impl From<oxidgene_core::types::Family> for GqlFamily {
    fn from(f: oxidgene_core::types::Family) -> Self {
        Self {
            id: ID(f.id.to_string()),
            tree_id: ID(f.tree_id.to_string()),
            created_at: f.created_at,
            updated_at: f.updated_at,
        }
    }
}

// ── Family Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyEdge {
    pub cursor: String,
    pub node: GqlFamily,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyConnection {
    pub edges: Vec<GqlFamilyEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Family>> for GqlFamilyConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Family>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlFamilyEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── FamilySpouseDetail ───────────────────────────────────────────────

/// A spouse with resolved person data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilySpouseDetail {
    pub id: ID,
    pub person: GqlPerson,
    pub role: GqlSpouseRole,
    pub sort_order: i32,
}

// ── FamilyChildDetail ────────────────────────────────────────────────

/// A child with resolved person data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyChildDetail {
    pub id: ID,
    pub person: GqlPerson,
    pub child_type: GqlChildType,
    pub sort_order: i32,
}

// ── Event ────────────────────────────────────────────────────────────

/// A genealogical event.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlEvent {
    pub id: ID,
    pub tree_id: ID,
    pub event_type: GqlEventType,
    pub date_value: Option<String>,
    pub date_sort: Option<String>,
    pub place_id: Option<ID>,
    pub person_id: Option<ID>,
    pub family_id: Option<ID>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlEvent {
    /// Resolved place for this event.
    async fn place(&self, ctx: &Context<'_>) -> Result<Option<GqlPlace>> {
        let Some(ref pid) = self.place_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(pid.as_str())?;
        match PlaceRepo::get(db, id).await {
            Ok(p) => Ok(Some(GqlPlace::from(p))),
            Err(_) => Ok(None),
        }
    }

    /// Resolved person for this event.
    async fn person(&self, ctx: &Context<'_>) -> Result<Option<GqlPerson>> {
        let Some(ref pid) = self.person_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(pid.as_str())?;
        match PersonRepo::get(db, id).await {
            Ok(p) => Ok(Some(GqlPerson::from(p))),
            Err(_) => Ok(None),
        }
    }

    /// Resolved family for this event.
    async fn family(&self, ctx: &Context<'_>) -> Result<Option<GqlFamily>> {
        let Some(ref fid) = self.family_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(fid.as_str())?;
        match oxidgene_db::repo::FamilyRepo::get(db, id).await {
            Ok(f) => Ok(Some(GqlFamily::from(f))),
            Err(_) => Ok(None),
        }
    }

    /// Citations for this event.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let source_params = PaginationParams {
            first: 100,
            after: None,
        };
        let sources = oxidgene_db::repo::SourceRepo::list(db, tree_id, &source_params).await?;
        let mut citations = Vec::new();
        for se in sources.edges {
            let cits = CitationRepo::list_by_source(db, se.node.id).await?;
            for c in cits {
                if c.event_id == Some(event_id) {
                    citations.push(GqlCitation::from(c));
                }
            }
        }
        Ok(citations)
    }

    /// Media linked to this event.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let media_params = PaginationParams {
            first: 100,
            after: None,
        };
        let media_list = MediaRepo::list(db, tree_id, &media_params).await?;
        let mut result = Vec::new();
        for me in media_list.edges {
            let links = MediaLinkRepo::list_by_media(db, me.node.id).await?;
            if links.iter().any(|l| l.event_id == Some(event_id)) {
                result.push(GqlMedia::from(me.node));
            }
        }
        Ok(result)
    }

    /// Notes for this event.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let notes = NoteRepo::list_by_entity(db, tree_id, None, Some(event_id), None, None).await?;
        Ok(notes.into_iter().map(GqlNote::from).collect())
    }
}

impl From<oxidgene_core::types::Event> for GqlEvent {
    fn from(e: oxidgene_core::types::Event) -> Self {
        Self {
            id: ID(e.id.to_string()),
            tree_id: ID(e.tree_id.to_string()),
            event_type: e.event_type.into(),
            date_value: e.date_value,
            date_sort: e.date_sort.map(|d| d.to_string()),
            place_id: e.place_id.map(|id| ID(id.to_string())),
            person_id: e.person_id.map(|id| ID(id.to_string())),
            family_id: e.family_id.map(|id| ID(id.to_string())),
            description: e.description,
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

// ── Event Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventEdge {
    pub cursor: String,
    pub node: GqlEvent,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventConnection {
    pub edges: Vec<GqlEventEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Event>> for GqlEventConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Event>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlEventEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Place ────────────────────────────────────────────────────────────

/// A geographic place.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlace {
    pub id: ID,
    pub tree_id: ID,
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Place> for GqlPlace {
    fn from(p: oxidgene_core::types::Place) -> Self {
        Self {
            id: ID(p.id.to_string()),
            tree_id: ID(p.tree_id.to_string()),
            name: p.name,
            latitude: p.latitude,
            longitude: p.longitude,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ── Place Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceEdge {
    pub cursor: String,
    pub node: GqlPlace,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceConnection {
    pub edges: Vec<GqlPlaceEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Place>> for GqlPlaceConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Place>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlPlaceEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Source ────────────────────────────────────────────────────────────

/// A bibliographic source.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlSource {
    pub id: ID,
    pub tree_id: ID,
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlSource {
    /// Citations from this source.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let cits = CitationRepo::list_by_source(db, id).await?;
        Ok(cits.into_iter().map(GqlCitation::from).collect())
    }
}

impl From<oxidgene_core::types::Source> for GqlSource {
    fn from(s: oxidgene_core::types::Source) -> Self {
        Self {
            id: ID(s.id.to_string()),
            tree_id: ID(s.tree_id.to_string()),
            title: s.title,
            author: s.author,
            publisher: s.publisher,
            abbreviation: s.abbreviation,
            repository_name: s.repository_name,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

// ── Source Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceEdge {
    pub cursor: String,
    pub node: GqlSource,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceConnection {
    pub edges: Vec<GqlSourceEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Source>> for GqlSourceConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Source>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlSourceEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Citation ─────────────────────────────────────────────────────────

/// A citation linking a source to an entity.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCitation {
    pub id: ID,
    pub source_id: ID,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub family_id: Option<ID>,
    pub page: Option<String>,
    pub confidence: GqlConfidence,
    pub text: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Citation> for GqlCitation {
    fn from(c: oxidgene_core::types::Citation) -> Self {
        Self {
            id: ID(c.id.to_string()),
            source_id: ID(c.source_id.to_string()),
            person_id: c.person_id.map(|id| ID(id.to_string())),
            event_id: c.event_id.map(|id| ID(id.to_string())),
            family_id: c.family_id.map(|id| ID(id.to_string())),
            page: c.page,
            confidence: c.confidence.into(),
            text: c.text,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

// ── Media ────────────────────────────────────────────────────────────

/// A media file.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMedia {
    pub id: ID,
    pub tree_id: ID,
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Media> for GqlMedia {
    fn from(m: oxidgene_core::types::Media) -> Self {
        Self {
            id: ID(m.id.to_string()),
            tree_id: ID(m.tree_id.to_string()),
            file_name: m.file_name,
            mime_type: m.mime_type,
            file_path: m.file_path,
            file_size: m.file_size,
            title: m.title,
            description: m.description,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

// ── Media Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaEdge {
    pub cursor: String,
    pub node: GqlMedia,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaConnection {
    pub edges: Vec<GqlMediaEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Media>> for GqlMediaConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Media>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlMediaEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── MediaLink ────────────────────────────────────────────────────────

/// A link between media and an entity.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaLink {
    pub id: ID,
    pub media_id: ID,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub source_id: Option<ID>,
    pub family_id: Option<ID>,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::MediaLink> for GqlMediaLink {
    fn from(l: oxidgene_core::types::MediaLink) -> Self {
        Self {
            id: ID(l.id.to_string()),
            media_id: ID(l.media_id.to_string()),
            person_id: l.person_id.map(|id| ID(id.to_string())),
            event_id: l.event_id.map(|id| ID(id.to_string())),
            source_id: l.source_id.map(|id| ID(id.to_string())),
            family_id: l.family_id.map(|id| ID(id.to_string())),
            sort_order: l.sort_order,
        }
    }
}

// ── FamilySpouse (raw) ──────────────────────────────────────────────

/// Raw family spouse record (returned from mutations).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilySpouse {
    pub id: ID,
    pub family_id: ID,
    pub person_id: ID,
    pub role: GqlSpouseRole,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::FamilySpouse> for GqlFamilySpouse {
    fn from(s: oxidgene_core::types::FamilySpouse) -> Self {
        Self {
            id: ID(s.id.to_string()),
            family_id: ID(s.family_id.to_string()),
            person_id: ID(s.person_id.to_string()),
            role: s.role.into(),
            sort_order: s.sort_order,
        }
    }
}

// ── FamilyChild (raw) ───────────────────────────────────────────────

/// Raw family child record (returned from mutations).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyChild {
    pub id: ID,
    pub family_id: ID,
    pub person_id: ID,
    pub child_type: GqlChildType,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::FamilyChild> for GqlFamilyChild {
    fn from(c: oxidgene_core::types::FamilyChild) -> Self {
        Self {
            id: ID(c.id.to_string()),
            family_id: ID(c.family_id.to_string()),
            person_id: ID(c.person_id.to_string()),
            child_type: c.child_type.into(),
            sort_order: c.sort_order,
        }
    }
}

// ── Note ─────────────────────────────────────────────────────────────

/// A textual note.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNote {
    pub id: ID,
    pub tree_id: ID,
    pub text: String,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub family_id: Option<ID>,
    pub source_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Note> for GqlNote {
    fn from(n: oxidgene_core::types::Note) -> Self {
        Self {
            id: ID(n.id.to_string()),
            tree_id: ID(n.tree_id.to_string()),
            text: n.text,
            person_id: n.person_id.map(|id| ID(id.to_string())),
            event_id: n.event_id.map(|id| ID(id.to_string())),
            family_id: n.family_id.map(|id| ID(id.to_string())),
            source_id: n.source_id.map(|id| ID(id.to_string())),
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}
