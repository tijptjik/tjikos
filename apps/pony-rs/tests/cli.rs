use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn piped_quote_and_regular_extra_name_collisions() {
    let run = |extra: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pony-rs"));
        command
            .args(["-f", "applejack", "-W", "100"])
            .env("TERM", "xterm-kitty");
        if extra {
            command.arg("--extra");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"A fresh piped quote\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("A fresh piped quote"));
        text
    };
    assert_ne!(run(false), run(true));
}

#[test]
fn listing_and_invalid_selection() {
    for (options, expected) in [
        (vec!["--list"], 474),
        (vec!["--all", "--list"], 577),
        (vec!["--extra", "--list"], 103),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pony-rs"))
            .args(options)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().lines().count(),
            expected
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_pony-rs"))
        .args(["-f", "missing-pony", "hello"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("Unknown pony"));
}
