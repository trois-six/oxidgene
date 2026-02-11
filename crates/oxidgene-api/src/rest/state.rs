//! Shared application state for Axum handlers.

use sea_orm::DatabaseConnection;

/// Shared state available to all Axum handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
}

impl AppState {
    /// Create a new `AppState` with the given database connection.
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}
