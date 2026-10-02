//! The browser frontend. It is a WebAssembly binary: every dependency is
//! declared for `wasm32` only, so a native build of the workspace compiles
//! none of them, and built for another target this binary only says where
//! it belongs. `just wasm` and the CI Clippy matrix check the real one.

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod observability;

#[cfg(target_arch = "wasm32")]
fn main() {
    app::main();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("oxidgene-web runs in a browser: build it with `just web-build`");
    std::process::exit(2);
}
