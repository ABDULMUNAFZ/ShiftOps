/// GitHub settings shared with Vite through `apps/desktop/.env`. They are
/// baked in at build time so the Rust side, not the web view, decides where
/// GitHub tokens are sent. Runtime environment variables still override them.
const GITHUB_ENV: &[&str] = &[
    "VITE_GITHUB_OAUTH_CLIENT_ID",
    "VITE_GITHUB_HOST",
    "VITE_GITHUB_API_BASE_URL",
];

fn forward_github_env() {
    let env_file =
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
            .join("../.env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    for name in GITHUB_ENV {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let from_file = std::fs::read_to_string(&env_file).unwrap_or_default();
    for name in GITHUB_ENV {
        let value = std::env::var(name).ok().or_else(|| {
            from_file.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == *name).then(|| v.trim().trim_matches(['"', '\'']).to_string())
            })
        });
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            println!("cargo:rustc-env=SHIFTOPS_BUILD_{name}={v}");
        }
    }
}

fn main() {
    forward_github_env();
    // On Windows, tauri-build embeds the Common-Controls v6 manifest only into
    // binaries, so `cargo test` executables fail to load with
    // STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139). Embed the same manifest
    // through the MSVC linker instead, which applies to binaries and tests.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let mut attributes = tauri_build::Attributes::new();
    if target_os == "windows" && target_env == "msvc" {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest =
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
                .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
