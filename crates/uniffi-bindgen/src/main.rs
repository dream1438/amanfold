//! Generates the Swift, Kotlin and Python bindings from the built library:
//! `cargo run -p uniffi-bindgen -- generate --library <lib> --language swift --out-dir <dir>`
fn main() {
    uniffi::uniffi_bindgen_main()
}
