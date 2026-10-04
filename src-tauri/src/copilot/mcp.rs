use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use github_copilot_sdk::{Client, IndexMap, McpHttpServerConfig, McpServerConfig};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use crate::error::{AppError, Result};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedMcp {
    pub name: String,
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpView {
    pub name: String,
    pub url: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInventory {
    pub reviewed: Vec<McpView>,
    pub imported: Vec<ImportedMcpView>,
    pub available_cli: Vec<CliMcpCandidate>,
    pub external: Vec<String>,
    pub warning: Option<String>,
}

#[derive(Clone)]
pub struct ReviewedCliMcp {
    pub name: String,
    pub config_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedMcpView {
    pub name: String,
    pub status: String,
    pub needs_review: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliMcpCandidate {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub endpoint_host: Option<String>,
    pub reviewed: bool,
    pub needs_review: bool,
}

pub struct SessionMcpSettings {
    pub servers: IndexMap<String, McpServerConfig>,
    pub disabled: Vec<String>,
    pub needs_review: Vec<String>,
}

fn canonical_config(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut names: Vec<_> = object.keys().collect();
            names.sort();
            let mut sorted = serde_json::Map::new();
            for name in names {
                sorted.insert(name.clone(), canonical_config(&object[name]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_config).collect()),
        other => other.clone(),
    }
}

pub fn config_fingerprint(config: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(&canonical_config(config)).map_err(anyhow::Error::from)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub fn list_imports(db: &Connection, host_id: &str) -> Result<Vec<ReviewedCliMcp>> {
    let mut statement = db.prepare(
        "SELECT name, config_sha256 FROM reviewed_cli_mcp WHERE host_id = ?1 ORDER BY name",
    )?;
    let rows = statement.query_map([host_id], |row| {
        Ok(ReviewedCliMcp {
            name: row.get(0)?,
            config_sha256: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn save_import(db: &Connection, host_id: &str, name: &str, fingerprint: &str) -> Result<()> {
    db.execute(
        "INSERT INTO reviewed_cli_mcp(host_id, name, config_sha256) VALUES (?1, ?2, ?3)
         ON CONFLICT(host_id, name) DO UPDATE SET
         config_sha256 = excluded.config_sha256,
         reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![host_id, name, fingerprint],
    )?;
    Ok(())
}

pub fn remove_import(db: &Connection, host_id: &str, name: &str) -> Result<()> {
    if db.execute(
        "DELETE FROM reviewed_cli_mcp WHERE host_id = ?1 AND name = ?2",
        params![host_id, name],
    )? == 0
    {
        return Err(AppError::NotFound(
            "Reviewed Copilot CLI MCP server not found".into(),
        ));
    }
    Ok(())
}

pub fn candidate(
    name: String,
    config: &Value,
    import: Option<&ReviewedCliMcp>,
) -> Result<CliMcpCandidate> {
    let kind = config.get("type").and_then(Value::as_str);
    let transport = kind.unwrap_or(if config.get("command").is_some() {
        "stdio"
    } else {
        "unknown"
    });
    let endpoint_host = config
        .get("url")
        .and_then(Value::as_str)
        .and_then(|value| Url::parse(value).ok())
        .and_then(|url| url.host_str().map(str::to_owned));
    let reviewed = import.is_some();
    let needs_review = match import {
        Some(item) => config_fingerprint(config)? != item.config_sha256,
        None => false,
    };
    Ok(CliMcpCandidate {
        name,
        transport: transport.to_owned(),
        command: config.get("command").and_then(Value::as_str).map(|value| {
            if value
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
            {
                "Complex command; review in Copilot CLI".into()
            } else {
                Path::new(value)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| {
                        name.chars().all(|character| {
                            character.is_ascii_alphanumeric()
                                || matches!(character, '-' | '_' | '.')
                        })
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| "Custom program; review in Copilot CLI".into())
            }
        }),
        endpoint_host,
        reviewed,
        needs_review,
    })
}

pub async fn user_config(client: &Client) -> Result<HashMap<String, Value>> {
    let global = tokio::time::timeout(Duration::from_secs(5), client.rpc().mcp().config().list())
        .await
        .map_err(|_| {
            AppError::Operation(anyhow::anyhow!(
                "Timed out checking remote Copilot CLI MCP configuration"
            ))
        })?
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!(
                "Could not check remote Copilot CLI MCP configuration: {error}"
            ))
        })?;
    Ok(global.servers)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthResult {
    pub authorization_url: Option<String>,
    pub note: String,
}

pub fn callback_port(authorization_url: &str) -> Result<Option<u16>> {
    let url = Url::parse(authorization_url)
        .map_err(|_| AppError::InvalidInput("MCP authorization URL is invalid".into()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::InvalidInput(
            "MCP authorization must use an HTTPS sign-in URL without embedded credentials".into(),
        ));
    }
    let Some(redirect) = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value.to_string())
    else {
        return Ok(None);
    };
    let callback = Url::parse(&redirect)
        .map_err(|_| AppError::InvalidInput("MCP OAuth redirect URI is invalid".into()))?;
    if callback.scheme() != "http"
        || !matches!(callback.host_str(), Some("127.0.0.1" | "localhost"))
        || !callback.username().is_empty()
        || callback.password().is_some()
    {
        return Ok(None);
    }
    Ok(callback.port().filter(|port| *port >= 1024))
}

pub fn list(db: &Connection, host_id: &str) -> Result<Vec<ReviewedMcp>> {
    let mut statement =
        db.prepare("SELECT name, url FROM reviewed_mcp_servers WHERE host_id = ?1 ORDER BY name")?;
    let servers = statement
        .query_map([host_id], |row| {
            Ok(ReviewedMcp {
                name: row.get(0)?,
                url: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(servers)
}

pub fn validate(name: &str, endpoint: &str) -> Result<String> {
    if name.len() < 2
        || name.len() > 48
        || !name.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(AppError::InvalidInput(
            "MCP server name must be 2-48 letters, numbers, hyphens or underscores".into(),
        ));
    }
    let url = Url::parse(endpoint)
        .map_err(|_| AppError::InvalidInput("Enter a valid HTTPS MCP endpoint".into()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::InvalidInput(
            "MCP endpoint must be HTTPS with no embedded credentials, query or fragment".into(),
        ));
    }
    Ok(url.to_string())
}

pub fn add(db: &Connection, host_id: &str, name: &str, url: &str) -> Result<ReviewedMcp> {
    let url = validate(name, url)?;
    db.execute(
        "INSERT INTO reviewed_mcp_servers (host_id, name, url) VALUES (?1, ?2, ?3)
         ON CONFLICT(host_id, name) DO UPDATE SET
           url = excluded.url, reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![host_id, name, url],
    )?;
    Ok(ReviewedMcp {
        name: name.into(),
        url,
    })
}

pub fn remove(db: &Connection, host_id: &str, name: &str) -> Result<()> {
    if db.execute(
        "DELETE FROM reviewed_mcp_servers WHERE host_id = ?1 AND name = ?2",
        params![host_id, name],
    )? == 0
    {
        return Err(AppError::NotFound("Reviewed MCP server not found".into()));
    }
    Ok(())
}

pub async fn session_settings(
    client: &Client,
    reviewed: &[ReviewedMcp],
    imports: &[ReviewedCliMcp],
) -> Result<SessionMcpSettings> {
    let global = user_config(client).await?;
    build_session_settings(reviewed, imports, &global)
}

fn build_session_settings(
    reviewed: &[ReviewedMcp],
    imports: &[ReviewedCliMcp],
    global: &HashMap<String, Value>,
) -> Result<SessionMcpSettings> {
    if let Some(conflict) = reviewed
        .iter()
        .find(|server| global.contains_key(&server.name))
    {
        return Err(AppError::InvalidInput(format!(
            "Reviewed MCP server {} conflicts with a remote global server; rename one before connecting",
            conflict.name
        )));
    }
    let mut servers = IndexMap::new();
    for server in reviewed {
        validate(&server.name, &server.url)?;
        servers.insert(
            server.name.clone(),
            McpServerConfig::Http(McpHttpServerConfig {
                url: server.url.clone(),
                headers: Default::default(),
                timeout: None,
                tools: None,
            }),
        );
    }
    let mut disabled: HashSet<_> = global.keys().cloned().collect();
    let mut needs_review = Vec::new();
    for import in imports {
        match global.get(&import.name) {
            Some(config) if config_fingerprint(config)? == import.config_sha256 => {
                disabled.remove(&import.name);
            }
            _ => needs_review.push(import.name.clone()),
        }
    }
    needs_review.sort();
    let mut disabled: Vec<_> = disabled.into_iter().collect();
    disabled.sort();
    Ok(SessionMcpSettings {
        servers,
        disabled,
        needs_review,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sessions::repository;

    #[test]
    fn requires_reviewed_https_and_persists_only_urls_without_tokens() -> Result<()> {
        for endpoint in [
            "http://example.com/mcp",
            "https://user:password@example.com/mcp",
            "https://example.com/mcp?access_token=secret",
        ] {
            assert!(validate("approved", endpoint).is_err());
        }
        let path = tempfile::tempdir()?;
        let db = Database::open(&path.path().join("sessions.sqlite3"))?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let server = add(
            &*db.connection()?,
            &host.id,
            "approved",
            "https://mcp.example.com/mcp",
        )?;
        assert_eq!(list(&*db.connection()?, &host.id)?.len(), 1);
        assert_eq!(server.url, "https://mcp.example.com/mcp");
        drop(db);
        let db = Database::open(&path.path().join("sessions.sqlite3"))?;
        assert_eq!(list(&*db.connection()?, &host.id)?[0].url, server.url);
        remove(&*db.connection()?, &host.id, "approved")?;
        assert!(list(&*db.connection()?, &host.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn only_forwards_a_known_loopback_oauth_callback() -> Result<()> {
        assert_eq!(callback_port("https://auth.example.com/authorize?redirect_uri=http%3A%2F%2F127.0.0.1%3A59342%2Fcallback")?, Some(59342));
        assert_eq!(callback_port("https://auth.example.com/authorize?redirect_uri=https%3A%2F%2Fexample.com%2Fcallback")?, None);
        assert_eq!(callback_port("https://auth.example.com/authorize")?, None);
        assert!(callback_port("http://auth.example.com/authorize").is_err());
        Ok(())
    }

    #[test]
    fn only_reviewed_servers_are_started_and_unreviewed_global_names_are_disabled() -> Result<()> {
        let reviewed = [ReviewedMcp {
            name: "approved".into(),
            url: "https://mcp.example.com/mcp".into(),
        }];
        let global = HashMap::from([(
            "unreviewed".to_owned(),
            serde_json::json!({"type": "stdio"}),
        )]);
        let settings = build_session_settings(&reviewed, &[], &global)?;
        assert_eq!(settings.servers.len(), 1);
        assert_eq!(settings.disabled, ["unreviewed"]);
        let config =
            serde_json::to_value(&settings.servers["approved"]).map_err(anyhow::Error::from)?;
        assert_eq!(config["type"], "http");
        assert_eq!(config["url"], "https://mcp.example.com/mcp");
        assert!(config.get("headers").is_none());
        assert!(build_session_settings(
            &reviewed,
            &[],
            &HashMap::from([("approved".into(), serde_json::json!({"type": "stdio"}))])
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn imported_cli_mcp_uses_remote_config_without_copying_secrets() -> Result<()> {
        let config = serde_json::json!({
            "type": "stdio", "command": "example-tools", "args": ["start"],
            "env": {"EXAMPLE_ACCESS_TOKEN": "private-token"}
        });
        let global = HashMap::from([
            ("example-tools".into(), config.clone()),
            (
                "other-tools".into(),
                serde_json::json!({"type":"stdio","command":"other-tools"}),
            ),
        ]);
        let approval = ReviewedCliMcp {
            name: "example-tools".into(),
            config_sha256: config_fingerprint(&config)?,
        };
        let settings = build_session_settings(&[], std::slice::from_ref(&approval), &global)?;
        assert!(settings.servers.is_empty());
        assert_eq!(settings.disabled, ["other-tools"]);
        assert!(settings.needs_review.is_empty());
        let summary = candidate("example-tools".into(), &config, Some(&approval))?;
        let rendered = serde_json::to_string(&summary).map_err(anyhow::Error::from)?;
        assert!(rendered.contains("example-tools"));
        assert!(!rendered.contains("private-token"));
        let unsafe_command = candidate(
            "complex".into(),
            &serde_json::json!({
                "type": "stdio", "command": "sh -c TOKEN=private-token"
            }),
            None,
        )?;
        assert_eq!(
            unsafe_command.command.as_deref(),
            Some("Complex command; review in Copilot CLI")
        );

        let changed = HashMap::from([(
            "example-tools".into(),
            serde_json::json!({"type":"stdio","command":"another-program"}),
        )]);
        let stale = build_session_settings(&[], &[approval], &changed)?;
        assert_eq!(stale.disabled, ["example-tools"]);
        assert_eq!(stale.needs_review, ["example-tools"]);
        Ok(())
    }

    #[test]
    fn only_config_fingerprint_is_saved_for_approved_cli_mcp() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db = Database::open(&dir.path().join("sessions.sqlite3"))?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let config = serde_json::json!({
            "command":"example-tools", "env":{"EXAMPLE_ACCESS_TOKEN":"private-token"}
        });
        save_import(
            &*db.connection()?,
            &host.id,
            "example-tools",
            &config_fingerprint(&config)?,
        )?;
        drop(db);
        let db = Database::open(&dir.path().join("sessions.sqlite3"))?;
        let imports = list_imports(&*db.connection()?, &host.id)?;
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].name, "example-tools");
        assert!(!imports[0].config_sha256.contains("private-token"));
        let other_host = repository::add_host(&*db.connection()?, "Second host", "second-host")?;
        assert!(list_imports(&*db.connection()?, &other_host.id)?.is_empty());
        remove_import(&*db.connection()?, &host.id, "example-tools")?;
        assert!(list_imports(&*db.connection()?, &host.id)?.is_empty());
        let first =
            serde_json::from_str::<Value>(r#"{"command":"example-tools","env":{"A":"1","B":"2"}}"#)
                .map_err(anyhow::Error::from)?;
        let reordered =
            serde_json::from_str::<Value>(r#"{"env":{"B":"2","A":"1"},"command":"example-tools"}"#)
                .map_err(anyhow::Error::from)?;
        assert_eq!(config_fingerprint(&first)?, config_fingerprint(&reordered)?);
        Ok(())
    }
}
