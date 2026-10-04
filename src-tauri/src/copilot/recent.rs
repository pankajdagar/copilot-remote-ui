use std::collections::HashSet;

use github_copilot_sdk::SessionMetadata;
use serde::Serialize;

use crate::error::{AppError, Result};
use crate::sessions::model::Session;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCliChat {
    pub id: String,
    pub summary: Option<String>,
    pub started_at: String,
    pub modified_at: String,
    pub same_workspace: bool,
    pub is_remote: bool,
    pub linked_to: Option<String>,
}

pub fn summarize(
    sessions: Vec<SessionMetadata>,
    matching_cwd: &HashSet<String>,
    app_sessions: &[Session],
    app_session_id: &str,
) -> Vec<RecentCliChat> {
    let mut recent: Vec<_> = sessions
        .into_iter()
        .map(|session| {
            let id = session.session_id.to_string();
            let linked_to = app_sessions
                .iter()
                .find(|app| app.copilot_session_id.as_deref() == Some(id.as_str()))
                .map(|app| {
                    if app.id == app_session_id {
                        "Current app session".into()
                    } else {
                        app.name.clone()
                    }
                });
            RecentCliChat {
                same_workspace: matching_cwd.contains(&id),
                id,
                summary: session
                    .summary
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(|text| text.chars().take(180).collect()),
                started_at: session.start_time,
                modified_at: session.modified_time,
                is_remote: session.is_remote,
                linked_to,
            }
        })
        .collect();
    recent.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    recent
}

pub fn validate_selection(
    selected_id: &str,
    sessions: &[SessionMetadata],
    matching_cwd: &HashSet<String>,
    app_sessions: &[Session],
    app_session_id: &str,
    allow_different_repo: bool,
) -> Result<()> {
    if selected_id.is_empty()
        || selected_id.len() > 256
        || selected_id.chars().any(char::is_control)
    {
        return Err(AppError::InvalidInput(
            "Select a listed Copilot CLI session".into(),
        ));
    }
    let selected = sessions
        .iter()
        .find(|entry| entry.session_id.as_ref() == selected_id)
        .ok_or_else(|| {
            AppError::NotFound(
                "This Copilot CLI session is no longer in the saved sessions on this remote host"
                    .into(),
            )
        })?;
    if selected.is_remote {
        return Err(AppError::InvalidInput(
            "Cloud-run Copilot sessions cannot be attached through this SSH host".into(),
        ));
    }
    if app_sessions.iter().any(|app| {
        app.id == app_session_id && app.copilot_session_id.as_deref() == Some(selected_id)
    }) {
        return Err(AppError::InvalidInput(
            "This Copilot chat is already linked; use Reconnect Copilot instead".into(),
        ));
    }
    if let Some(app) = app_sessions.iter().find(|app| {
        app.id != app_session_id && app.copilot_session_id.as_deref() == Some(selected_id)
    }) {
        return Err(AppError::InvalidInput(format!(
            "This Copilot conversation is already linked to {}",
            app.name
        )));
    }
    if !matching_cwd.contains(selected_id) && !allow_different_repo {
        return Err(AppError::InvalidInput(
            "This CLI session was not saved in the selected repository. Confirm that it may be resumed in this workspace before attaching it.".into()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::model::{Host, Workspace};

    fn entry(id: &str, modified: &str, remote: bool) -> SessionMetadata {
        serde_json::from_value(serde_json::json!({
            "sessionId": id,
            "startTime": "2026-09-25T00:00:00Z",
            "modifiedTime": modified,
            "summary": "Fix the callback",
            "isRemote": remote
        }))
        .expect("valid Copilot session metadata")
    }

    #[test]
    fn orders_and_labels_remote_cli_sessions_without_changing_identity() {
        let host = Host {
            id: "host-1".into(),
            name: "remote host".into(),
            ssh_host: Some("alias".into()),
            created_at: "".into(),
        };
        let app = Session {
            id: "other".into(),
            workspace_id: "workspace-1".into(),
            name: "Already in this app".into(),
            tmux_session_name: "crui_other".into(),
            copilot_session_id: Some("cli-2".into()),
            copilot_has_messages: true,
            command: "bash".into(),
            pinned: false,
            created_at: "".into(),
            last_opened_at: None,
            workspace: Workspace {
                id: "workspace-1".into(),
                host_id: "host-1".into(),
                repo_path: "/repo".into(),
                display_name: "Repo".into(),
                created_at: "".into(),
            },
            host,
        };
        let recent = summarize(
            vec![
                entry("cli-1", "2026-09-25T01:00:00Z", false),
                entry("cli-2", "2026-09-25T02:00:00Z", false),
                entry("cloud", "2026-09-25T03:00:00Z", true),
            ],
            &HashSet::from(["cli-1".into()]),
            &[app],
            "current",
        );
        assert_eq!(
            recent
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["cloud", "cli-2", "cli-1"]
        );
        assert!(!recent[0].same_workspace && recent[0].is_remote);
        assert_eq!(recent[1].linked_to.as_deref(), Some("Already in this app"));
        assert!(recent[2].same_workspace);
    }

    #[test]
    fn selection_requires_same_repository_or_explicit_confirmation() {
        let list = [
            entry("same", "2026-09-25T01:00:00Z", false),
            entry("other", "2026-09-25T02:00:00Z", false),
            entry("cloud", "2026-09-25T03:00:00Z", true),
        ];
        let matching = HashSet::from(["same".into()]);
        let args = (&list[..], &matching, &[][..], "app");
        assert!(validate_selection("same", args.0, args.1, args.2, args.3, false).is_ok());
        assert!(validate_selection("other", args.0, args.1, args.2, args.3, false).is_err());
        assert!(validate_selection("other", args.0, args.1, args.2, args.3, true).is_ok());
        assert!(validate_selection("cloud", args.0, args.1, args.2, args.3, true).is_err());
        assert!(validate_selection("unknown", args.0, args.1, args.2, args.3, true).is_err());
    }

    #[test]
    fn does_not_replace_current_link_by_reselecting_the_same_cli_session() {
        let host = Host {
            id: "host-1".into(),
            name: "remote host".into(),
            ssh_host: Some("alias".into()),
            created_at: "".into(),
        };
        let app = Session {
            id: "current".into(),
            workspace_id: "workspace-1".into(),
            name: "Current".into(),
            tmux_session_name: "crui_current".into(),
            copilot_session_id: Some("cli-current".into()),
            copilot_has_messages: true,
            command: "bash".into(),
            pinned: false,
            created_at: "".into(),
            last_opened_at: None,
            workspace: Workspace {
                id: "workspace-1".into(),
                host_id: "host-1".into(),
                repo_path: "/repo".into(),
                display_name: "Repo".into(),
                created_at: "".into(),
            },
            host,
        };
        let list = [entry("cli-current", "2026-09-25T01:00:00Z", false)];
        let matching = HashSet::from(["cli-current".into()]);
        let items = summarize(
            list.to_vec(),
            &matching,
            std::slice::from_ref(&app),
            "current",
        );
        assert_eq!(items[0].linked_to.as_deref(), Some("Current app session"));
        assert!(
            validate_selection("cli-current", &list, &matching, &[app], "current", false).is_err()
        );
    }

    #[test]
    fn does_not_drop_older_cli_conversations_from_searchable_results() {
        let sessions: Vec<_> = (0..220)
            .map(|index| {
                entry(
                    &format!("cli-{index}"),
                    &format!("2026-09-25T{:02}:{:02}:00Z", index / 60, index % 60),
                    false,
                )
            })
            .collect();
        let recent = summarize(sessions, &HashSet::new(), &[], "app");
        assert_eq!(recent.len(), 220);
        assert!(recent.iter().any(|item| item.id == "cli-0"));
    }
}
