use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    // Recheck provenance on every build, including changes in shared packages.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=../../packages/notjik-sync");
    println!("cargo:rerun-if-changed=../../packages/tjik-ui");
    for name in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&[
        "status",
        "--porcelain",
        "--untracked-files=normal",
        "--",
        ".",
        "../../packages/notjik-sync",
        "../../packages/tjik-ui",
    ])
    .is_none_or(|status| !status.is_empty());
    println!("cargo:rustc-env=TJIKUP_SOURCE_REVISION={revision}");
    println!("cargo:rustc-env=TJIKUP_SOURCE_DIRTY={dirty}");
}
