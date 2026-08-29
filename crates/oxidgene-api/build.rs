//! Gzip-compresses the reference-data JSON files (occupation sheets, given
//! name meanings) at build time so the plain-text JSON stays diffable in
//! git while only the compressed bytes get embedded into the binary via
//! `include_bytes!` (see `src/reference/loader.rs`).

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Map, Value, json};
use syn::{Block, Expr, Item, Pat, Stmt};

const DATA_FILES: &[&str] = &[
    "occupations.fr.json",
    "occupations.en.json",
    "given_names.fr.json",
    "given_names.en.json",
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let data_dir = Path::new(&manifest_dir).join("src/reference/data");

    for file_name in DATA_FILES {
        let src_path = data_dir.join(file_name);
        println!("cargo:rerun-if-changed={}", src_path.display());

        let json = std::fs::read(&src_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));

        let out_path = Path::new(&out_dir).join(format!("{file_name}.gz"));
        let out_file = File::create(&out_path)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", out_path.display()));
        let mut encoder = GzEncoder::new(out_file, Compression::best());
        encoder
            .write_all(&json)
            .unwrap_or_else(|e| panic!("failed to compress {}: {e}", src_path.display()));
        encoder
            .finish()
            .unwrap_or_else(|e| panic!("failed to finish gzip stream for {file_name}: {e}"));
    }

    generate_openapi(Path::new(&manifest_dir), Path::new(&out_dir));
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

    let output_path = out_dir.join("openapi.json");
    let output = serde_json::to_vec_pretty(&document).expect("serialize generated OpenAPI");
    std::fs::write(&output_path, output)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", output_path.display()));
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
            } else if name.ends_with("_id") || name == "progress_id" {
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
