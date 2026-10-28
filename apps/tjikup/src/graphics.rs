//! Kitty Unicode placeholders scroll with the text; no absolute image overlays.
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
use unicode_width::UnicodeWidthChar;

pub fn geometry() -> Option<(usize, usize)> {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(2, libc::TIOCGWINSZ, &mut size) } == 0
        && size.ws_col > 0
        && size.ws_row > 0
    {
        let w = usize::from(size.ws_xpixel / size.ws_col);
        let h = usize::from(size.ws_ypixel / size.ws_row);
        if (4..=128).contains(&w) && (16..=256).contains(&h) {
            return Some((w, h));
        }
    }
    None
}
pub struct Graphics {
    sprites: BTreeMap<String, String>,
    images: BTreeMap<(String, usize, usize), u32>,
    pub size: (usize, usize),
    pub herdr: bool,
    next_id: u32,
    compact: bool,
}
impl Graphics {
    pub fn new(path: &str, tty: bool) -> Option<Self> {
        let herdr = std::env::var("HERDR_ENV").as_deref() == Ok("1")
            && std::env::var("TERM_PROGRAM").as_deref() == Ok("herdr");
        if !tty
            || ["TJIKUP_ICONS", "TJIKUP_KITTY_ICONS"]
                .iter()
                .any(|k| std::env::var(k).as_deref() == Ok("0"))
            || ["TMUX", "STY"]
                .iter()
                .any(|k| std::env::var_os(k).is_some())
            || !(herdr || std::env::var("TERM").as_deref() == Ok("xterm-kitty"))
        {
            return None;
        }
        let size = geometry().or_else(|| herdr.then(query_cell_size).flatten())?;
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if data["size"] != 32 || data["format"] != "zlib-base64-rgba" {
            return None;
        }
        Some(Self {
            compact: std::env::var("TJIKUP_COMPACT").as_deref() == Ok("1"),
            sprites: serde_json::from_value(data["sprites"].clone()).ok()?,
            images: BTreeMap::new(),
            size,
            herdr,
            next_id: (std::process::id() * 257) % 0xffff00 + 1,
        })
    }
    pub fn has_icon(&self, key: &str) -> bool {
        self.sprites.contains_key(key)
    }
    pub fn rows(&self) -> usize {
        if self.compact {
            1
        } else {
            2
        }
    }
    pub fn columns(&self) -> usize {
        2 * 32_usize.div_ceil(2 * self.size.0)
    }
    fn image(&mut self, label: &str) -> Option<u32> {
        self.size = geometry().unwrap_or(self.size);
        let key = if self.sprites.contains_key(label) {
            label
        } else {
            label.split(" (").next()?
        };
        let key = if self.sprites.contains_key(key) {
            key
        } else {
            "*"
        };
        let cache = (key.to_owned(), self.size.0, self.size.1);
        if let Some(id) = self.images.get(&cache) {
            return Some(*id);
        }
        let source = STANDARD.decode(self.sprites.get(key)?).ok()?;
        let mut pixels = Vec::new();
        ZlibDecoder::new(&source[..])
            .take(4097)
            .read_to_end(&mut pixels)
            .ok()?;
        if pixels.len() != 4096 {
            return None;
        }
        let (width, height) = (self.columns() * self.size.0, self.rows() * self.size.1);
        let mut canvas = vec![0; width * height * 4];
        let side = 32.min(height);
        let left = (width - side) / 2;
        let top = (if self.herdr { self.size.1 } else { height }).saturating_sub(side) / 2;
        // Area sampling keeps small icons readable at a single cell's height.
        for y in 0..side {
            for x in 0..side {
                for channel in 0..4 {
                    let mut sum = 0u32;
                    let mut samples = 0;
                    for sy in y * 32 / side..((y + 1) * 32 / side) {
                        for sx in x * 32 / side..((x + 1) * 32 / side) {
                            sum += u32::from(pixels[(sy * 32 + sx) * 4 + channel]);
                            samples += 1;
                        }
                    }
                    canvas[((top + y) * width + left + x) * 4 + channel] = (sum / samples) as u8;
                }
            }
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&canvas).ok()?;
        let data = STANDARD.encode(encoder.finish().ok()?);
        let id = self.next_id;
        self.next_id = self.next_id % 0xffffff + 1;
        let chunks: Vec<_> = data.as_bytes().chunks(4096).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let header = if i == 0 {
                format!(
                    "a=T,f=32,o=z,s={width},v={height},i={id},U=1,c={},r={},q=2,",
                    self.columns(),
                    self.rows()
                )
            } else {
                String::new()
            };
            eprint!(
                "\x1b_G{header}m={};{}\x1b\\",
                usize::from(i + 1 < chunks.len()),
                std::str::from_utf8(chunk).ok()?
            );
        }
        self.images.insert(cache, id);
        Some(id)
    }
    fn placeholders(&self, id: u32, row: usize) -> String {
        let marks = [
            '\u{305}', '\u{30d}', '\u{30e}', '\u{310}', '\u{312}', '\u{33d}', '\u{33e}', '\u{33f}',
        ];
        let mut text = format!("\x1b[38;2;{};{};{}m", id >> 16, (id >> 8) & 255, id & 255);
        for col in 0..self.columns() {
            text.push('\u{10eeee}');
            text.push(marks[row]);
            text.push(marks[col]);
        }
        text.push_str("\x1b[0m");
        text
    }
    pub fn text(&self, text: &str) -> String {
        if self.herdr || self.compact {
            text.to_owned()
        } else {
            centred(text)
        }
    }
    pub fn row(&mut self, label: &str, prefix: &str, rest: &str) {
        if let Some(id) = self.image(label) {
            let indent = crate::progress::width(&crate::report::plain(prefix));
            eprint!(
                "{}{}{}\r\n",
                self.text(prefix),
                self.placeholders(id, 0),
                self.text(rest)
            );
            if self.rows() == 2 {
                if indent > 0 {
                    eprint!("\x1b[{indent}C");
                }
                eprint!("{}\r\n", self.placeholders(id, 1));
            }
        } else {
            eprintln!("{prefix}{rest}");
            if self.rows() == 2 {
                eprintln!();
            }
        }
    }
}
fn centred(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut style = String::new();
    let mut block = String::new();
    let mut cells = 0;
    fn flush(out: &mut String, block: &mut String, cells: &mut usize, style: &str) {
        if !block.is_empty() {
            out.push_str(&format!(
                "{style}\x1b]66;s=2:n=1:d=2:v=2:w=1;{}{}\x1b\\\x1b[0m",
                block,
                " ".repeat(2_usize.saturating_sub(*cells))
            ));
            block.clear();
            *cells = 0;
        }
    }
    // SGR changes occur at even-cell boundaries in our status prefix and padded
    // operation cells. Keep a pair's initial style, as the original renderer did.
    let mut block_style = String::new();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            let mut sgr = String::from(c);
            if chars.peek() == Some(&'[') {
                sgr.push(chars.next().unwrap());
                for c in chars.by_ref() {
                    sgr.push(c);
                    if c == 'm' {
                        break;
                    }
                }
            }
            if sgr == "\x1b[0m" {
                style.clear();
            } else {
                style.push_str(&sgr);
            }
            continue;
        }
        let w = c.width().unwrap_or(0);
        if cells + w > 2 {
            flush(&mut result, &mut block, &mut cells, &block_style);
        }
        if block.trim().is_empty() {
            block_style = style.clone();
        }
        block.push(c);
        cells += w;
    }
    flush(&mut result, &mut block, &mut cells, &block_style);
    result
}
// Query only before workers start. Restore modes even if the terminal does not
// answer; never consume input already waiting at startup.
fn query_cell_size() -> Option<(usize, usize)> {
    use std::os::fd::AsRawFd;
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let fd = tty.as_raw_fd();
    let mut original: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
        return None;
    }
    let mut mode = original;
    mode.c_lflag &= !(libc::ICANON | libc::ECHO);
    mode.c_cc[libc::VMIN] = 0;
    mode.c_cc[libc::VTIME] = 0;
    struct Restore(i32, libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(self.0, libc::TCSANOW, &self.1);
            }
        }
    }
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &mode) } != 0 {
        return None;
    }
    let _restore = Restore(fd, original);
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut poll, 1, 0) } != 0 {
        return None;
    }
    (&tty).write_all(b"\x1b[16t").ok()?;
    let start = std::time::Instant::now();
    let mut response = Vec::new();
    while response.len() < 32 && start.elapsed().as_millis() < 200 {
        if unsafe { libc::poll(&mut poll, 1, 10) } <= 0 {
            continue;
        }
        let mut byte = [0];
        if (&tty).read(&mut byte).ok()? != 1 {
            continue;
        }
        response.push(byte[0]);
        if byte[0] == b't' {
            break;
        }
    }
    let text = std::str::from_utf8(&response)
        .ok()?
        .strip_prefix("\x1b[6;")?
        .strip_suffix('t')?;
    let (h, w) = text.split_once(';')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    ((4..=128).contains(&w) && (16..=256).contains(&h)).then_some((w, h))
}
