//! Host metrics: one agentless engine that reads a host over the SSH connection the app already keeps, and that every
//! view shares.
//!
//! * [`script`]: the read-only shell that prints a host's counters (one run per reading, no agent, nothing installed).
//! * [`parse`]: its output as raw counters, tolerant of anything missing.
//! * [`derive`]: two readings to a [`model::Sample`]: percentages and rates, with the rules for the first reading, a
//!   reboot, a reset or wrapped counter, and a gap. What cannot be measured is `None`, never 0.
//! * [`manager`]: [`manager::HostMetricsManager`], which decides when a host is actually read (shared recent samples,
//!   one collection at a time per host, bounded concurrency, back-off, cancellation) and keeps a bounded history.
//!
//! Nothing here knows about Tauri or the vault, so it is all tested on its own, including against this machine's own
//! `/proc`. What a host's connection looks like is behind [`manager::HostExec`].
//!
//! Linux is the platform with a collector. Any other system is reported as unsupported with its name, and none of its
//! figures are guessed.

pub mod derive;
pub mod logins;
pub mod manager;
pub mod model;
pub mod parse;
pub mod ports;
pub mod procs;
pub mod script;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_detail;
