//! Access scopes (docs/data-model.md §6.5, docs/authorization.md §6.5):
//! `access_scope`, the definition of the persons a share is limited to, and
//! `access_scope_person`, those persons materialized.
//!
//! The persons are derived data, as an exclusion's are: the triggers that
//! move a tree's `sosa_state.generation` on every change to a family link or
//! a deletion (docs/data-model.md §4.8) make every scope of the tree stale,
//! and the API rebuilds a stale scope at the next read that needs it. What
//! a definition reaches is `oxidgene_core::sharing`'s to decide.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::sharing::ScopeDefinition;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbBackend, Set, Statement, Value};
use uuid::Uuid;

use crate::entities::access_scope::{self, ActiveModel, Column, Entity};
use crate::repo::batch::{MAX_BOUND_IDS, in_chunks};
use crate::repo::db_err;

/// Rows one insert statement writes: two values each.
const INSERT_BATCH: usize = 2_000;

/// One stored scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessScope {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub definition: ScopeDefinition,
    /// The tree's ancestry generation its persons were built from.
    pub built_generation: Option<i64>,
    pub created_at: DateTime<Utc>,
}

fn into_domain(model: access_scope::Model) -> Result<AccessScope, OxidGeneError> {
    let definition = serde_json::from_str(&model.definition)
        .map_err(|error| OxidGeneError::Database(format!("unreadable scope: {error}")))?;
    Ok(AccessScope {
        id: model.id,
        tree_id: model.tree_id,
        definition,
        built_generation: model.built_generation,
        created_at: model.created_at,
    })
}

/// `n` placeholders for `backend`, numbered from `first` on PostgreSQL.
fn placeholders(backend: DbBackend, first: usize, n: usize) -> String {
    (first..first + n)
        .map(|i| match backend {
            DbBackend::Sqlite => "?".to_string(),
            _ => format!("${i}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads and writes of the scopes and their persons.
pub struct AccessScopeRepo;

impl AccessScopeRepo {
    /// Store a new scope of `tree_id` with `definition`, its persons not
    /// built yet.
    pub async fn create(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        definition: &ScopeDefinition,
    ) -> Result<AccessScope, OxidGeneError> {
        let json = serde_json::to_string(definition)
            .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
        let model = ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree_id),
            definition: Set(json),
            built_generation: Set(None),
            created_at: Set(Utc::now()),
        }
        .insert(db)
        .await
        .map_err(db_err)?;
        into_domain(model)
    }

    /// Scope `id`, if it exists.
    pub async fn get(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Option<AccessScope>, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .transpose()
    }

    /// The scopes of `ids`, by id.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, AccessScope>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|model| into_domain(model).map(|scope| (scope.id, scope)))
            .collect()
    }

    /// Delete scope `id`, and with it what it limits: a membership, an
    /// invitation or a share link holding it goes too, never widened.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
        Ok(())
    }

    /// Re-root every scope of `tree_id` whose root is `from` at `to`: a
    /// merge that absorbs a scope's root moves the scope to the person kept
    /// (docs/authorization.md §6.5). Their persons are rebuilt at the next
    /// read, the merge having moved the tree's ancestry generation.
    pub async fn move_root(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        from: Uuid,
        to: Uuid,
    ) -> Result<(), OxidGeneError> {
        let scopes = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        for model in scopes {
            let mut scope = into_domain(model.clone())?;
            if scope.definition.person_id != from {
                continue;
            }
            scope.definition.person_id = to;
            let json = serde_json::to_string(&scope.definition)
                .map_err(|error| OxidGeneError::Internal(error.to_string()))?;
            let mut active: ActiveModel = model.into();
            active.definition = Set(json);
            active.built_generation = Set(None);
            active.update(db).await.map_err(db_err)?;
        }
        Ok(())
    }

    /// The tree's current SOSA ancestry generation, and the one scope `id`
    /// was built from (none before its first build): the scope is current
    /// while they are equal. The generation is read before any family link a
    /// rebuild reads, so a change committed meanwhile leaves the scope stale
    /// rather than marked current.
    pub async fn generations(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Option<(i64, Option<i64>)>, OxidGeneError> {
        let backend = db.get_database_backend();
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT s.generation, a.built_generation FROM access_scope a \
                     JOIN sosa_state s ON s.tree_id = a.tree_id WHERE a.id = {}",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(id)],
            ))
            .await
            .map_err(db_err)?;
        row.map(|row| {
            Ok((
                row.try_get("", "generation").map_err(db_err)?,
                row.try_get("", "built_generation").map_err(db_err)?,
            ))
        })
        .transpose()
    }

    /// Replace the persons of scope `id` with `persons`, current as of
    /// `generation`.
    pub async fn replace_persons(
        db: &impl ConnectionTrait,
        id: Uuid,
        persons: &HashSet<Uuid>,
        generation: i64,
    ) -> Result<(), OxidGeneError> {
        let backend = db.get_database_backend();
        db.execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "DELETE FROM access_scope_person WHERE scope_id = {}",
                placeholders(backend, 1, 1)
            ),
            [Value::from(id)],
        ))
        .await
        .map_err(db_err)?;
        let persons: Vec<Uuid> = persons.iter().copied().collect();
        for batch in persons.chunks(INSERT_BATCH) {
            let tuples = (0..batch.len())
                .map(|i| format!("({})", placeholders(backend, 2 * i + 1, 2)))
                .collect::<Vec<_>>()
                .join(", ");
            let values = batch
                .iter()
                .flat_map(|person| [Value::from(id), Value::from(*person)]);
            db.execute_raw(Statement::from_sql_and_values(
                backend,
                format!("INSERT INTO access_scope_person (scope_id, person_id) VALUES {tuples}"),
                values,
            ))
            .await
            .map_err(db_err)?;
        }
        Entity::update_many()
            .col_expr(
                Column::BuiltGeneration,
                sea_orm::sea_query::Expr::value(generation),
            )
            .filter(Column::Id.eq(id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// The persons of scope `id`, as last built.
    pub async fn persons(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        let backend = db.get_database_backend();
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT person_id FROM access_scope_person WHERE scope_id = {}",
                    placeholders(backend, 1, 1)
                ),
                [Value::from(id)],
            ))
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|row| row.try_get::<Uuid>("", "person_id").map_err(db_err))
            .collect()
    }

    /// Whether person `person_id` is in scope `id`, as last built: one
    /// indexed read.
    pub async fn contains(
        db: &impl ConnectionTrait,
        id: Uuid,
        person_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let backend = db.get_database_backend();
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT 1 AS found FROM access_scope_person \
                     WHERE scope_id = {} AND person_id = {}",
                    placeholders(backend, 1, 1),
                    placeholders(backend, 2, 1)
                ),
                [Value::from(id), Value::from(person_id)],
            ))
            .await
            .map_err(db_err)?;
        Ok(row.is_some())
    }

    /// What the records `ids` of `tree_id` name: the persons they are or
    /// belong to — a person itself, the owner of an event, a name, a note,
    /// a citation, a witness, and the persons a medium, a media link or a
    /// vignette shows — and the families they are or belong to, with those
    /// families' spouses. An identifier of nothing in the tree names
    /// nothing. What an Editor's write is checked against
    /// (docs/authorization.md §3.3, docs/privacy.md §6.4).
    pub async fn records_named(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<NamedRecords, OxidGeneError> {
        let mut named = Named::default();
        // A write's body may name any number of ids. The media-link query
        // binds a slice and the media it names, at most twice as many, so a
        // third of the bound stays under it.
        for slice in ids.chunks(MAX_BOUND_IDS / 3) {
            named.records(db, tree_id, slice).await?;
            named.attachments(db, tree_id, slice).await?;
        }
        named.events_and_families(db, tree_id).await?;
        let spouses = named.spouses(db).await?;
        Ok(NamedRecords {
            persons: named.persons,
            families: named.families,
            spouses,
        })
    }

    /// Those of `persons` linked to a live family, as a spouse or a child.
    pub async fn linked_persons(
        db: &impl ConnectionTrait,
        persons: &[Uuid],
    ) -> Result<HashSet<Uuid>, OxidGeneError> {
        use crate::entities::{family_child, family_spouse};
        use sea_orm::QuerySelect;
        if persons.is_empty() {
            return Ok(HashSet::new());
        }
        let mut linked: HashSet<Uuid> = in_chunks(persons, |slice| async move {
            family_spouse::Entity::find()
                .select_only()
                .column(family_spouse::Column::PersonId)
                .filter(family_spouse::Column::PersonId.is_in(slice))
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?
        .into_iter()
        .collect();
        linked.extend(
            in_chunks(persons, |slice| async move {
                family_child::Entity::find()
                    .select_only()
                    .column(family_child::Column::PersonId)
                    .filter(family_child::Column::PersonId.is_in(slice))
                    .into_tuple::<Uuid>()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?,
        );
        Ok(linked)
    }

    /// Delete the scopes nothing holds any more — no membership, invitation,
    /// share link or tree visibility names them — returning how many: what
    /// an expired membership or invitation leaves behind. The worker's
    /// maintenance pass runs it.
    pub async fn delete_orphans(db: &impl ConnectionTrait) -> Result<u64, OxidGeneError> {
        let result = db
            .execute_raw(Statement::from_string(
                db.get_database_backend(),
                "DELETE FROM access_scope WHERE \
                 NOT EXISTS (SELECT 1 FROM tree_membership m WHERE m.scope_id = access_scope.id) \
                 AND NOT EXISTS (SELECT 1 FROM tree_invitation i WHERE i.scope_id = access_scope.id) \
                 AND NOT EXISTS (SELECT 1 FROM share_link l WHERE l.scope_id = access_scope.id) \
                 AND NOT EXISTS (SELECT 1 FROM tree t WHERE t.visitor_scope_id = access_scope.id)",
            ))
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected())
    }
}

/// What a write names ([`AccessScopeRepo::records_named`]).
#[derive(Debug, Default)]
pub struct NamedRecords {
    /// The persons it names itself, or through a record of theirs.
    pub persons: HashSet<Uuid>,
    /// The families it names itself, or through a record of theirs.
    pub families: HashSet<Uuid>,
    /// The spouses of those families.
    pub spouses: HashSet<Uuid>,
}

impl NamedRecords {
    /// Every person named: those named directly and the spouses of the
    /// families named.
    pub fn all_persons(&self) -> HashSet<Uuid> {
        self.persons.union(&self.spouses).copied().collect()
    }
}

/// What a write names, gathered kind by kind: the persons, and the events
/// and families whose persons it names too.
#[derive(Default)]
struct Named {
    persons: HashSet<Uuid>,
    events: HashSet<Uuid>,
    families: HashSet<Uuid>,
}

/// The persons, events and families a note, a citation or a media link is
/// attached to.
type Owners = (Option<Uuid>, Option<Uuid>, Option<Uuid>);

impl Named {
    /// `ids` that are persons, families, names or witnesses, and events to
    /// read.
    async fn records(
        &mut self,
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        use crate::entities::{event_witness, family, person, person_name};
        use sea_orm::QuerySelect;
        let ids_of = || ids.iter().copied();
        self.persons.extend(
            person::Entity::find()
                .select_only()
                .column(person::Column::Id)
                .filter(person::Column::TreeId.eq(tree_id))
                .filter(person::Column::Id.is_in(ids_of()))
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        self.families.extend(
            family::Entity::find()
                .select_only()
                .column(family::Column::Id)
                .filter(family::Column::TreeId.eq(tree_id))
                .filter(family::Column::Id.is_in(ids_of()))
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        self.events.extend(ids_of());
        // A name belongs to a person; a witness is one, of an event.
        self.persons.extend(
            person_name::Entity::find()
                .select_only()
                .column(person_name::Column::PersonId)
                .filter(person_name::Column::Id.is_in(ids_of()))
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        let witnesses: Vec<(Uuid, Uuid)> = event_witness::Entity::find()
            .select_only()
            .column(event_witness::Column::PersonId)
            .column(event_witness::Column::EventId)
            .filter(event_witness::Column::Id.is_in(ids_of()))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        for (person, event) in witnesses {
            self.persons.insert(person);
            self.events.insert(event);
        }
        Ok(())
    }

    /// What the notes, citations, media links, vignettes and media among
    /// `ids` are attached to: a medium shows what its links name, a
    /// vignette what it names and what its medium shows.
    async fn attachments(
        &mut self,
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<(), OxidGeneError> {
        use crate::entities::{citation, media_link, note, vignette};
        use sea_orm::QuerySelect;
        let ids_of = || ids.iter().copied();
        let mut owners: Vec<Owners> = note::Entity::find()
            .select_only()
            .columns([
                note::Column::PersonId,
                note::Column::EventId,
                note::Column::FamilyId,
            ])
            .filter(note::Column::TreeId.eq(tree_id))
            .filter(note::Column::Id.is_in(ids_of()))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        owners.extend(
            citation::Entity::find()
                .select_only()
                .columns([
                    citation::Column::PersonId,
                    citation::Column::EventId,
                    citation::Column::FamilyId,
                ])
                .filter(citation::Column::Id.is_in(ids_of()))
                .into_tuple::<Owners>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        let vignettes: Vec<(Option<Uuid>, Option<Uuid>, Uuid)> = vignette::Entity::find()
            .select_only()
            .columns([
                vignette::Column::PersonId,
                vignette::Column::EventId,
                vignette::Column::MediaId,
            ])
            .filter(vignette::Column::Id.is_in(ids_of()))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut media: HashSet<Uuid> = ids_of().collect();
        for (person, event, medium) in vignettes {
            owners.push((person, event, None));
            media.insert(medium);
        }
        owners.extend(
            media_link::Entity::find()
                .select_only()
                .columns([
                    media_link::Column::PersonId,
                    media_link::Column::EventId,
                    media_link::Column::FamilyId,
                ])
                .filter(
                    sea_orm::Condition::any()
                        .add(media_link::Column::Id.is_in(ids_of()))
                        .add(media_link::Column::MediaId.is_in(media)),
                )
                .into_tuple::<Owners>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        for (person, event, family) in owners {
            self.persons.extend(person);
            self.events.extend(event);
            self.families.extend(family);
        }
        Ok(())
    }

    /// The persons and families of the events gathered: an event belongs
    /// to its person, or to its family.
    async fn events_and_families(
        &mut self,
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        use crate::entities::event;
        use sea_orm::QuerySelect;
        let events: Vec<Uuid> = self.events.iter().copied().collect();
        let event_owners: Vec<(Option<Uuid>, Option<Uuid>)> =
            in_chunks(&events, |slice| async move {
                event::Entity::find()
                    .select_only()
                    .columns([event::Column::PersonId, event::Column::FamilyId])
                    .filter(event::Column::TreeId.eq(tree_id))
                    .filter(event::Column::Id.is_in(slice))
                    .into_tuple()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?;
        for (person, family) in event_owners {
            self.persons.extend(person);
            self.families.extend(family);
        }
        Ok(())
    }

    /// The spouses of the families gathered.
    async fn spouses(&self, db: &impl ConnectionTrait) -> Result<HashSet<Uuid>, OxidGeneError> {
        use crate::entities::family_spouse;
        use sea_orm::QuerySelect;
        if self.families.is_empty() {
            return Ok(HashSet::new());
        }
        let families: Vec<Uuid> = self.families.iter().copied().collect();
        Ok(in_chunks(&families, |slice| async move {
            family_spouse::Entity::find()
                .select_only()
                .column(family_spouse::Column::PersonId)
                .filter(family_spouse::Column::FamilyId.is_in(slice))
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?
        .into_iter()
        .collect())
    }
}
