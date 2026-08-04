fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !cfg!(windows) {
        panic!("Shenava Windows release artifacts must be built on a native Windows host so their icon and version resources cannot be omitted");
    }

    const ICON: &str = "../../windows/ShenavaOverlay/Shenava.ico";
    println!("cargo:rerun-if-changed={ICON}");

    let mut resource = winres::WindowsResource::new();
    resource
        .set_icon(ICON)
        .set("ProductName", "Shenava")
        .set("FileDescription", "Shenava")
        .set("ProductVersion", "0.1.0")
        .set("OriginalFilename", "Shenava.exe")
        .set("InternalName", "Shenava");
    resource
        .compile()
        .expect("compile Shenava Windows icon and version resources");
}
