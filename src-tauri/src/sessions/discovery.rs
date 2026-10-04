use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::ssh::openssh;

fn config_directive(line: &str) -> Result<Option<(String, Vec<String>)>> {
    let line = line.trim();
    let end = line
        .find(|character: char| character.is_whitespace() || character == '=')
        .unwrap_or(line.len());
    let keyword = line[..end].to_ascii_lowercase();
    if !matches!(keyword.as_str(), "host" | "include") {
        return Ok(None);
    }
    let arguments = line[end..]
        .trim_start()
        .trim_start_matches('=')
        .trim_start();
    let mut comment = arguments.len();
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in arguments.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == '#' && quote.is_none() {
            comment = index;
            break;
        }
    }
    let arguments = shell_words::split(&arguments[..comment]).map_err(anyhow::Error::from)?;
    if arguments.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "SSH {keyword} directive has no arguments"
        )));
    }
    Ok(Some((keyword, arguments)))
}

fn collect_hosts(
    path: &Path,
    home: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    hosts: &mut HashSet<String>,
) -> Result<()> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("Cannot resolve SSH config {}", path.display()))?;
    if visited.contains(&canonical) {
        return Ok(());
    }
    if depth > 16 || visited.len() >= 256 {
        return Err(AppError::InvalidInput(
            "SSH discovery exceeded 16 Include levels or 256 config files; add the host manually"
                .into(),
        ));
    }
    visited.insert(canonical.clone());
    let config = std::fs::read_to_string(&canonical)
        .with_context(|| format!("Cannot read SSH config {}", path.display()))?;
    for (index, line) in config.lines().enumerate() {
        let Some((keyword, arguments)) = config_directive(line)
            .with_context(|| format!("Invalid SSH config {}:{}", path.display(), index + 1))?
        else {
            continue;
        };
        if keyword == "host" {
            hosts.extend(
                arguments
                    .into_iter()
                    .filter(|alias| openssh::validate_alias(alias).is_ok()),
            );
        } else {
            for argument in arguments {
                if argument.starts_with('~') && !argument.starts_with("~/")
                    || argument.contains('%')
                    || argument.contains("${")
                {
                    tracing::warn!(config = %path.display(), line = index + 1, "SSH discovery cannot expand this Include; add its hosts manually");
                    continue;
                }
                let pattern = if let Some(relative) = argument.strip_prefix("~/") {
                    home.join(relative)
                } else {
                    let pattern = PathBuf::from(argument);
                    if pattern.is_absolute() {
                        pattern
                    } else {
                        // OpenSSH resolves relative user-config Includes against ~/.ssh.
                        home.join(".ssh").join(pattern)
                    }
                };
                let pattern = pattern.to_str().ok_or_else(|| {
                    AppError::InvalidInput("SSH Include path is not valid UTF-8".into())
                })?;
                let matches = glob::glob_with(
                    pattern,
                    glob::MatchOptions {
                        case_sensitive: true,
                        require_literal_separator: true,
                        require_literal_leading_dot: true,
                    },
                )
                .with_context(|| format!("Invalid SSH Include pattern {pattern}"))?;
                for matched in matches {
                    let included = matched
                        .with_context(|| format!("Cannot read SSH Include pattern {pattern}"))?;
                    collect_hosts(&included, home, depth + 1, visited, hosts)?;
                }
            }
        }
    }
    Ok(())
}

fn configured_hosts(home: &Path) -> Result<Vec<String>> {
    let path = home.join(".ssh/config");
    if !path.try_exists()? {
        return Ok(Vec::new());
    }
    let mut hosts = HashSet::new();
    collect_hosts(&path, home, 0, &mut HashSet::new(), &mut hosts)?;
    let mut hosts: Vec<_> = hosts.into_iter().collect();
    hosts.sort();
    Ok(hosts)
}

fn vscode_storage(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = home.join("Library/Application Support/Code/User/globalStorage");
    #[cfg(not(target_os = "macos"))]
    let base = home.join(".config/Code/User/globalStorage");
    base.join("storage.json")
}

fn authority_alias(value: &str) -> Option<String> {
    let decoded = percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .ok()?;
    let alias = decoded.strip_prefix("ssh-remote+")?;
    if alias.starts_with("7b22") {
        if let Ok(bytes) = hex::decode(alias) {
            if let Ok(payload) = serde_json::from_slice::<Value>(&bytes) {
                let host = payload.get("hostName")?.as_str()?;
                openssh::validate_alias(host).ok()?;
                return Some(host.to_owned());
            }
        }
    }
    openssh::validate_alias(alias).ok()?;
    Some(alias.to_owned())
}

fn remote_folder(uri: &str) -> Option<(String, String)> {
    let remainder = uri.strip_prefix("vscode-remote://")?;
    let (authority, path) = remainder.split_once('/')?;
    let host = authority_alias(authority)?;
    let path = path.split(['?', '#']).next()?;
    let path = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .ok()?;
    let path = format!("/{}", path.trim_start_matches('/'));
    if path == "/" || path.contains('\0') || path.contains('\n') || path.contains("/.copilot/") {
        return None;
    }
    Some((host, path))
}

fn recent_folders(storage: &Value) -> HashSet<(String, String)> {
    let backups = storage
        .pointer("/backupWorkspaces/folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|folder| folder.get("folderUri").and_then(Value::as_str));
    let profiles = storage
        .pointer("/profileAssociations/workspaces")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|items| items.keys().map(String::as_str));
    backups.chain(profiles).filter_map(remote_folder).collect()
}

pub fn sync(connection: &mut Connection, home: &Path) -> Result<()> {
    let hosts = configured_hosts(home)?;
    if hosts.is_empty() {
        return Ok(());
    }

    let storage_path = vscode_storage(home);
    let folders = if storage_path.exists() {
        let raw = std::fs::read_to_string(&storage_path)
            .with_context(|| format!("Cannot read {}", storage_path.display()))?;
        let value: Value = serde_json::from_str(&raw)
            .with_context(|| format!("Cannot parse {}", storage_path.display()))?;
        recent_folders(&value)
    } else {
        HashSet::new()
    };

    let transaction = connection.transaction()?;
    let mut host_ids = HashMap::new();
    for alias in hosts {
        transaction.execute(
            "INSERT INTO hosts (id, name, ssh_host) VALUES (?1, ?2, ?3)
             ON CONFLICT(ssh_host) DO NOTHING",
            params![ulid::Ulid::new().to_string(), alias, alias],
        )?;
        let id: String = transaction.query_row(
            "SELECT id FROM hosts WHERE ssh_host = ?1",
            [&alias],
            |row| row.get(0),
        )?;
        host_ids.insert(alias, id);
    }
    for (alias, path) in folders {
        if let Some(host_id) = host_ids.get(&alias) {
            let display = path
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or(&path);
            transaction.execute(
                "INSERT INTO workspaces (id, host_id, repo_path, display_name)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(host_id, repo_path) DO NOTHING",
                params![ulid::Ulid::new().to_string(), host_id, path, display],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
