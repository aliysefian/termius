//! SSHVault: zero-knowledge, folder-synced SSH manager.

#[cfg(test)]
mod acceptance_tests;
pub mod agent;
pub mod algorithms;
pub mod ansible;
pub mod certs;
pub mod commands;
pub mod completion;
pub mod config;
pub mod containers;
pub mod control;
pub mod crashlog;
pub mod crypto;
pub mod deadline;
pub mod exportfile;
pub mod idle;
pub mod csvimport;
pub mod db;
pub mod display;
pub mod dial;
pub mod files;
pub mod forward;
pub mod health;
pub mod hooks;
pub mod hostkeys;
pub mod inventory;
pub mod hostcreds;
pub mod keychain;
pub mod kbdint;
pub mod mask;
pub mod neterr;
pub mod keymanager;
pub mod proxyapproval;
pub mod kube;
pub mod keys;
pub mod knownhosts;
pub mod localpty;
pub mod localshells;
pub mod mosh;
pub mod models;
pub mod monitor;
pub mod mobaxterm;
pub mod putty;
pub mod rawterm;
pub mod rdp;
pub mod release;
pub mod remoteedit;
pub mod reveal;
pub mod runbook;
pub mod runbookhistory;
pub mod runbookrun;
pub mod runner;
pub mod selfupdate;
pub mod session;
pub mod sessionlog;
pub mod sftp;
pub mod spawnlint;
pub mod ssh;
pub mod sshconfig;
pub mod sync;
pub mod unlockguard;
pub mod vault;
pub mod vnc;
pub mod wol;
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
            commands::set_keep_key_on_lock,
            commands::crash_log,
            commands::clear_crash_log,
            commands::vault_activity,
            commands::accept_rollbacks,
            commands::vault_health,
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
            commands::test_proxy,
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
            commands::completion_lookup,
            commands::completion_lookup_local,
            commands::ssh_resize,
            commands::ssh_disconnect,
            commands::local_spawn,
            commands::local_shells,
            commands::mosh_available,
            commands::mosh_connect,
            commands::rdp_connect,
            commands::rdp_input,
            commands::rdp_close,
            commands::vnc_connect,
            commands::vnc_input,
            commands::vnc_close,
            commands::wake_on_lan,
            commands::run_hook,
            commands::runbooks::list_runbooks,
            commands::runbooks::save_runbook,
            commands::runbooks::delete_runbook,
            commands::runbooks::runbook_check,
            commands::runbooks::runbook_plan,
            commands::runbooks::runbook_start,
            commands::runbooks::runbook_cancel,
            commands::runbooks::runbook_history_list,
            commands::runbooks::runbook_history_get,
            commands::runbooks::runbook_history_delete,
            commands::runbooks::runbook_history_clear,
            commands::inventory_run,
            commands::inventory_scan,
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
            commands::list_db_connections,
            commands::save_db_connection,
            commands::delete_db_connection,
            commands::db_open,
            commands::db_test,
            commands::db_close,
            commands::db_query,
            commands::db_cancel,
            commands::db_children,
            commands::db_table_info,
            commands::db_preview_update,
            commands::db_apply_update,
            commands::db_save_export,
            commands::containers_open,
            commands::containers_close,
            commands::kube_open,
            commands::kube_close,
            commands::kube_pods,
            commands::kube_resources,
            commands::kube_describe_resource,
            commands::kube_namespaces,
            commands::kube_describe,
            commands::kube_delete_pod,
            commands::kube_logs_start,
            commands::kube_forward_start,
            commands::kube_stop,
            commands::export_text_file,
            commands::containers_list,
            commands::containers_act,
            commands::containers_inspect,
            commands::containers_logs_start,
            commands::containers_logs_stop,
            commands::containers_stats,
            commands::containers_resources,
            commands::containers_remove,
            commands::containers_prune_preview,
            commands::containers_prune_run,
            commands::containers_pull,
            commands::containers_compose,
            commands::monitor_open,
            commands::monitor_exec,
            commands::monitor_close,
            commands::monitor_stream_start,
            commands::monitor_stream_stop,
            commands::sftp_edit_start,
            commands::sftp_edit_stop,
            commands::sftp_open,
            commands::scp_open,
            commands::ftp_open,
            commands::local_home,
            commands::files_caps,
            commands::files_home,
            commands::files_list,
            commands::files_mkdir,
            commands::files_rename,
            commands::files_remove,
            commands::files_chmod,
            commands::files_preview,
            commands::files_close,
            commands::files_transfer_start,
            commands::transfer_cancel,
            commands::transfer_pause,
            commands::agent_status,
            commands::cli_status,
            commands::updater_info,
            selfupdate::release_check,
            selfupdate::release_install,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Quitting is a lock too: it releases this device's "open here" marker in the synced folder (otherwise other
            // devices show a session that is gone), writes down what this device has seen of it, and drops the key.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = tauri::Manager::try_state::<commands::AppState>(app) {
                    state.session.lock();
                }
            }
        });
}
