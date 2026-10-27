use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut generated = String::from("pub static PONIES: &[Pony] = &[\n");
    for (group, extra, tty) in [
        ("ponies", false, false),
        ("extraponies", true, false),
        ("ttyponies", false, true),
        ("extrattyponies", true, true),
    ] {
        let dir = root.join("assets").join(group);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "pony"))
            .collect();
        files.sort();
        for path in files {
            let name = path.file_stem().unwrap().to_str().unwrap();
            generated.push_str(&format!(
                "Pony {{ name: {name:?}, extra: {extra}, tty: {tty}, art: include_str!({:?}) }},\n",
                path.to_str().unwrap()
            ));
        }
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ponies.rs"),
        generated,
    )
    .unwrap();
}
