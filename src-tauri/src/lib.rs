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
pub mod tray;
pub mod unlockguard;
pub mod vault;
pub mod vnc;
pub mod wol;
pub mod x11;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rustls is built with both ring and aws-lc-rs (via reqwest and the updater), so it can't choose a default on its
    // own and panics at the first TLS handshake that doesn't install one: RDP did exactly that. Pick ring up front.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tauri::Builder::default()
        // First, so a second launch hands over to this one (and shows its window) before setting anything up. With
        // the window closed into the tray, launching the app again is how many people will look for it.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| commands::tray::show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Everything but visibility: quitting from the tray while the window is hidden must not make the next start
        // open hidden.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::all() & !tauri_plugin_window_state::StateFlags::VISIBLE)
                .build(),
        )
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
            commands::updates::undelete_record,
            commands::list_identities,
            commands::reveal_identity,
            commands::reveal_close,
            commands::identity_public_key,
            commands::generate_key,
            commands::keys::list_keys,
            commands::keys::generate_ssh_key,
            commands::keys::import_private_key,
            commands::keys::import_private_key_file,
            commands::keys::import_public_key,
            commands::keys::update_key,
            commands::keys::change_key_passphrase,
            commands::keys::key_usage,
            commands::keys::delete_key,
            commands::keys::export_private_key,
            commands::runhosts::ssh_connect_adhoc,
            commands::knownhosts::known_hosts_list,
            commands::knownhosts::known_hosts_import,
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
            commands::monitoring::list_workspaces,
            commands::monitoring::save_workspace,
            commands::monitoring::delete_workspace,
            commands::monitoring::get_vault_settings,
            commands::monitoring::save_vault_settings,
            commands::knownhosts::answer_host_key,
            commands::knownhosts::known_hosts_forget,
            commands::sshconfig::ssh_config_preview,
            commands::sshconfig::ssh_config_import,
            commands::keys::save_identity,
            commands::keys::delete_identity,
            commands::keys::list_snippets,
            commands::keys::save_snippet,
            commands::keys::delete_snippet,
            commands::ssh_connect,
            commands::ssh_write,
            commands::completion_lookup,
            commands::completion_lookup_local,
            commands::ssh_resize,
            commands::ssh_disconnect,
            commands::localterm::local_spawn,
            commands::localterm::local_shells,
            commands::localterm::mosh_available,
            commands::localterm::mosh_connect,
            commands::rdp::rdp_connect,
            commands::rdp::rdp_input,
            commands::rdp::rdp_close,
            commands::rdp::vnc_connect,
            commands::rdp::vnc_input,
            commands::rdp::vnc_close,
            commands::rdp::wake_on_lan,
            commands::rdp::run_hook,
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
            commands::rdp::inventory_run,
            commands::rdp::inventory_scan,
            commands::rawconsole::local_write,
            commands::rawconsole::local_resize,
            commands::rawconsole::local_close,
            commands::rawconsole::raw_telnet,
            commands::rawconsole::raw_serial,
            commands::rawconsole::raw_write,
            commands::rawconsole::raw_resize,
            commands::rawconsole::raw_close,
            commands::rawconsole::serial_ports,
            commands::check_hosts,
            commands::ansible_preview,
            commands::export_ssh_config,
            commands::ssh_log_start,
            commands::ssh_log_stop,
            commands::runhosts::run_on_hosts,
            commands::runhosts::run_cancel,
            commands::databases::list_db_connections,
            commands::databases::save_db_connection,
            commands::databases::delete_db_connection,
            commands::databases::db_open,
            commands::databases::db_test,
            commands::databases::db_close,
            commands::databases::db_query,
            commands::databases::db_cancel,
            commands::databases::db_children,
            commands::databases::db_table_info,
            commands::databases::db_preview_update,
            commands::databases::db_apply_update,
            commands::databases::db_save_export,
            commands::containers::containers_open,
            commands::containers::containers_close,
            commands::containers::kube_open,
            commands::containers::kube_close,
            commands::containers::kube_pods,
            commands::containers::kube_resources,
            commands::containers::kube_describe_resource,
            commands::containers::kube_namespaces,
            commands::containers::kube_describe,
            commands::containers::kube_delete_pod,
            commands::containers::kube_logs_start,
            commands::containers::kube_forward_start,
            commands::containers::kube_stop,
            commands::export_text_file,
            commands::containers::containers_list,
            commands::containers::containers_act,
            commands::containers::containers_inspect,
            commands::containers::containers_logs_start,
            commands::containers::containers_logs_stop,
            commands::containers::containers_stats,
            commands::containers::containers_resources,
            commands::containers::containers_remove,
            commands::containers::containers_prune_preview,
            commands::containers::containers_prune_run,
            commands::containers::containers_pull,
            commands::containers::containers_compose,
            commands::monitoring::monitor_open,
            commands::monitoring::monitor_exec,
            commands::monitoring::monitor_close,
            commands::monitoring::monitor_stream_start,
            commands::monitoring::monitor_stream_stop,
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
            commands::updates::updater_info,
            commands::tray::tray_available,
            commands::tray::window_hide_to_tray,
            commands::tray::window_show,
            commands::tray::app_exit_ack,
            commands::tray::app_exit,
            selfupdate::release_check,
            selfupdate::release_install,
            commands::cli_set_enabled,
            commands::answer_cli_request,
            commands::agent_set_enabled,
            commands::answer_agent_request,
            commands::set_key_agent,
            commands::transfer_resume,
            commands::forwarding::list_forwards,
            commands::forwarding::save_forward,
            commands::forwarding::delete_forward,
            commands::forwarding::forward_start,
            commands::forwarding::forward_stop,
            commands::forwarding::forward_statuses,
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
            // macOS: clicking the Dock icon of an app whose window is hidden brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                commands::tray::show_main(app);
            }
        });
}
