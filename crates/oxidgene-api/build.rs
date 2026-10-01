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

        let json = std::fs::read(&src_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));
        write_compressed(&Path::new(&out_dir).join(format!("{file_name}.br")), &json);
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

        let parameters = path_parameters(&operation.path);
        let tag = operation
            .path
            .strip_prefix("/api/v1/")
            .and_then(|path| path.split('/').next())
            .unwrap_or("rest");
        let operation_id = operation.handler.replace("::", "_");
        path_item.insert(
            operation.method,
            json!({
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
            }),
        );
    }

    let document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "OxidGene REST API",
            "version": std::env::var("CARGO_PKG_VERSION").unwrap(),
            "description": "Machine-readable contract generated from the Axum REST router at build time.",
            "license": {
                "name": "AGPL-3.0-only",
                "identifier": "AGPL-3.0-only"
            }
        },
        "servers": [{ "url": "/" }],
        "paths": paths,
        "components": {
            "schemas": {
                "ErrorEnvelope": {
                    "type": "object",
                    "required": ["error", "message"],
                    "properties": {
                        "error": { "type": "string" },
                        "message": { "type": "string" },
                        "request_id": { "type": "string", "format": "uuid" }
                    }
                }
            }
        }
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

fn path_parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|segment| {
            let name = segment.strip_prefix('{')?.strip_suffix('}')?;
            let schema = if name == "number" {
                json!({ "type": "integer", "format": "int64", "minimum": 1 })
            } else if name.ends_with("_id") {
                json!({ "type": "string", "format": "uuid" })
            } else {
                json!({ "type": "string" })
            };
            Some(json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": schema
            }))
        })
        .collect()
}
