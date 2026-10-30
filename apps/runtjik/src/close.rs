//! Close retries are bound to an owned process identity and delivered through pidfds.
use crate::{discovery::Entry, store::Store, util::*, windows};
use anyhow::{bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
};
pub type Pending = BTreeMap<i32, (String, i64)>;
pub type Tracking = HashMap<String, HashSet<String>>;
pub fn identity(pid: i32) -> Option<String> {
    if pid <= 1 || pid == std::process::id() as i32 {
        return None;
    }
    let dir = std::path::PathBuf::from(format!("/proc/{pid}"));
    if dir.metadata().ok()?.uid() != unsafe { libc::getuid() } {
        return None;
    }
    let stat = fs::read_to_string(dir.join("stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    if ["Z", "X"].contains(fields.first()?) {
        return None;
    }
    Some(format!(
        "{}:{}",
        fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()?
            .trim(),
        fields.get(19)?
    ))
}
pub fn signal(pid: i32, expected: &str, signum: i32) -> Result<bool> {
    if ![libc::SIGTERM, libc::SIGKILL].contains(&signum) {
        bail!("Invalid close signal");
    }
    if identity(pid).as_deref() != Some(expected) {
        return Ok(false);
    }
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(err.into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    if identity(pid).as_deref() != Some(expected) {
        return Ok(false);
    }
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signum,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    } < 0
    {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(err.into());
    }
    Ok(true)
}
pub fn pending(store: &Store, e: &Entry, settings: &Value) -> Result<Pending> {
    let classes = settings["overrides"][&e.id]
        .get("window_classes")
        .unwrap_or(&e.data["classes"]);
    if e.provider == "commands" && e.data["gui"] != true && strings(classes).is_empty() {
        return Ok(Pending::new());
    }
    let mut stmt = store
        .0
        .prepare("SELECT pid,identity,stage FROM close_requests WHERE entry_id=?")?;
    let rows = stmt
        .query_map([&e.id], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Pending::new();
    for (pid, id, stage) in rows {
        if identity(pid).as_deref() == Some(&id) {
            out.insert(pid, (id, stage));
        } else {
            store.0.execute(
                "DELETE FROM close_requests WHERE entry_id=? AND pid=?",
                params![e.id, pid],
            )?;
        }
    }
    Ok(out)
}
pub fn close_windows(clients: &[Value]) -> Result<()> {
    for c in clients {
        windows::focus_commands(c, &[], true)?;
    }
    let lua = command(&["hyprctl", "eval", "return"]).is_ok_and(|v| v.trim() == "ok");
    for c in clients {
        let address = s(&c["address"]);
        let args = if lua {
            vec![
                "hyprctl".into(),
                "dispatch".into(),
                format!("hl.dsp.window.close({{window=\"address:{address}\"}})"),
            ]
        } else {
            vec![
                "hyprctl".into(),
                "dispatch".into(),
                "closewindow".into(),
                format!("address:{address}"),
            ]
        };
        let out = run(&args, None, 3)?;
        if out.trim() != "ok" {
            bail!("Window close request failed: {}", out.trim());
        }
    }
    Ok(())
}
pub fn close(
    e: &Entry,
    store: &Store,
    settings: &Value,
    mut clients: Vec<Value>,
    address: &str,
    mut pending: Pending,
    tracking: &mut Tracking,
) -> Result<Value> {
    if !address.is_empty() {
        clients.retain(|c| c["address"] == address);
        if clients.is_empty() {
            return Ok(json!({"kind":"message","message":"That window has already closed"}));
        }
        pending.retain(|pid, _| clients.iter().any(|c| c["pid"] == *pid));
    }
    if clients.is_empty() && pending.is_empty() {
        return Ok(
            json!({"kind":"message","message":format!("No running window or pending close request for {}",e.name)}),
        );
    }
    let mut groups: BTreeMap<i32, (String, Vec<Value>)> = BTreeMap::new();
    let mut untracked = vec![];
    for c in &clients {
        let pid = c["pid"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(0);
        if let Some(id) = identity(pid) {
            groups.entry(pid).or_insert((id, vec![])).1.push(c.clone());
        } else {
            untracked.push(c.clone());
        }
    }
    let mut stages = BTreeSet::new();
    if !untracked.is_empty() {
        close_windows(&untracked)?;
        stages.insert(0);
    }
    let pids: BTreeSet<_> = groups.keys().chain(pending.keys()).copied().collect();
    for pid in pids {
        let (id, windows) = groups
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| (pending[&pid].0.clone(), vec![]));
        let stage = pending
            .get(&pid)
            .filter(|(old, _)| *old == id)
            .map(|(_, stage)| (stage + 1).min(2))
            .unwrap_or(0);
        if stage > 0 {
            if !signal(
                pid,
                &id,
                if stage == 1 {
                    libc::SIGTERM
                } else {
                    libc::SIGKILL
                },
            )? {
                continue;
            }
        } else {
            close_windows(&windows)?;
        }
        store.0.execute(
            "INSERT OR REPLACE INTO close_requests VALUES(?,?,?,?)",
            params![e.id, pid, id, stage],
        )?;
        stages.insert(stage);
    }
    if stages.is_empty() {
        return Ok(
            json!({"kind":"message","message":format!("The previous process has already exited; launch again to reopen {}",e.name)}),
        );
    }
    if settings["history"]["enabled"] == true {
        store.record(&e.id, "close", true)?;
    }
    tracking
        .entry(e.id.clone())
        .or_default()
        .extend(clients.iter().map(|c| s(&c["address"]).to_owned()));
    let labels = ["graceful close", "SIGTERM", "SIGKILL"];
    Ok(
        json!({"kind":"activated","action":"close","demo":false,"entry":e.id,"message":format!("Close requested for {}: {}",e.name,stages.into_iter().map(|n|labels[n as usize]).collect::<Vec<_>>().join(", "))}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_stale_identity_without_signalling() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let id = identity(pid).unwrap();
        assert!(!signal(pid, "old-boot:0", libc::SIGTERM).unwrap());
        assert!(child.try_wait().unwrap().is_none());
        assert!(signal(pid, &id, libc::SIGTERM).unwrap());
        child.wait().unwrap();
        assert!(!signal(pid, &id, libc::SIGKILL).unwrap());
    }
    #[test]
    fn refuses_self() {
        assert!(identity(std::process::id() as i32).is_none());
        assert!(identity(1).is_none());
    }
}
