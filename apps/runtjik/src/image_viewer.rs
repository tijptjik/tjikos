use crate::util::*;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{env, fs, path::PathBuf, process::Command, time::Duration};
fn dimensions(m: &Value) -> (f64, f64) {
    let (mut w, mut h) = (
        m["width"].as_f64().unwrap_or(1.),
        m["height"].as_f64().unwrap_or(1.),
    );
    if m["transform"].as_i64().unwrap_or(0) % 2 != 0 {
        std::mem::swap(&mut w, &mut h);
    }
    let scale = m["scale"].as_f64().unwrap_or(1.).max(0.01);
    (w / scale, h / scale)
}
fn size(iw: f64, ih: f64, w: f64, h: f64) -> (i64, i64) {
    let height = (h * 0.9).round_ties_even().max(1.);
    (
        ((height * iw / ih)
            .round_ties_even()
            .min((w - h + height).round_ties_even())
            .max(1.)) as i64,
        height as i64,
    )
}
fn monitors() -> Result<Vec<Value>> {
    Ok(serde_json::from_str(&command(&[
        "hyprctl", "-j", "monitors",
    ])?)?)
}
pub fn open(spec: &Value) -> Result<()> {
    let viewer = strings(&spec["viewer"]);
    let paths = strings(&spec["paths"]);
    if paths.is_empty() {
        anyhow::ensure!(
            Command::new(viewer.first().context("Viewer required")?)
                .args(&viewer[1..])
                .status()?
                .success(),
            "Image viewer failed"
        );
        return Ok(());
    }
    let first = paths.first().context("Image path required")?;
    let mut args = viewer.clone();
    args.extend(["-n".into(), first.clone()]);
    if paths.len() > 1 {
        args.extend(paths.clone());
    } else {
        args.push(
            PathBuf::from(first)
                .parent()
                .context("Image directory missing")?
                .to_string_lossy()
                .into_owned(),
        );
    }
    let setup = || -> Result<(f64, f64, i64, i64)> {
        let ms = monitors()?;
        let m = ms
            .iter()
            .find(|m| m["focused"] == true)
            .or(ms.first())
            .context("No monitor")?;
        let output = command(&[
            "magick",
            "identify",
            "-ping",
            "-format",
            "%w %h",
            &format!("{first}[0]"),
        ])?;
        let dims = output
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        anyhow::ensure!(dims.len() == 2 && dims[1] > 0., "Invalid image dimensions");
        let (w, h) = dimensions(m);
        let (w, h) = size(dims[0], dims[1], w, h);
        Ok((dims[0], dims[1], w, h))
    };
    let Ok((iw, ih, w, h)) = setup() else {
        anyhow::ensure!(
            Command::new(&args[0]).args(&args[1..]).status()?.success(),
            "Image viewer failed"
        );
        return Ok(());
    };
    let sources = [
        env::var_os("imv_config").map(PathBuf::from),
        Some(xdg("config").parent().unwrap().join("imv/config")),
        Some(home().join(".imv_config")),
        Some(home().join(".imv/config")),
        Some("/usr/local/etc/imv_config".into()),
        Some("/etc/imv_config".into()),
    ];
    let content = sources
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .map(fs::read_to_string)
        .transpose()?
        .unwrap_or_default();
    // Preserve bindings and all unrelated options; replace only our three keys.
    let mut lines = Vec::new();
    let mut options = false;
    let mut found = false;
    for line in content.lines() {
        let trim = line.trim();
        if trim.starts_with('[') {
            options = trim == "[options]";
            if options {
                found = true;
                lines.push(line.to_owned());
                lines.push(format!("width={w}\nheight={h}\nscaling_mode=full"));
                continue;
            }
        }
        if options
            && line
                .split_once('=')
                .is_some_and(|(k, _)| ["width", "height", "scaling_mode"].contains(&k.trim()))
        {
            continue;
        }
        lines.push(line.to_owned());
    }
    if !found {
        lines.push(format!(
            "[options]\nwidth={w}\nheight={h}\nscaling_mode=full"
        ));
    }
    let temp = tempfile::Builder::new().prefix("imv-lightbox-").tempdir()?;
    let config = temp.path().join("config");
    atomic(&config, lines.join("\n").as_bytes())?;
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .env("imv_config", config)
        .spawn()?;
    let place = || -> Result<()> {
        for _ in 0..40 {
            if child.try_wait()?.is_some() {
                break;
            }
            if let Some(c) = crate::windows::clients()?
                .iter()
                .find(|c| c["pid"].as_u64() == Some(child.id() as u64) && c["mapped"] == true)
            {
                let ms = monitors()?;
                let m = ms
                    .iter()
                    .find(|m| m["id"] == c["monitor"])
                    .context("Monitor disconnected")?;
                let (sw, sh) = dimensions(m);
                let (w, h) = size(iw, ih, sw, sh);
                let x = (m["x"].as_f64().unwrap_or(0.) + (sw - w as f64) / 2.).round_ties_even();
                let y = (m["y"].as_f64().unwrap_or(0.) + (sh - h as f64) / 2.).round_ties_even();
                std::thread::sleep(Duration::from_millis(150));
                let target = serde_json::to_string(&format!("address:{}", s(&c["address"])))?;
                for cmd in [
                    format!("hl.dsp.window.resize({{x={w},y={h},relative=false,window={target}}})"),
                    format!("hl.dsp.window.move({{x={x},y={y},relative=false,window={target}}})"),
                ] {
                    command(&["hyprctl", "dispatch", &cmd])?;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    };
    let mut place = place;
    let _ = place();
    anyhow::ensure!(child.wait()?.success(), "Image viewer failed");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proportional_lightbox() {
        assert_eq!(size(1920., 1080., 1920., 1080.), (1728, 972));
        assert_eq!(size(4000., 1000., 1920., 1080.), (1812, 972));
    }
}

pub fn cli(args: &[String]) -> Result<()> {
    let mut viewer = vec!["imv".to_owned()];
    let mut paths = Vec::new();
    let mut i = 0;
    let mut positional = false;
    while i < args.len() {
        if !positional && args[i] == "--viewer" {
            i += 1;
            viewer = serde_json::from_str(args.get(i).context("Missing viewer argv")?)?;
        } else if !positional && args[i] == "--" {
            positional = true;
        } else {
            let raw = &args[i];
            let path = if let Some(p) = raw.strip_prefix("~/") {
                home().join(p)
            } else {
                PathBuf::from(raw)
            };
            paths.push(fs::canonicalize(path)?.to_string_lossy().into_owned());
        }
        i += 1;
    }
    anyhow::ensure!(
        !viewer.is_empty() && viewer.iter().all(|v| !v.is_empty()),
        "Viewer must be a nonempty argv array"
    );
    open(&serde_json::json!({"viewer":viewer,"paths":paths}))
}
