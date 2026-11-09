fn main() {
    if std::env::var_os("CARGO_FEATURE_GTK_UI").is_some() {
        println!("cargo:rustc-link-arg-bin=notjik-gtk=-Wl,-rpath,$ORIGIN/../lib");
    }
}
