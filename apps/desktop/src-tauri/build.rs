fn main() {
    println!("cargo:rerun-if-env-changed=MOMENT_GROVE_BUILD_ID");
    println!("cargo:rerun-if-env-changed=MOMENT_GROVE_CHANNEL");
    tauri_build::build()
}
