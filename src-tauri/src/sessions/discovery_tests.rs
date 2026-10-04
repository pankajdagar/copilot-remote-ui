use super::*;
use crate::db::Database;
use crate::sessions::repository;

#[test]
fn imports_ssh_hosts_and_vscode_folders_without_overwriting_custom_names() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let home = directory.path();
    std::fs::create_dir_all(home.join(".ssh"))?;
    std::fs::write(
        home.join(".ssh/config"),
        "Host alpha\n  HostName server.example.com\n\
         Host beta\n  Port 2222\nHost *\n  User coder\n",
    )?;
    let storage_path = vscode_storage(home);
    std::fs::create_dir_all(storage_path.parent().ok_or_else(|| {
        crate::error::AppError::InvalidInput("Storage directory missing".into())
    })?)?;
    let legacy = hex::encode(r#"{"hostName":"alpha","user":"coder"}"#);
    let storage = serde_json::json!({
        "backupWorkspaces": {
            "folders": [
                { "folderUri": "vscode-remote://ssh-remote%2Balpha/home/coder/main%20repo" },
                { "folderUri": "vscode-remote://ssh-remote%2Bunknown_machine/home/coder/other" },
                { "folderUri": "vscode-remote://ssh-remote%2Bbeta/home/coder/.copilot/files" }
            ]
        },
        "profileAssociations": {
            "workspaces": {
                format!("vscode-remote://ssh-remote%2B{legacy}/home/coder/other-repo"): "__default__profile__"
            }
        }
    });
    std::fs::write(
        &storage_path,
        serde_json::to_vec(&storage).map_err(anyhow::Error::from)?,
    )?;
    let db = Database::open(&home.join("app/sessions.sqlite3"))?;
    sync(&mut *db.connection()?, home)?;

    let hosts = repository::list_hosts(&*db.connection()?)?;
    assert_eq!(hosts.len(), 3);
    let alpha = hosts
        .iter()
        .find(|host| host.ssh_host.as_deref() == Some("alpha"))
        .ok_or_else(|| crate::error::AppError::NotFound("Discovered host missing".into()))?;
    assert_eq!(alpha.name, "alpha");
    let workspaces = repository::list_all_workspaces(&*db.connection()?)?;
    assert_eq!(workspaces.len(), 2);
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.repo_path == "/home/coder/main repo"));
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.repo_path == "/home/coder/other-repo"));

    repository::add_workspace(
        &*db.connection()?,
        &alpha.id,
        "/home/coder/manual",
        "My repo",
    )?;
    db.connection()?.execute(
        "UPDATE hosts SET name = 'My host' WHERE id = ?1",
        [&alpha.id],
    )?;
    sync(&mut *db.connection()?, home)?;
    let hosts = repository::list_hosts(&*db.connection()?)?;
    assert_eq!(hosts.len(), 3);
    assert_eq!(
        repository::get_host(&*db.connection()?, &alpha.id)?.name,
        "My host"
    );
    assert_eq!(
        repository::list_all_workspaces(&*db.connection()?)?.len(),
        3
    );
    Ok(())
}

#[test]
fn absent_ssh_config_does_not_change_existing_metadata() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let db = Database::open(&directory.path().join("sessions.sqlite3"))?;
    sync(&mut *db.connection()?, directory.path())?;
    assert_eq!(repository::list_hosts(&*db.connection()?)?.len(), 1);
    Ok(())
}

#[test]
fn discovers_all_literal_aliases_and_skips_patterns_comments_and_options() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let home = directory.path();
    std::fs::create_dir_all(home.join(".ssh"))?;
    std::fs::write(
        home.join(".ssh/config"),
        "# Host ignored\nHost = primary secondary *.example.com !excluded\n\
         HOST=third # ignored-alias\nHost \"fourth\"\nHost * dev-? invalid/alias -option\n\
         HostName hostname-only.example.com\nMatch exec \"do-not-run\"\n  User coder\n",
    )?;
    assert_eq!(
        configured_hosts(home)?,
        ["fourth", "primary", "secondary", "third"]
    );
    Ok(())
}

#[test]
fn follows_quoted_glob_home_and_absolute_includes_without_include_cycles() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let home = directory.path();
    let ssh = home.join(".ssh");
    std::fs::create_dir_all(ssh.join("conf.d"))?;
    std::fs::write(
        ssh.join("config"),
        format!(
            "Include conf.d/*.conf \"extra hosts.conf\" ~/more.conf \"{}\" missing-*.conf\nHost main\n",
            home.join("absolute.conf").display()
        ),
    )?;
    std::fs::write(
        ssh.join("conf.d/first.conf"),
        "Host included\nInclude conf.d/nested.conf\n",
    )?;
    std::fs::write(
        ssh.join("conf.d/nested.conf"),
        "Host nested included\nInclude config\n",
    )?;
    std::fs::write(ssh.join("extra hosts.conf"), "Host quoted\n")?;
    std::fs::write(home.join("more.conf"), "Host home-alias\n")?;
    std::fs::write(home.join("absolute.conf"), "Host absolute\n")?;
    assert_eq!(
        configured_hosts(home)?,
        [
            "absolute",
            "home-alias",
            "included",
            "main",
            "nested",
            "quoted"
        ]
    );
    Ok(())
}

#[test]
fn discovery_preserves_saved_host_workspace_and_session_identity() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let home = directory.path();
    std::fs::create_dir_all(home.join(".ssh"))?;
    std::fs::write(home.join(".ssh/config"), "Host saved-alias\n")?;
    let db = Database::open(&home.join("sessions.sqlite3"))?;
    let host = repository::add_host(&*db.connection()?, "My existing host", "saved-alias")?;
    let workspace =
        repository::add_workspace(&*db.connection()?, &host.id, "/home/coder/repo", "My repo")?;
    let session = repository::add_session(
        &*db.connection()?,
        &workspace.id,
        "My task",
        "legacy_session_01",
        "bash",
    )?;
    repository::set_copilot_session_id(&*db.connection()?, &session.id, "saved-cli-chat")?;
    repository::pin(&*db.connection()?, &session.id, true)?;
    sync(&mut *db.connection()?, home)?;
    std::fs::write(home.join(".ssh/config"), "Host new-alias\n")?;
    sync(&mut *db.connection()?, home)?;
    assert_eq!(
        repository::get_host(&*db.connection()?, &host.id)?.name,
        "My existing host"
    );
    let saved = repository::get_session(&*db.connection()?, &session.id)?;
    assert_eq!(saved.workspace.id, workspace.id);
    assert_eq!(saved.host.id, host.id);
    assert_eq!(saved.name, "My task");
    assert_eq!(saved.tmux_session_name, "legacy_session_01");
    assert_eq!(saved.copilot_session_id.as_deref(), Some("saved-cli-chat"));
    assert!(saved.pinned);
    Ok(())
}

#[test]
fn malformed_includes_and_excessive_depth_report_errors_before_importing() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let home = directory.path();
    let ssh = home.join(".ssh");
    std::fs::create_dir_all(&ssh)?;
    std::fs::write(ssh.join("config"), "Host valid\nInclude \"unterminated\n")?;
    assert!(configured_hosts(home).is_err());
    std::fs::write(ssh.join("config"), "Include level-0.conf\n")?;
    for index in 0..18 {
        std::fs::write(
            ssh.join(format!("level-{index}.conf")),
            format!("Host alias-{index}\nInclude level-{}.conf\n", index + 1),
        )?;
    }
    assert!(configured_hosts(home).is_err());
    Ok(())
}
