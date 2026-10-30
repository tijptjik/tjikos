use crate::{actions, providers, util::*};
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
pub async fn controls(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("") {
        "master" => actions::master(args.get(1).context("missing volume action")?).await?,
        "route" => actions::route(args.get(1).context("missing route")?).await?,
        "toggle-output" => {
            let _lock = actions::FileLock::acquire("audio-route.lock").await?;
            let (sinks, active) = providers::audio_state().await?;
            let target = arr(&sinks)
                .iter()
                .find(|s| {
                    if providers::headphones(&active) {
                        s["description"]
                            .as_str()
                            .unwrap_or("")
                            .starts_with("Navi 21/23 HDMI/DP")
                    } else {
                        providers::headphones(s)
                    }
                })
                .context("The selected output is unavailable")?;
            actions::route(s(&target["name"])).await?;
        }
        "output-watch" => {
            let mut p = tokio::process::Command::new("pactl")
                .arg("subscribe")
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let mut lines = BufReader::new(p.stdout.take().unwrap()).lines();
            let mut previous = String::new();
            loop {
                let (_, active) = providers::audio_state().await?;
                let mode = if providers::headphones(&active) {
                    "headphones"
                } else {
                    "speakers"
                };
                if mode != previous {
                    crate::emit(
                        &json!({"text":if mode=="headphones"{"headphones"}else{"volume_up"},"class":mode}),
                    );
                    previous = mode.into();
                }
                loop {
                    let line = lines
                        .next_line()
                        .await?
                        .context("audio subscription ended")?;
                    if line.contains(" on server ") || line.contains(" on sink ") {
                        break;
                    }
                }
            }
        }
        _ => anyhow::bail!("unknown audio command"),
    };
    Ok(())
}
struct Capture {
    task: tokio::task::JoinHandle<()>,
    peak: Arc<Mutex<f64>>,
    title: String,
    sink: String,
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub async fn levels(microphone: bool) -> Result<()> {
    let mut captures: HashMap<String, Capture> = HashMap::new();
    let mut refresh = 0.;
    let mut default = String::new();
    loop {
        if now() >= refresh {
            refresh = now() + 1.;
            let result = async {
                let sinks = jc(&["pactl", "-f", "json", "list", "sinks"]).await?;
                default = cmd(&["pactl", "get-default-sink"]).await?;
                let streams = jc(&["pactl", "-f", "json", "list", "sink-inputs"]).await?;
                let mut wanted: HashMap<String, (String, String, String, Option<String>)> =
                    HashMap::new();
                for stream in arr(&streams) {
                    let p = &stream["properties"];
                    let title = p["application.name"]
                        .as_str()
                        .or(p["node.description"].as_str())
                        .or(p["node.name"].as_str())
                        .unwrap_or("");
                    if truth(&stream["corked"])
                        || truth(&stream["mute"])
                        || format!("{title}{}", s(&p["node.name"]))
                            .to_lowercase()
                            .contains("speech-dispatcher-dummy")
                    {
                        continue;
                    }
                    if let Some(sink) = arr(&sinks).iter().find(|s| s["index"] == stream["sink"]) {
                        wanted.insert(
                            format!("{}:{}", stream["index"], s(&sink["name"])),
                            (
                                title.into(),
                                st(&sink["name"]),
                                st(&sink["monitor_source"]),
                                Some(format!("--monitor-stream={}", stream["index"])),
                            ),
                        );
                    }
                }
                if microphone {
                    for source in arr(&jc(&["pactl", "-f", "json", "list", "sources"]).await?) {
                        let name = s(&source["name"]);
                        if name.starts_with("alsa_input.usb-046d_Logitech_BRIO_")
                            && !truth(&source["mute"])
                        {
                            wanted.insert(
                                format!("microphone:{name}"),
                                ("__input".into(), name.into(), name.into(), None),
                            );
                        }
                    }
                }
                Ok::<_, anyhow::Error>(wanted)
            }
            .await;
            match result {
                Ok(wanted) => {
                    captures.retain(|k, c| wanted.contains_key(k) && !c.task.is_finished());
                    for (k, (title, sink, monitor, option)) in wanted {
                        if captures.contains_key(&k) {
                            continue;
                        }
                        let mut args = vec![
                            "--raw".into(),
                            "--format=float32ne".into(),
                            "--rate=48000".into(),
                            "--channels=2".into(),
                            "--latency-msec=100".into(),
                            "--process-time-msec=50".into(),
                            "--client-name=Audio panel levels".into(),
                            format!(
                                "--stream-name={}",
                                if title == "__input" {
                                    "Microphone level"
                                } else {
                                    "Playback level"
                                }
                            ),
                            format!("--device={monitor}"),
                        ];
                        if let Some(o) = option {
                            args.push(o);
                        }
                        let mut p = tokio::process::Command::new("parec")
                            .args(args)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::null())
                            .kill_on_drop(true)
                            .spawn()?;
                        let mut out = p.stdout.take().unwrap();
                        let peak = Arc::new(Mutex::new(0f64));
                        let value = peak.clone();
                        let task = tokio::spawn(async move {
                            let mut buffer = [0u8; 65536];
                            let mut tail = vec![];
                            loop {
                                let n = match out.read(&mut buffer).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => n,
                                };
                                tail.extend_from_slice(&buffer[..n]);
                                let end = tail.len() / 4 * 4;
                                let max = tail[..end]
                                    .chunks_exact(4)
                                    .map(|b| f32::from_ne_bytes(b.try_into().unwrap()).abs())
                                    .filter(|f| f.is_finite())
                                    .fold(0f32, f32::max)
                                    .min(1.) as f64;
                                {
                                    let mut value = value.lock().unwrap();
                                    *value = value.max(max);
                                }
                                tail.drain(..end);
                            }
                            let _ = p.kill().await;
                        });
                        captures.insert(
                            k,
                            Capture {
                                task,
                                peak,
                                title,
                                sink,
                            },
                        );
                    }
                }
                Err(_) => captures.clear(),
            }
        }
        let mut levels = json!({});
        for c in captures.values() {
            let mut p = c.peak.lock().unwrap();
            levels[&c.title] = json!(num(&levels[&c.title]).max(*p));
            if c.sink == default {
                levels["__output"] = json!(num(&levels["__output"]).max(*p));
            }
            *p *= 0.65;
        }
        crate::emit(&levels);
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    }
}
