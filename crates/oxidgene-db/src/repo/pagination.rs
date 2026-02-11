//! Cursor-based pagination helpers.
//!
//! Uses UUID v7 as the cursor — since UUID v7 is time-ordered, `ORDER BY id`
//! gives chronological insertion order. The cursor is the hex-encoded UUID string.

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Edge, PageInfo};
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Select};
use uuid::Uuid;

/// Default page size.
pub const DEFAULT_PAGE_SIZE: u64 = 25;
/// Maximum page size.
pub const MAX_PAGE_SIZE: u64 = 100;

/// Parameters for cursor-based pagination.
#[derive(Debug, Clone)]
pub struct PaginationParams {
    /// Number of items to return.
    pub first: u64,
    /// Cursor to start after (UUID string).
    pub after: Option<String>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            first: DEFAULT_PAGE_SIZE,
            after: None,
        }
    }
}

impl PaginationParams {
    /// Clamp `first` to [1, MAX_PAGE_SIZE].
    pub fn clamped_first(&self) -> u64 {
        self.first.clamp(1, MAX_PAGE_SIZE)
    }

    /// Decode the `after` cursor into a UUID.
    pub fn decode_cursor(&self) -> Result<Option<Uuid>, OxidGeneError> {
        match &self.after {
            None => Ok(None),
            Some(cursor) => {
                let id = Uuid::parse_str(cursor)
                    .map_err(|_| OxidGeneError::Validation(format!("Invalid cursor: {cursor}")))?;
                Ok(Some(id))
            }
        }
    }
}

/// Encode a UUID as a pagination cursor string.
pub fn encode_cursor(id: &Uuid) -> String {
    id.to_string()
}

/// Execute a paginated query returning a `Connection<T>`.
///
/// # Parameters
/// - `db`: database connection
/// - `base_query`: a `Select<E>` with all filters applied but **no** ordering or limit
/// - `id_column`: the column to use for cursor comparison and ordering
/// - `params`: pagination parameters
/// - `convert`: converts a SeaORM model into `(Uuid, T)` — the UUID is used for the cursor
pub async fn paginate<E, M, T, F>(
    db: &DatabaseConnection,
    base_query: Select<E>,
    id_column: E::Column,
    params: &PaginationParams,
    convert: F,
) -> Result<Connection<T>, OxidGeneError>
where
    E: EntityTrait<Model = M>,
    M: sea_orm::ModelTrait + sea_orm::FromQueryResult + Send + Sync,
    T: Clone,
    F: Fn(M) -> (Uuid, T),
{
    let limit = params.clamped_first();
    let cursor_id = params.decode_cursor()?;

    // Count total matching rows (before cursor/limit).
    let total_count = PaginatorTrait::count(base_query.clone(), db)
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

    // Build the paginated query.
    let mut query = base_query.order_by(id_column, Order::Asc);

    if let Some(after_id) = cursor_id {
        query = query.filter(Condition::all().add(id_column.gt(after_id)));
    }

    // Fetch limit + 1 to detect has_next_page.
    let rows = query
        .limit(limit + 1)
        .all(db)
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))?;

    let has_next_page = rows.len() as u64 > limit;
    let items: Vec<M> = rows.into_iter().take(limit as usize).collect();

    let edges: Vec<Edge<T>> = items
        .into_iter()
        .map(|model| {
            let (id, node) = convert(model);
            Edge {
                cursor: encode_cursor(&id),
                node,
            }
        })
        .collect();

    let end_cursor = edges.last().map(|e| e.cursor.clone());

    Ok(Connection {
        edges,
        page_info: PageInfo {
            has_next_page,
            end_cursor,
        },
        total_count: total_count as i64,
    })
}
