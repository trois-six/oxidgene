//! HTTP API client for communicating with the OxidGene backend.
//!
//! Provides a typed client wrapping [`reqwest::Client`] that maps to the
//! REST API defined in `oxidgene-api`.  All methods return domain types
//! from [`oxidgene_core`] directly, since those types already derive
//! `Serialize` / `Deserialize`.

use oxidgene_core::types::{
    Citation, Connection, Event, Family, FamilyChild, FamilySpouse, Note, Person, PersonAncestry,
    PersonName, Place, Source, Tree,
};
use oxidgene_core::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Re-usable request / response DTOs (client-side mirrors) ─────────

/// Paginated response returned by list endpoints.
/// Re-uses the same shape as `oxidgene_core::types::Connection<T>`.
type PaginatedResponse<T> = Connection<T>;

// ── Tree request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateTreeBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateTreeBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

// ── Person request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonBody {
    pub sex: Sex,
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sex: Option<Sex>,
}

// ── PersonName request bodies ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonNameBody {
    pub name_type: NameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonNameBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_type: Option<NameType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_names: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_primary: Option<bool>,
}

// ── Family member request bodies ────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AddSpouseBody {
    pub person_id: Uuid,
    pub role: SpouseRole,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct AddChildBody {
    pub person_id: Uuid,
    pub child_type: ChildType,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Event request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateEventBody {
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<chrono::NaiveDate>,
    pub place_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateEventBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_sort: Option<Option<chrono::NaiveDate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

// ── Place request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePlaceBody {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UpdatePlaceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<Option<f64>>,
}

// ── Source request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateSourceBody {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSourceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_name: Option<Option<String>>,
}

// ── Citation request bodies ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateCitationBody {
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub page: Option<String>,
    pub confidence: Confidence,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateCitationBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Option<String>>,
}

// ── Note request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateNoteBody {
    pub text: String,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct UpdateNoteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// ── GEDCOM DTOs ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImportGedcomBody {
    pub gedcom: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportGedcomResult {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub media_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportGedcomResult {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

// ── API Client ──────────────────────────────────────────────────────

/// Typed HTTP client for the OxidGene REST API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
}

/// Errors returned by the API client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
}

impl ApiClient {
    /// Create a new API client pointing at the given base URL.
    ///
    /// The `base_url` should include scheme and port, e.g.
    /// `http://127.0.0.1:3000`.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Helper: send a GET request and deserialize JSON response.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let resp = self.client.get(self.url(path)).send().await?;
        Self::handle_response(resp).await
    }

    /// Helper: send a GET request with query parameters.
    async fn get_with_query<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<T, ApiError> {
        let resp = self.client.get(self.url(path)).query(query).send().await?;
        Self::handle_response(resp).await
    }

    /// Helper: send a POST request with a JSON body.
    async fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let resp = self.client.post(self.url(path)).json(body).send().await?;
        Self::handle_response(resp).await
    }

    /// Helper: send a PUT request with a JSON body.
    async fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let resp = self.client.put(self.url(path)).json(body).send().await?;
        Self::handle_response(resp).await
    }

    /// Helper: send a DELETE request expecting 204 No Content.
    async fn delete_no_content(&self, path: &str) -> Result<(), ApiError> {
        let resp = self.client.delete(self.url(path)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(())
    }

    /// Handle HTTP response: check status, parse JSON.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T, ApiError> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp.json::<T>().await?)
    }

    // ── Trees ───────────────────────────────────────────────────────

    pub async fn list_trees(
        &self,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Tree>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query("/api/v1/trees", &params).await
    }

    pub async fn get_tree(&self, id: Uuid) -> Result<Tree, ApiError> {
        self.get(&format!("/api/v1/trees/{id}")).await
    }

    pub async fn create_tree(&self, body: &CreateTreeBody) -> Result<Tree, ApiError> {
        self.post("/api/v1/trees", body).await
    }

    pub async fn update_tree(&self, id: Uuid, body: &UpdateTreeBody) -> Result<Tree, ApiError> {
        self.put(&format!("/api/v1/trees/{id}"), body).await
    }

    pub async fn delete_tree(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{id}")).await
    }

    // ── Persons ─────────────────────────────────────────────────────

    pub async fn list_persons(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Person>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons"), &params)
            .await
    }

    pub async fn get_person(&self, tree_id: Uuid, id: Uuid) -> Result<Person, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await
    }

    pub async fn create_person(
        &self,
        tree_id: Uuid,
        body: &CreatePersonBody,
    ) -> Result<Person, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/persons"), body)
            .await
    }

    pub async fn update_person(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePersonBody,
    ) -> Result<Person, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/persons/{id}"), body)
            .await
    }

    pub async fn delete_person(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await
    }

    pub async fn get_ancestors(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<PersonAncestry>, ApiError> {
        let mut params = Vec::new();
        if let Some(d) = max_depth {
            params.push(("max_depth", d.to_string()));
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/ancestors"),
            &params,
        )
        .await
    }

    pub async fn get_descendants(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<PersonAncestry>, ApiError> {
        let mut params = Vec::new();
        if let Some(d) = max_depth {
            params.push(("max_depth", d.to_string()));
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/descendants"),
            &params,
        )
        .await
    }

    // ── Person Names ────────────────────────────────────────────────

    pub async fn list_person_names(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<PersonName>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names"
        ))
        .await
    }

    pub async fn create_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        body: &CreatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
            body,
        )
        .await
    }

    pub async fn update_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
        body: &UpdatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
            body,
        )
        .await
    }

    pub async fn delete_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"
        ))
        .await
    }

    // ── Families ────────────────────────────────────────────────────

    pub async fn list_families(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Family>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/families"), &params)
            .await
    }

    pub async fn get_family(&self, tree_id: Uuid, id: Uuid) -> Result<Family, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await
    }

    pub async fn create_family(&self, tree_id: Uuid) -> Result<Family, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families"),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn delete_family(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await
    }

    // ── Family Spouses ──────────────────────────────────────────────

    pub async fn list_family_spouses(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/spouses"
        ))
        .await
    }

    pub async fn add_spouse(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddSpouseBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
            body,
        )
        .await
    }

    pub async fn remove_spouse(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        spouse_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}"
        ))
        .await
    }

    // ── Family Children ─────────────────────────────────────────────

    pub async fn list_family_children(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilyChild>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children"
        ))
        .await
    }

    pub async fn add_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddChildBody,
    ) -> Result<serde_json::Value, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
            body,
        )
        .await
    }

    pub async fn remove_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        child_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children/{child_id}"
        ))
        .await
    }

    // ── Events ──────────────────────────────────────────────────────

    pub async fn list_events(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
        event_type: Option<EventType>,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
    ) -> Result<PaginatedResponse<Event>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        if let Some(et) = event_type {
            params.push((
                "event_type",
                serde_json::to_string(&et)
                    .unwrap()
                    .trim_matches('"')
                    .to_string(),
            ));
        }
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/events"), &params)
            .await
    }

    pub async fn get_event(&self, tree_id: Uuid, id: Uuid) -> Result<Event, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/events/{id}"))
            .await
    }

    pub async fn create_event(
        &self,
        tree_id: Uuid,
        body: &CreateEventBody,
    ) -> Result<Event, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/events"), body)
            .await
    }

    pub async fn update_event(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateEventBody,
    ) -> Result<Event, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/events/{id}"), body)
            .await
    }

    pub async fn delete_event(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/events/{id}"))
            .await
    }

    // ── Places ──────────────────────────────────────────────────────

    pub async fn list_places(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
        search: Option<&str>,
    ) -> Result<PaginatedResponse<Place>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        if let Some(s) = search {
            params.push(("search", s.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/places"), &params)
            .await
    }

    pub async fn get_place(&self, tree_id: Uuid, id: Uuid) -> Result<Place, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/places/{id}"))
            .await
    }

    pub async fn create_place(
        &self,
        tree_id: Uuid,
        body: &CreatePlaceBody,
    ) -> Result<Place, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/places"), body)
            .await
    }

    pub async fn update_place(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePlaceBody,
    ) -> Result<Place, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/places/{id}"), body)
            .await
    }

    pub async fn delete_place(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/places/{id}"))
            .await
    }

    // ── Sources ─────────────────────────────────────────────────────

    pub async fn list_sources(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Source>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/sources"), &params)
            .await
    }

    pub async fn get_source(&self, tree_id: Uuid, id: Uuid) -> Result<Source, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/sources/{id}"))
            .await
    }

    pub async fn create_source(
        &self,
        tree_id: Uuid,
        body: &CreateSourceBody,
    ) -> Result<Source, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/sources"), body)
            .await
    }

    pub async fn update_source(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateSourceBody,
    ) -> Result<Source, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/sources/{id}"), body)
            .await
    }

    pub async fn delete_source(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/sources/{id}"))
            .await
    }

    // ── Citations ────────────────────────────────────────────────────

    pub async fn create_citation(
        &self,
        tree_id: Uuid,
        body: &CreateCitationBody,
    ) -> Result<Citation, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/citations"), body)
            .await
    }

    pub async fn update_citation(
        &self,
        tree_id: Uuid,
        citation_id: Uuid,
        body: &UpdateCitationBody,
    ) -> Result<Citation, ApiError> {
        self.put(
            &format!("/api/v1/trees/{tree_id}/citations/{citation_id}"),
            body,
        )
        .await
    }

    pub async fn delete_citation(&self, tree_id: Uuid, citation_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/citations/{citation_id}"))
            .await
    }

    // ── Notes ─────────────────────────────────────────────────────────

    pub async fn list_notes(
        &self,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> Result<Vec<Note>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(eid) = event_id {
            params.push(("event_id", eid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        if let Some(sid) = source_id {
            params.push(("source_id", sid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/notes"), &params)
            .await
    }

    pub async fn create_note(
        &self,
        tree_id: Uuid,
        body: &CreateNoteBody,
    ) -> Result<Note, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/notes"), body)
            .await
    }

    pub async fn update_note(
        &self,
        tree_id: Uuid,
        note_id: Uuid,
        body: &UpdateNoteBody,
    ) -> Result<Note, ApiError> {
        self.put(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"), body)
            .await
    }

    pub async fn delete_note(&self, tree_id: Uuid, note_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"))
            .await
    }

    // ── GEDCOM ──────────────────────────────────────────────────────

    pub async fn import_gedcom(
        &self,
        tree_id: Uuid,
        gedcom: &str,
    ) -> Result<ImportGedcomResult, ApiError> {
        self.post(
            &format!("/api/v1/trees/{tree_id}/import"),
            &ImportGedcomBody {
                gedcom: gedcom.to_string(),
            },
        )
        .await
    }

    pub async fn export_gedcom(&self, tree_id: Uuid) -> Result<ExportGedcomResult, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/export")).await
    }
}
