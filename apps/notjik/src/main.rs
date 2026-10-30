mod bridge;
mod comparison;
mod config;
mod cycle;
mod demo;
mod history;
mod lock;
mod media;
mod native;
mod recording;
mod telegram;
mod telegram_text;
mod util;
mod worker;
use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use util::*;
#[derive(Parser)]
#[command(name = "notjikctl", version)]
struct Cli {
    #[command(subcommand)]
    op: Op,
}
#[derive(Subcommand)]
enum Op {
    Pause,
    Resume,
    Toggle,
    Status,
    List,
    Monitors,
    Reload,
    Unfocus,
    Send {
        title: String,
        #[arg(default_value = "")]
        body: String,
        #[arg(long, default_value = "notjikctl")]
        app: String,
        #[arg(long = "type", default_value = "standard")]
        kind: String,
        #[arg(long,default_value="normal",value_parser=["low","normal","critical"])]
        urgency: String,
        #[arg(long)]
        timeout: Option<String>,
        #[arg(long, default_value = "")]
        icon: String,
        #[arg(long, default_value = "")]
        image: String,
        #[arg(long, conflicts_with = "id")]
        replace: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        group: Option<String>,
        #[arg(long, default_value = "{}")]
        data: String,
        #[arg(long, default_value = "[]")]
        actions: String,
        #[arg(long)]
        resident: bool,
        #[arg(long)]
        transient: bool,
    },
    Dismiss {
        id: String,
    },
    Action {
        id: String,
        index: i64,
    },
    History {
        #[arg(default_value="toggle",value_parser=["show","hide","toggle"])]
        mode: String,
    },
    HistoryData,
    HistoryAction {
        key: String,
        #[arg(value_parser=["invoke","dismiss"])]
        action: String,
        #[arg(long, default_value_t = 0)]
        index: i64,
    },
    Navigate {
        #[arg(value_parser=["previous","next"])]
        direction: String,
    },
    Logs {
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        event: Option<String>,
        #[arg(long)]
        since: Option<String>,
    },
    Init {
        #[arg(long,default_value="monochrome",value_parser=["monochrome","rose-punk"])]
        theme: String,
    },
    Config,
    Telegram {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Worker,
    Demo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Compare {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(hide = true)]
    CompareServe {
        root: PathBuf,
        dunst: PathBuf,
    },
    #[command(hide = true)]
    MediaRender {
        source: PathBuf,
        output: PathBuf,
        format: String,
    },
    #[command(hide = true)]
    Record {
        #[arg(long, hide = true)]
        synthetic: bool,
    },
}
fn root() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("NOTJIK_HOME") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe()?;
    let p = exe.parent().unwrap().parent().unwrap();
    if p.join("qml/shell.qml").is_file() {
        return Ok(p.into());
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if source.join("qml/shell.qml").is_file() {
        return Ok(source);
    }
    Ok(home().join(".local/lib/notjik"))
}
fn duration(v: &str) -> Result<f64> {
    if ["never", "until-dismissed", "0"].contains(&v) {
        return Ok(0.);
    }
    if v == "default" {
        return Ok(-1.);
    }
    let re = regex::Regex::new(r"^(\d+(?:\.\d+)?)(ms|s|m|h)?$")?;
    let c = re.captures(v).ok_or_else(|| {
        anyhow::anyhow!("use default, never, or a duration such as 8s / 500ms / 2m")
    })?;
    let n = c[1].parse::<f64>()?
        * match c.get(2).map(|m| m.as_str()) {
            Some("ms") => 1.,
            Some("m") => 60000.,
            Some("h") => 3600000.,
            _ => 1000.,
        };
    ensure!(n.is_finite(), "Invalid duration");
    Ok(n)
}
fn launch(root: &Path, args: &[String]) -> Result<()> {
    #[derive(Parser)]
    struct Launch {
        #[arg(long)]
        daemon: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    }
    let opts =
        Launch::parse_from(std::iter::once("notjik".to_string()).chain(args.iter().cloned()));
    if let Some(p) = opts.config {
        std::env::set_var("NOTJIK_CONFIG", p.canonicalize()?);
    }
    config::load(root)?;
    let out = run(
        Command::new("busctl").args([
            "--user",
            "call",
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            "s",
            "org.freedesktop.Notifications",
        ]),
        Duration::from_secs(5),
    )?;
    ensure!(!String::from_utf8_lossy(&out).contains("true"),"org.freedesktop.Notifications already has an owner. Stop the existing notification daemon before starting notjik.");
    let mut cmd = Command::new("quickshell");
    cmd.arg("--path")
        .arg(root.join("qml"))
        .arg("--no-duplicate");
    if opts.daemon {
        cmd.arg("--daemonize");
    }
    Err(cmd.exec().into())
}
fn cli(root: &Path, op: Op) -> Result<()> {
    let request = match op {
        Op::Config => {
            println!("{}", serde_json::to_string_pretty(&config::load(root)?)?);
            return Ok(());
        }
        Op::Worker => return worker::run(root),
        Op::Demo { args } => return demo::main(root, &args),
        Op::Compare { args } => return comparison::main(root, &args),
        Op::CompareServe { root, dunst } => return comparison::serve(&root, &dunst),
        Op::MediaRender {
            source,
            output,
            format,
        } => return media::render(root, &source, &output, &format),
        Op::Record { synthetic } => {
            ensure!(
                synthetic || !root.join(".synthetic-only").exists(),
                "Microphone access is disabled in the demo cycle"
            );
            let args = ["-re", "-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono"].map(str::to_string);
            return recording::record(if synthetic { Some(&args) } else { None });
        }
        Op::Telegram { args } => return telegram::main(root, &args),
        Op::Init { theme } => {
            let p = config_path();
            fs::create_dir_all(p.parent().unwrap())?;
            let mut v = config::defaults();
            v["theme"] = json!(theme);
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&p)?;
            writeln!(f, "{}", serde_json::to_string_pretty(&v)?)?;
            println!("{}", p.display());
            return Ok(());
        }
        Op::Logs {
            limit,
            follow,
            json,
            app,
            event,
            since,
        } => {
            ensure!(limit > 0, "--limit must be positive");
            let mut filters = HashMap::new();
            for (k, v) in [("app", app), ("event", event)] {
                if let Some(v) = v {
                    filters.insert(k.to_string(), v);
                }
            }
            if let Some(s) = since {
                let dt = parse_since(&s)?;
                filters.insert(
                    "since".into(),
                    dt.with_timezone(&chrono::Utc)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                );
            }
            return history::logs(limit, follow, json, filters);
        }
        Op::Send {
            title,
            body,
            app,
            kind,
            urgency,
            timeout,
            icon,
            image,
            replace,
            id,
            group,
            data,
            actions,
            resident,
            transient,
        } => {
            let mut n = json!({"title":title,"body":body,"app":app,"type":kind,"urgency":urgency,"icon":icon,"image":image,"replace":replace,"id":id,"group":group,"data":serde_json::from_str::<Value>(&data)?,"actions":serde_json::from_str::<Value>(&actions)?,"resident":resident,"transient":transient});
            if let Some(t) = timeout {
                n["timeout"] = json!(duration(&t)?);
            }
            for k in ["icon", "image"] {
                let s = n[k].as_str().unwrap();
                if ["/", "~", "./", "../"]
                    .iter()
                    .any(|prefix| s.starts_with(prefix))
                {
                    let p = expand(s);
                    let p = if p.is_absolute() {
                        p
                    } else {
                        std::env::current_dir()?.join(p)
                    };
                    n[k] = json!(uri(&p)?);
                }
            }
            let v = ipc(root, &json!({"op":"send","notice":n}))?;
            println!("{}", v["id"].as_str().unwrap_or(""));
            return Ok(());
        }
        Op::Pause => json!({"op":"pause"}),
        Op::Resume => json!({"op":"resume"}),
        Op::Toggle => json!({"op":"toggle"}),
        Op::Status => json!({"op":"status"}),
        Op::List => json!({"op":"list"}),
        Op::Monitors => json!({"op":"monitors"}),
        Op::Reload => json!({"op":"reload"}),
        Op::Unfocus => json!({"op":"unfocus"}),
        Op::Dismiss { id } => json!({"op":"dismiss","id":id}),
        Op::Action { id, index } => json!({"op":"action","id":id,"index":index}),
        Op::History { mode } => json!({"op":"history","mode":mode}),
        Op::HistoryData => json!({"op":"history-data"}),
        Op::HistoryAction { key, action, index } => {
            json!({"op":"history-action","key":key,"action":action,"index":index})
        }
        Op::Navigate { direction } => json!({"op":"navigate","direction":direction}),
    };
    println!("{}", serde_json::to_string_pretty(&ipc(root, &request)?)?);
    Ok(())
}
fn main() {
    unsafe {
        libc::umask(0o077);
    }
    let result = (|| -> Result<()> {
        let root = root()?;
        let args = std::env::args().collect::<Vec<_>>();
        if Path::new(&args[0]).file_name().and_then(|s| s.to_str()) == Some("notjik") {
            launch(&root, &args[1..])
        } else if args.get(1).map(String::as_str) == Some("launch") {
            launch(&root, &args[2..])
        } else {
            cli(&root, Cli::parse().op)
        }
    })();
    if let Err(e) = result {
        eprintln!("notjik: {e:#}");
        std::process::exit(1)
    }
}

fn parse_since(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::TimeZone;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    let dt = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|f| chrono::NaiveDateTime::parse_from_str(s, f).ok())
    .or_else(|| {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    })
    .ok_or_else(|| anyhow::anyhow!("--since must be an ISO 8601 date or time"))?;
    Ok(chrono::Local
        .from_local_datetime(&dt)
        .earliest()
        .ok_or_else(|| anyhow::anyhow!("Invalid local time"))?
        .with_timezone(&chrono::Utc))
}
#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn durations_and_since() {
        for (s, n) in [
            ("default", -1.),
            ("never", 0.),
            ("500ms", 500.),
            ("2m", 120000.),
            ("0.5", 500.),
        ] {
            assert_eq!(duration(s).unwrap(), n);
        }
        assert!(duration("NaN").is_err());
        assert!(parse_since("2026-10-30").is_ok());
        assert_eq!(
            parse_since("2026-10-30T08:00:00+08:00")
                .unwrap()
                .to_rfc3339(),
            "2026-10-30T00:00:00+00:00"
        );
    }
}
