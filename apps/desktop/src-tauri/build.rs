fn main() {
    println!("cargo:rerun-if-env-changed=MOMENT_GROVE_BUILD_ID");
    println!("cargo:rerun-if-env-changed=MOMENT_GROVE_CHANNEL");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let source = "src/imaging/live_pair.m";
        println!("cargo:rerun-if-changed={source}");
        cc::Build::new()
            .file(source)
            .flag("-fobjc-arc")
            .compile("grove_live_pair");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
    }
    tauri_build::build()
}
