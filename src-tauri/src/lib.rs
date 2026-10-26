mod commands;
mod docker;
mod flow;
mod hostinfo;
mod keyhook;
mod local;
mod rdp;
mod rdp_clipboard;
mod session;
mod sftp;
mod shells;
mod sshconfig;
mod ssh;
mod tunnel;
mod vault;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(session::SessionManager::default())
        .manage(sftp::Transfers::default())
        .manage(tunnel::Tunnels::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            app.manage(vault::Vault::open(&dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_shells,
            commands::pty_spawn,
            commands::ssh_connect,
            commands::rdp_connect,
            commands::rdp_input,
            commands::rdp_capture,
            commands::hosts_list,
            commands::host_save,
            commands::host_delete,
            commands::credentials_list,
            commands::credential_save,
            commands::credential_delete,
            commands::credential_generate_key,
            commands::vault_export,
            commands::vault_import,
            commands::settings_get,
            commands::settings_set,
            commands::ssh_config_scan,
            commands::ssh_config_import,
            commands::session_write,
            commands::session_ack,
            commands::session_send_secret,
            commands::session_resize,
            commands::session_kill,
            commands::host_metrics,
            commands::docker_list,
            commands::docker_action,
            commands::sftp_home,
            commands::sftp_list,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_download,
            commands::sftp_upload,
            commands::transfer_cancel,
            commands::download_dir,
            commands::tunnels_list,
            commands::tunnel_save,
            commands::tunnel_delete,
            commands::tunnel_start,
            commands::tunnel_stop,
            commands::tunnel_statuses,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Nexo");
}
