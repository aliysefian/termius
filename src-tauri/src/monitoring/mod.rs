//! Watching hosts and telling someone when they go wrong.
//!
//! * [`model`]: the records kept in the vault (hosts to watch, alert rules, notification channels).
//! * [`ping`], [`checks`], [`http`], [`metrics`]: the kinds of check (ICMP ping, TCP port, a web page, CPU / memory / disk
//!   over SSH), and [`scan`], the port scan that runs when asked.
//! * [`rules`]: judging readings and remembering each state, so only a confirmed change is reported.
//! * [`notify`]: Telegram, Bale and email, and the dispatcher that sends an alert to the channels a rule allows.
//!
//! Nothing here knows about Tauri or the vault's lock, so it is all tested on its own. What a vault, a window and the
//! SSH client look like to the engine is behind a trait in `engine.rs`.

pub mod checks;
pub mod engine;
pub mod http;
pub mod metrics;
pub mod model;
pub mod notify;
pub mod ping;
pub mod rules;
pub mod scan;
