fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/icon.ico")
        .set("ProductName", "Batch Audio")
        .set("FileDescription", "Batch Audio")
        .compile()
        .expect("could not embed the Windows icon");
}
