use std::process::Command;

use crate::error::{AppError, Result};

#[tauri::command]
pub async fn open_url(url: String) -> Result<()> {
    let parsed = url::Url::parse(&url)
        .map_err(|error| AppError::InvalidInput(format!("Invalid link: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::InvalidInput(
            "Only HTTP(S) links can be opened".into(),
        ));
    }
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(target_os = "linux")]
        let program = "xdg-open";
        #[cfg(target_os = "windows")]
        let program = "explorer";
        let status = Command::new(program).arg(&url).status()?;
        if !status.success() {
            return Err(AppError::Operation(anyhow::anyhow!(
                "Could not open link: {status}"
            )));
        }
        Ok(())
    })
    .await?
}
