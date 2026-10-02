//! What the generic guards know about the two surfaces: which fixture record
//! a parameter names, request bodies built from the OpenAPI schemas, and
//! GraphQL arguments built from introspection.

use serde_json::{Map, Value, json};

use crate::common::populated::Populated;

/// Path and body parameters that hold no record id.
pub const NOT_RECORDS: &[&str] = &[
    "tree_id",
    "lang",
    "field",
    "record_type",
    "version",
    "tag",
    "number",
];

/// The fixture record a parameter names, by its snake_case name; `link_id`
/// and `job_id` depend on the route.
pub fn record_key(name: &str, path: &str) -> Option<&'static str> {
    Some(match name {
        "link_id" if path.contains("/media-links/") => "media_link_id",
        "link_id" => "source_link_id",
        "job_id" if path.contains("/export-jobs/") => "export_job_id",
        "job_id" => "job_id",
        "person_id"
        | "duplicate_id"
        | "person_ids"
        | "sosa_root_person_id"
        | "self_person_id"
        | "kept_id"
        | "subject_id"
        | "entity_id" => "person_id",
        "root_person_id" | "root_person_ids" => "root_person_id",
        "other_person_id" | "other_person_ids" => "other_person_id",
        "spouse_id" => "spouse_id",
        "child_id" => "child_id",
        "family_id" | "family_ids" => "family_id",
        "event_id" | "event_ids" | "left_out_events" => "event_id",
        "name_id" => "name_id",
        "place_id" => "place_id",
        "source_id" => "source_id",
        "repository_id" => "repository_id",
        "citation_id" => "citation_id",
        "note_id" => "note_id",
        // The bytes of a medium are a page's: a document holds none.
        "media_id" if path.ends_with("/file") || path.ends_with("/thumbnail") => "page_id",
        "media_id" if path.ends_with("/download") => "page_id",
        "media_id" | "document_id" | "parent_media_id" | "media_ids" => "media_id",
        "page_id" | "page_ids" => "page_id",
        "vignette_id" | "vignette_ids" => "vignette_id",
        "witness_id" => "witness_id",
        "allowed_link_id" | "left_out_media_links" => "media_link_id",
        "entry_id" => "entry_id",
        "record_id" => "record_id",
        _ => return None,
    })
}

/// The value of a path parameter that names no record.
pub fn plain_value(name: &str, schema: &Value) -> String {
    if let Some(first) = schema["enum"].get(0).and_then(Value::as_str) {
        return first.to_string();
    }
    match name {
        "record_type" => "person".into(),
        "lang" => "en".into(),
        "tag" => "fictitious".into(),
        _ => "1".into(),
    }
}

/// The OpenAPI document the router serves.
pub struct Spec(pub Value);

impl Spec {
    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        match schema["$ref"].as_str() {
            Some(reference) => {
                let name = reference.rsplit('/').next().unwrap();
                &self.0["components"]["schemas"][name]
            }
            None => schema,
        }
    }

    /// `path` with its parameters filled: the tree's id, the fixture's
    /// records, plain values; `Err` names a parameter of no known kind.
    pub fn fill(
        &self,
        path: &str,
        operation: &Value,
        tree_id: &str,
        records: &Populated,
    ) -> Result<(String, bool), String> {
        let mut uri = path.to_string();
        let mut names_a_record = false;
        let mut query = Vec::new();
        for parameter in operation["parameters"].as_array().into_iter().flatten() {
            if parameter["in"] == "query" && parameter["required"] == true {
                let name = parameter["name"].as_str().unwrap();
                let value = match record_key(name, path) {
                    Some(key) => records.id(key).to_string(),
                    None => plain_value(name, &parameter["schema"]),
                };
                query.push(format!("{name}={value}"));
            }
            if parameter["in"] != "path" {
                continue;
            }
            let name = parameter["name"].as_str().unwrap();
            let value = if name == "tree_id" {
                tree_id.to_string()
            } else if let Some(key) = record_key(name, path) {
                names_a_record = true;
                records.id(key).to_string()
            } else if NOT_RECORDS.contains(&name) {
                plain_value(name, &parameter["schema"])
            } else {
                return Err(name.to_string());
            };
            uri = uri.replace(&format!("{{{name}}}"), &value);
        }
        if !query.is_empty() {
            uri = format!("{uri}?{}", query.join("&"));
        }
        Ok((uri, names_a_record))
    }

    /// The JSON body of `operation`, when it takes one.
    pub fn request_body(
        &self,
        operation: &Value,
        records: &Populated,
        path: &str,
    ) -> Option<Value> {
        let schema = &operation["requestBody"]["content"]["application/json"]["schema"];
        (!schema.is_null()).then(|| self.body(schema, records, path, 0))
    }

    /// A minimal body for `schema`: its required fields, plus every field
    /// naming a record, which takes the fixture's.
    fn body(&self, schema: &Value, records: &Populated, path: &str, depth: usize) -> Value {
        let schema = self.resolve(schema);
        let schema = schema["allOf"]
            .get(0)
            .map_or(schema, |first| self.resolve(first));
        if depth > 4 {
            return Value::Null;
        }
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut object = Map::new();
        for (name, field) in schema["properties"].as_object().into_iter().flatten() {
            if !required.contains(&name.as_str()) && record_key(name, path).is_none() {
                continue;
            }
            object.insert(name.clone(), self.value(name, field, records, path, depth));
        }
        Value::Object(object)
    }

    fn value(
        &self,
        name: &str,
        schema: &Value,
        records: &Populated,
        path: &str,
        depth: usize,
    ) -> Value {
        let schema = self.resolve(schema);
        let schema = schema["anyOf"]
            .as_array()
            .and_then(|options| options.iter().find(|o| o["type"] != "null"))
            .map_or(schema, |o| self.resolve(o));
        if let Some(key) = record_key(name, path) {
            let id = json!(records.id(key));
            return if schema["type"] == "array" {
                json!([id])
            } else {
                id
            };
        }
        if let Some(first) = schema["enum"].get(0) {
            return first.clone();
        }
        let ty = match &schema["type"] {
            Value::Array(types) => types
                .iter()
                .find(|t| *t != "null")
                .cloned()
                .unwrap_or_default(),
            other => other.clone(),
        };
        match ty.as_str() {
            Some("string") => json!("Fictitious"),
            Some("integer" | "number") => json!(1),
            Some("boolean") => json!(false),
            Some("array") => json!([]),
            _ => self.body(schema, records, path, depth + 1),
        }
    }
}
