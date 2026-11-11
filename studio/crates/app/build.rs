//! Build script: Cargo runs this before compiling the app.
//!
//! On Windows it puts the Duck Forge icon and version details into
//! DuckForgeStudio.exe, so File Explorer, the taskbar and desktop
//! shortcuts show them. On other systems it does nothing.

fn main() {
    // Only run again when the icon or the version number changes.
    println!("cargo:rerun-if-changed=../../brand/duck-forge.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");

    // Only Windows programs carry an icon like this.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../brand/duck-forge.ico");
    resource.set("ProductName", "Duck Forge Studio");
    resource.set("FileDescription", "Duck Forge Studio");
    if let Err(error) = resource.compile() {
        // On your own PC a missing icon is not worth failing the whole
        // build over. On GitHub's build machines (which set CI), it is:
        // the download must have its icon.
        if std::env::var_os("CI").is_some() {
            panic!("Could not add the icon to DuckForgeStudio.exe: {error}");
        }
        println!("cargo:warning=Could not add the icon to DuckForgeStudio.exe: {error}");
    }
}
