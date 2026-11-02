fn main() {
    // The icons go into the program here, and tauri_build does not ask to run again when only they change.
    println!("cargo:rerun-if-changed=icons");

    tauri_build::build()
}
