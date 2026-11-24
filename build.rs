fn main() {
    #[cfg(windows)]
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=assets/icons/app_icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/app_icon.ico");

    if let Err(error) = resource.compile() {
        panic!("failed to compile Windows icon resource: {error}");
    }
}
