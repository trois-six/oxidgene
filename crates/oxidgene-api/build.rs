//! Brotli-compresses the data embedded into the binary: the reference-data
//! JSON files (occupation sheets, given name meanings), which stay plain and
//! diffable in git, and the OpenAPI document generated from the router. Only
//! the compressed bytes are embedded via `include_bytes!` (see
//! `src/embedded.rs`).

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value, json};
use syn::{Block, Expr, Item, Pat, Stmt};

/// The settings every embedded data file is compressed with; the place
/// dictionary generator uses the same. Quality 11 with a 16 MiB window gives
/// the best ratio Brotli has, and decoding stays as fast whatever the quality.
const BROTLI_QUALITY: u32 = 11;
const BROTLI_WINDOW_BITS: u32 = 24;

const DATA_FILES: &[&str] = &[
    "occupations.fr.json",
    "occupations.en.json",
    "occupations.de.json",
    "occupations.es.json",
    "occupations.it.json",
    "occupations.nl.json",
    "occupations.pl.json",
    "occupations.pt.json",
    "given_names.fr.json",
    "given_names.en.json",
    "given_names.de.json",
    "given_names.es.json",
    "given_names.it.json",
    "given_names.nl.json",
    "given_names.pl.json",
    "given_names.pt.json",
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let data_dir = Path::new(&manifest_dir).join("../../assets/reference");

    for file_name in DATA_FILES {
        let src_path = data_dir.join(file_name);
        println!("cargo:rerun-if-changed={}", src_path.display());

        // The script reruns whenever a source the OpenAPI document is read
        // from changes; a data file older than its compressed copy is left be.
        let out_path = Path::new(&out_dir).join(format!("{file_name}.br"));
        let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let (Some(source), Some(compressed)) = (modified(&src_path), modified(&out_path))
            && compressed >= source
        {
            continue;
        }
        let json = std::fs::read(&src_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));
        write_compressed(&out_path, &json);
    }

    generate_openapi(Path::new(&manifest_dir), Path::new(&out_dir));
}

fn write_compressed(path: &Path, bytes: &[u8]) {
    let mut compressed = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(
            &mut compressed,
            1 << 16,
            BROTLI_QUALITY,
            BROTLI_WINDOW_BITS,
        );
        encoder
            .write_all(bytes)
            .unwrap_or_else(|e| panic!("failed to compress {}: {e}", path.display()));
    }
    std::fs::write(path, compressed)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

#[derive(Clone)]
struct Operation {
    method: String,
    path: String,
    handler: String,
}

fn generate_openapi(manifest_dir: &Path, out_dir: &Path) {
    let router_path = manifest_dir.join("src/router.rs");
    println!("cargo:rerun-if-changed={}", router_path.display());

    let source = std::fs::read_to_string(&router_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", router_path.display()));
    let syntax = syn::parse_file(&source)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", router_path.display()));
    let build_router = syntax
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "build_router" => Some(&function.block),
            _ => None,
        })
        .unwrap_or_else(|| panic!("build_router not found in {}", router_path.display()));

    let operations = collect_operations(build_router);
    if operations.is_empty() {
        panic!("no REST operations found in {}", router_path.display());
    }

    let types = TypeIndex::load(manifest_dir);
    let mut schemas = Map::new();
    schemas.insert(
        "ErrorEnvelope".to_string(),
        json!({
            "type": "object",
            "required": ["error", "message"],
            "properties": {
                "error": { "type": "string" },
                "message": { "type": "string" },
                "request_id": { "type": "string", "format": "uuid" }
            }
        }),
    );

    let mut paths = Map::new();
    for operation in operations {
        let path_item = paths
            .entry(operation.path.clone())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("OpenAPI path item must be an object");
        if path_item.contains_key(&operation.method) {
            panic!(
                "duplicate OpenAPI operation: {} {}",
                operation.method, operation.path
            );
        }

        let inputs = types.handler_inputs(&operation.handler);
        let mut parameters = path_parameters(&operation.path, inputs.path.as_ref(), &types);
        for query in &inputs.query {
            parameters.extend(types.query_parameters(query, &mut schemas));
        }
        let tag = operation
            .path
            .strip_prefix("/api/v1/")
            .and_then(|path| path.split('/').next())
            .unwrap_or("rest");
        let operation_id = operation.handler.replace("::", "_");
        let mut entry = json!({
            "operationId": operation_id,
            "tags": [tag],
            "parameters": parameters,
            "responses": {
                "2XX": { "description": "Successful response" },
                "default": {
                    "description": "Error response",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorEnvelope" }
                        }
                    }
                }
            }
        });
        if let Some(body) = &inputs.body {
            let (content_type, schema) = match body {
                Body::Json(ty) => ("application/json", types.schema(ty, &mut schemas)),
                Body::Multipart => ("multipart/form-data", json!({ "type": "object" })),
                Body::Raw => (
                    "application/octet-stream",
                    json!({ "type": "string", "format": "binary" }),
                ),
            };
            entry["requestBody"] = json!({
                "required": true,
                "content": { content_type: { "schema": schema } }
            });
        }
        path_item.insert(operation.method, entry);
    }

    let document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "OxidGene REST API",
            "version": std::env::var("CARGO_PKG_VERSION").unwrap(),
            "description": "Machine-readable contract generated from the Axum REST router and its handlers' extractors at build time: paths, methods, path and query parameters and request bodies. Response bodies are described in the API specification, not here.",
            "license": {
                "name": "AGPL-3.0-only",
                "identifier": "AGPL-3.0-only"
            }
        },
        "servers": [{ "url": "/" }],
        "paths": paths,
        "components": { "schemas": schemas }
    });

    let output = serde_json::to_vec_pretty(&document).expect("serialize generated OpenAPI");
    write_compressed(&out_dir.join("openapi.json.br"), &output);
}

fn collect_operations(block: &Block) -> Vec<Operation> {
    let mut routers = HashMap::<String, Vec<Operation>>::new();

    for statement in &block.stmts {
        let Stmt::Local(local) = statement else {
            continue;
        };
        let Pat::Ident(binding) = &local.pat else {
            continue;
        };
        let Some(initializer) = &local.init else {
            continue;
        };

        let operations = evaluate_router(&initializer.expr, &routers);
        routers.insert(binding.ident.to_string(), operations);
    }

    routers
        .remove("rest_router")
        .expect("build_router must bind its REST routes to rest_router")
}

fn evaluate_router(expr: &Expr, routers: &HashMap<String, Vec<Operation>>) -> Vec<Operation> {
    match expr {
        Expr::MethodCall(call) => {
            let mut operations = evaluate_router(&call.receiver, routers);
            match call.method.to_string().as_str() {
                "route" => {
                    let path = call
                        .args
                        .first()
                        .and_then(string_literal)
                        .expect("Router::route path must be a string literal");
                    let method_router = call.args.iter().nth(1).expect("Router::route handler");
                    for (method, handler) in evaluate_methods(method_router) {
                        operations.push(Operation {
                            method,
                            path: path.clone(),
                            handler,
                        });
                    }
                }
                "merge" => {
                    if let Some(name) = call.args.first().and_then(path_name) {
                        operations.extend(
                            routers
                                .get(&name)
                                .unwrap_or_else(|| panic!("unknown merged router {name}"))
                                .clone(),
                        );
                    }
                }
                "nest" => {
                    let prefix = call
                        .args
                        .first()
                        .and_then(string_literal)
                        .expect("Router::nest prefix must be a string literal");
                    let nested_expr = call
                        .args
                        .iter()
                        .nth(1)
                        .expect("Router::nest target must be a router expression");
                    operations.extend(evaluate_router(nested_expr, routers).into_iter().map(
                        |mut operation| {
                            operation.path = join_paths(&prefix, &operation.path);
                            operation
                        },
                    ));
                }
                _ => {}
            }
            operations
        }
        Expr::Path(path) => path
            .path
            .get_ident()
            .and_then(|name| routers.get(&name.to_string()))
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn evaluate_methods(expr: &Expr) -> Vec<(String, String)> {
    match expr {
        Expr::Call(call) => {
            let Some(method) = path_name(&call.func) else {
                return Vec::new();
            };
            let Some(handler) = call.args.first().and_then(path_name) else {
                return Vec::new();
            };
            vec![(method, handler)]
        }
        Expr::MethodCall(call) => {
            let mut methods = evaluate_methods(&call.receiver);
            let method = call.method.to_string();
            if matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete")
                && let Some(handler) = call.args.first().and_then(path_name)
            {
                methods.push((method, handler));
            }
            methods
        }
        _ => Vec::new(),
    }
}

fn path_name(expr: &Expr) -> Option<String> {
    let Expr::Path(path) = expr else {
        return None;
    };
    Some(
        path.path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
    )
}

fn string_literal(expr: &Expr) -> Option<String> {
    let Expr::Lit(literal) = expr else {
        return None;
    };
    let syn::Lit::Str(value) = &literal.lit else {
        return None;
    };
    Some(value.value())
}

fn join_paths(prefix: &str, path: &str) -> String {
    if path == "/" {
        return prefix.trim_end_matches('/').to_string();
    }
    format!("/{}/{}", prefix.trim_matches('/'), path.trim_matches('/'))
}

/// The path's parameters, typed from the handler's `Path<…>` extractor —
/// one type per `{segment}`, in order — and from their names where the
/// extractor says nothing.
fn path_parameters(path: &str, extractor: Option<&syn::Type>, types: &TypeIndex) -> Vec<Value> {
    let names: Vec<&str> = path
        .split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .collect();
    let declared: Vec<&syn::Type> = match extractor {
        Some(syn::Type::Tuple(tuple)) => tuple.elems.iter().collect(),
        Some(single) => vec![single],
        None => Vec::new(),
    };
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let mut schema = match declared
                .get(index)
                .filter(|_| declared.len() == names.len())
            {
                Some(ty) => types.schema(ty, &mut Map::new()),
                None if name.ends_with("_id") => json!({ "type": "string", "format": "uuid" }),
                None => json!({ "type": "string" }),
            };
            if *name == "number" || *name == "version" {
                schema["minimum"] = json!(1);
            }
            // Taken as a string and checked by the handler.
            if *name == "lang" {
                schema["enum"] = json!(["fr", "en", "de", "es", "it", "nl", "pl", "pt"]);
            }
            json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": schema
            })
        })
        .collect()
}

/// What a handler takes from a request, read from its extractors.
#[derive(Default)]
struct HandlerInputs {
    path: Option<syn::Type>,
    query: Vec<syn::Type>,
    body: Option<Body>,
}

enum Body {
    Json(Box<syn::Type>),
    Multipart,
    Raw,
}

/// A named struct's fields, or an enum's serialized variant names.
enum Shape {
    Struct {
        fields: Vec<Field>,
    },
    /// Unit variants as serde writes them; `None` when a variant carries data.
    Enum(Option<Vec<String>>),
}

struct Field {
    name: String,
    ty: syn::Type,
    required: bool,
    flatten: bool,
}

/// The types and handlers of the crates the REST surface is written in, read
/// from their sources: enough to describe parameters and bodies without
/// annotating every type for a schema generator.
struct TypeIndex {
    shapes: HashMap<String, Shape>,
    handlers: HashMap<String, HandlerInputs>,
}

impl TypeIndex {
    fn load(manifest_dir: &Path) -> Self {
        let mut index = Self {
            shapes: HashMap::new(),
            handlers: HashMap::new(),
        };
        // The API's own types win over a namesake in core or db.
        for dir in [
            manifest_dir.join("src"),
            manifest_dir.join("../oxidgene-core/src"),
            manifest_dir.join("../oxidgene-db/src"),
        ] {
            println!("cargo:rerun-if-changed={}", dir.display());
            index.read_dir(&dir, &manifest_dir.join("src/rest"));
        }
        index
    }

    fn read_dir(&mut self, dir: &Path, rest_dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                self.read_dir(&path, rest_dir);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(file) = syn::parse_file(&source) else {
                    continue;
                };
                let module = (path.parent() == Some(rest_dir))
                    .then(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .flatten();
                self.read_items(&file.items, module.as_deref());
            }
        }
    }

    fn read_items(&mut self, items: &[Item], module: Option<&str>) {
        for item in items {
            match item {
                Item::Struct(item) => {
                    if let syn::Fields::Named(named) = &item.fields {
                        let container = serde_attrs(&item.attrs);
                        let rename_all = container.rename_all;
                        let fields = named
                            .named
                            .iter()
                            .filter_map(|field| {
                                let attrs = serde_attrs(&field.attrs);
                                if attrs.skip {
                                    return None;
                                }
                                let ident = field.ident.as_ref()?.to_string();
                                let ident = ident.trim_start_matches("r#").to_string();
                                Some(Field {
                                    name: attrs.rename.unwrap_or_else(|| {
                                        rename_field(&ident, rename_all.as_deref())
                                    }),
                                    required: !container.default
                                        && !attrs.default
                                        && !is_option(&field.ty),
                                    flatten: attrs.flatten,
                                    ty: field.ty.clone(),
                                })
                            })
                            .collect();
                        self.shapes
                            .entry(item.ident.to_string())
                            .or_insert(Shape::Struct { fields });
                    }
                }
                Item::Enum(item) => {
                    let rename_all = serde_attrs(&item.attrs).rename_all;
                    let values = item
                        .variants
                        .iter()
                        .map(|variant| {
                            matches!(variant.fields, syn::Fields::Unit).then(|| {
                                serde_attrs(&variant.attrs).rename.unwrap_or_else(|| {
                                    rename_variant(
                                        &variant.ident.to_string(),
                                        rename_all.as_deref(),
                                    )
                                })
                            })
                        })
                        .collect::<Option<Vec<_>>>();
                    self.shapes
                        .entry(item.ident.to_string())
                        .or_insert(Shape::Enum(values));
                }
                Item::Fn(function) => {
                    if let Some(module) = module {
                        self.handlers.insert(
                            format!("{module}::{}", function.sig.ident),
                            handler_inputs(&function.sig),
                        );
                    }
                }
                Item::Mod(inner) => {
                    if let Some((_, items)) = &inner.content {
                        self.read_items(items, None);
                    }
                }
                Item::Macro(item) if item.mac.path.is_ident("string_enum") => {
                    if let Ok((name, values)) = item.mac.parse_body_with(string_enum) {
                        self.shapes.entry(name).or_insert(Shape::Enum(Some(values)));
                    }
                }
                _ => {}
            }
        }
    }

    /// The extractors of the handler a route names (`module::function`, or a
    /// longer path ending in those two).
    fn handler_inputs(&self, handler: &str) -> HandlerInputs {
        let segments: Vec<&str> = handler.split("::").collect();
        let key = segments[segments.len().saturating_sub(2)..].join("::");
        match self.handlers.get(&key) {
            Some(inputs) => HandlerInputs {
                path: inputs.path.clone(),
                query: inputs.query.clone(),
                body: inputs.body.as_ref().map(|body| match body {
                    Body::Json(ty) => Body::Json(ty.clone()),
                    Body::Multipart => Body::Multipart,
                    Body::Raw => Body::Raw,
                }),
            },
            None => HandlerInputs::default(),
        }
    }

    /// One query parameter per field of `query`, flattened fields included.
    fn query_parameters(&self, query: &syn::Type, schemas: &mut Map<String, Value>) -> Vec<Value> {
        let Some(Shape::Struct { fields }) = type_name(query).and_then(|n| self.shapes.get(&n))
        else {
            return Vec::new();
        };
        let mut parameters = Vec::new();
        for field in fields {
            if field.flatten {
                parameters.extend(self.query_parameters(&field.ty, schemas));
                continue;
            }
            parameters.push(json!({
                "name": field.name,
                "in": "query",
                "required": field.required,
                "schema": self.schema(&field.ty, schemas)
            }));
        }
        parameters
    }

    /// The JSON schema of `ty`. A struct of ours is described once under
    /// `components.schemas` and referenced; anything the index cannot read
    /// is left open (`{}`).
    fn schema(&self, ty: &syn::Type, schemas: &mut Map<String, Value>) -> Value {
        let ty = match ty {
            syn::Type::Reference(reference) => &*reference.elem,
            syn::Type::Paren(paren) => &*paren.elem,
            other => other,
        };
        let syn::Type::Path(path) = ty else {
            return json!({});
        };
        let Some(last) = path.path.segments.last() else {
            return json!({});
        };
        let name = last.ident.to_string();
        let args: Vec<&syn::Type> = match &last.arguments {
            syn::PathArguments::AngleBracketed(args) => args
                .args
                .iter()
                .filter_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => Some(ty),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        match (name.as_str(), args.as_slice()) {
            ("Option" | "Box" | "Arc", [inner]) => self.schema(inner, schemas),
            ("Vec" | "HashSet" | "BTreeSet", [inner]) => {
                json!({ "type": "array", "items": self.schema(inner, schemas) })
            }
            ("HashMap" | "BTreeMap", [_, value]) => {
                json!({ "type": "object", "additionalProperties": self.schema(value, schemas) })
            }
            ("String" | "str" | "char", _) => json!({ "type": "string" }),
            ("Uuid", _) => json!({ "type": "string", "format": "uuid" }),
            ("bool", _) => json!({ "type": "boolean" }),
            ("i8" | "i16" | "i32" | "u8" | "u16", _) => {
                json!({ "type": "integer", "format": "int32" })
            }
            ("i64" | "u32" | "u64" | "usize" | "isize", _) => {
                json!({ "type": "integer", "format": "int64" })
            }
            ("f32" | "f64", _) => json!({ "type": "number" }),
            ("NaiveDate", _) => json!({ "type": "string", "format": "date" }),
            ("DateTime", _) => json!({ "type": "string", "format": "date-time" }),
            _ => self.named_schema(&name, schemas),
        }
    }

    fn named_schema(&self, name: &str, schemas: &mut Map<String, Value>) -> Value {
        match self.shapes.get(name) {
            Some(Shape::Enum(Some(values))) => json!({ "type": "string", "enum": values }),
            Some(Shape::Struct { fields }) => {
                let reference = json!({ "$ref": format!("#/components/schemas/{name}") });
                if schemas.contains_key(name) {
                    return reference;
                }
                // Registered before its fields are, so a type that contains
                // itself refers to itself instead of recursing forever.
                schemas.insert(name.to_string(), json!({ "type": "object" }));
                let mut properties = Map::new();
                let mut required = Vec::new();
                let mut all_of = Vec::new();
                for field in fields {
                    let schema = self.schema(&field.ty, schemas);
                    if field.flatten {
                        all_of.push(schema);
                        continue;
                    }
                    if field.required {
                        required.push(field.name.clone());
                    }
                    properties.insert(field.name.clone(), schema);
                }
                let mut object = json!({ "type": "object", "properties": properties });
                if !required.is_empty() {
                    object["required"] = json!(required);
                }
                let described = if all_of.is_empty() {
                    object
                } else {
                    all_of.push(object);
                    json!({ "allOf": all_of })
                };
                schemas.insert(name.to_string(), described);
                reference
            }
            _ => json!({}),
        }
    }
}

/// A handler's path, query and body extractors.
fn handler_inputs(signature: &syn::Signature) -> HandlerInputs {
    let mut inputs = HandlerInputs::default();
    for input in &signature.inputs {
        let syn::FnArg::Typed(typed) = input else {
            continue;
        };
        let syn::Type::Path(path) = &*typed.ty else {
            continue;
        };
        let Some(last) = path.path.segments.last() else {
            continue;
        };
        let inner = match &last.arguments {
            syn::PathArguments::AngleBracketed(args) => {
                args.args.iter().find_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => Some(ty.clone()),
                    _ => None,
                })
            }
            _ => None,
        };
        match last.ident.to_string().as_str() {
            "Path" => inputs.path = inner,
            "Query" => inputs.query.extend(inner),
            "Json" => inputs.body = inner.map(|ty| Body::Json(Box::new(ty))),
            "Multipart" => inputs.body = Some(Body::Multipart),
            "Body" | "Bytes" => inputs.body = Some(Body::Raw),
            _ => {}
        }
    }
    inputs
}

/// The name and serialized values of an enum declared through core's
/// `string_enum!`: `pub enum Name { Variant => "text", … }`.
fn string_enum(input: syn::parse::ParseStream<'_>) -> syn::Result<(String, Vec<String>)> {
    syn::Attribute::parse_outer(input)?;
    input.parse::<syn::Visibility>()?;
    input.parse::<syn::Token![enum]>()?;
    let name = input.parse::<syn::Ident>()?.to_string();
    let content;
    syn::braced!(content in input);
    let mut values = Vec::new();
    while !content.is_empty() {
        syn::Attribute::parse_outer(&content)?;
        content.parse::<syn::Ident>()?;
        content.parse::<syn::Token![=>]>()?;
        values.push(content.parse::<syn::LitStr>()?.value());
        if content.peek(syn::Token![,]) {
            content.parse::<syn::Token![,]>()?;
        }
    }
    Ok((name, values))
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn is_option(ty: &syn::Type) -> bool {
    type_name(ty).as_deref() == Some("Option")
}

/// The serde attributes that change a field's or a variant's wire shape.
#[derive(Default)]
struct SerdeAttrs {
    rename: Option<String>,
    rename_all: Option<String>,
    default: bool,
    flatten: bool,
    skip: bool,
}

fn serde_attrs(attrs: &[syn::Attribute]) -> SerdeAttrs {
    let mut found = SerdeAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        // An attribute this cannot read leaves the defaults: the schema is
        // then looser than the type, never wrong about what is accepted.
        let _ = attr.parse_nested_meta(|meta| read_serde_meta(&mut found, meta));
    }
    found
}

/// One `key`, `key = value` or `key(…)` of a `#[serde(…)]` attribute.
fn read_serde_meta(
    found: &mut SerdeAttrs,
    meta: syn::meta::ParseNestedMeta<'_>,
) -> syn::Result<()> {
    let key = meta
        .path
        .get_ident()
        .map(ToString::to_string)
        .unwrap_or_default();
    let has_value = meta.input.peek(syn::Token![=]);
    match key.as_str() {
        "rename" | "rename_all" if has_value => {
            let value = Some(meta.value()?.parse::<syn::LitStr>()?.value());
            if key == "rename" {
                found.rename = value;
            } else {
                found.rename_all = value;
            }
            return Ok(());
        }
        "default" => found.default = true,
        "flatten" => found.flatten = true,
        "skip" | "skip_deserializing" => found.skip = true,
        _ => {}
    }
    skip_meta_value(meta)
}

/// Consume what follows a key the schema does not need.
fn skip_meta_value(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(skip_meta_value)?;
    }
    Ok(())
}

/// A snake_case field name as a `rename_all` rule writes it.
fn rename_field(name: &str, rule: Option<&str>) -> String {
    let words: Vec<&str> = name.split('_').filter(|w| !w.is_empty()).collect();
    case(&words, rule.unwrap_or("snake_case"))
}

/// A PascalCase variant name as a `rename_all` rule writes it; serde's
/// default keeps it as it is.
fn rename_variant(name: &str, rule: Option<&str>) -> String {
    let Some(rule) = rule else {
        return name.to_string();
    };
    let mut words: Vec<String> = Vec::new();
    for c in name.chars() {
        if c.is_uppercase() || words.is_empty() {
            words.push(String::new());
        }
        if let Some(word) = words.last_mut() {
            word.push(c.to_ascii_lowercase());
        }
    }
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    case(&words, rule)
}

fn case(words: &[&str], rule: &str) -> String {
    let capitalized = |word: &str| {
        let mut chars = word.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default()
    };
    match rule {
        "lowercase" => words.concat().to_lowercase(),
        "UPPERCASE" => words.concat().to_uppercase(),
        "kebab-case" => words.join("-").to_lowercase(),
        "SCREAMING_SNAKE_CASE" => words.join("_").to_uppercase(),
        "SCREAMING-KEBAB-CASE" => words.join("-").to_uppercase(),
        "camelCase" => words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.to_lowercase()
                } else {
                    capitalized(w)
                }
            })
            .collect(),
        "PascalCase" => words.iter().map(|w| capitalized(w)).collect(),
        _ => words.join("_").to_lowercase(),
    }
}
