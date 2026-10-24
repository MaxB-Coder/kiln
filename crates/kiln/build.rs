//! Windows: puts the app icon (assets/icon/kiln.ico, made by scripts/make-icons.sh) and the
//! version details into kiln.exe, so Explorer, the taskbar and the installer's shortcuts show
//! it. Nothing to do elsewhere: macOS and Linux packages carry the icon as files.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/kiln.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../../assets/icon/kiln.ico")
            .set("ProductName", "Kiln")
            .set("FileDescription", "Kiln");
        resource
            .compile()
            .expect("could not put the icon into kiln.exe");
    }
}
