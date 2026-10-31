//! The write tools, offered only to a session started with
//! [`Access::ReadWrite`](super::Access).
//!
//! Each tool is one REST write: it resolves the tree as the REST tree guard
//! does, then calls the service its REST handler and GraphQL resolver call,
//! with the body type REST deserializes flattened into its parameters. The
//! service validates, checks that every ID belongs to the tree, rewrites the
//! projections and records the change in the history, in one transaction;
//! nothing of that is repeated here.
//!
//! Writes that add or change are annotated `destructiveHint: false`. Those
//! that delete a record or remove a link are annotated `destructiveHint:
//! true`, so a client asks before running them, and come last.

use oxidgene_core::OxidGeneError;
use oxidgene_core::types::RecordKind;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{OxidGeneMcp, TreeParams, respond};
use crate::rest::dto::DeleteSourceQuery;
use crate::rest::tag::{CreateTagRequest, TagView};
use crate::service::citation::{self, CitationPatch, NewCitation};
use crate::service::event::{self, EventPatch, NewEvent, NewWitness};
use crate::service::family::{self, FamilyPatch, NewChild, NewSpouse};
use crate::service::note::{self, NewNote, NotePatch};
use crate::service::person::{self, NewPerson, PersonPatch};
use crate::service::person_name::{self, NewPersonName, PersonNamePatch};
use crate::service::place::{self, NewPlace, PlacePatch};
use crate::service::source::{self, NewSource, SourcePatch};
use crate::service::tag;

// ── Parameters ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePersonParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub person: NewPerson,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdatePersonParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a person of that tree.
    pub person_id: Uuid,
    #[serde(flatten)]
    pub patch: PersonPatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddPersonNameParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the person the name is given to.
    pub person_id: Uuid,
    #[serde(flatten)]
    pub name: NewPersonName,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdatePersonNameParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the person bearing the name.
    pub person_id: Uuid,
    /// ID of the name, the `name_id` of a profile's names.
    pub name_id: Uuid,
    #[serde(flatten)]
    pub patch: PersonNamePatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PersonNameParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the person bearing the name.
    pub person_id: Uuid,
    /// ID of the name, the `name_id` of a profile's names.
    pub name_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateEventParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub event: NewEvent,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateEventParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of an event of that tree.
    pub event_id: Uuid,
    #[serde(flatten)]
    pub patch: EventPatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddWitnessParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the event witnessed.
    pub event_id: Uuid,
    #[serde(flatten)]
    pub witness: NewWitness,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WitnessParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the event witnessed.
    pub event_id: Uuid,
    /// ID of the witness link, from `list_event_witnesses`.
    pub witness_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFamilyParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a family of that tree.
    pub family_id: Uuid,
    #[serde(flatten)]
    pub patch: FamilyPatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddSpouseParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the family.
    pub family_id: Uuid,
    #[serde(flatten)]
    pub spouse: NewSpouse,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddChildParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the family.
    pub family_id: Uuid,
    #[serde(flatten)]
    pub child: NewChild,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpouseLinkParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the family.
    pub family_id: Uuid,
    /// ID of the spouse link: the `id` of an entry of `get_family`'s `spouses`, not the
    /// person's ID.
    pub spouse_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChildLinkParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the family.
    pub family_id: Uuid,
    /// ID of the child link: the `id` of an entry of `get_family`'s `children`, not the
    /// person's ID.
    pub child_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePlaceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub place: NewPlace,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdatePlaceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a place of that tree.
    pub place_id: Uuid,
    #[serde(flatten)]
    pub patch: PlacePatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSourceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub source: NewSource,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateSourceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a source of that tree.
    pub source_id: Uuid,
    #[serde(flatten)]
    pub patch: SourcePatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteSourceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a source of that tree.
    pub source_id: Uuid,
    #[serde(flatten)]
    pub query: DeleteSourceQuery,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCitationParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub citation: NewCitation,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateCitationParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a citation of that tree.
    pub citation_id: Uuid,
    #[serde(flatten)]
    pub patch: CitationPatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CitationParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a citation of that tree.
    pub citation_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateNoteParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub note: NewNote,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateNoteParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a note of that tree.
    pub note_id: Uuid,
    #[serde(flatten)]
    pub patch: NotePatch,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a note of that tree.
    pub note_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTagParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    #[serde(flatten)]
    pub tag: CreateTagRequest,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordTagParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of the tag, from `list_tags` or `create_tag`.
    pub tag_id: Uuid,
    /// The kind of record `record_id` names.
    pub kind: RecordKind,
    /// ID of the record of that kind.
    pub record_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeletePersonParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a person of that tree.
    pub person_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteEventParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of an event of that tree.
    pub event_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteFamilyParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a family of that tree.
    pub family_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeletePlaceParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a place of that tree.
    pub place_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteTagParams {
    /// ID of the tree, from `list_trees`.
    pub tree_id: Uuid,
    /// ID of a tag of that tree.
    pub tag_id: Uuid,
}

// ── Results ──────────────────────────────────────────────────────────────────

/// A tag, and whether this call created it: REST tells the two apart by
/// `201 Created` and `200 OK`, which a tool result has no status for.
#[derive(Debug, Serialize)]
struct CreatedTag {
    #[serde(flatten)]
    tag: TagView,
    created: bool,
}

/// Whether a source was deleted: REST answers `204` deleted, `200` kept.
#[derive(Debug, Serialize)]
struct SourceDeletion {
    deleted: bool,
}

/// The result of a write REST answers with no body, `204 No Content`.
fn no_content() -> Map<String, Value> {
    Map::new()
}

// ── Tools ────────────────────────────────────────────────────────────────────

#[tool_router(router = write_tool_router, vis = "pub(super)")]
impl OxidGeneMcp {
    /// Create a person, with no name yet: give them one with `add_person_name`. Returns
    /// the person.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_person(&self, params: Parameters<CreatePersonParams>) -> CallToolResult {
        let p = params.0;
        respond("create_person", async {
            self.tree(p.tree_id).await?;
            person::create_person(&self.db, &self.profiles, p.tree_id, p.person).await
        })
        .await
    }

    /// Change a person's sex or privacy; an absent field is kept. Returns the person.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_person(&self, params: Parameters<UpdatePersonParams>) -> CallToolResult {
        let p = params.0;
        respond("update_person", async {
            self.tree(p.tree_id).await?;
            person::update_person(&self.db, &self.profiles, p.tree_id, p.person_id, p.patch).await
        })
        .await
    }

    /// Give a person a name: birth name, married name, also known as… Set `is_primary` on
    /// the name the person is shown by. `surname` is the root of the surname, its particle
    /// ("de la", "van der") going in `surname_prefix`. Returns the name.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn add_person_name(&self, params: Parameters<AddPersonNameParams>) -> CallToolResult {
        let p = params.0;
        respond("add_person_name", async {
            self.tree(p.tree_id).await?;
            person_name::create_person_name(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.person_id,
                p.name,
            )
            .await
        })
        .await
    }

    /// Change a person's name: an absent field is kept, a null one is cleared. Returns the
    /// name.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_person_name(
        &self,
        params: Parameters<UpdatePersonNameParams>,
    ) -> CallToolResult {
        let p = params.0;
        respond("update_person_name", async {
            self.tree(p.tree_id).await?;
            person_name::update_person_name(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.person_id,
                p.name_id,
                p.patch,
            )
            .await
        })
        .await
    }

    /// Record an event or an attribute of a person (`person_id`) or of a couple
    /// (`family_id`, for a marriage say), never both: its type, date, place, description.
    /// Returns the event.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_event(&self, params: Parameters<CreateEventParams>) -> CallToolResult {
        let p = params.0;
        respond("create_event", async {
            self.tree(p.tree_id).await?;
            event::create_event(&self.db, &self.profiles, p.tree_id, p.event).await
        })
        .await
    }

    /// Change an event: an absent field is kept, a null one is cleared. Its person or
    /// family never changes. Returns the event.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_event(&self, params: Parameters<UpdateEventParams>) -> CallToolResult {
        let p = params.0;
        respond("update_event", async {
            self.tree(p.tree_id).await?;
            event::update_event(&self.db, &self.profiles, p.tree_id, p.event_id, p.patch).await
        })
        .await
    }

    /// Name a person of the tree as a witness of an event (a godparent at a baptism, a
    /// witness at a marriage), with the relation the record gives. Returns the witness.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn add_event_witness(&self, params: Parameters<AddWitnessParams>) -> CallToolResult {
        let p = params.0;
        respond("add_event_witness", async {
            self.tree(p.tree_id).await?;
            event::add_witness(&self.db, p.tree_id, p.event_id, p.witness).await
        })
        .await
    }

    /// Create a family: a couple, empty until spouses and children are added with
    /// `add_spouse` and `add_child`. Returns the family.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_family(&self, params: Parameters<TreeParams>) -> CallToolResult {
        let p = params.0;
        respond("create_family", async {
            self.tree(p.tree_id).await?;
            family::create_family(&self.db, p.tree_id).await
        })
        .await
    }

    /// Change a family's privacy; an absent field is kept. Returns the family.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_family(&self, params: Parameters<UpdateFamilyParams>) -> CallToolResult {
        let p = params.0;
        respond("update_family", async {
            self.tree(p.tree_id).await?;
            family::update_family(&self.db, p.tree_id, p.family_id, p.patch).await
        })
        .await
    }

    /// Add a person to a family as a spouse (`husband` or `wife`). Returns the spouse link.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn add_spouse(&self, params: Parameters<AddSpouseParams>) -> CallToolResult {
        let p = params.0;
        respond("add_spouse", async {
            self.tree(p.tree_id).await?;
            family::add_spouse(&self.db, &self.profiles, p.tree_id, p.family_id, p.spouse).await
        })
        .await
    }

    /// Add a person to a family as a child, with how they are its child (biological,
    /// adopted, foster…). Returns the child link.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn add_child(&self, params: Parameters<AddChildParams>) -> CallToolResult {
        let p = params.0;
        respond("add_child", async {
            self.tree(p.tree_id).await?;
            family::add_child(&self.db, &self.profiles, p.tree_id, p.family_id, p.child).await
        })
        .await
    }

    /// Create a place: its name, as the tree writes places, and optional coordinates.
    /// Returns the place; events take its ID as `place_id`.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_place(&self, params: Parameters<CreatePlaceParams>) -> CallToolResult {
        let p = params.0;
        respond("create_place", async {
            self.tree(p.tree_id).await?;
            place::create_place(&self.db, p.tree_id, p.place).await
        })
        .await
    }

    /// Change a place's name or coordinates: an absent field is kept, a null coordinate
    /// is cleared. Every event at the place shows the change. Returns the place.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_place(&self, params: Parameters<UpdatePlaceParams>) -> CallToolResult {
        let p = params.0;
        respond("update_place", async {
            self.tree(p.tree_id).await?;
            place::update_place(&self.db, &self.profiles, p.tree_id, p.place_id, p.patch).await
        })
        .await
    }

    /// Create a source: a register, a census, a book… Citations take its ID as
    /// `source_id`. Returns the source.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_source(&self, params: Parameters<CreateSourceParams>) -> CallToolResult {
        let p = params.0;
        respond("create_source", async {
            self.tree(p.tree_id).await?;
            source::create_source(&self.db, p.tree_id, p.source).await
        })
        .await
    }

    /// Change a source: an absent field is kept, a null one is cleared. Returns the source.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_source(&self, params: Parameters<UpdateSourceParams>) -> CallToolResult {
        let p = params.0;
        respond("update_source", async {
            self.tree(p.tree_id).await?;
            source::update_source(&self.db, p.tree_id, p.source_id, p.patch).await
        })
        .await
    }

    /// Cite a source for a person, an event or a family: where in the source (`page`),
    /// what it says (`text`) and how far it is trusted (`confidence`). Returns the
    /// citation.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_citation(&self, params: Parameters<CreateCitationParams>) -> CallToolResult {
        let p = params.0;
        respond("create_citation", async {
            self.tree(p.tree_id).await?;
            citation::create_citation(&self.db, &self.profiles, p.tree_id, p.citation).await
        })
        .await
    }

    /// Change a citation: an absent field is kept, a null one is cleared. What it is
    /// attached to never changes. Returns the citation.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_citation(&self, params: Parameters<UpdateCitationParams>) -> CallToolResult {
        let p = params.0;
        respond("update_citation", async {
            self.tree(p.tree_id).await?;
            citation::update_citation(&self.db, &self.profiles, p.tree_id, p.citation_id, p.patch)
                .await
        })
        .await
    }

    /// Write a note about a person, an event, a family, a source, a repository or a
    /// document (`media_id`). The text may be simple HTML, which is sanitized. Returns the
    /// note.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = false
    ))]
    async fn create_note(&self, params: Parameters<CreateNoteParams>) -> CallToolResult {
        let p = params.0;
        respond("create_note", async {
            self.tree(p.tree_id).await?;
            note::create_note(&self.db, &self.profiles, p.tree_id, p.note).await
        })
        .await
    }

    /// Replace a note's text. Returns the note.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn update_note(&self, params: Parameters<UpdateNoteParams>) -> CallToolResult {
        let p = params.0;
        respond("update_note", async {
            self.tree(p.tree_id).await?;
            note::update_note(&self.db, &self.profiles, p.tree_id, p.note_id, p.patch).await
        })
        .await
    }

    /// Create a tag in the tree's vocabulary, or return the existing tag whose label is
    /// the same once case and accents are folded; `created` says which. Returns the tag.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn create_tag(&self, params: Parameters<CreateTagParams>) -> CallToolResult {
        let p = params.0;
        respond("create_tag", async {
            self.tree(p.tree_id).await?;
            let (usage, created) =
                tag::create(&self.db, p.tree_id, &p.tag.label, p.tag.color).await?;
            Ok::<_, OxidGeneError>(CreatedTag {
                tag: usage.into(),
                created,
            })
        })
        .await
    }

    /// Put a tag on a record of any kind. Doing it again changes nothing.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn tag_record(&self, params: Parameters<RecordTagParams>) -> CallToolResult {
        let p = params.0;
        respond("tag_record", async {
            self.tree(p.tree_id).await?;
            tag::tag_record(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.tag_id,
                p.kind,
                p.record_id,
            )
            .await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    // ── Removals and deletions ───────────────────────────────────────────────

    /// Take a tag off a record. Doing it again, or on a record without the tag, changes
    /// nothing.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn untag_record(&self, params: Parameters<RecordTagParams>) -> CallToolResult {
        let p = params.0;
        respond("untag_record", async {
            self.tree(p.tree_id).await?;
            tag::untag_record(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.tag_id,
                p.kind,
                p.record_id,
            )
            .await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Remove a witness from an event. The person stays in the tree.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn remove_event_witness(&self, params: Parameters<WitnessParams>) -> CallToolResult {
        let p = params.0;
        respond("remove_event_witness", async {
            self.tree(p.tree_id).await?;
            event::remove_witness(&self.db, p.tree_id, Some(p.event_id), p.witness_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Remove a spouse from a family, by the spouse link's ID. The person stays in the tree.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn remove_spouse(&self, params: Parameters<SpouseLinkParams>) -> CallToolResult {
        let p = params.0;
        respond("remove_spouse", async {
            self.tree(p.tree_id).await?;
            family::remove_spouse(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.family_id,
                p.spouse_id,
            )
            .await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Remove a child from a family, by the child link's ID. The person stays in the tree.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn remove_child(&self, params: Parameters<ChildLinkParams>) -> CallToolResult {
        let p = params.0;
        respond("remove_child", async {
            self.tree(p.tree_id).await?;
            family::remove_child(&self.db, &self.profiles, p.tree_id, p.family_id, p.child_id)
                .await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete one of a person's names.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_person_name(&self, params: Parameters<PersonNameParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_person_name", async {
            self.tree(p.tree_id).await?;
            person_name::delete_person_name(
                &self.db,
                &self.profiles,
                p.tree_id,
                p.person_id,
                p.name_id,
            )
            .await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete an event.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_event(&self, params: Parameters<DeleteEventParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_event", async {
            self.tree(p.tree_id).await?;
            event::delete_event(&self.db, &self.profiles, p.tree_id, p.event_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a citation.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_citation(&self, params: Parameters<CitationParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_citation", async {
            self.tree(p.tree_id).await?;
            citation::delete_citation(&self.db, &self.profiles, p.tree_id, p.citation_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a note.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_note(&self, params: Parameters<NoteParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_note", async {
            self.tree(p.tree_id).await?;
            note::delete_note(&self.db, &self.profiles, p.tree_id, p.note_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a tag from the vocabulary, and take it off every record carrying it.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_tag(&self, params: Parameters<DeleteTagParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_tag", async {
            self.tree(p.tree_id).await?;
            tag::delete(&self.db, &self.profiles, p.tree_id, p.tag_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a source. With `only_if_unused`, a source still cited is kept instead, and
    /// `deleted` is false.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_source(&self, params: Parameters<DeleteSourceParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_source", async {
            self.tree(p.tree_id).await?;
            let deleted =
                source::delete_source(&self.db, p.tree_id, p.source_id, p.query.only_if_unused)
                    .await?;
            Ok::<_, OxidGeneError>(SourceDeletion { deleted })
        })
        .await
    }

    /// Delete a place. The events at the place lose it.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_place(&self, params: Parameters<DeletePlaceParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_place", async {
            self.tree(p.tree_id).await?;
            place::delete_place(&self.db, &self.profiles, p.tree_id, p.place_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a family (a couple). Its spouses and children stay in the tree.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_family(&self, params: Parameters<DeleteFamilyParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_family", async {
            self.tree(p.tree_id).await?;
            family::delete_family(&self.db, &self.profiles, p.tree_id, p.family_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }

    /// Delete a person. The families they belong to stay in the tree.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn delete_person(&self, params: Parameters<DeletePersonParams>) -> CallToolResult {
        let p = params.0;
        respond("delete_person", async {
            self.tree(p.tree_id).await?;
            person::delete_person(&self.db, &self.profiles, p.tree_id, p.person_id).await?;
            Ok::<_, OxidGeneError>(no_content())
        })
        .await
    }
}
