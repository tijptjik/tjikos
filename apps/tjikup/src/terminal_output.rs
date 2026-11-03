//! Submit a display frame together, without exposing intermediate cursor positions.
use std::{cell::RefCell, fmt::Arguments, io::Write};

thread_local! {
    static BUFFER: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

pub struct Frame(bool);
impl Frame {
    pub fn new() -> Self {
        Self(BUFFER.with_borrow_mut(|buffer| {
            if buffer.is_some() {
                false
            } else {
                *buffer = Some(Vec::new());
                true
            }
        }))
    }
}
impl Drop for Frame {
    fn drop(&mut self) {
        if self.0 {
            if let Some(bytes) = BUFFER.with_borrow_mut(Option::take) {
                let mut output = std::io::stderr().lock();
                let _ = output.write_all(&bytes);
                let _ = output.flush();
            }
        }
    }
}
pub fn write(args: Arguments<'_>) {
    BUFFER.with_borrow_mut(|buffer| {
        if let Some(buffer) = buffer {
            let _ = buffer.write_fmt(args);
        } else {
            let _ = std::io::stderr().lock().write_fmt(args);
        }
    });
}
macro_rules! eprint {
    ($($args:tt)*) => { $crate::terminal_output::write(format_args!($($args)*)) };
}
macro_rules! eprintln {
    () => { $crate::terminal_output::write(format_args!("\n")) };
    ($($args:tt)*) => { $crate::terminal_output::write(format_args!("{}\n", format_args!($($args)*))) };
}
pub(crate) use {eprint, eprintln};
