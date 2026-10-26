fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // ScreenCaptureKit's Swift bridge uses the OS Swift concurrency runtime.
        // Cargo does not add Swift's normal runtime search path to a Rust binary.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        // Some Swift bridge dependencies assume a full Xcode installation. The
        // standalone Command Line Tools keep compatibility archives elsewhere.
        if let Ok(output) = std::process::Command::new("xcrun")
            .args(["--find", "swift"])
            .output()
            && output.status.success()
        {
            let swift = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if let Some(usr) = swift.parent().and_then(|p| p.parent()) {
                let libraries = usr.join("lib/swift/macosx");
                if libraries.is_dir() {
                    println!("cargo:rustc-link-search=native={}", libraries.display());
                }
            }
        }
    }
}
