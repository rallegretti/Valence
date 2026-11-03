//! Windows: embed the app icon and version info in valence.exe. Only when
//! building on Windows (rc.exe); cross-checks from other hosts skip it.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/valence.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/valence.ico")
            .set("ProductName", "Valence")
            .set("FileDescription", "Valence: build molecules in 3D");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {e}");
        }
    }
}
