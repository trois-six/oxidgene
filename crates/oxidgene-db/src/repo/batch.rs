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
