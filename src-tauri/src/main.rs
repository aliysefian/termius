// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `sshvault list`, `sshvault run …`: act as a command-line client of the
    // running app instead of opening a window.
    if sshvault_lib::control::is_cli(&args) {
        #[cfg(windows)]
        attach_parent_console();
        std::process::exit(sshvault_lib::control::client_main(&args));
    }
    sshvault_lib::run()
}

/// Release builds are GUI programs on Windows, with no console of their
/// own; print to the terminal the CLI was started from.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: plain Win32 call; failure (no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
