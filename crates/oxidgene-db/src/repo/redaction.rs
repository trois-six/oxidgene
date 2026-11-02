//! What a restricted viewer may not see of a tree's records
//! (docs/privacy.md §3.2, §7): what a paged list leaves out in SQL
//! ([`LeaveOut`]), and the hidden records themselves, read in one statement
//! for the direct reads and the nested lists that decide record by record
//! ([`RedactionRepo::hidden_records`]). The rules are [`Unseen`]'s; nothing
//! here decides.

use oxidgene_core::error::OxidGeneError;
use sea_orm::sea_query::{PostgresQueryBuilder, SqliteQueryBuilder};
use sea_orm::{ColumnTrait, Condition};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use uuid::Uuid;

use oxidgene_core::types::RecordKind;

use crate::repo::db_err;
use crate::repo::hiding::Unseen;

/// What a paged list leaves out for a viewer who may not see everything,
/// applied in SQL so that the list's pages and its `total_count` agree.
#[derive(Debug, Clone, Copy)]
pub struct LeaveOut {
    /// The persons and records the viewer may not see.
    pub unseen: Unseen,
    /// For notes: leave out the confidential ones too.
    pub confidential: bool,
}

impl LeaveOut {
    /// Nothing left out: the owner's lists.
    pub const NONE: LeaveOut = LeaveOut {
        unseen: Unseen::NONE,
        confidential: false,
    };

    /// The predicate keeping what the viewer may see of a list of records
    /// of `kind` whose identifier is `column`.
    pub(crate) fn keeping<C: ColumnTrait>(&self, kind: RecordKind, column: C) -> Condition {
        self.unseen.keeping(kind, column)
    }
}

/// The reads of the redaction's hidden records.
pub struct RedactionRepo;

impl RedactionRepo {
    /// The redaction generation of tree `tree_id`
    /// (`projection_state.redaction_generation`, docs/data-model.md §4.1):
    /// triggers move it in the transaction of every write to a row a
    /// restricted viewer's redaction reads, whatever process makes it, so a
    /// redaction built after reading it stays what a new build would make
    /// while it has not moved. `None` for a tree that does not exist (any
    /// more).
    ///
    /// One primary-key read.
    pub async fn generation(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Option<i64>, OxidGeneError> {
        let backend = db.get_database_backend();
        let placeholder = match backend {
            DbBackend::Postgres => "$1",
            _ => "?",
        };
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT redaction_generation FROM projection_state WHERE tree_id = {placeholder}"
                ),
                [tree_id.into()],
            ))
            .await
            .map_err(db_err)?;
        row.map(|row| row.try_get::<i64>("", "redaction_generation"))
            .transpose()
            .map_err(db_err)
    }

    /// Lock tree `tree_id`'s `projection_state` row until the transaction
    /// `txn` ends, under PostgreSQL: a rebuild of a derived row the
    /// redaction reads takes it before `sosa_state` or a scope's row, the
    /// order every writer of the tree takes them in (docs/data-model.md
    /// §4.1). SQLite has one writer and no row lock: nothing to do.
    pub async fn lock_tree(txn: &impl ConnectionTrait, tree_id: Uuid) -> Result<(), OxidGeneError> {
        let backend = txn.get_database_backend();
        if backend != DbBackend::Postgres {
            return Ok(());
        }
        txn.query_one_raw(Statement::from_sql_and_values(
            backend,
            "SELECT tree_id FROM projection_state WHERE tree_id = $1 FOR UPDATE",
            [tree_id.into()],
        ))
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Every record of the tree `unseen` hides, as `(kind, id)`: `kind` is
    /// `event`, `citation`, `media`, `source`, `repository`, `place`,
    /// `note`, `media_link` or `vignette`. One statement, its bound values
    /// the same few however many records are hidden.
    pub async fn hidden_records(
        db: &impl ConnectionTrait,
        unseen: &Unseen,
    ) -> Result<Vec<(String, Uuid)>, OxidGeneError> {
        let backend = db.get_database_backend();
        let query = unseen.hidden_records_query();
        let (sql, values) = match backend {
            DbBackend::Postgres => query.build(PostgresQueryBuilder),
            _ => query.build(SqliteQueryBuilder),
        };
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(backend, sql, values))
            .await
            .map_err(db_err)?;
        rows.iter()
            .map(|row| {
                Ok((
                    row.try_get::<String>("", "kind").map_err(db_err)?,
                    row.try_get::<Uuid>("", "id").map_err(db_err)?,
                ))
            })
            .collect()
    }

    /// The person each of the media links `links` attaches a medium to, one
    /// entry per link that names a person: what a shown person's media
    /// count leaves out when those links are hidden.
    pub async fn persons_of_media_links(
        db: &impl ConnectionTrait,
        links: &[Uuid],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        use crate::entities::media_link;
        use sea_orm::{EntityTrait, QueryFilter, QuerySelect};
        let mut persons = Vec::new();
        for chunk in links.chunks(1_000) {
            let found: Vec<Option<Uuid>> = media_link::Entity::find()
                .select_only()
                .column(media_link::Column::PersonId)
                .filter(media_link::Column::Id.is_in(chunk.iter().copied()))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            persons.extend(found.into_iter().flatten());
        }
        Ok(persons)
    }

    /// How many live confidential notes are about each person of tree
    /// `tree_id` who has any: what their note count leaves out for a viewer
    /// without `read_confidential`.
    pub async fn confidential_notes_by_person(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(Uuid, u32)>, OxidGeneError> {
        use crate::entities::note;
        use sea_orm::{EntityTrait, QueryFilter, QuerySelect};
        let persons: Vec<Option<Uuid>> = note::Entity::find()
            .select_only()
            .column(note::Column::PersonId)
            .filter(note::Column::TreeId.eq(tree_id))
            .filter(note::Column::Confidential.eq(true))
            .filter(note::Column::PersonId.is_not_null())
            .filter(note::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut counts: std::collections::HashMap<Uuid, u32> = std::collections::HashMap::new();
        for person in persons.into_iter().flatten() {
            *counts.entry(person).or_default() += 1;
        }
        Ok(counts.into_iter().collect())
    }
}
