use super::*;
use std::collections::BTreeSet;
pub fn nss_render(original: &str) -> String {
    let mut seen = BTreeSet::new();
    let mut output = String::new();
    for line in original.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        if let Some((left, right)) = trimmed.split_once(':') {
            let name = left.trim();
            if ["shadow", "gshadow"].contains(&name) {
                seen.insert(name);
                let (providers, comment) = right
                    .split_once('#')
                    .map(|(p, c)| (p, format!("#{c}")))
                    .unwrap_or((right, String::new()));
                if !providers.split_whitespace().any(|p| p == "systemd") {
                    let leading = &providers[..providers.len() - providers.trim_start().len()];
                    let core = providers.trim();
                    let gap = &providers[providers.trim_end().len()..];
                    output.push_str(&format!(
                        "{left}:{leading}{core} systemd{}{}{newline}",
                        if comment.is_empty() {
                            ""
                        } else if gap.is_empty() {
                            " "
                        } else {
                            gap
                        },
                        comment
                    ));
                    continue;
                }
            }
        }
        output.push_str(line);
    }
    for db in ["shadow", "gshadow"] {
        if !seen.contains(db) {
            output = format!("{}\n{db}: files systemd\n", output.trim_end_matches('\n'));
        }
    }
    output
}
pub fn config(policy: &Value) -> Result<Value> {
    read(Path::new(text(&policy["storage"], "config")?))
}
pub fn check_mounts(policy: &Value, config: &Value, docker_only: bool) -> Result<()> {
    let mounts = config["mounts"].as_object().context("mount map missing")?;
    let docker = text(&policy["storage"], "docker_mount")?;
    for (path, uuid) in mounts {
        if docker_only && path != docker {
            continue;
        }
        let actual = command(&["findmnt", "-rn", "-o", "UUID,FSTYPE", "--mountpoint", path])?;
        if actual.split_whitespace().collect::<Vec<_>>()
            != [uuid.as_str().context("mount UUID missing")?, "ext4"]
        {
            bail!("{path}: expected ext4 identity is not mounted");
        }
    }
    if docker_only && !mounts.contains_key(docker) {
        bail!("Docker mount is not declared");
    }
    if !docker_only
        && command(&[
            "findmnt",
            "-rn",
            "-o",
            "FSTYPE",
            "--mountpoint",
            text(&policy["storage"], "pool")?,
        ])?
        .trim()
            != "fuse.mergerfs"
    {
        bail!("storage pool is not mounted as mergerfs");
    }
    Ok(())
}
fn render_fstab(original: &str, policy: &Value, config: &Value) -> Result<String> {
    let policy = &policy["storage"];
    let begin = text(policy, "begin")?;
    let end = text(policy, "end")?;
    let pool = text(policy, "pool")?;
    let swap = text(policy, "swap")?;
    let mounts = config["mounts"].as_object().context("mount map missing")?;
    let mut kept = Vec::new();
    let (mut inside, mut seen) = (false, false);
    for line in original.lines() {
        if line == begin || policy["previous_begin"].as_str() == Some(line) {
            if inside || seen {
                bail!("duplicate managed fstab block");
            }
            inside = true;
            seen = true;
            continue;
        }
        if line == end || policy["previous_end"].as_str() == Some(line) {
            if !inside {
                bail!("unmatched fstab block end");
            }
            inside = false;
            continue;
        }
        if inside {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if !line.trim_start().starts_with('#') && fields.len() >= 2 {
            if mounts.contains_key(fields[1])
                || policy["retired_mounts"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == fields[1]))
                || fields[1] == pool
                || fields[0] == swap
            {
                continue;
            }
            if policy["array_prefixes"]
                .as_array()
                .context("array prefixes missing")?
                .iter()
                .any(|v| v.as_str().is_some_and(|p| fields[1].starts_with(p)))
            {
                bail!("unrecognized array mount; review manually");
            }
        }
        kept.push(line);
    }
    if inside {
        bail!("unterminated fstab block");
    }
    let data: Vec<_> = config["layout"]
        .as_array()
        .context("storage layout missing")?
        .iter()
        .filter(|r| r[0] == "data")
        .map(|r| r[2].as_str().context("data mount missing"))
        .collect::<Result<_>>()?;
    let mut block = vec![begin.to_owned()];
    for (path, uuid) in mounts {
        block.push(format!(
            "UUID={} {path} ext4 defaults,nofail 0 2",
            uuid.as_str().context("mount UUID missing")?
        ));
    }
    let mut options = text(policy, "pool_options")?.to_owned();
    for path in &data {
        options.push_str(&format!(",x-systemd.requires-mounts-for={path}"));
    }
    block.push(format!(
        "{} {pool} fuse.mergerfs {options} 0 0",
        data.join(":")
    ));
    block.push(format!("{swap} none swap defaults,nofail 0 0"));
    block.push(end.into());
    Ok(format!(
        "{}\n\n{}\n",
        kept.join("\n").trim_end(),
        block.join("\n")
    ))
}
pub fn cli(policy: &Value, args: &[String]) -> Result<i32> {
    if args.len() != 1 {
        bail!("storage requires one operation");
    }
    let config = config(policy)?;
    if ["--check", "--check-docker"].contains(&args[0].as_str()) {
        check_mounts(policy, &config, args[0] == "--check-docker")?;
        println!("Expected storage is mounted.");
        return Ok(0);
    }
    if !["--print-fstab", "--write-fstab"].contains(&args[0].as_str()) {
        bail!("unknown storage operation");
    }
    let path = Path::new(text(&policy["storage"], "fstab")?);
    let original = fs::read_to_string(path)?;
    let rendered = render_fstab(&original, policy, &config)?;
    if args[0] == "--print-fstab" {
        print!("{rendered}");
        return Ok(0);
    }
    for (mount, uuid) in config["mounts"].as_object().context("mount map missing")? {
        let device = command(&["blkid", "-U", uuid.as_str().context("UUID missing")?])?;
        if device.trim().is_empty()
            || command(&["blkid", "-s", "TYPE", "-o", "value", device.trim()])?.trim() != "ext4"
        {
            bail!("{mount}: expected ext4 device is missing");
        }
    }
    if rendered == original {
        println!("fstab already matches; no changes.");
        return Ok(0);
    }
    if path.is_symlink() {
        bail!("refusing symlinked fstab");
    }
    let backup = path.with_file_name(format!("fstab.before-tjikup-{}", stamp()));
    command(&[
        "cp",
        "--preserve=all",
        "--",
        path.to_str().unwrap(),
        backup.to_str().unwrap(),
    ])?;
    let temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    command(&[
        "cp",
        "--preserve=all",
        "--",
        path.to_str().unwrap(),
        temp.path().to_str().unwrap(),
    ])?;
    fs::write(temp.path(), rendered)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    println!("fstab updated with backup; no disks mounted and no services restarted.");
    Ok(0)
}
fn getent(database: &str, key: &str) -> Result<Option<String>> {
    let output = crate::command_io::capture(
        Command::new("getent").args([database, key]),
        std::time::Duration::from_secs(10),
    )?;
    match output.code {
        0 => Ok(Some(output.stdout.trim().into())),
        2 => Ok(None),
        _ => bail!("identity lookup failed"),
    }
}
pub fn provision(policy: &Value, args: &[String]) -> Result<i32> {
    if args.is_empty() || args.len() > 2 || args.get(1).is_some_and(|s| s != "--apply") {
        bail!("provision requires action [--apply]");
    }
    let apply = args.len() == 2;
    let p = &policy["provision"];
    match args[0].as_str() {
        "accounts" => {
            let mut plan = Vec::new();
            for entry in p["accounts"].as_array().context("accounts missing")? {
                let name = text(entry, "name")?;
                let uid = entry["uid"].as_u64().context("UID missing")?.to_string();
                let gid = entry["gid"].as_u64().context("GID missing")?.to_string();
                let u = getent("passwd", name)?;
                let byid = getent("passwd", &uid)?;
                let g = getent("group", name)?;
                let bygid = getent("group", &gid)?;
                if u.as_ref().is_some_and(|s| {
                    let f: Vec<_> = s.split(':').collect();
                    f.len() < 7
                        || f[2] != uid
                        || f[3] != gid
                        || f[5] != entry["home"].as_str().unwrap_or("")
                }) || byid
                    .as_ref()
                    .is_some_and(|s| s.split(':').next() != Some(name))
                {
                    bail!("user identity collision: {name}");
                }
                if g.as_ref()
                    .is_some_and(|s| s.split(':').nth(2) != Some(gid.as_str()))
                    || bygid
                        .as_ref()
                        .is_some_and(|s| s.split(':').next() != Some(name))
                {
                    bail!("group identity collision: {name}");
                }
                plan.push((entry, u.is_some(), g.is_some(), uid, gid));
            }
            for (e, exists, gexists, uid, gid) in plan {
                let name = text(e, "name")?;
                println!(
                    "{name}: {}",
                    if exists {
                        "preserve existing account"
                    } else {
                        "create account"
                    }
                );
                if apply {
                    if !gexists {
                        command(&["groupadd", "--gid", &gid, name])?;
                    }
                    if !exists {
                        command(&[
                            "useradd",
                            "--uid",
                            &uid,
                            "--gid",
                            &gid,
                            "--home-dir",
                            text(e, "home")?,
                            "--shell",
                            text(e, "shell")?,
                            "--create-home",
                            name,
                        ])?;
                    }
                }
            }
            if apply {
                for e in p["memberships"].as_array().context("memberships missing")? {
                    let user = text(e, "user")?;
                    let group = text(e, "group")?;
                    if getent("passwd", user)?.is_some() && getent("group", group)?.is_some() {
                        command(&["usermod", "-aG", group, user])?;
                    }
                }
            }
        }
        "directories" => {
            check_mounts(policy, &config(policy)?, false)?;
            let entries = read(Path::new(text(p, "directories")?))?;
            let mut missing = Vec::new();
            for (target, attrs) in entries
                .as_object()
                .context("directories must be an object")?
            {
                if !p["directory_prefixes"]
                    .as_array()
                    .context("directory prefixes missing")?
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| target.starts_with(s)))
                {
                    bail!("unexpected directory");
                }
                let path = Path::new(target);
                if fs::symlink_metadata(path).is_ok() {
                    if !path.is_dir() {
                        bail!("not a directory: {target}");
                    }
                    println!("Preserve {target}");
                } else {
                    missing.push((target, attrs));
                }
            }
            let links = p["links"].as_object().context("links missing")?;
            for (target, source) in links {
                let path = Path::new(target);
                if path.is_symlink()
                    && path.canonicalize().ok().as_deref() == source.as_str().map(Path::new)
                {
                    continue;
                }
                if fs::symlink_metadata(path).is_ok() {
                    bail!("refusing existing link destination: {target}");
                }
            }
            for (target, attrs) in missing {
                println!("Create {target}");
                if apply {
                    command(&[
                        "install",
                        "-d",
                        "-m",
                        text(attrs, "mode")?,
                        "-o",
                        &attrs["uid"].to_string(),
                        "-g",
                        &attrs["gid"].to_string(),
                        target,
                    ])?;
                }
            }
            for (target, source) in links {
                let path = Path::new(target);
                if path.is_symlink() {
                    continue;
                }
                println!("Link {target}");
                if apply {
                    std::os::unix::fs::symlink(
                        source.as_str().context("link source missing")?,
                        path,
                    )?;
                }
            }
        }
        "mergerfs" => {
            let pin = read(Path::new(text(p, "mergerfs")?))?;
            let present = crate::command_io::capture(
                Command::new("rpm").args(["-q", "mergerfs"]),
                std::time::Duration::from_secs(30),
            )?;
            if present.code == 0 {
                println!("Preserving installed mergerfs (no implicit upgrade)");
                return Ok(0);
            }
            if present.code != 1 {
                bail!("RPM query failed");
            }
            if command(&["rpm", "-E", "%fedora"])?.trim() != text(&pin, "fedora")?
                || command(&["uname", "-m"])?.trim() != text(&pin, "arch")?
            {
                bail!("pinned mergerfs does not match this OS/architecture");
            }
            println!("Install pinned mergerfs");
            if apply {
                let temp = tempfile::tempdir()?;
                let rpm = temp.path().join("mergerfs.rpm");
                command(&[
                    "curl",
                    "--proto",
                    "=https",
                    "--proto-redir",
                    "=https",
                    "-fsSL",
                    "--max-time",
                    "120",
                    "-o",
                    rpm.to_str().unwrap(),
                    text(&pin, "url")?,
                ])?;
                use sha2::Digest;
                if format!("{:x}", sha2::Sha256::digest(fs::read(&rpm)?)) != text(&pin, "sha256")? {
                    bail!("mergerfs checksum mismatch");
                }
                command(&[
                    "dnf",
                    "install",
                    "-y",
                    "--nogpgcheck",
                    rpm.to_str().unwrap(),
                ])?;
            }
        }
        "beets" => {
            check_mounts(policy, &config(policy)?, false)?;
            let source = Path::new(text(&p["beets"], "source")?);
            let target = Path::new(text(&p["beets"], "target")?);
            if !source.is_dir() || !target.is_dir() {
                bail!("provision or restore Beets directories first");
            }
            fn seed(source: &Path, target: &Path, apply: bool, uid: u32, gid: u32) -> Result<()> {
                for entry in fs::read_dir(source)? {
                    let entry = entry?;
                    let kind = entry.file_type()?;
                    let dst = target.join(entry.file_name());
                    if kind.is_symlink() {
                        bail!("unexpected source symlink");
                    }
                    if kind.is_dir() {
                        if dst.is_symlink() {
                            bail!("unexpected destination symlink");
                        }
                        if apply {
                            fs::create_dir_all(&dst)?;
                        }
                        seed(&entry.path(), &dst, apply, uid, gid)?;
                    } else if kind.is_file() {
                        if fs::symlink_metadata(&dst).is_ok() {
                            println!("Preserve {}", dst.display());
                            continue;
                        }
                        println!("Seed {}", dst.display());
                        if apply {
                            use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
                            let mut out = fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .mode(0o644)
                                .open(&dst)?;
                            std::io::copy(&mut fs::File::open(entry.path())?, &mut out)?;
                            if unsafe { libc::fchown(out.as_raw_fd(), uid, gid) } != 0 {
                                return Err(std::io::Error::last_os_error().into());
                            }
                        }
                    }
                }
                Ok(())
            }
            for tree in ["plugins", "genres"] {
                if apply {
                    fs::create_dir_all(target.join(tree))?;
                }
                seed(
                    &source.join(tree),
                    &target.join(tree),
                    apply,
                    p["beets"]["uid"].as_u64().context("Beets UID missing")? as u32,
                    p["beets"]["gid"].as_u64().context("Beets GID missing")? as u32,
                )?;
            }
        }
        _ => bail!("unknown provisioning action"),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nss_preserves_comments_and_other_databases() {
        let original = "passwd: files\nshadow: files # policy\ngshadow: files systemd\n";
        let result = nss_render(original);
        assert_eq!(
            result,
            "passwd: files\nshadow: files systemd # policy\ngshadow: files systemd\n"
        );
        assert_eq!(nss_render(&result), result);
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fstab_preserves_root_retires_known_mounts_and_rejects_unknown_or_broken_blocks() {
        let p = json!({"storage":{"begin":"# BEGIN native array","end":"# END native array","previous_begin":"# BEGIN previous array","previous_end":"# END previous array","pool":"/mnt/storage","swap":"/mnt/data/disk22/swap","retired_mounts":["/mnt/data/disk4"],"array_prefixes":["/mnt/data/","/mnt/parity/"],"pool_options":"defaults,allow_other"}});
        let c = json!({"mounts":{"/mnt/data/disk22":"disk-uuid"},"layout":[["data","d1","/mnt/data/disk22"]]});
        let original="UUID=root / btrfs defaults 0 0\nUUID=efi /boot/efi vfat defaults 0 2\nserver:/share /srv nfs defaults 0 0\nUUID=retired /mnt/data/disk4 ext4 defaults 0 0\n";
        let result = render_fstab(original, &p, &c).unwrap();
        assert!(result.contains("UUID=root / btrfs"));
        assert!(result.contains("UUID=efi /boot/efi"));
        assert!(result.contains("server:/share /srv"));
        assert!(!result.contains("retired"));
        assert_eq!(render_fstab(&result, &p, &c).unwrap(), result);
        for invalid in [
            "UUID=x /mnt/data/disk99 ext4 defaults 0 0\n",
            "# BEGIN native array\n",
            "# END native array\n",
            "# BEGIN native array\n# BEGIN native array\n",
        ] {
            assert!(render_fstab(invalid, &p, &c).is_err());
        }
        let previous = "# BEGIN previous array\nold\n# END previous array\n";
        assert!(render_fstab(previous, &p, &c)
            .unwrap()
            .contains("# BEGIN native array"));
    }
}
