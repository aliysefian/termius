//! SSHVault: zero-knowledge, folder-synced SSH manager.

pub mod commands;
pub mod config;
pub mod crypto;
pub mod forward;
pub mod models;
pub mod session;
pub mod sftp;
pub mod ssh;
pub mod sync;
pub mod vault;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(commands::setup)
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::set_vault_path,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::change_master_password,
            commands::list_hosts,
            commands::save_host,
            commands::delete_host,
            commands::list_identities,
            commands::save_identity,
            commands::delete_identity,
            commands::list_snippets,
            commands::save_snippet,
            commands::delete_snippet,
            commands::ssh_connect,
            commands::ssh_write,
            commands::ssh_resize,
            commands::ssh_disconnect,
            commands::sftp_open,
            commands::sftp_list,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_close,
            commands::local_home,
            commands::local_list,
            commands::local_mkdir,
            commands::local_rename,
            commands::local_remove,
            commands::transfer_start,
            commands::transfer_cancel,
            commands::list_forwards,
            commands::save_forward,
            commands::delete_forward,
            commands::forward_start,
            commands::forward_stop,
            commands::forward_statuses,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
