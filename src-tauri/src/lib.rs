mod commands;
mod copilot;
mod db;
mod error;
mod git;
mod sessions;
mod ssh;
mod terminal;
mod tmux;

use std::sync::Arc;

use tauri::Manager;

use crate::copilot::runtime::RemoteCopilotRuntime;
use crate::copilot::session::CopilotAgentBackend;
use crate::db::Database;
use crate::sessions::manager::SessionManager;
use crate::ssh::openssh_tmux::OpenSshTmuxBackend;

pub fn run() {
    if let Err(error) = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
    {
        eprintln!("Could not initialize tracing subscriber: {error}");
    }

    let result = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db = Database::open(&data_dir.join("sessions.sqlite3"))?;
            let sessions = Arc::new(SessionManager {
                db,
                backend: Arc::new(OpenSshTmuxBackend::new()),
            });
            let runtime = Arc::new(RemoteCopilotRuntime::new(Arc::clone(&sessions)));
            app.manage(Arc::new(CopilotAgentBackend::new(
                Arc::clone(&sessions),
                Arc::clone(&runtime),
            )));
            app.manage(runtime);
            app.manage(sessions);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::hosts::list_hosts,
            commands::hosts::list_all_workspaces,
            commands::hosts::add_host,
            commands::hosts::list_workspaces,
            commands::hosts::add_workspace,
            commands::sessions::list_sessions,
            commands::sessions::touch_session,
            commands::changes::list_changes,
            commands::copilot::connect_copilot_host,
            commands::copilot::disconnect_copilot_host,
            commands::copilot::list_port_forwards,
            commands::copilot::pause_chat_tunnel,
            commands::copilot::resume_chat_tunnel,
            commands::copilot::list_mcp_servers,
            commands::copilot::add_mcp_server,
            commands::copilot::import_cli_mcp_server,
            commands::copilot::activate_cli_mcp_server,
            commands::copilot::remove_mcp_server,
            commands::copilot::authenticate_mcp_server,
            commands::copilot::connect_chat,
            commands::copilot::list_recent_cli_chats,
            commands::copilot::attach_cli_chat,
            commands::copilot::create_chat,
            commands::copilot::replace_chat,
            commands::copilot::send_chat_message,
            commands::copilot::abort_chat,
            commands::copilot::chat_health,
            commands::copilot::list_copilot_models,
            commands::copilot::get_copilot_model_state,
            commands::copilot::set_copilot_model,
            commands::copilot::respond_copilot_permission,
            commands::copilot::disconnect_chat,
            commands::copilot::set_copilot_permission_mode,
            commands::copilot::reviewed_mcp_approval_state,
            commands::copilot::set_reviewed_mcp_approval,
            commands::copilot::set_copilot_autopilot,
            commands::create_session::create_session,
            commands::attach_session::attach_session,
            commands::rename_session::rename_session,
            commands::rename_session::pin_session,
            commands::delete_session::delete_session,
            commands::delete_session::forget_session,
            commands::delete_session::restart_session,
            commands::resize_terminal::write_terminal,
            commands::resize_terminal::resize_terminal,
            commands::resize_terminal::sync_terminal_size,
            commands::resize_terminal::enter_scrollback,
            commands::resize_terminal::disconnect_terminal,
            commands::open_url::open_url,
        ])
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                if let Some(manager) = window.app_handle().try_state::<Arc<SessionManager>>() {
                    if let Err(error) = manager.backend.shutdown() {
                        tracing::error!(%error, "Could not close attached PTY on window close");
                    }
                }
                if let Some(runtime) = window.app_handle().try_state::<Arc<RemoteCopilotRuntime>>()
                {
                    let runtime = Arc::clone(runtime.inner());
                    match tauri::async_runtime::block_on(tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        runtime.close_local_tunnels(),
                    )) {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::error!(%error, "Could not close local Copilot tunnels")
                        }
                        Err(_) => tracing::error!("Timed out closing local Copilot tunnels"),
                    }
                }
                if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                    window.app_handle().exit(0);
                }
            }
        })
        .run(tauri::generate_context!());

    if let Err(error) = result {
        tracing::error!(%error, "Application failed");
        eprintln!("Copilot Remote UI could not start: {error}");
        std::process::exit(1);
    }
}
