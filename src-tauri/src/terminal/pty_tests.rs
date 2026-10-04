use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::ipc::{Channel, InvokeResponseBody};

use super::*;

struct TestTmuxSocket(String);

impl Drop for TestTmuxSocket {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

#[test]
#[ignore = "requires local tmux"]
fn pty_stream_resize_and_disconnect_leave_tmux_alive() -> Result<()> {
    let socket = format!("crui_pty_test_{}", ulid::Ulid::new());
    let name = format!("crui_{}", ulid::Ulid::new());
    let _cleanup = TestTmuxSocket(socket.clone());
    let status = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            &name,
            "-x",
            "100",
            "-y",
            "30",
            "--",
            "sh",
            "-c",
            "seq 1 200; exec cat",
        ])
        .status()?;
    assert!(status.success());
    let policy_command = crate::tmux::commands::interaction_policy(&name);
    let policy = std::process::Command::new("tmux")
        .args(["-L", &socket])
        .args(&policy_command[1..])
        .status()?;
    assert!(policy.success(), "Could not configure app-owned session");

    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = Arc::clone(&output);
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            let value: serde_json::Value = serde_json::from_str(&json)?;
            if value["kind"] == "output" {
                let mut buffer = sink.lock().expect("test buffer lock");
                for byte in value["bytes"].as_array().expect("byte array") {
                    buffer.push(byte.as_u64().expect("byte") as u8);
                }
            }
        }
        Ok(())
    });
    let args = vec![
        "tmux".into(),
        "-L".into(),
        socket.clone(),
        "attach-session".into(),
        "-t".into(),
        format!("={name}"),
    ];
    let mut pty = PtyConnection::spawn(None, &args, 100, 30, channel)?;
    pty.resize(120, 40)?;
    pty.writer.write_all(b"remote-pty-marker\n")?;
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if output
            .lock()
            .expect("test buffer lock")
            .windows(b"remote-pty-marker".len())
            .any(|window| window == b"remote-pty-marker")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        output
            .lock()
            .expect("test buffer lock")
            .windows(b"remote-pty-marker".len())
            .any(|window| window == b"remote-pty-marker"),
        "PTY output did not reach the channel"
    );
    assert!(
        output
            .lock()
            .expect("test buffer lock")
            .windows(b"?1006h".len())
            .any(|window| window == b"?1006h"),
        "tmux did not enable SGR mouse reporting on the terminal"
    );
    let size = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "display-message",
            "-p",
            "-t",
            &format!("={name}:0"),
            "#{window_width}x#{window_height}",
        ])
        .output()?;
    assert!(size.status.success());
    assert_eq!(String::from_utf8_lossy(&size.stdout).trim(), "120x40");

    let split = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "split-window",
            "-h",
            "-t",
            &format!("={name}:0"),
            "--",
            "sh",
            "-c",
            "seq 1 200; exec cat",
        ])
        .status()?;
    assert!(split.success());
    for (column, expected_pane) in [(10, "0:1"), (80, "1:1")] {
        pty.writer
            .write_all(format!("\x1b[<0;{column};10M\x1b[<0;{column};10m").as_bytes())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut selected = false;
        while Instant::now() < deadline {
            let panes = std::process::Command::new("tmux")
                .args([
                    "-L",
                    &socket,
                    "list-panes",
                    "-t",
                    &format!("={name}:0"),
                    "-F",
                    "#{pane_index}:#{pane_active}",
                ])
                .output()?;
            if panes.status.success()
                && String::from_utf8_lossy(&panes.stdout)
                    .lines()
                    .any(|line| line == expected_pane)
            {
                selected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            selected,
            "Mouse click did not select tmux pane {expected_pane}"
        );
    }

    pty.writer.write_all(b"\x1b[<64;80;10M")?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut copy_mode = false;
    while Instant::now() < deadline {
        let status = std::process::Command::new("tmux")
            .args([
                "-L",
                &socket,
                "display-message",
                "-p",
                "-t",
                &format!("={name}:0"),
                "#{pane_in_mode}",
            ])
            .output()?;
        if status.status.success() && String::from_utf8_lossy(&status.stdout).trim() == "1" {
            copy_mode = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(copy_mode, "Mouse wheel did not enter tmux scrollback");
    let cancel = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "send-keys",
            "-t",
            &format!("={name}:0"),
            "-X",
            "cancel",
        ])
        .status()?;
    assert!(cancel.success());
    let scrollback_command = crate::tmux::commands::scrollback(&name);
    let scrollback = std::process::Command::new("tmux")
        .args(["-L", &socket])
        .args(&scrollback_command[1..])
        .status()?;
    assert!(scrollback.success());
    let mode = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "display-message",
            "-p",
            "-t",
            &format!("={name}:0"),
            "#{pane_in_mode}",
        ])
        .output()?;
    assert_eq!(String::from_utf8_lossy(&mode.stdout).trim(), "1");
    let position = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "display-message",
            "-p",
            "-t",
            &format!("={name}:0"),
            "#{scroll_position}",
        ])
        .output()?;
    let lines: u16 = String::from_utf8_lossy(&position.stdout)
        .trim()
        .parse()
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!("Invalid tmux scroll position: {error}"))
        })?;
    assert!(lines > 0, "Scrollback did not move up through tmux history");

    let wrong_size = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "resize-window",
            "-t",
            &format!("={name}:0"),
            "-x",
            "50",
            "-y",
            "10",
        ])
        .status()?;
    assert!(wrong_size.success());
    pty.force_resize(120, 40)?;
    let restore_size = std::process::Command::new("tmux")
        .args(["-L", &socket])
        .args(&policy_command[1..])
        .status()?;
    assert!(restore_size.success());
    let restored = std::process::Command::new("tmux")
        .args([
            "-L",
            &socket,
            "display-message",
            "-p",
            "-t",
            &format!("={name}:0"),
            "#{window_width}x#{window_height}",
        ])
        .output()?;
    assert!(restored.status.success());
    assert_eq!(String::from_utf8_lossy(&restored.stdout).trim(), "120x40");
    pty.close()?;
    let surviving = std::process::Command::new("tmux")
        .args(["-L", &socket, "has-session", "-t", &format!("={name}")])
        .status()?;
    assert!(surviving.success(), "tmux session died when PTY detached");
    Ok(())
}
