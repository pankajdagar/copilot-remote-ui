use std::time::{Duration, Instant};

use tauri::ipc::Channel;

use super::*;

#[test]
#[ignore = "requires local tmux"]
fn cached_clients_disconnect_independently_and_leave_both_sessions_running() -> Result<()> {
    let socket = format!("crui_cache_test_{}", ulid::Ulid::new());
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .output();
        }
    }
    let _cleanup = Cleanup(socket.clone());
    let slot = TerminalSlot(Mutex::new(HashMap::new()));
    let names = ["crui_cached_a", "crui_cached_b"];
    for name in names {
        let created = std::process::Command::new("tmux")
            .args([
                "-L",
                &socket,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                name,
                "--",
                "cat",
            ])
            .status()?;
        assert!(created.success());
        let channel = Channel::new(|_| Ok(()));
        let args = vec![
            "tmux".into(),
            "-L".into(),
            socket.clone(),
            "attach-session".into(),
            "-t".into(),
            format!("={name}"),
        ];
        let pty = PtyConnection::spawn(None, &args, 100, 30, channel)?;
        slot.0
            .lock()
            .map_err(|_| AppError::InvalidInput("Test lock".into()))?
            .insert(
                name.into(),
                ActiveTerminal {
                    connection_id: name.into(),
                    pty,
                },
            );
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let clients = std::process::Command::new("tmux")
            .args(["-L", &socket, "list-clients", "-F", "#{client_session}"])
            .output()?;
        if clients.status.success() && String::from_utf8_lossy(&clients.stdout).lines().count() == 2
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(slot.connected_session_ids()?.len(), 2);
    slot.with_connection(names[0], names[0], |pty| pty.resize(120, 40))?;
    slot.disconnect(names[0], names[0])?;
    assert_eq!(
        slot.connected_session_ids()?,
        HashSet::from([names[1].to_owned()])
    );
    slot.shutdown()?;
    assert!(slot.connected_session_ids()?.is_empty());
    for name in names {
        let surviving = std::process::Command::new("tmux")
            .args(["-L", &socket, "has-session", "-t", &format!("={name}")])
            .status()?;
        assert!(
            surviving.success(),
            "Cached client shutdown killed tmux {name}"
        );
    }
    Ok(())
}
