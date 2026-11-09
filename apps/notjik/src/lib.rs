//! Native Notjik services shared by the CLI and future desktop frontends.
//! UI toolkits and their main loops stay outside this crate API.
pub mod backend;
pub mod backend_channel;
mod bridge;
mod cli;
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
mod telegram_clipboard;
mod telegram_text;
mod util;
mod worker;

pub(crate) use cli::duration;
pub use cli::run_cli;
