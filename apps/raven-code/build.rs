//! Windows resource embedding: icons `raven-code.exe` with the Raven brand
//! icon and sets version metadata.
//!
//! On non-Windows targets this is a no-op. If the resource compiler is
//! unavailable (e.g. some cross-compilation setups), the build continues
//! with a warning instead of failing — the binary works, it just lacks the
//! embedded icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/raven-code.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/raven-code.ico");
        resource.set(
            "FileDescription",
            "Raven Code - the Raven ecosystem's lightweight code editor",
        );
        resource.set("ProductName", "Raven Code");
        resource.set("LegalCopyright", "MIT License - Raven Developers");
        resource.set("OriginalFilename", "raven-code.exe");
        if let Err(error) = resource.compile() {
            println!(
                "cargo:warning=raven-code: could not embed Windows resources (icon will be missing): {error}"
            );
        }
    }
}
