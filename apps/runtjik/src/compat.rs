use anyhow::{bail, Result};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
pub fn root() -> PathBuf {
    std::env::var_os("RUNTJIK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(PathBuf::from));
            exe.filter(|p| p.join("qml").is_dir())
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        })
}
pub struct Host {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Host {
    pub fn start() -> Result<Self> {
        let mut child = Command::new("python3")
            .arg("-B")
            .arg(root().join("compat_host.py"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        })
    }
    pub fn request(&mut self, v: &Value) -> Result<Value> {
        writeln!(self.input, "{}", v)?;
        self.input.flush()?;
        let mut line = String::new();
        if self.output.read_line(&mut line)? == 0 {
            bail!("Provider host exited");
        }
        Ok(serde_json::from_str(&line)?)
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
