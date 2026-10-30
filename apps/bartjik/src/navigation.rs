use crate::{tray, util::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};
pub fn uri(value: &str, kind: &str) -> String {
    if !["artist", "playlist", "track"].contains(&kind) {
        return String::new();
    }
    for pattern in [
        format!(r"^spotify:{kind}:([A-Za-z0-9]{{22}})$"),
        format!(
            r"^https://open\.spotify\.com/(?:intl-[a-zA-Z-]+/)?{kind}/([A-Za-z0-9]{{22}})(?:\?[^\s]*)?$"
        ),
    ] {
        if let Some(c) = regex::Regex::new(&pattern).unwrap().captures(value) {
            return format!("spotify:{kind}:{}", &c[1]);
        }
    }
    String::new()
}
#[derive(Clone)]
enum Wire<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Other,
}
fn fields(data: &[u8]) -> Result<HashMap<u64, Vec<Wire<'_>>>> {
    fn var(data: &[u8], off: &mut usize) -> Result<u64> {
        let mut v = 0;
        for shift in (0..70).step_by(7) {
            let b = *data.get(*off).context("Incomplete Spotify state")?;
            *off += 1;
            ensure!(shift != 63 || b <= 1, "Invalid Spotify state integer");
            v |= ((b & 127) as u64) << shift;
            if b < 128 {
                return Ok(v);
            }
        }
        anyhow::bail!("Invalid Spotify state integer")
    }
    let mut off = 0;
    let mut result: HashMap<u64, Vec<Wire>> = HashMap::new();
    while off < data.len() {
        let tag = var(data, &mut off)?;
        let (number, wire) = (tag >> 3, tag & 7);
        ensure!(
            number > 0 && number < (1 << 29),
            "Invalid Spotify state field"
        );
        let v = match wire {
            0 => Wire::Number(var(data, &mut off)?),
            1 | 2 | 5 => {
                let size = if wire == 2 {
                    var(data, &mut off)?
                } else if wire == 1 {
                    8
                } else {
                    4
                };
                ensure!(
                    size <= (data.len() - off) as u64,
                    "Incomplete Spotify state field"
                );
                let bytes = &data[off..off + size as usize];
                off += size as usize;
                if wire == 2 {
                    Wire::Bytes(bytes)
                } else {
                    Wire::Other
                }
            }
            _ => anyhow::bail!("Unsupported Spotify state encoding"),
        };
        result.entry(number).or_default().push(v);
    }
    Ok(result)
}
fn one<'a>(m: &HashMap<u64, Vec<Wire<'a>>>, n: u64) -> Result<&'a [u8]> {
    let v = m.get(&n).context("Missing Spotify state field")?;
    ensure!(v.len() == 1, "Ambiguous Spotify state field");
    match v[0] {
        Wire::Bytes(b) => Ok(b),
        _ => anyhow::bail!("Invalid Spotify state field"),
    }
}
pub fn parse_state(raw: &[u8], id: &str) -> Result<Value> {
    ensure!(
        regex::Regex::new(r"^[A-Za-z0-9]{22}$")?.is_match(id),
        "Spotify track unavailable"
    );
    ensure!(raw.len() <= 2097152, "Spotify state exceeds size limit");
    let split = raw
        .iter()
        .position(|b| *b == b'#')
        .context("Unsupported Spotify saved state")?;
    ensure!(
        (10..=16).contains(&split) && raw[..split].iter().all(u8::is_ascii_digit),
        "Unsupported Spotify saved state"
    );
    let mut m = fields(&raw[split + 1..])?;
    ensure!(
        matches!(m.get(&1).map(Vec::as_slice), Some([Wire::Number(15)])),
        "Unsupported Spotify saved-state version"
    );
    for n in [3, 3, 1, 3] {
        m = fields(one(&m, n)?)?;
    }
    ensure!(
        std::str::from_utf8(one(&m, 2)?)? == format!("spotify:track:{id}"),
        "Spotify has not saved this track's navigation context yet"
    );
    let mut meta = HashMap::new();
    for v in m.get(&3).into_iter().flatten() {
        let Wire::Bytes(bytes) = v else {
            anyhow::bail!("Invalid Spotify track metadata")
        };
        let pair = fields(bytes)?;
        let k = std::str::from_utf8(one(&pair, 1)?)?;
        let v = std::str::from_utf8(one(&pair, 2)?)?;
        if ["artist_uri", "context_uri"].contains(&k) {
            ensure!(
                meta.get(k).is_none_or(|old| *old == v),
                "Ambiguous Spotify navigation context"
            );
            meta.insert(k, v);
        }
    }
    Ok(
        json!({"artist":uri(meta.get("artist_uri").copied().unwrap_or(""),"artist"),"playlist":uri(meta.get("context_uri").copied().unwrap_or(""),"playlist")}),
    )
}
pub fn context(id: &str) -> Result<Value> {
    let roots = [
        xdg("XDG_CACHE_HOME", ".cache").join("spotify/Users"),
        home().join(".var/app/com.spotify.Client/cache/spotify/Users"),
        home().join("snap/spotify/common/.cache/spotify/Users"),
    ];
    let mut candidates = vec![];
    for root in roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let p = entry.path().join("context_player_state_restore");
            if let Ok(m) = p.symlink_metadata() {
                if m.is_file() && m.uid() == unsafe { libc::getuid() } {
                    candidates.push((m.mtime(), m.mtime_nsec(), p));
                }
            }
        }
    }
    candidates.sort();
    let p = candidates
        .last()
        .context("Spotify desktop navigation context is unavailable")?
        .2
        .clone();
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(p)?;
    let before = f.metadata()?;
    ensure!(
        before.is_file() && before.uid() == unsafe { libc::getuid() } && before.len() <= 2097152,
        "Invalid Spotify saved state"
    );
    let mut data = vec![];
    (&f).take(2097153).read_to_end(&mut data)?;
    let after = f.metadata()?;
    ensure!(
        (before.mtime(), before.mtime_nsec(), before.len())
            == (after.mtime(), after.mtime_nsec(), after.len()),
        "Spotify navigation context is updating; try again"
    );
    parse_state(&data, id)
}
pub fn navigation_command(uri_: &str, pid: u32) -> Result<Vec<String>> {
    ensure!(
        !uri(uri_, "artist").is_empty() || !uri(uri_, "playlist").is_empty(),
        "Invalid Spotify navigation destination"
    );
    ensure!(pid > 0, "Spotify process unavailable");
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    let info = process.join("root/.flatpak-info");
    match std::fs::read(&info) {
        Ok(raw) => {
            ensure!(raw.len() <= 65536, "Invalid Spotify instance metadata");
            let parsed = crate::details::ini(std::str::from_utf8(&raw)?);
            let id = s(&parsed["Instance"]["instance-id"]);
            ensure!(
                parsed["Application"]["name"] == "com.spotify.Client"
                    && regex::Regex::new(r"^[0-9]{1,20}$")?.is_match(id),
                "Spotify Flatpak instance unavailable"
            );
            Ok(vec![
                "flatpak".into(),
                "enter".into(),
                id.into(),
                "/app/extra/bin/spotify".into(),
                format!("--uri={uri_}"),
            ])
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let exe = std::fs::read_link(process.join("exe"))?;
            ensure!(
                exe.is_absolute() && exe.file_name().is_some_and(|n| n == "spotify"),
                "Spotify native executable unavailable"
            );
            Ok(vec![
                exe.to_string_lossy().into_owned(),
                format!("--uri={uri_}"),
            ])
        }
        Err(e) => Err(e.into()),
    }
}
async fn current(player: &str) -> Result<String> {
    let data = tray::get(
        player,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
        "Metadata",
    )
    .await?;
    let u = uri(s(&data["xesam:url"]["data"]), "track");
    if !u.is_empty() {
        return Ok(u.rsplit(':').next().unwrap().into());
    }
    let re = regex::Regex::new(r"^/com/spotify/track/([A-Za-z0-9]{22})$")?;
    Ok(re
        .captures(s(&data["mpris:trackid"]["data"]))
        .map(|c| c[1].to_owned())
        .unwrap_or_default())
}
pub async fn navigate(r: &Value) -> Result<()> {
    let player = s(&r["player"]);
    ensure!(
        regex::Regex::new(r"^org\.mpris\.MediaPlayer2\.spotify(?:[.][A-Za-z0-9_-]+)?$")?
            .is_match(player),
        "Exact media navigation is currently supported for Spotify"
    );
    let id = s(&r["trackId"]);
    ensure!(
        !id.is_empty() && current(player).await? == id,
        "The track changed; click its updated label"
    );
    let target = s(&r["target"]);
    ensure!(
        ["artist", "playlist"].contains(&target),
        "Unsupported Spotify navigation action"
    );
    let mut destination = uri(s(&r["uri"]), target);
    if destination.is_empty() {
        destination = st(&context(id)?[target]);
    }
    ensure!(
        !destination.is_empty(),
        "Spotify navigation destination unavailable"
    );
    ensure!(
        current(player).await? == id,
        "The track changed; click its updated label"
    );
    let result = jc(&[
        "busctl",
        "--user",
        "--json=short",
        "call",
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        "s",
        player,
    ])
    .await?;
    let pid = result["data"][0]
        .as_u64()
        .context("Spotify process unavailable")? as u32;
    command(
        &navigation_command(&destination, pid)?,
        6.,
        262144,
        &[],
        None,
    )
    .await?;
    let _ = cmd(&[
        "busctl",
        "--user",
        "call",
        player,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2",
        "Raise",
    ])
    .await;
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        tray::focus("spotify").await?;
    }
    Ok(())
}
pub async fn play() -> Result<()> {
    async fn running() -> bool {
        cmd(&["playerctl", "-l"])
            .await
            .map(|s| {
                s.lines()
                    .any(|n| n == "spotify" || n.starts_with("spotify."))
            })
            .unwrap_or(false)
    }
    if !running().await {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|p| p.join("spotify").is_file());
        crate::actions::detach(
            &if executable {
                vec!["spotify".into()]
            } else {
                vec!["flatpak".into(), "run".into(), "com.spotify.Client".into()]
            },
            &json!({}),
        )?;
    }
    for _ in 0..60 {
        if running().await {
            cmd(&["playerctl", "--player=spotify", "play"]).await?;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("Spotify did not become available for playback within 30 seconds")
}
