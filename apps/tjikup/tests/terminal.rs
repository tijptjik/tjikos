use serde_json::json;
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Terminal {
    master: File,
    child: Child,
    text: Vec<u8>,
}
impl Terminal {
    fn spawn(args: &[&str], dir: &std::path::Path, graphics: bool) -> Self {
        Self::spawn_spacing(args, dir, graphics, true)
    }
    fn spawn_spacing(args: &[&str], dir: &std::path::Path, graphics: bool, compact: bool) -> Self {
        Self::spawn_terminal(args, dir, graphics, compact, false)
    }
    fn spawn_terminal(
        args: &[&str],
        dir: &std::path::Path,
        graphics: bool,
        compact: bool,
        herdr: bool,
    ) -> Self {
        let mut master = 0;
        let mut slave = 0;
        let size = libc::winsize {
            ws_row: 40,
            ws_col: 100,
            ws_xpixel: 1000,
            ws_ypixel: 800,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    &size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut command = Command::new(env!("CARGO_BIN_EXE_tjikup"));
        command
            .args(args)
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("TJIKUP_NOTIFY", "0")
            .env("TERM", "xterm-kitty")
            .env("HERDR_ENV", if herdr { "1" } else { "0" })
            .env("TERM_PROGRAM", if herdr { "herdr" } else { "kitty" })
            .env("TJIKUP_COLOR", "1")
            .env_remove("TJIKUP_COMPACT")
            .env("TJIKUP_KITTY_ICONS", if graphics { "1" } else { "0" })
            .env_remove("TJIKUP_NO_SPIN")
            .env_remove("TMUX")
            .env_remove("STY")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        if compact {
            command.env("TJIKUP_COMPACT", "1");
        }
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        unsafe {
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
        }
        Self {
            master,
            child,
            text: vec![],
        }
    }
    fn poll(&mut self) {
        let mut buffer = [0; 8192];
        loop {
            match self.master.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => self.text.extend_from_slice(&buffer[..n]),
                Err(_) => break,
            }
        }
    }
    fn until(&mut self, needle: &str) {
        let start = Instant::now();
        while !String::from_utf8_lossy(&self.text).contains(needle) {
            self.poll();
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "missing {needle}: {}",
                String::from_utf8_lossy(&self.text)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self) -> String {
        self.finish_with_code(0)
    }
    fn finish_with_code(&mut self, expected: i32) -> String {
        let start = Instant::now();
        loop {
            self.poll();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.poll();
                assert_eq!(
                    status.code(),
                    Some(expected),
                    "{}",
                    String::from_utf8_lossy(&self.text)
                );
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(10));
        }
        String::from_utf8_lossy(&self.text).into_owned()
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn visible_text(text: &str) -> String {
    let mut visible = String::new();
    for chunk in text.split('\x1b') {
        if let Some(packet) = chunk.strip_prefix("]66;") {
            // Kitty may split a label across scaled-text packets.
            visible.push_str(packet.split_once(';').unwrap().1);
        } else if let Some(csi) = chunk.strip_prefix('[') {
            if let Some(end) = csi.find(|c: char| ('@'..='~').contains(&c)) {
                visible.push_str(&csi[end + 1..]);
            }
        } else if let Some(rest) = chunk.strip_prefix('\\') {
            visible.push_str(rest);
        } else if !chunk.starts_with([']', '_']) {
            visible.push_str(chunk);
        }
    }
    visible
}
#[test]
fn real_terminal_prompt_keeps_foreground_input_and_filters_fragmented_status() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("prompt.sh"),"printf 'SK'\nsleep 0.1\nprintf 'IP    ✓'\nsleep 0.1\nprintf ' Fonts    no changes\\n'\nprintf '__stage_note__:internal only\\n'\nprintf 'Continue? '\nread -r answer\n[ \"$answer\" = yes ] || exit 2\nprintf 'UPDATE  ✓ Bartjik    updated\\n'\n").unwrap();
    fs::write(dir.path().join("config.json"),serde_json::to_vec(&json!({"version":2,"recipes":[{"id":"before","apply":["sleep","0.15"]},{"id":"prompt","needs":["before"],"interactive":true,"apply":["sh",dir.path().join("prompt.sh")],"display":{"reports":true}}]})).unwrap()).unwrap();
    let mut terminal = Terminal::spawn(
        &["--config", dir.path().join("config.json").to_str().unwrap()],
        dir.path(),
        false,
    );
    terminal.until("Continue? ");
    let before_prompt = String::from_utf8_lossy(&terminal.text);
    assert!(before_prompt.contains("\x1b[?25l"));
    assert!(
        before_prompt.rfind("\x1b[?25h") > before_prompt.rfind("\x1b[?25l"),
        "cursor must be visible for input"
    );
    terminal.master.write_all(b"yes\n").unwrap();
    let text = terminal.finish();
    assert!(
        !text.contains("__stage_note__"),
        "internal status markers must stay private"
    );
    assert!(!text.contains("SKIP"), "{text}");
    assert_eq!(text.matches("Bartjik").count(), 1);
    assert!(text.contains("\x1b[1;38;2;255;95;95mA"));
}
#[test]
fn kitty_protocol_uses_scrollable_sprites_without_retransmission_on_ticks() {
    let dir = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(&["--demo", "changed"], dir.path(), true);
    let text = terminal.finish();
    assert_eq!(
        text.matches("a=T,f=32").count(),
        6,
        "three repo images, two result images and the activity fallback, uploaded once"
    );
    assert!(text.contains("\u{10eeee}"));
    assert!(text.contains(",r=1,"));
    assert!(!text.contains(",r=2,"));
    assert!(!text.contains("\u{10eeee}\u{30d}"));
    assert!(!text.contains("\x1b]66;"));
    assert!(!text.contains("\x1b[0C"));
    assert!(text.contains("\x1b[94;1mRecipes"));
    assert!(text.contains("38;2;255;95;95"));
    assert_eq!(
        text.matches("\x1b[?25l").count(),
        1,
        "hide once, not on every frame"
    );
    assert_eq!(
        text.matches("\x1b[?25h").count(),
        1,
        "do not flash the cursor between frames"
    );
    assert!(text.ends_with("\x1b[?25h"), "restore on completion");
    let begin = "\x1b[?2026h";
    let end = "\x1b[?2026l";
    assert!(
        text.matches(begin).count() > 3,
        "animation uses synchronized frames"
    );
    assert_eq!(text.matches(begin).count(), text.matches(end).count());
    for frame in text.split(begin).skip(1) {
        let frame = frame.split_once(end).unwrap().0;
        assert!(
            !frame.contains("\x1b[2K"),
            "steady frames must not blank the activity line"
        );
    }
    assert!(
        text.contains("\x1b[K"),
        "clear stale text only after overwriting"
    );
}
#[test]
fn resize_during_repository_activity_keeps_graphics_and_completes() {
    let dir = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(&["--demo", "attention"], dir.path(), true);
    terminal.until("a=T,f=32");
    let size = libc::winsize {
        ws_row: 40,
        ws_col: 48,
        ws_xpixel: 480,
        ws_ypixel: 800,
    };
    assert_eq!(
        unsafe { libc::ioctl(terminal.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
    let text = terminal.finish();
    assert_eq!(text.matches("a=T,f=32").count(), 6);
    assert!(text.contains("\x1b[J"));
    assert!(text.contains("\x1b[31;1mN"));
}

#[test]
fn interrupted_spinner_restores_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        serde_json::to_vec(
            &json!({"version":2,"recipes":[{"id":"waiting","apply":["sleep","30"]}]}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut terminal = Terminal::spawn(&["--config", path.to_str().unwrap()], dir.path(), false);
    terminal.until("\x1b[?25l");
    unsafe {
        libc::kill(terminal.child.id() as i32, libc::SIGINT);
    }
    let start = Instant::now();
    loop {
        terminal.poll();
        if let Some(status) = terminal.child.try_wait().unwrap() {
            terminal.poll();
            assert_eq!(status.code(), Some(130));
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = String::from_utf8_lossy(&terminal.text);
    assert!(
        text.rfind("\x1b[?25h") > text.rfind("\x1b[?25l"),
        "cursor remains hidden after interrupt: {text}"
    );
}

#[test]
fn repository_counts_reach_terminal_with_positive_and_zero_totals() {
    let dir = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn(&["--demo", "changed"], dir.path(), false);
    let text = terminal.finish();
    for expected in ["↓3", "↑1", "↓0", "↑0", "↓12", "↑2"] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
}

#[test]
fn dirty_repository_counts_stay_in_rows_with_failure_logs_below() {
    for graphics in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("icons32.json"),
            include_bytes!("fixtures/icons32.json"),
        )
        .unwrap();
        let mut recipes = Vec::new();
        for (label, count) in [("dotjik", 2), ("tjikOS", 3)] {
            let repo = dir.path().join(label);
            fs::create_dir(&repo).unwrap();
            let initialized = Command::new("git")
                .args(["init", "--quiet"])
                .arg(&repo)
                .output()
                .unwrap();
            assert!(initialized.status.success());
            for index in 0..count {
                fs::write(repo.join(format!("fixture-{index}")), "synthetic").unwrap();
            }
            recipes.push(json!({
                "id":format!("{label}:pull"),
                "git":{"repository":repo,"operation":"pull-ff"},
                "display":{"repository":label,"operation":"pull"}
            }));
            recipes.push(json!({
                "id":format!("{label}:push"),"needs":[format!("{label}:pull")],
                "git":{"repository":repo,"operation":"push"},
                "display":{"repository":label,"operation":"push"}
            }));
        }
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"version":2,"jobs":2,"recipes":recipes,
                "presentation":{"icons":"{config_dir}/icons32.json",
                                "glyphs":{"dotjik":"H","tjikOS":"T"}}
            }))
            .unwrap(),
        )
        .unwrap();
        let mut terminal =
            Terminal::spawn(&["--config", path.to_str().unwrap()], dir.path(), graphics);
        let text = terminal.finish_with_code(1);
        let visible = visible_text(&text);
        for (label, count) in [("dotjik", 2), ("tjikOS", 3)] {
            let rows = visible.split("Failed:").next().unwrap();
            let row = rows.rsplit_once(label).unwrap().1;
            let count = format!("({count} files)");
            assert!(row.contains(&count), "missing final count: {visible}");
            assert!(row.find("PULL") < row.find("PUSH"), "{visible}");
            assert!(row.find("PUSH") < row.find(&count), "{visible}");
        }
        for hidden in ["commit or stash", "dirty repository"] {
            assert!(!visible.contains(hidden), "unexpected {hidden}: {visible}");
        }
        assert!(visible.contains("Log:"), "{visible}");
        assert!(visible.contains("dotjik:pull.log"), "{visible}");
        assert!(visible.contains("tjikOS:pull.log"), "{visible}");
        if graphics {
            assert_eq!(text.matches("a=T,f=32").count(), 2);
        }
    }
}

#[test]
fn recipe_rows_prefer_explicit_artwork_over_known_label_artwork() {
    let dir = tempfile::tempdir().unwrap();
    let mut sprites: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/icons32.json")).unwrap();
    sprites["sprites"]["Templates"] = sprites["sprites"]["Systemd"].clone();
    fs::write(
        dir.path().join("icons32.json"),
        serde_json::to_vec(&sprites).unwrap(),
    )
    .unwrap();
    let recipes: Vec<_> = ["Tjikup", "Templates (system)", "Templates (home)"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            json!({
                "id":format!("fixture-{index}"), "label":label, "apply":["true"],
                "display":{"icon":"Tjikup", "section":"Preparation", "verb":"CHECK"}
            })
        })
        .collect();
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"version":2,"recipes":recipes,
            "presentation":{"alphabetical_results":true,"icons":"{config_dir}/icons32.json",
                "glyphs":{"Tjikup":"U","Templates":"G"}}
        }))
        .unwrap(),
    )
    .unwrap();
    for graphics in [true, false] {
        let mut terminal =
            Terminal::spawn(&["--config", path.to_str().unwrap()], dir.path(), graphics);
        let text = terminal.finish();
        for subtype in ["system", "home"] {
            assert!(text.contains(&format!("({subtype}")));
        }
        if graphics {
            assert_eq!(
                text.matches("a=T,f=32").count(),
                1,
                "all running indicators and queued results must reuse the Tjikup image"
            );
        } else {
            assert_eq!(
                text.matches("✓\x1b[0m U \x1b[97mTemplates").count(),
                2,
                "{text}"
            );
            assert!(!text.contains("G \x1b[97mTemplates"), "{text}");
        }
    }
}

#[test]
fn subtask_rows_resolve_recipe_and_alias_icons_in_graphics_and_glyph_modes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("icons32.json"),
        include_bytes!("fixtures/icons32.json"),
    )
    .unwrap();
    let recipes: Vec<_> = [
        ("plugins", "Herdr", "Plugin : File viewer"),
        ("units", "Systemd", "Common Units"),
        ("voice", "Systemd", "Hyprvoice"),
        ("backup", "Systemd", "Cloudflare backup"),
        ("network", "Tailscale", "Tailnet connection"),
        ("zed", "Templates", "Zed"),
        ("solaar", "Templates", "Solaar"),
        ("fcitx", "Templates", "Fcitx"),
        ("codex", "Templates", "Codex"),
        ("zapzap", "Templates", "Zapzap"),
        ("bookmarks", "Templates", "Gtk-bookmarks"),
    ].into_iter().map(|(id, icon, subject)| json!({
        "id":id, "label":"Unrelated display label",
        "apply":["sh","-c","printf 'SKIP\t✓\t%s\tavailable\n' \"$$1\" > \"$$TJIKUP_REPORT_FILE\"","fixture",subject],
        "display":{"icon":icon,"reports":true}
    })).collect();
    let config = json!({"version":2,"presentation":{
        "icons":"{config_dir}/icons32.json",
        "icon_aliases":{"Cloudflare backup":"Storage"},
        "glyphs":{"Herdr":"H","Systemd":"S","Hyprvoice":"V","Storage":"D","Tailscale":"T","Zed":"Z","Solaar":"O","Fcitx":"F","Codex":"C","Zapzap":"W","Gtk-bookmarks":"B","Python":"P"}
    },"prerequisites":[{"id":"require:python","check":["true"],"display":{"icon":"Python"}}],"recipes":recipes});
    let path = dir.path().join("config.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    for graphics in [true, false] {
        let mut terminal = Terminal::spawn(
            &["--config", path.to_str().unwrap(), "--show-skips"],
            dir.path(),
            graphics,
        );
        let text = terminal.finish();
        assert!(text.contains("Prerequisites"));
        if !graphics {
            assert!(text.contains("REQUIRE"));
        }
        assert!(!text.contains("require:python"));
        assert!(text.contains("Recipes"));
        assert!(!text.contains("Receipt:"));
        if graphics {
            assert_eq!(
                text.matches("a=T,f=32").count(),
                13,
                "known subjects and the activity fallback each upload once"
            );
        } else {
            for (glyph, label) in [
                ("H", "Plugin : File viewer"),
                ("S", "Common Units"),
                ("V", "Hyprvoice"),
                ("D", "Cloudflare backup"),
                ("T", "Tailnet connection"),
                ("Z", "Zed"),
                ("O", "Solaar"),
                ("F", "Fcitx"),
                ("C", "Codex"),
                ("W", "Zapzap"),
                ("B", "Gtk-bookmarks"),
            ] {
                assert!(
                    text.contains(&format!("{glyph} \x1b[97m{label}")),
                    "missing {glyph} for {label}: {text}"
                );
            }
        }
    }
}

#[test]
fn new_device_prompts_for_role_subscriptions_and_confirmation_without_applying() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("enrollment.json");
    let profile = dir.path().join("device.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({"version":2,"recipes":[
            {"id":"base","apply":["touch","must-not-run"]},
            {"id":"editor","label":"Editor","optional":true,"roles":["laptop"],"apply":["true"]},
            {"id":"server","label":"Server","optional":true,"roles":["server"],"apply":["true"]}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let mut terminal = Terminal::spawn(
        &[
            "new",
            "device",
            "--config",
            config.to_str().unwrap(),
            "--output",
            profile.to_str().unwrap(),
        ],
        dir.path(),
        false,
    );
    terminal.until("Device type");
    terminal.master.write_all(b"laptop\n").unwrap();
    terminal.until("Select numbers");
    terminal.master.write_all(b"1\n").unwrap();
    terminal.until("Save this device profile?");
    assert!(!profile.exists());
    terminal.master.write_all(b"y\n").unwrap();
    let text = terminal.finish();
    assert!(text.contains("Editor [editor]"));
    assert!(!text.contains("Server [server]"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(profile).unwrap()).unwrap();
    assert_eq!(value["role"], "laptop");
    assert_eq!(value["subscriptions"], json!(["editor"]));
    assert!(!dir.path().join("must-not-run").exists());
}

#[test]
fn spacious_graphics_is_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut terminal = Terminal::spawn_spacing(&["--demo", "changed"], dir.path(), true, false);
    let text = terminal.finish();
    assert!(text.contains(",r=2,"));
    assert!(text.contains("\u{10eeee}\u{30d}"));
    assert!(text.contains("\x1b]66;"));
    assert!(
        text.contains("\r\x1b7"),
        "repository patches anchor their cursor origin"
    );
    assert!(
        text.contains("\r\x1b7\x1b[7A"),
        "three two-line repo rows plus the spinner gap"
    );
    assert_eq!(text.matches("\x1b7").count(), text.matches("\x1b8").count());
}

#[test]
fn herdr_keeps_ordinary_text_beside_single_row_icons() {
    let dir = tempfile::tempdir().unwrap();
    let mut terminal =
        Terminal::spawn_terminal(&["--demo", "changed"], dir.path(), true, true, true);
    let text = terminal.finish();
    assert!(text.contains(",r=1,"));
    assert!(text.contains("\u{10eeee}"));
    assert!(!text.contains("\x1b]66;"));
}

#[test]
fn execution_groups_stream_settled_rows_before_slow_peers_finish() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, serde_json::to_vec(&json!({"version":2,"jobs":2,
        "presentation":{"grouped_results":true},"recipes":[
        {"id":"slow","label":"Slow worker","display":{"section":"Configuration"},
         "apply":["sh","-c","touch slow-started; n=0; while [ ! -e release ]; do n=$$((n+1)); [ $$n -lt 100 ] || exit 8; sleep 0.05; done; touch slow-done; echo 'APPLY   ✓ Finished slow'"]},
        {"id":"fast","label":"Fast worker","display":{"section":"Configuration"},
         "apply":["sh","-c","sleep 0.1; echo 'APPLY   ✓ Finished fast'"]},
        {"id":"update","label":"Update worker","display":{"section":"Package updates"},
         "apply":["sh","-c","test -e slow-done && touch update-started"]}
    ]})).unwrap()).unwrap();
    let mut terminal = Terminal::spawn(&["--config", path.to_str().unwrap()], dir.path(), false);
    terminal.until("Finished fast");
    assert!(dir.path().join("slow-started").exists());
    assert!(
        !dir.path().join("slow-done").exists(),
        "fast result must not wait for its group"
    );
    assert!(
        !dir.path().join("update-started").exists(),
        "next group must wait"
    );
    let text = visible_text(&String::from_utf8_lossy(&terminal.text));
    let spinner = text
        .split('\r')
        .find(|s| s.contains("Slow worker") && s.trim_end().ends_with('s'))
        .expect("activity line");
    let spinner = spinner.lines().next().unwrap();
    assert!(
        spinner.contains("Slow worker                    "),
        "timing belongs on the right"
    );
    fs::write(dir.path().join("release"), "").unwrap();
    let text = terminal.finish();
    assert!(dir.path().join("update-started").exists());
    assert_eq!(text.matches("Configuration").count(), 1);
    assert!(text.find("Finished fast").unwrap() < text.find("Finished slow").unwrap());
}

#[test]
fn credential_refresh_is_visible_while_the_refresh_command_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path,serde_json::to_vec(&json!({"version":2,"recipes":[{
        "id":"auth","label":"Service","apply":["sh","-c","[ -e refreshed ] || exit 77"],
        "credential_recovery":{"secret_ids":["11111111-2222-3333-4444-555555555555"],"failure_codes":[77],"retry_safe":true,
        "refresh":["sh","-c","n=0; while [ ! -e release ]; do n=$$((n+1)); [ $$n -lt 100 ] || exit 8; sleep 0.05; done; touch refreshed"]}
    }]})).unwrap()).unwrap();
    let mut terminal = Terminal::spawn(&["--config", path.to_str().unwrap()], dir.path(), false);
    terminal.until("Credentials failed; refreshing from Bitwarden");
    assert!(!dir.path().join("refreshed").exists());
    fs::write(dir.path().join("release"), "").unwrap();
    let text = terminal.finish();
    assert!(!text.contains("FAIL    "));
    assert!(dir.path().join("refreshed").exists());
}

#[test]
fn active_row_uses_application_art_and_dimmed_subtitle() {
    for (graphics, compact) in [(false, true), (true, true), (true, false)] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("icons32.json"),
            include_bytes!("fixtures/icons32.json"),
        )
        .unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "version":2,
                "presentation":{"icons":"{config_dir}/icons32.json","glyphs":{"Fedora":"F"}},
                "recipes":[{"id":"rpm","label":"Fedora (RPM packages)",
                    "display":{"icon":"Fedora"},
                    "apply":["sh","-c","while [ ! -e release ]; do sleep 0.05; done"]}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let mut terminal = Terminal::spawn_spacing(
            &["--config", path.to_str().unwrap()],
            dir.path(),
            graphics,
            compact,
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while String::from_utf8_lossy(&terminal.text)
            .matches("\x1b[?2026l")
            .count()
            < 3
        {
            terminal.poll();
            assert!(
                Instant::now() < deadline,
                "expected three live activity frames"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let active = String::from_utf8_lossy(&terminal.text);
        assert!(!active.contains("APPLY"), "inspect before completion");
        if graphics {
            assert!(active.contains("\u{10eeee}"), "active row has artwork");
            assert_eq!(
                active.matches("a=T,f=32").count(),
                1,
                "ticks reuse uploaded image"
            );
            if !compact {
                assert!(
                    active.contains("\u{10eeee}\u{30d}"),
                    "second icon row is present"
                );
            }
        } else {
            assert!(active.contains("F \x1b[97mFedora"));
        }
        if compact {
            assert!(
                active.contains("\x1b[90m (RPM packages)"),
                "same dim subtitle as completed rows"
            );
        } else {
            assert!(
                active.contains("\x1b[90m\x1b]66;"),
                "dim styling reaches two-row text"
            );
        }
        fs::write(dir.path().join("release"), "").unwrap();
        let text = terminal.finish();
        if graphics {
            assert_eq!(
                text.matches("a=T,f=32").count(),
                1,
                "completion reuses activity artwork"
            );
        }
    }
}

#[test]
fn alphabetical_review_buffers_each_section_but_keeps_parallel_execution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path, serde_json::to_vec(&json!({"version":2,"jobs":2,
      "recipes":[
       {"id":"slow","label":"Worker A","display":{"section":"Configuration","reports":true},
        "apply":["sh","-c","touch started; while [ ! -e release ]; do sleep 0.05; done; echo 'APPLY   ✓ Alpha result'"]},
       {"id":"fast","label":"Worker B","display":{"section":"Configuration","reports":true},
        "apply":["sh","-c","touch fast-done; echo 'APPLY   ✓ Zulu result'"]},
       {"id":"last","label":"Final","display":{"section":"Services"},
        "apply":["true"]}
      ]})).unwrap()).unwrap();
    let mut terminal = Terminal::spawn(
        &["--config", path.to_str().unwrap(), "--alphabetical"],
        dir.path(),
        false,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !dir.path().join("fast-done").exists() || !dir.path().join("started").exists() {
        terminal.poll();
        assert!(Instant::now() < deadline, "both section workers must start");
        std::thread::sleep(Duration::from_millis(10));
    }
    terminal.poll();
    assert!(!String::from_utf8_lossy(&terminal.text).contains("Zulu result"));
    fs::write(dir.path().join("release"), "").unwrap();
    let output = terminal.finish();
    assert!(output.find("Alpha result").unwrap() < output.find("Zulu result").unwrap());
    assert!(output.find("Zulu result").unwrap() < output.find("Services").unwrap());
}

#[test]
fn repository_cells_replace_the_spinner_but_later_tasks_keep_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.json"), serde_json::to_vec(&json!({"version":2,"recipes":[
        {"id":"repo","label":"Duplicate repository activity","apply":["sleep","0.2"],"display":{"repository":"dotjik","operation":"pull"}},
        {"id":"later","label":"Later worker","needs":["repo"],"apply":["sleep","0.2"]}
    ]})).unwrap()).unwrap();
    let mut terminal = Terminal::spawn(
        &["--config", dir.path().join("config.json").to_str().unwrap()],
        dir.path(),
        false,
    );
    let text = terminal.finish();
    assert!(text.contains("PULL"));
    assert!(
        !text.contains("Duplicate repository activity"),
        "repository activity belongs only in its permanent cells"
    );
    assert!(
        text.matches("Later worker").count() > 1,
        "later work retains live activity and its completion row"
    );
}
