fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=../../assets/desktop/icon.ico");
        println!("cargo:rerun-if-changed=../../assets/desktop/windows.rc");
        embed_resource::compile("../../assets/desktop/windows.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile Windows resources");
    }
}
