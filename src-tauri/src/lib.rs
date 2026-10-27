mod commands;
mod docker;
mod flow;
mod hostinfo;
mod k8s;
mod k8s_forward;
mod k8s_term;
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
    // Plugins `exec` do kubeconfig (aws, gke-gcloud-auth-plugin, kubelogin…) sem abrir uma
    // janela de console a cada token: o app não tem console para herdar.
    #[cfg(windows)]
    std::env::set_var("KUBE_RS_UNSTABLE_CREATE_NO_WINDOW", "1");

    tauri::Builder::default()
        .manage(session::SessionManager::default())
        .manage(sftp::Transfers::default())
        .manage(tunnel::Tunnels::default())
        .manage(k8s::Kube::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            commands::k8s_contexts,
            commands::k8s_namespaces,
            commands::k8s_list,
            commands::k8s_yaml,
            commands::k8s_action,
            commands::k8s_containers,
            commands::k8s_ports,
            commands::k8s_terminal,
            commands::k8s_forward_start,
            commands::k8s_forward_stop,
            commands::k8s_forwards,
            commands::k8s_apply,
            commands::k8s_metrics,
            commands::k8s_custom_resources,
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
