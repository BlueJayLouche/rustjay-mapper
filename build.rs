//! Build script for rusty_mapper

fn main() {
    #[cfg(target_os = "macos")]
    {
        // ===== Syphon Framework =====
        // Resolve path relative to CARGO_MANIFEST_DIR so it works on any machine.
        let syphon_framework_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| {
                let candidate = p.join("crates/syphon/syphon-lib");
                if candidate.join("Syphon.framework").exists() {
                    Some(candidate)
                } else {
                    None
                }
            })
            .or_else(|| {
                std::env::var("SYPHON_FRAMEWORK_DIR").ok().map(std::path::PathBuf::from)
            })
            .expect(
                "Syphon.framework not found. Set SYPHON_FRAMEWORK_DIR to the directory \
                 containing Syphon.framework, or place it at <workspace>/../crates/syphon/syphon-lib/",
            );

        let syphon_dir = syphon_framework_dir.to_string_lossy().into_owned();

        // Framework search + link + rpath
        println!("cargo:rustc-link-arg=-F{}", syphon_dir);
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=Syphon");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", syphon_dir);
        println!("cargo:warning=Syphon rpath: {}", syphon_dir);

        // ===== NDI Library =====
        // Honour NDI_SDK_DIR env var, then fall back to standard macOS install locations.
        let ndi_lib_dir = std::env::var("NDI_SDK_DIR")
            .ok()
            .map(|sdk| {
                let p = std::path::PathBuf::from(&sdk).join("lib/macOS");
                if p.exists() { p } else { std::path::PathBuf::from(sdk).join("lib") }
            })
            .or_else(|| {
                let candidates = [
                    "/Library/NDI SDK for Apple/lib/macOS",
                    "/Library/NDI SDK for macOS/lib/macOS",
                    "/Library/NDI 6 SDK/lib/macOS",
                    "/Library/NDI SDK/lib/macOS",
                    "/Library/NewTek/NDI SDK/lib/macOS",
                    "/usr/local/lib",
                ];
                candidates.iter()
                    .find(|p| std::path::Path::new(p).join("libndi.dylib").exists())
                    .map(|p| std::path::PathBuf::from(p))
            });

        if let Some(ref dir) = ndi_lib_dir {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
            println!("cargo:warning=NDI rpath: {}", dir.display());
        }

        // Standard executable-relative rpaths for bundled deployments
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path/../Frameworks");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");

        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-env-changed=NDI_SDK_DIR");
        println!("cargo:rerun-if-env-changed=SYPHON_FRAMEWORK_DIR");
    }
}
