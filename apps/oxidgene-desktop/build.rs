fn main() {
    #[cfg(target_os = "windows")]
    windows_resources();
}

/// Compiles `assets/desktop/windows.rc` with its placeholders filled in: the
/// icon's full path, and the package version as text and as the four numbers
/// of a `VERSIONINFO`.
#[cfg(target_os = "windows")]
fn windows_resources() {
    use std::env;
    use std::path::{Path, PathBuf};

    // The workspace root, two levels up, without `..` in the path.
    let desktop = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"))
        .ancestors()
        .nth(2)
        .expect("apps/oxidgene-desktop is two levels deep")
        .join("assets")
        .join("desktop");
    let template = desktop.join("windows.rc");
    let icon = desktop.join("icon.ico");
    println!("cargo:rerun-if-changed={}", template.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    let number = |part| env::var(part).expect("set by Cargo");
    let numbers = format!(
        "{},{},{},0",
        number("CARGO_PKG_VERSION_MAJOR"),
        number("CARGO_PKG_VERSION_MINOR"),
        number("CARGO_PKG_VERSION_PATCH"),
    );
    let resources = std::fs::read_to_string(&template)
        .expect("assets/desktop/windows.rc is readable")
        // Resource scripts escape backslashes in quoted paths.
        .replace("@ICON@", &icon.display().to_string().replace('\\', "\\\\"))
        .replace("@VERSION_NUMBERS@", &numbers)
        .replace("@VERSION@", &number("CARGO_PKG_VERSION"));
    let script = PathBuf::from(env::var("OUT_DIR").expect("set by Cargo")).join("windows.rc");
    std::fs::write(&script, resources).expect("OUT_DIR is writable");

    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_required()
        .expect("failed to compile Windows resources");
}
