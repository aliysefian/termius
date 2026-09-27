//! SSHVault: zero-knowledge, folder-synced SSH manager.

#[cfg(test)]
mod acceptance_tests;
pub mod agent;
pub mod ansible;
pub mod commands;
pub mod config;
pub mod control;
pub mod crypto;
pub mod csvimport;
pub mod dial;
pub mod forward;
pub mod health;
pub mod hostkeys;
pub mod hostcreds;
pub mod keychain;
pub mod keymanager;
pub mod keys;
pub mod knownhosts;
pub mod localpty;
pub mod models;
pub mod mobaxterm;
pub mod putty;
pub mod rawterm;
pub mod remoteedit;
pub mod reveal;
pub mod runner;
pub mod session;
pub mod sessionlog;
pub mod sftp;
pub mod ssh;
pub mod sshconfig;
pub mod sync;
pub mod vault;
pub mod x11;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .setup(commands::setup)
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::set_vault_path,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::change_master_password,
            commands::unlock_with_device,
            commands::unlock_with_recovery,
            commands::forget_device,
            commands::remember_device,
            commands::set_recovery_key,
            commands::remove_recovery_key,
            commands::vault_info,
            commands::list_backups,
            commands::create_backup,
            commands::restore_backup,
            commands::verify_integrity,
            commands::list_conflicts,
            commands::resolve_conflict,
            commands::move_vault,
            commands::list_hosts,
            commands::save_host,
            commands::save_host_with_credentials,
            commands::delete_host,
            commands::undelete_record,
            commands::list_identities,
            commands::reveal_identity,
            commands::reveal_close,
            commands::identity_public_key,
            commands::generate_key,
            commands::list_keys,
            commands::generate_ssh_key,
            commands::import_private_key,
            commands::import_private_key_file,
            commands::import_public_key,
            commands::update_key,
            commands::change_key_passphrase,
            commands::key_usage,
            commands::delete_key,
            commands::export_private_key,
            commands::ssh_connect_adhoc,
            commands::known_hosts_list,
            commands::known_hosts_import,
            commands::sftp_chmod,
            commands::sftp_preview,
            commands::local_preview,
            commands::read_text_file,
            commands::csv_preview,
            commands::putty_sessions,
            commands::mobaxterm_preview,
            commands::list_groups,
            commands::save_group,
            commands::delete_group,
            commands::list_proxies,
            commands::save_proxy,
            commands::delete_proxy,
            commands::list_workspaces,
            commands::save_workspace,
            commands::delete_workspace,
            commands::get_vault_settings,
            commands::save_vault_settings,
            commands::answer_host_key,
            commands::known_hosts_forget,
            commands::ssh_config_preview,
            commands::ssh_config_import,
            commands::save_identity,
            commands::delete_identity,
            commands::list_snippets,
            commands::save_snippet,
            commands::delete_snippet,
            commands::ssh_connect,
            commands::ssh_write,
            commands::ssh_resize,
            commands::ssh_disconnect,
            commands::local_spawn,
            commands::local_write,
            commands::local_resize,
            commands::local_close,
            commands::raw_telnet,
            commands::raw_serial,
            commands::raw_write,
            commands::raw_resize,
            commands::raw_close,
            commands::serial_ports,
            commands::check_hosts,
            commands::ansible_preview,
            commands::export_ssh_config,
            commands::ssh_log_start,
            commands::ssh_log_stop,
            commands::run_on_hosts,
            commands::run_cancel,
            commands::sftp_edit_start,
            commands::sftp_edit_stop,
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
            commands::transfer_pause,
            commands::agent_status,
            commands::cli_status,
            commands::updater_info,
            commands::cli_set_enabled,
            commands::answer_cli_request,
            commands::agent_set_enabled,
            commands::answer_agent_request,
            commands::set_key_agent,
            commands::transfer_resume,
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
