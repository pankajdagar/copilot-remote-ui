use std::collections::HashSet;
use std::process::Output;

use crate::error::{AppError, Result};
use crate::sessions::model::Host;
use crate::ssh::manager;
use crate::terminal::TerminalDiagnostics;
use crate::tmux::commands;

fn run(host: &Host, args: Vec<String>) -> Result<Output> {
    Ok(manager::command(host, &args).output()?)
}

fn ensure_success(output: Output, operation: &str) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Operation(anyhow::anyhow!(
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

pub fn create(
    host: &Host,
    name: &str,
    path: &str,
    argv: &[String],
    cols: u16,
    rows: u16,
) -> Result<()> {
    ensure_success(
        run(host, commands::create(name, path, argv, cols, rows))?,
        "Could not create tmux session",
    )?;
    if let Err(error) = set_interaction_policy(host, name) {
        if let Err(cleanup) = kill(host, name) {
            tracing::error!(%cleanup, %name, "Could not clean up tmux session after configuration failed");
        }
        return Err(error);
    }
    Ok(())
}

pub fn set_interaction_policy(host: &Host, name: &str) -> Result<()> {
    ensure_success(
        run(host, commands::interaction_policy(name))?,
        "Could not configure tmux mouse and window sizing",
    )
}

pub fn enter_scrollback(host: &Host, name: &str) -> Result<()> {
    ensure_success(
        run(host, commands::scrollback(name))?,
        "Could not enter tmux scrollback",
    )
}

fn parse_diagnostics(line: &str) -> Result<TerminalDiagnostics> {
    let fields: Vec<_> = line.trim().split('|').collect();
    let [mouse, alternate, pane_rows, client_rows, pane_count] = fields.as_slice() else {
        return Err(AppError::Operation(anyhow::anyhow!(
            "Unexpected tmux diagnostics format"
        )));
    };
    let flag = |value: &str| match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(AppError::Operation(anyhow::anyhow!(
            "Invalid tmux mouse or screen flag"
        ))),
    };
    let pane_rows = pane_rows.parse().map_err(|error| {
        AppError::Operation(anyhow::anyhow!("Invalid tmux pane height: {error}"))
    })?;
    let client_rows = if client_rows.is_empty() {
        None
    } else {
        Some(client_rows.parse().map_err(|error| {
            AppError::Operation(anyhow::anyhow!("Invalid tmux client height: {error}"))
        })?)
    };
    let pane_count = pane_count.parse().map_err(|error| {
        AppError::Operation(anyhow::anyhow!("Invalid tmux pane count: {error}"))
    })?;
    Ok(TerminalDiagnostics {
        application_mouse: flag(mouse)?,
        alternate_screen: flag(alternate)?,
        pane_rows,
        client_rows,
        pane_count,
    })
}

pub fn inspect(host: &Host, name: &str) -> Result<TerminalDiagnostics> {
    let output = run(host, commands::diagnostics(name))?;
    if !output.status.success() {
        return Err(AppError::Operation(anyhow::anyhow!(
            "Could not inspect tmux terminal: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let line = String::from_utf8(output.stdout).map_err(anyhow::Error::from)?;
    parse_diagnostics(&line)
}

pub fn list(host: &Host) -> Result<HashSet<String>> {
    let output = run(host, commands::list())?;
    if !output.status.success() {
        if output.status.code() == Some(255) && host.ssh_host.is_some() {
            return Err(AppError::SshTunnel {
                host: host.name.clone(),
                reason: "Could not connect through the configured SSH host".into(),
            });
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.code() == Some(1)
            && (stderr.contains("no server running")
                || (stderr.contains("error connecting to")
                    && stderr.contains("No such file or directory")))
        {
            return Ok(HashSet::new());
        }
        return Err(AppError::Operation(anyhow::anyhow!(
            "Could not list tmux sessions on {}: {}",
            host.name,
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

pub fn kill(host: &Host, name: &str) -> Result<()> {
    ensure_success(
        run(host, commands::kill(name))?,
        "Could not kill tmux session",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_distinguish_outer_mouse_from_application_mouse() -> Result<()> {
        let active = parse_diagnostics("1|1|40|40|1\n")?;
        assert!(active.application_mouse);
        assert!(active.alternate_screen);
        assert_eq!(active.pane_rows, 40);
        assert_eq!(active.client_rows, Some(40));
        assert_eq!(active.pane_count, 1);

        let inactive = parse_diagnostics("0|0|29||2\n")?;
        assert!(!inactive.application_mouse);
        assert_eq!(inactive.client_rows, None);
        assert!(parse_diagnostics("enabled|1|40|40|1").is_err());
        assert!(parse_diagnostics("1|1|bad|40|1").is_err());
        Ok(())
    }
}
