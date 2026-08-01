fn main() {
    // The Windows application icon is compiled into the executable by this
    // build script. Cargo does not otherwise know that changing this asset
    // invalidates the generated resource object.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    tauri_build::build()
}
