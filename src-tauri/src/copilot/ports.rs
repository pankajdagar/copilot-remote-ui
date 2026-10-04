use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use crate::error::{AppError, Result};
use crate::ssh::openssh;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredForward {
    pub direction: String,
    pub spec: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatForward {
    pub local_port: Option<u16>,
    pub remote_port: Option<u16>,
    pub paused: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInventory {
    pub chat: ChatForward,
    pub configured: Vec<ConfiguredForward>,
}

fn parse_forwards(config: &str) -> Vec<ConfiguredForward> {
    config
        .lines()
        .filter_map(|line| {
            let (option, spec) = line.trim().split_once(char::is_whitespace)?;
            let direction = match option.to_ascii_lowercase().as_str() {
                "localforward" => "Local",
                "remoteforward" => "Remote",
                "dynamicforward" => "Dynamic",
                _ => return None,
            };
            let spec = spec.trim();
            if spec.is_empty() || spec.len() > 512 {
                return None;
            }
            Some(ConfiguredForward {
                direction: direction.into(),
                spec: spec.into(),
            })
        })
        .collect()
}

pub async fn configured(alias: Option<&str>) -> Result<Vec<ConfiguredForward>> {
    let Some(alias) = alias else {
        return Ok(Vec::new());
    };
    openssh::validate_alias(alias)?;
    let mut command = Command::new("ssh");
    command.args(["-G", "--", alias]).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| AppError::SshTunnel {
            host: alias.into(),
            reason: "Timed out reading effective SSH forwarding configuration".into(),
        })??;
    if !output.status.success() {
        return Err(AppError::SshTunnel {
            host: alias.into(),
            reason: format!(
                "Could not read effective SSH forwarding configuration ({})",
                output.status
            ),
        });
    }
    if output.stdout.len() > 256 * 1024 {
        return Err(AppError::InvalidInput(
            "SSH configuration is too large to display safely".into(),
        ));
    }
    let config = String::from_utf8(output.stdout).map_err(|_| {
        AppError::InvalidInput("SSH forwarding configuration is not valid UTF-8".into())
    })?;
    Ok(parse_forwards(&config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exposes_forwarding_specs_from_effective_ssh_config() {
        let config = "hostname internal.example\nidentityfile /private/key\n\
            localforward 4443 127.0.0.1:4443\nremoteforward 0.0.0.0:9000 localhost:9000\n\
            dynamicforward 127.0.0.1:8123\nproxycommand secrets-not-shown";
        let forwards = parse_forwards(config);
        assert_eq!(forwards.len(), 3);
        assert_eq!(forwards[0].direction, "Local");
        assert_eq!(forwards[0].spec, "4443 127.0.0.1:4443");
        assert_eq!(forwards[1].direction, "Remote");
        assert_eq!(forwards[1].spec, "0.0.0.0:9000 localhost:9000");
        let rendered = serde_json::to_string(&forwards).expect("serializable port list");
        assert!(!rendered.contains("private/key"));
        assert!(!rendered.contains("secrets-not-shown"));
    }
}
