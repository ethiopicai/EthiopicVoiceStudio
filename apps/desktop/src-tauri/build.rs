fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/mac_fn.m");
        cc::Build::new()
            .file("src/mac_fn.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("studio_fn");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
    tauri_build::build()
}
