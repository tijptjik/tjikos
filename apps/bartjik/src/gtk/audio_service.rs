//! Audio transport and bounded worker; independent of GTK presentation.
//! PulseAudio compatibility transport, with existing core routing/meter policy.
use crate::util::{arr, cmd, command, jc, root, s};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub kind: String,
    pub index: u64,
    pub identity: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub targets: Vec<Target>,
    pub volume: f64,
    pub muted: bool,
    pub microphone: bool,
    pub master: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub groups: Vec<Group>,
    pub route: bool,
    pub headphones: bool,
}
#[derive(Clone)]
pub enum Action {
    Volume(Vec<Target>, f64),
    Mute(Vec<Target>, bool),
    Route,
}
pub enum Update {
    State(Result<Snapshot, String>, bool),
    Peaks(Value),
}
fn identity(kind: &str, v: &Value) -> String {
    if kind == "sink-input" {
        format!(
            "{}:{}:{}",
            v["client"],
            v["properties"]["object.serial"],
            v["properties"]["application.process.id"]
        )
    } else {
        s(&v["name"]).into()
    }
}
fn node(kind: &str, v: &Value, title: String, key: String, master: bool) -> Group {
    let channels: Vec<_> = v["volume"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.values())
        .filter_map(|v| v["value"].as_f64())
        .collect();
    Group {
        key,
        title,
        detail: s(&v["description"]).into(),
        targets: vec![Target {
            kind: kind.into(),
            index: v["index"].as_u64().unwrap_or(u64::MAX),
            identity: identity(kind, v),
        }],
        volume: if channels.is_empty() {
            0.
        } else {
            channels.iter().sum::<f64>() / channels.len() as f64 / 65536.
        },
        muted: v["mute"] == true,
        microphone: kind == "source",
        master,
    }
}
async fn snapshot() -> anyhow::Result<Snapshot> {
    let (sinks, sources, streams, default, input) = tokio::try_join!(
        jc(&["pactl", "-f", "json", "list", "sinks"]),
        jc(&["pactl", "-f", "json", "list", "sources"]),
        jc(&["pactl", "-f", "json", "list", "sink-inputs"]),
        cmd(&["pactl", "get-default-sink"]),
        async {
            Ok::<_, anyhow::Error>(
                cmd(&["pactl", "get-default-source"])
                    .await
                    .unwrap_or_default(),
            )
        },
    )?;
    let mut groups = vec![];
    let output = arr(&sinks).iter().find(|v| v["name"] == default);
    if let Some(v) = output {
        groups.push(node("sink", v, "Output".into(), "__output".into(), true));
    }
    let microphone = arr(&sources)
        .iter()
        .find(|v| s(&v["name"]).starts_with("alsa_input.usb-046d_Logitech_BRIO_"))
        .or_else(|| {
            arr(&sources)
                .iter()
                .find(|v| v["name"] == input && !s(&v["name"]).ends_with(".monitor"))
        });
    if let Some(v) = microphone {
        groups.push(node(
            "source",
            v,
            "Microphone".into(),
            "__input".into(),
            true,
        ));
    }
    for v in arr(&streams).iter().take(128) {
        let title = v["properties"]["application.name"]
            .as_str()
            .or(v["properties"]["media.name"].as_str())
            .unwrap_or("Playback");
        if format!("{title}{}", s(&v["properties"]["node.name"]))
            .to_lowercase()
            .contains("speech-dispatcher-dummy")
        {
            continue;
        }
        let group = node("sink-input", v, title.into(), title.into(), false);
        if let Some(old) = groups.iter_mut().find(|g| !g.master && g.key == title) {
            let n = old.targets.len() as f64;
            old.volume = (old.volume * n + group.volume) / (n + 1.);
            old.muted &= group.muted;
            old.targets.extend(group.targets);
        } else {
            groups.push(group);
        }
    }
    Ok(Snapshot {
        groups,
        headphones: output.is_some_and(|v| v["description"] == "USB Audio Speakers"),
        route: arr(&sinks)
            .iter()
            .any(|v| v["description"] == "USB Audio Speakers")
            && arr(&sinks)
                .iter()
                .any(|v| s(&v["description"]).starts_with("Navi 21/23 HDMI/DP")),
    })
}
async fn execute(action: Action) -> anyhow::Result<()> {
    let (targets, setting, value) = match action {
        Action::Route => {
            command(
                &[
                    root().join("bin/bartjik-gtk-core").to_string_lossy().into(),
                    "helper".into(),
                    "audio".into(),
                    "toggle-output".into(),
                ],
                35.,
                262144,
                &[],
                None,
            )
            .await?;
            return Ok(());
        }
        Action::Volume(targets, value) => {
            anyhow::ensure!(
                value.is_finite() && (0. ..=1.).contains(&value),
                "Invalid volume"
            );
            (targets, "volume", format!("{}%", (value * 100.).round()))
        }
        Action::Mute(targets, value) => (targets, "mute", if value { "1" } else { "0" }.into()),
    };
    // Revalidate the entire group before issuing its first mutation.
    for target in &targets {
        let list = match target.kind.as_str() {
            "sink" => "sinks",
            "source" => "sources",
            "sink-input" => "sink-inputs",
            _ => anyhow::bail!("Unsupported audio target"),
        };
        let current = jc(&["pactl", "-f", "json", "list", list]).await?;
        anyhow::ensure!(arr(&current).iter().any(|v|v["index"]==target.index && identity(&target.kind,v)==target.identity),"Audio device or stream disappeared");
    }
    for target in targets {
        cmd(&[
            "pactl",
            &format!("set-{}-{setting}", target.kind),
            &target.index.to_string(),
            &value,
        ])
        .await?;
    }
    Ok(())
}
struct Meter(tokio::process::Child);
impl Drop for Meter {
    fn drop(&mut self) {
        if let Some(id) = self.0.id() {
            unsafe {
                libc::kill(-(id as i32), libc::SIGKILL);
            }
        }
    }
}
fn meter() -> std::io::Result<Meter> {
    let mut command = tokio::process::Command::new(root().join("bin/bartjik-gtk-core"));
    command
        .args(["helper", "audio-levels"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let parent = std::process::id();
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() as u32 != parent {
                return Err(std::io::Error::other("audio owner exited"));
            }
            Ok(())
        });
    }
    Ok(Meter(command.spawn()?))
}
fn subscription() -> std::io::Result<tokio::process::Child> {
    tokio::process::Command::new("pactl")
        .arg("subscribe")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
}
fn relevant_event(line: &str) -> bool {
    ["server", "sink", "source", "sink-input", "card"]
        .iter()
        .any(|kind| line.contains(&format!(" on {kind} ")))
}
pub struct Session {
    commands: tokio::sync::mpsc::Sender<Action>,
    stop: tokio::sync::watch::Sender<bool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}
impl Session {
    pub fn send(&self, action: Action) -> Result<(), String> {
        self.commands
            .try_send(action)
            .map_err(|_| "Audio request queue is full".into())
    }
}
pub fn start(demo: bool) -> (Session, async_channel::Receiver<Update>) {
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let (stop, mut cancelled) = tokio::sync::watch::channel(false);
    let (send, updates) = async_channel::bounded(2);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move{
        let work=async{
            let mut fake:Snapshot=serde_json::from_str(include_str!("../../tests/fixtures/audio-card.json")).unwrap();
            // Subscribe before the initial snapshot so startup changes are not lost.
            let mut subscription = if demo { None } else { subscription().ok() };
            let mut events = subscription.as_mut().and_then(|p| p.stdout.take()).map(|out| BufReader::new(out).lines());
            let mut refresh_at: Option<tokio::time::Instant> = None;
            let initial=if demo{Ok(fake.clone())}else{snapshot().await.map_err(|_|"Audio service is unavailable".into())};
            if send.send(Update::State(initial,false)).await.is_err(){return;}
            let mut meter=if demo{None}else{meter().ok()};
            let stdout=meter.as_mut().and_then(|m|m.0.stdout.take());let mut lines=stdout.map(|out|BufReader::new(out).lines());
            let mut interval=tokio::time::interval_at(tokio::time::Instant::now()+Duration::from_secs(2),Duration::from_secs(2));interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop{let update=tokio::select!{
                line=async{events.as_mut().unwrap().next_line().await},if events.is_some()=>{
                    match line {
                        Ok(Some(line)) => if relevant_event(&line) && refresh_at.is_none() {
                            refresh_at=Some(tokio::time::Instant::now()+Duration::from_millis(40));
                        },
                        _ => { events=None; subscription=None; }
                    }
                    continue;
                },
                _=async{tokio::time::sleep_until(refresh_at.unwrap()).await},if refresh_at.is_some()=>{
                    refresh_at=None;
                    Update::State(snapshot().await.map_err(|_|"Audio service is unavailable".into()),false)
                },
                action=requests.recv()=>{let Some(action)=action else{break;};let result=if demo{apply_fake(&mut fake,action);Ok(fake.clone())}else{match execute(action).await{Ok(())=>snapshot().await.map_err(|_|"Audio changed; status is unavailable".into()),Err(_)=>Err("Audio request failed; the device may have disappeared or access was denied".into())}};Update::State(result,true)},
                _=interval.tick()=>{if !demo && subscription.is_none(){subscription=self::subscription().ok();events=subscription.as_mut().and_then(|p|p.stdout.take()).map(|out|BufReader::new(out).lines());}if !demo && meter.is_none(){meter= self::meter().ok();lines=meter.as_mut().and_then(|m|m.0.stdout.take()).map(|s|BufReader::new(s).lines());}Update::State(if demo{Ok(fake.clone())}else{snapshot().await.map_err(|_|"Audio service is unavailable".into())},false)},
                line=async{lines.as_mut().unwrap().next_line().await},if lines.is_some()=>{match line{Ok(Some(line))if line.len()<262144=>Update::Peaks(serde_json::from_str(&line).unwrap_or_default()),_=>{lines=None;meter=None;Update::Peaks(serde_json::json!({}))}}}
            };if send.send(update).await.is_err(){break;}}
        };tokio::select!{_=cancelled.changed()=>{},_=work=>{}}
    });
    });
    (Session { commands, stop }, updates)
}
fn apply_fake(state: &mut Snapshot, action: Action) {
    match action {
        Action::Route => state.headphones = !state.headphones,
        Action::Volume(targets, value) => {
            for g in &mut state.groups {
                if g.targets == targets {
                    g.volume = value;
                }
            }
        }
        Action::Mute(targets, value) => {
            for g in &mut state.groups {
                if g.targets == targets {
                    g.muted = value;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_volume_mute_and_teardown() {
        let (session, updates) = start(true);
        let Update::State(Ok(state), _) = updates.recv_blocking().unwrap() else {
            panic!()
        };
        session
            .send(Action::Volume(state.groups[0].targets.clone(), 0.27))
            .unwrap();
        let Update::State(Ok(state), true) = updates.recv_blocking().unwrap() else {
            panic!()
        };
        assert_eq!(state.groups[0].volume, 0.27);
        session
            .send(Action::Mute(state.groups[0].targets.clone(), true))
            .unwrap();
        let Update::State(Ok(state), true) = updates.recv_blocking().unwrap() else {
            panic!()
        };
        assert!(state.groups[0].muted);
        drop(session);
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while !updates.is_closed() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(updates.is_closed());
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires private executable fixtures; never run against the host audio service"]
    async fn audio_private_tools() {
        assert_eq!(
            std::env::var("BARTJIK_AUDIO_TEST_TOOLS").as_deref(),
            Ok("1")
        );
        let state = snapshot().await.unwrap();
        assert_eq!(state.groups.len(), 3);
        assert!(state.route);
        assert!(state.headphones);
        execute(Action::Volume(state.groups[0].targets.clone(), 0.27))
            .await
            .unwrap();
        assert!((snapshot().await.unwrap().groups[0].volume - 0.27).abs() < 0.001);
        execute(Action::Mute(state.groups[1].targets.clone(), true))
            .await
            .unwrap();
        assert!(snapshot().await.unwrap().groups[1].muted);
        let mut stale = state.groups[2].targets.clone();
        stale[0].identity.push_str("-old-generation");
        assert!(execute(Action::Volume(stale, 0.9)).await.is_err());
        assert!(
            execute(Action::Volume(state.groups[0].targets.clone(), f64::NAN))
                .await
                .is_err()
        );
        execute(Action::Route).await.unwrap();
        assert!(!snapshot().await.unwrap().headphones);
        // External output changes must arrive before the two-second recovery poll.
        let (session, updates) = start(false);
        loop {
            if matches!(updates.recv().await.unwrap(), Update::State(Ok(_), _)) {
                break;
            }
        }
        // Allow the startup subscription event to settle before changing state.
        tokio::time::sleep(Duration::from_millis(150)).await;
        while updates.try_recv().is_ok() {}
        let path = std::env::var("BARTJIK_AUDIO_STATE").unwrap();
        let mut fixture: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        fixture["default"] = "usb".into();
        let began = std::time::Instant::now();
        std::fs::write(path, serde_json::to_vec(&fixture).unwrap()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Update::State(Ok(state), _) = updates.recv().await.unwrap() {
                    if state.headphones {
                        break;
                    }
                }
            }
        })
        .await
        .expect("subscription must refresh before the recovery poll");
        println!("External route event observed in {:?}", began.elapsed());
        drop(session);
    }
}
