//! Native backup engines. Policies and secrets remain outside the executable.
mod app_backup;
mod app_pull;
mod app_restore;
mod cloudflare;
mod drive;
mod private_assets;
mod process;
mod protection;
mod protection_migration;
mod snapraid;
mod snapraid_notify;

pub fn protection_exec(name: &str, args: &[String]) -> anyhow::Result<()> {
    let runner = process::Runner::new()?;
    unsafe {
        libc::umask(0o077);
    }
    match name {
        "protection-migration" => protection_migration::main(args, &runner),
        "drive" => drive::main(args, &runner),
        "app-backup" => app_backup::main(args, &runner),
        "app-pull" => app_pull::main(args, &runner),
        "app-restore" => app_restore::main(args, &runner),
        "snapraid" => snapraid::main(args, &runner),
        "snapraid-notify" => snapraid_notify::main(args, &runner),
        _ => anyhow::bail!("Unknown protection engine"),
    }
}

pub fn is_busy(error: &anyhow::Error) -> bool {
    error.downcast_ref::<process::Busy>().is_some()
}

pub fn exec(cloudflare: bool, args: &[String]) -> anyhow::Result<()> {
    let runner = process::Runner::new()?;
    if cloudflare {
        cloudflare::main(args, &runner)
    } else {
        private_assets::main(args, &runner)
    }
}
