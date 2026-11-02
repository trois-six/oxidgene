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
}
