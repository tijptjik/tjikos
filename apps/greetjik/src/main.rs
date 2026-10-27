// SPDX-License-Identifier: GPL-3.0-or-later
use greetjik::{render, PONIES};
use std::{
    env,
    fs::File,
    io::{self, IsTerminal, Read, Write},
    process,
};

fn columns() -> usize {
    #[cfg(unix)]
    for fd in [libc::STDOUT_FILENO, libc::STDERR_FILENO, libc::STDIN_FILENO] {
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        // SAFETY: ioctl writes a winsize into the valid stack allocation.
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } == 0 && size.ws_col > 0 {
            return size.ws_col as usize;
        }
    }
    env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(80)
}

fn random_index(len: usize) -> io::Result<usize> {
    let mut source = File::open("/dev/urandom")?;
    // Rejection sampling avoids modulo bias even for non-power-of-two catalogues.
    let bound = u64::MAX - u64::MAX % len as u64;
    loop {
        let mut bytes = [0; 8];
        source.read_exact(&mut bytes)?;
        let number = u64::from_ne_bytes(bytes);
        if number < bound {
            return Ok((number % len as u64) as usize);
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let (mut name, mut extra, mut all, mut list, mut tty, mut width) = (
        None,
        false,
        false,
        false,
        env::var("TERM").is_ok_and(|s| s == "linux"),
        columns(),
    );
    let mut message = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("greetjik — native pony artwork and piped quotes\n\nUsage: fortune | greetjik [OPTIONS]\n       greetjik [OPTIONS] MESSAGE...\n\n  -f, --pony NAME    Select any regular or extra pony\n  -l, --list         List regular pony names\n  -a, --all          Include extras in random selection/listing\n  -A, --extra        Select/list extras only\n      --tty          Use the complete upstream TTY artwork\n  -W, --width N      Wrap the quote to N terminal columns\n  -V, --version      Print version\n\nNo daemon, Docker, Python, network, or external artwork files required.");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("greetjik {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-f" | "--pony" => name = Some(args.next().ok_or("--pony needs a name")?),
            "-l" | "--list" => list = true,
            "-a" | "--all" => all = true,
            "-A" | "--extra" => extra = true,
            "--tty" => tty = true,
            "-W" | "--width" => {
                width = args
                    .next()
                    .ok_or("--width needs a number")?
                    .parse()
                    .map_err(|_| "Invalid width")?;
                if !(12..=4096).contains(&width) {
                    return Err("Width must be between 12 and 4096 columns".into());
                }
            }
            "--" => {
                message.extend(args);
                break;
            }
            value if value.starts_with('-') => return Err(format!("Unknown option: {value}")),
            _ => {
                message.push(arg);
                message.extend(args);
                break;
            }
        }
    }
    let candidates: Vec<_> = PONIES
        .iter()
        .filter(|p| p.tty == tty && (all || p.extra == extra))
        .collect();
    let mut stdout = io::stdout().lock();
    if list {
        for pony in candidates {
            writeln!(stdout, "{}", pony.name).map_err(io_error)?;
        }
        return Ok(());
    }
    let pony = if let Some(name) = name {
        let name = name.strip_suffix(".pony").unwrap_or(&name);
        PONIES
            .iter()
            .find(|p| p.name == name && p.tty == tty && (!extra || p.extra))
            .ok_or_else(|| format!("Unknown pony: {name}. Use --all --list."))?
    } else {
        candidates[random_index(candidates.len()).map_err(io_error)?]
    };
    let message = if message.is_empty() {
        if io::stdin().is_terminal() {
            return Err(
                "Pipe a quote into greetjik, or supply a message. Try: fortune | greetjik".into(),
            );
        }
        let mut text = String::new();
        io::stdin().read_to_string(&mut text).map_err(io_error)?;
        text
    } else {
        message.join(" ")
    };
    stdout
        .write_all(render(pony.art, &message, width.clamp(12, 4096))?.as_bytes())
        .map_err(io_error)
}

fn io_error(e: io::Error) -> String {
    if e.kind() == io::ErrorKind::BrokenPipe {
        process::exit(0);
    }
    e.to_string()
}

fn main() {
    if let Err(error) = run() {
        eprintln!("greetjik: {error}");
        process::exit(2);
    }
}
