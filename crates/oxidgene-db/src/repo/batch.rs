//! Reading rows by a list of ids that may be longer than a query can bind.
//!
//! SQLite accepts at most 32 766 bound parameters in one statement and
//! PostgreSQL 65 535. Whole-tree reads pass one id per person, family or
//! medium, so before this every tree past about 32 000 persons failed to
//! import or rebuild with "too many SQL variables".

use std::future::Future;

use oxidgene_core::error::OxidGeneError;
use uuid::Uuid;

/// Most ids one `IN (…)` list binds. The rest of a query binds a few more
/// values, and a list this long is already far past where it stops helping.
pub(crate) const MAX_BOUND_IDS: usize = 10_000;

/// The condition that `column` names none of `ids`: one `NOT IN` per
/// bounded slice, all of them holding. What a list leaves out for a viewer
/// who may not see some persons (docs/privacy.md §7).
pub fn none_of<C: sea_orm::ColumnTrait>(column: C, ids: &[Uuid]) -> sea_orm::Condition {
    ids.chunks(MAX_BOUND_IDS)
        .fold(sea_orm::Condition::all(), |condition, chunk| {
            condition.add(column.is_not_in(chunk.iter().copied()))
        })
}

/// The persons a list leaves out for a restricted viewer (docs/privacy.md
/// §7): `ids`, and with a `scope` everyone outside the access scope, read
/// from `access_scope_person` (docs/authorization.md §6.5).
#[derive(Debug, Clone, Copy, Default)]
pub struct Unseen<'a> {
    pub ids: &'a [Uuid],
    pub scope: Option<Uuid>,
}

impl Unseen<'_> {
    /// The condition that person `column` names someone the viewer sees.
    pub fn condition<C: sea_orm::ColumnTrait>(&self, column: C) -> sea_orm::Condition {
        use sea_orm::sea_query::{Alias, Expr, ExprTrait, Query};
        let condition = none_of(column, self.ids);
        match self.scope {
            Some(scope) => condition.add(
                column.in_subquery(
                    Query::select()
                        .column(Alias::new("person_id"))
                        .from(Alias::new("access_scope_person"))
                        .and_where(ExprTrait::eq(Expr::col(Alias::new("scope_id")), scope))
                        .to_owned(),
                ),
            ),
            None => condition,
        }
    }
}

/// Run `query` over `ids` one bounded slice at a time and concatenate the rows.
///
/// An empty `ids` runs no query at all.
pub(crate) async fn in_chunks<T, F, Fut>(
    ids: &[Uuid],
    mut query: F,
) -> Result<Vec<T>, OxidGeneError>
where
    F: FnMut(Vec<Uuid>) -> Fut,
    Fut: Future<Output = Result<Vec<T>, OxidGeneError>>,
{
    let mut rows = Vec::new();
    for chunk in ids.chunks(MAX_BOUND_IDS) {
        rows.extend(query(chunk.to_vec()).await?);
    }
    Ok(rows)
}
