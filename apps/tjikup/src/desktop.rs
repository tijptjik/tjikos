//! Desktop installation maintenance, with recoverable local changes.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};

pub fn ini_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == format!("[{section}]");
        } else if inside && !line.starts_with(['#', ';']) {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    found = Some(v.trim());
                }
            }
        }
    }
    found
}
pub fn ini_setting(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let mut inside = false;
    for i in 0..lines.len() {
        let line = lines[i].trim();
        if line.starts_with('[') {
            if inside {
                lines.insert(i, format!("{key}={value}\n"));
                return lines.concat();
            }
            inside = line == format!("[{section}]");
        } else if inside && line.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            lines[i] = format!("{key}={value}\n");
            return lines.concat();
        }
    }
    if inside {
        format!("{}\n{key}={value}\n", text.trim_end_matches('\n'))
    } else {
        format!(
            "{}{}[{section}]\n{key}={value}\n",
            text.trim_end_matches('\n'),
            if text.is_empty() { "" } else { "\n\n" }
        )
    }
}
fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    fs::create_dir_all(path.parent().context("missing parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(bytes)?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn json_file(path: &Path) -> Result<Value> {
    if path.is_file() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        Ok(json!({}))
    }
}
fn save_json(path: &Path, value: &Value) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?, 0o600)
}
fn copy_preserve(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to.parent().context("backup parent missing")?)?;
    if !Command::new("cp")
        .args(["--preserve=all", "--"])
        .arg(from)
        .arg(to)
        .status()?
        .success()
    {
        bail!("could not preserve file metadata");
    }
    Ok(())
}
fn zed(repo: &Path, args: &[String]) -> Result<i32> {
    if args.len() < 4
        || args[0] != "--channel"
        || args[2] != "--"
        || !["stable", "preview", "nightly"].contains(&args[1].as_str())
    {
        bail!("desktop zed --channel stable|preview|nightly -- COMMAND ...");
    }
    let channel = &args[1];
    let suffix = if channel == "stable" {
        String::new()
    } else {
        format!("-{channel}")
    };
    let id = if channel == "stable" {
        "dev.zed.Zed".into()
    } else {
        format!(
            "dev.zed.Zed-{}{}",
            channel[..1].to_uppercase(),
            &channel[1..]
        )
    };
    let home = crate::commands::home()?;
    let target = home.join(format!(".local/share/applications/{id}.desktop"));
    let ctx = crate::workflow::context(repo)?;
    let host = ctx["hostname"].as_str().context("missing hostname")?;
    let specific = repo.join(format!("native/home/files-{host}.json"));
    let manifest = if specific.is_file() {
        specific
    } else {
        repo.join("native/home/files-device.json")
    };
    let managed = crate::managed_files::source_path(&manifest, &ctx, &target)?.is_some();
    if !managed {
        return Ok(Command::new(&args[3])
            .args(&args[4..])
            .status()?
            .code()
            .unwrap_or(1));
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".local/state"))
        .join("tjikup/zed-launchers");
    fs::create_dir_all(&state)?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
    let backup = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(&state)?
        .keep();
    let before = match fs::symlink_metadata(&target) {
        Ok(info) => {
            if !info.is_file() || info.uid() != unsafe { libc::geteuid() } {
                bail!("launcher must be a regular file owned by this user");
            }
            copy_preserve(&target, &backup.join(target.file_name().unwrap()))?;
            Some(fs::read(&target)?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let code = Command::new(&args[3])
        .args(&args[4..])
        .status()?
        .code()
        .unwrap_or(1);
    if target.is_symlink() {
        bail!(
            "launcher changed to a symlink; backup: {}",
            backup.display()
        );
    }
    let after = if target.exists() {
        Some(fs::read(&target)?)
    } else {
        None
    };
    if before == after {
        return Ok(code);
    }
    if let Some(after) = &after {
        let app = home.join(format!(".local/zed{suffix}.app"));
        let dir = app.join("share/applications");
        let mut vendor = dir.join(format!("{id}.desktop"));
        if !vendor.is_file() {
            vendor = dir.join(format!("zed{suffix}.desktop"));
        }
        let rendered = fs::read_to_string(vendor)?
            .replace(
                "Icon=zed",
                &format!(
                    "Icon={}",
                    app.join("share/icons/hicolor/512x512/apps/zed.png")
                        .display()
                ),
            )
            .replace(
                "Exec=zed",
                &format!("Exec={}", app.join("bin/zed").display()),
            );
        if after != rendered.as_bytes() {
            bail!("unexpected launcher edit; backup: {}", backup.display());
        }
    }
    if before.is_some() {
        let temporary = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
        copy_preserve(&backup.join(target.file_name().unwrap()), temporary.path())?;
        temporary.persist(&target).map_err(|e| e.error)?;
    } else if target.exists() {
        fs::remove_file(&target)?;
    }
    Ok(code)
}
fn cleanup(repo: &Path, apply: bool) -> Result<i32> {
    let policy: Value =
        serde_json::from_slice(&fs::read(repo.join("native/desktop-maintenance.json"))?)?;
    let replacements = policy["launcher_replacements"]
        .as_object()
        .context("launcher replacements missing")?;
    let home = crate::commands::home()?;
    let apps = home.join(".local/share/applications");
    let registry = home.join(".local/share/firefoxpwa/config.json");
    let sites = if registry.is_file() {
        Some(json_file(&registry)?)
    } else {
        None
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let archive = home.join(format!(
        ".local/state/runtjik/desktop-entry-archives/{stamp}"
    ));
    let mut changes = Vec::new();
    for (old, new) in replacements {
        let new = new.as_str().context("replacement name required")?;
        if [old.as_str(), new]
            .iter()
            .any(|s| Path::new(s).components().count() != 1)
        {
            bail!("invalid launcher name");
        }
        let source = apps.join(old);
        let replacement = apps.join(new);
        if !source.is_file() || !replacement.is_file() {
            continue;
        }
        if old.starts_with("FFPWA-") {
            let old_id = old
                .trim_start_matches("FFPWA-")
                .trim_end_matches(".desktop");
            let new_id = new
                .trim_start_matches("FFPWA-")
                .trim_end_matches(".desktop");
            if sites
                .as_ref()
                .is_none_or(|s| !s["sites"][old_id].is_null() || s["sites"][new_id].is_null())
            {
                continue;
            }
        }
        changes.push((old.clone(), new.to_owned()));
    }
    let mime = home.join(".config/mimeapps.list");
    let original = fs::read_to_string(&mime).unwrap_or_default();
    let mut revised = original.clone();
    for (old, new) in &changes {
        revised = revised.replace(old, new);
    }
    for (old, new) in policy["mime_replacements"]
        .as_object()
        .context("mime replacements missing")?
    {
        revised = revised.replace(old, new.as_str().context("mime replacement required")?);
    }
    if !apply {
        return Ok(i32::from(!changes.is_empty() || original != revised));
    }
    for (old, new) in &changes {
        let source = apps.join(old);
        let replacement = apps.join(new);
        copy_preserve(&source, &archive.join(source.strip_prefix(&home)?))?;
        if policy["preserve_launcher_contents"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == old))
        {
            copy_preserve(
                &replacement,
                &archive.join(replacement.strip_prefix(&home)?),
            )?;
            atomic(
                &replacement,
                &fs::read(&source)?,
                fs::metadata(&replacement)?.mode() & 0o777,
            )?;
        }
        fs::remove_file(source)?;
    }
    if original != revised {
        copy_preserve(&mime, &archive.join(mime.strip_prefix(&home)?))?;
        atomic(
            &mime,
            revised.as_bytes(),
            fs::metadata(&mime)?.mode() & 0o777,
        )?;
    }
    if apps.is_dir() {
        cache(Command::new("update-desktop-database").arg(apps))?;
    }
    println!(
        "{}",
        json!({"changes":changes,"mime_changed":original!=revised,"archive":if changes.is_empty()&&original==revised{None}else{Some(archive)}})
    );
    Ok(0)
}
fn cache(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        bail!("desktop cache refresh failed");
    }
    Ok(())
}
fn walk(path: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        output.push(path.clone());
        if kind.is_dir() {
            walk(&path, output)?;
        }
    }
    Ok(())
}
fn icons(repo: &Path) -> Result<i32> {
    let home = crate::commands::home()?;
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".local/share"));
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home.join(".local/state"))
        .join("rose-punk-icons");
    fs::create_dir_all(&state)?;
    fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
    let mut records = json_file(&state.join("desktop-overrides.json"))?;
    let mut backups = json_file(&state.join("theme-switch-backups.json"))?;
    let source = repo.join("native/home/files/.local/share/applications");
    let mut managed = BTreeMap::new();
    for item in fs::read_dir(source)? {
        let path = item?.path();
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        if let Some(icon) = ini_value(&text, "Desktop Entry", "Icon")
            .filter(|s| !s.starts_with("rose-punk-") && !s.contains("{{"))
        {
            managed.insert(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .trim_end_matches(".j2")
                    .to_owned(),
                icon.to_owned(),
            );
        }
    }
    let vendors = [
        home.join(".local/share/flatpak/exports/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    let mut changed = 0;
    let applications = data.join("applications");
    if applications.is_dir() {
        for item in fs::read_dir(&applications)? {
            let path = item?.path();
            if path.extension().is_none_or(|e| e != "desktop") || !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&path)?;
            let icon = ini_value(&text, "Desktop Entry", "Icon").unwrap_or("");
            let record = records[&name].clone();
            let mut replacement: Option<Option<String>> = None;
            if icon.starts_with("rose-punk-") && managed.contains_key(&name) {
                replacement = Some(Some(ini_setting(
                    &text,
                    "Desktop Entry",
                    "Icon",
                    &managed[&name],
                )));
            } else if record["installed"].as_str() == Some(&text) {
                replacement = Some(record["original"].as_str().map(str::to_owned));
            } else if icon.starts_with("rose-punk-") {
                let mut candidates = Vec::new();
                if let Some(s) = record["original"].as_str() {
                    candidates.push(s.to_owned());
                }
                for root in &vendors {
                    if let Ok(s) = fs::read_to_string(root.join(&name)) {
                        candidates.push(s);
                    }
                }
                if let Some(icon) = candidates
                    .iter()
                    .filter_map(|s| ini_value(s, "Desktop Entry", "Icon"))
                    .find(|s| !s.is_empty() && !s.starts_with("rose-punk-"))
                {
                    replacement = Some(Some(ini_setting(&text, "Desktop Entry", "Icon", icon)));
                } else {
                    println!("WARN    ! Icon override needs review: {name}");
                }
            } else {
                records
                    .as_object_mut()
                    .context("invalid override records")?
                    .remove(&name);
            }
            if let Some(replacement) = replacement {
                if backups.get(&name).is_none() {
                    backups[&name] = json!(text);
                    save_json(&state.join("theme-switch-backups.json"), &backups)?;
                }
                if let Some(text) = replacement {
                    atomic(&path, text.as_bytes(), fs::metadata(&path)?.mode() & 0o777)?;
                } else {
                    fs::remove_file(&path)?;
                }
                records
                    .as_object_mut()
                    .context("invalid override records")?
                    .remove(&name);
                changed += 1;
            }
        }
    }
    save_json(&state.join("desktop-overrides.json"), &records)?;
    let themes = ["rose-punk", "rose-punk-illustrated", "rose-punk-framed"];
    let roots: Vec<_> = themes
        .iter()
        .map(|s| data.join("icons").join(s))
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    let mut paths = Vec::new();
    walk(&data.join("icons/hicolor"), &mut paths)?;
    let mut removed = 0;
    for path in paths {
        if path.is_symlink()
            && path
                .canonicalize()
                .is_ok_and(|p| roots.iter().any(|root| p.starts_with(root)))
        {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    let primary = fs::read_to_string(data.join("icons/rose-punk/index.theme")).unwrap_or_default();
    let mut migrated = false;
    if ini_value(&primary, "Icon Theme", "Name") == Some("Rose Punk")
        && !ini_value(&primary, "Icon Theme", "Hidden")
            .is_some_and(|s| ["true", "yes", "1", "on"].contains(&s.to_ascii_lowercase().as_str()))
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".config"));
        for (relative, section, key) in [
            ("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name"),
            ("gtk-4.0/settings.ini", "Settings", "gtk-icon-theme-name"),
            ("qt6ct/qt6ct.conf", "Appearance", "icon_theme"),
        ] {
            let path = config.join(relative);
            if let Ok(text) = fs::read_to_string(&path) {
                if ini_value(&text, section, key) == Some("rose-punk-illustrated") {
                    atomic(
                        &path,
                        ini_setting(&text, section, key, "rose-punk").as_bytes(),
                        fs::metadata(&path)?.mode() & 0o777,
                    )?;
                    migrated = true;
                }
            }
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
            let out = Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "icon-theme"])
                .output()?;
            if !out.status.success() {
                bail!("cannot read selected icon theme");
            }
            if String::from_utf8(out.stdout)?.trim().trim_matches('\'') == "rose-punk-illustrated" {
                cache(Command::new("gsettings").args([
                    "set",
                    "org.gnome.desktop.interface",
                    "icon-theme",
                    "rose-punk",
                ]))?;
                migrated = true;
            }
        }
    }
    for theme in themes {
        let dir = data.join("icons").join(theme);
        if dir.join("index.theme").is_file() {
            cache(
                Command::new("gtk-update-icon-cache")
                    .arg("--force")
                    .arg(dir),
            )?;
        }
    }
    if data.join("icons/hicolor").is_dir() {
        cache(
            Command::new("gtk-update-icon-cache")
                .args(["--force", "--ignore-theme-index"])
                .arg(data.join("icons/hicolor")),
        )?;
    }
    if applications.is_dir() {
        cache(Command::new("update-desktop-database").arg(applications))?;
    }
    println!(
        "__stage_note__:{}",
        if changed == 0 && removed == 0 && !migrated {
            "no changes".into()
        } else {
            format!(
                "{changed} launcher overrides retired; {removed} fallback links removed{}",
                if migrated { "; theme ID migrated" } else { "" }
            )
        }
    );
    Ok(0)
}
pub fn cli(repo: &Path, args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str){
    Some("zed")=>zed(repo,&args[1..]),
    Some("icons") if args.len()==1=>icons(repo),
    Some("cleanup") if args.len()==2&&["--check","--apply"].contains(&args[1].as_str())=>cleanup(repo,args[1]=="--apply"),
    Some("ini-has") if args.len()==4=>Ok(i32::from(ini_value(&fs::read_to_string(&args[1])?,&args[2],&args[3]).is_none())),
    _=>bail!("desktop icons | cleanup --check|--apply | zed --channel CHANNEL -- COMMAND | ini-has FILE SECTION KEY"),
}
}
