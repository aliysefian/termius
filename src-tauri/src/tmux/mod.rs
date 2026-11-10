//! tmux on hosts: discovery, previews, search, changes and resource figures, shared by every view.
//!
//! * [`model`]: a host's tmux server as the app sees it (sessions, windows, panes by tmux's own ids), and the sockets
//!   and versions that go with it.
//! * [`script`]: the shell that talks to tmux, and the commands typed into a terminal to attach. Output is read through
//!   a per-run nonce, so a window named like a section header cannot forge data.
//! * [`parse`]: its output as a snapshot, strict about structure.
//! * [`ops`]: every command that changes a server, validated, quoted, and checked afterwards.
//! * [`search`], [`resources`]: searching captured text, and CPU/memory by process tree.
//! * [`manager`]: [`manager::TmuxManager`], which decides when a host is read (shared, ordered, bounded, one poll loop
//!   per host) and runs the operations.
//!
//! Nothing here knows about Tauri, the vault or terminal tabs: how a script reaches a host is behind
//! `hostmetrics::manager::HostExec`. Tmux sessions belong to the remote server; closing a tab never touches them.

pub mod manager;
pub mod model;
pub mod ops;
pub mod parse;
pub mod resources;
pub mod script;
pub mod search;

#[cfg(test)]
mod tests;
