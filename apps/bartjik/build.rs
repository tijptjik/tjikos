fn main() {
    if std::env::var_os("CARGO_FEATURE_GTK_UI").is_some() {
        println!("cargo:rustc-link-arg-bin=bartjik-gtk=-Wl,-rpath,$ORIGIN/../lib");
    }
}
