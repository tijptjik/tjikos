//! Terminal primitives shared by TjikOS tools; no scheduler or recipe types.
use std::io::{IsTerminal, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
pub fn columns() -> usize {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe {
        libc::ioctl(2, libc::TIOCGWINSZ, &mut size);
    }
    if size.ws_col > 0 {
        usize::from(size.ws_col)
    } else {
        80
    }
}
pub fn fit(text: &str, limit: usize) -> String {
    if width(text) <= limit {
        return text.into();
    }
    if limit == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        let n = c.width().unwrap_or(0);
        if used + n >= limit {
            break;
        }
        out.push(c);
        used += n;
    }
    out.push('…');
    out
}
pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
pub fn colour(code: &str, text: &str) -> String {
    if std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.into()
    }
}
pub fn heading(text: &str) {
    eprintln!(
        "\n{}\n",
        colour(
            "1;34",
            &format!("{:^width$}", safe(text), width = columns())
        )
    );
}
pub struct Cursor;
impl Cursor {
    pub fn hide() -> Self {
        if std::io::stderr().is_terminal() {
            eprint!("\x1b[?25l");
            let _ = std::io::stderr().flush();
        }
        Self
    }
}
impl Drop for Cursor {
    fn drop(&mut self) {
        if std::io::stderr().is_terminal() {
            eprint!("\x1b[?25h");
            let _ = std::io::stderr().flush();
        }
    }
}
