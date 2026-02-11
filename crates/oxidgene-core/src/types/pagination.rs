use serde::{Deserialize, Serialize};

/// Relay-style cursor-based pagination info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

/// A single edge in a paginated connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge<T> {
    pub cursor: String,
    pub node: T,
}

/// A Relay-style paginated connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection<T> {
    pub edges: Vec<Edge<T>>,
    pub page_info: PageInfo,
    pub total_count: i64,
}

impl<T> Connection<T> {
    /// Creates an empty connection with no results.
    pub fn empty() -> Self {
        Self {
            edges: Vec::new(),
            page_info: PageInfo {
                has_next_page: false,
                end_cursor: None,
            },
            total_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_connection() {
        let conn: Connection<String> = Connection::empty();
        assert!(conn.edges.is_empty());
        assert!(!conn.page_info.has_next_page);
        assert!(conn.page_info.end_cursor.is_none());
        assert_eq!(conn.total_count, 0);
    }
}
