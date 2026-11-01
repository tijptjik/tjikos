fn main() {
    unsafe {
        libc::umask(0o077);
    }
    if let Err(e) = notjik_sync::client::command(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("notjik-sync: {e:#}");
        std::process::exit(1);
    }
}
