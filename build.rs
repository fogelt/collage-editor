fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

    #[cfg(windows)]
    {
        // Fail softly: the app still builds (and gets a window icon at runtime)
        // if the Windows resource compiler is unavailable.
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/icons/icon.ico");
        resource.set("ProductName", "Collage Editor");
        resource.set("FileDescription", "Collage editor for arranging photos");

        if let Err(error) = resource.compile() {
            println!("cargo:warning=Could not embed the Windows icon: {error}");
        }
    }
}
