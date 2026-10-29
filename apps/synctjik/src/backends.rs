//! Native backup engines. Policies and secrets remain outside the executable.
mod cloudflare;
mod private_assets;
mod process;

pub fn exec(cloudflare: bool, args: &[String]) -> anyhow::Result<()> {
    let runner = process::Runner::new()?;
    if cloudflare {
        cloudflare::main(args, &runner)
    } else {
        private_assets::main(args, &runner)
    }
}
