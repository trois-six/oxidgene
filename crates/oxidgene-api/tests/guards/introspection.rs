//! The GraphQL half of `surface`: arguments built from introspection, and
//! selections deep enough to tell an empty answer from a record.

use serde_json::Value;

use super::surface::record_key;
use crate::common::populated::Populated;

/// The introspection query the GraphQL guards read the schema with.
pub const INTROSPECTION: &str = r#"{
  __schema {
    queryType { name }
    mutationType { name }
    types {
      name kind
      fields { name args { name type { ...T } } type { ...T } }
      inputFields { name defaultValue type { ...T } }
      enumValues { name }
    }
  }
}
fragment T on __Type {
  kind name ofType { kind name ofType { kind name ofType { kind name ofType { kind name } } } }
}"#;

/// Record kinds in GraphQL names, longest match first: `deletePersonName`
/// acts on a name, `mediaPages` on a media.
const KINDS: &[(&str, &str)] = &[
    ("SourceRepository", "source_link_id"),
    ("PersonName", "name_id"),
    ("EventWitness", "witness_id"),
    ("MediaLink", "media_link_id"),
    ("MediaPage", "page_id"),
    ("AuditEntry", "entry_id"),
    ("Vignette", "vignette_id"),
    ("Spouse", "spouse_id"),
    ("Child", "child_id"),
    ("Citation", "citation_id"),
    ("Repository", "repository_id"),
    ("Person", "person_id"),
    ("Family", "family_id"),
    ("Event", "event_id"),
    ("Place", "place_id"),
    ("Source", "source_id"),
    ("Note", "note_id"),
    ("Media", "media_id"),
    ("Record", "record_id"),
    ("ExportJob", "export_job_id"),
    ("ImportJob", "job_id"),
];

pub fn snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The record an `ID` argument names: by its own name, or, for a bare `id`,
/// `ids` or `jobId`, by the field's.
pub fn record_of(arg: &str, field: &str) -> Option<&'static str> {
    if !matches!(arg, "id" | "ids" | "jobId") {
        return record_key(&snake(arg), "");
    }
    let upper = format!("{}{}", field[..1].to_uppercase(), &field[1..]);
    KINDS
        .iter()
        .filter(|(kind, _)| upper.contains(kind))
        .max_by_key(|(kind, _)| kind.len())
        .map(|(_, key)| *key)
}

/// The introspected schema.
pub struct Schema(pub Value);

impl Schema {
    pub fn ty(&self, name: &str) -> &Value {
        self.0["types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or(&Value::Null)
    }

    /// The root fields of `root` (`queryType` or `mutationType`).
    pub fn root_fields(&self, root: &str) -> Vec<Value> {
        let name = self.0[root]["name"].as_str().unwrap_or_default();
        self.ty(name)["fields"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// A GraphQL literal for an argument of type `ty`.
    pub fn literal(
        &self,
        ty: &Value,
        arg: &str,
        field: &str,
        records: &Populated,
        depth: usize,
    ) -> Option<String> {
        match ty["kind"].as_str()? {
            "NON_NULL" => self.literal(&ty["ofType"], arg, field, records, depth),
            "LIST" => {
                let item = self.literal(&ty["ofType"], arg, field, records, depth)?;
                Some(format!("[{item}]"))
            }
            "SCALAR" => scalar_literal(ty["name"].as_str()?, arg, field, records),
            "ENUM" => self.ty(ty["name"].as_str()?)["enumValues"][0]["name"]
                .as_str()
                .map(str::to_string),
            "INPUT_OBJECT" if depth < 4 => {
                self.input_literal(ty["name"].as_str()?, field, records, depth)
            }
            _ => None,
        }
    }

    /// An input object with its required fields and every field naming a
    /// record; a required field with a default is left to it.
    fn input_literal(
        &self,
        name: &str,
        field: &str,
        records: &Populated,
        depth: usize,
    ) -> Option<String> {
        let mut fields = Vec::new();
        for f in self.ty(name)["inputFields"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let name = f["name"].as_str().unwrap();
            let required = f["type"]["kind"] == "NON_NULL" && f["defaultValue"].is_null();
            let names_record = is_id(&f["type"]) && record_of(name, field).is_some();
            if required || names_record {
                let value = self.literal(&f["type"], name, field, records, depth + 1)?;
                fields.push(format!("{name}: {value}"));
            }
        }
        Some(format!("{{ {} }}", fields.join(", ")))
    }

    /// The literal arguments of `field` against `tree_id`: `Ok(None)` when
    /// none names a record, `Err` with the argument no table knows.
    pub fn arguments(
        &self,
        field: &Value,
        tree_id: &str,
        records: &Populated,
    ) -> Result<Option<String>, String> {
        let name = field["name"].as_str().unwrap();
        let mut rendered = Vec::new();
        let mut names_record = false;
        for arg in field["args"].as_array().into_iter().flatten() {
            let arg_name = arg["name"].as_str().unwrap();
            if arg_name == "treeId" {
                rendered.push(format!("treeId: \"{tree_id}\""));
                continue;
            }
            let required = arg["type"]["kind"] == "NON_NULL";
            let id_arg = is_id(&arg["type"]);
            if id_arg && record_of(arg_name, name).is_none() {
                return Err(arg_name.to_string());
            }
            if !(required || id_arg || named(&arg["type"]).ends_with("Input")) {
                continue;
            }
            match self.literal(&arg["type"], arg_name, name, records, 0) {
                Some(value) => {
                    names_record |= records.ids.values().any(|id| value.contains(id.as_str()));
                    rendered.push(format!("{arg_name}: {value}"));
                }
                None if required => return Err(arg_name.to_string()),
                None => {}
            }
        }
        Ok(names_record.then(|| rendered.join(", ")))
    }

    /// A selection for a field of type `ty`: its typename and, two levels
    /// down, every list or object field that takes no required argument —
    /// enough for [`holds_nothing`] to tell an empty answer from a record.
    pub fn selection(&self, ty: &Value, depth: usize) -> String {
        let name = named(ty);
        let object = self.ty(name);
        if object["kind"] != "OBJECT" {
            return String::new();
        }
        let mut fields = vec!["__typename".to_string()];
        for field in object["fields"].as_array().into_iter().flatten() {
            let needs_args = field["args"]
                .as_array()
                .is_some_and(|args| args.iter().any(|a| a["type"]["kind"] == "NON_NULL"));
            let inner = self.ty(named(&field["type"]));
            let composite = inner["kind"] == "OBJECT" || is_list(&field["type"]);
            if depth < 2 && composite && !needs_args {
                let name = field["name"].as_str().unwrap();
                fields.push(format!(
                    "{name}{}",
                    self.selection(&field["type"], depth + 1)
                ));
            }
        }
        format!(" {{ {} }}", fields.join(" "))
    }
}

/// Whether `ty` is an `ID` (or a list of them).
pub fn is_id(ty: &Value) -> bool {
    match ty["kind"].as_str() {
        Some("NON_NULL" | "LIST") => is_id(&ty["ofType"]),
        _ => matches!(ty["name"].as_str(), Some("ID" | "UUID")),
    }
}

/// The name of the named type under `ty`'s wrappers.
pub fn named(ty: &Value) -> &str {
    match ty["kind"].as_str() {
        Some("NON_NULL" | "LIST") => named(&ty["ofType"]),
        _ => ty["name"].as_str().unwrap_or_default(),
    }
}

fn is_list(ty: &Value) -> bool {
    match ty["kind"].as_str() {
        Some("NON_NULL") => is_list(&ty["ofType"]),
        Some("LIST") => true,
        _ => false,
    }
}

/// Whether an answer holds nothing: `null`, `false`, `0`, an empty list, or
/// an object whose fields (its typename aside) all hold nothing.
pub fn holds_nothing(data: &Value) -> bool {
    match data {
        Value::Null | Value::Bool(false) => true,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields
            .iter()
            .filter(|(name, _)| *name != "__typename")
            .all(|(_, value)| holds_nothing(value)),
        _ => false,
    }
}

/// The literal of a scalar argument: a record's id, or a plain value.
fn scalar_literal(scalar: &str, arg: &str, field: &str, records: &Populated) -> Option<String> {
    Some(match scalar {
        "ID" | "UUID" => format!("\"{}\"", records.id(record_of(arg, field)?)),
        "Int" => "1".into(),
        "Float" => "1.0".into(),
        "Boolean" => "false".into(),
        _ => "\"Fictitious\"".into(),
    })
}
