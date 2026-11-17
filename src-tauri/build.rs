fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/microphone.m");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Foundation");
        cc::Build::new()
            .file("src/microphone.m")
            .flag("-fobjc-arc")
            .compile("soliton_microphone");
    }
    tauri_build::build()
}
