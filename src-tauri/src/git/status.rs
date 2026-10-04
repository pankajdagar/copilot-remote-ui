use serde::Serialize;

use crate::error::{AppError, Result};
use crate::sessions::model::Host;
use crate::ssh::manager;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
}

fn parse_porcelain(data: &[u8]) -> Result<Vec<ChangedFile>> {
    let mut entries = data
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    let mut changes = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 || entry[2] != b' ' {
            return Err(AppError::Operation(anyhow::anyhow!(
                "Invalid git status output"
            )));
        }
        let status = String::from_utf8(entry[..2].to_vec()).map_err(anyhow::Error::from)?;
        let path = String::from_utf8(entry[3..].to_vec()).map_err(anyhow::Error::from)?;
        if status.contains('R') || status.contains('C') {
            entries.next().ok_or_else(|| {
                AppError::Operation(anyhow::anyhow!("Git omitted the original renamed path"))
            })?;
        }
        changes.push(ChangedFile { path, status });
    }
    Ok(changes)
}

pub fn list(host: &Host, repo_path: &str) -> Result<Vec<ChangedFile>> {
    let args = vec![
        "git".into(),
        "-C".into(),
        repo_path.into(),
        "status".into(),
        "--porcelain=v1".into(),
        "-z".into(),
        "--untracked-files=all".into(),
    ];
    let output = manager::command(host, &args).output()?;
    if !output.status.success() {
        return Err(AppError::Operation(anyhow::anyhow!(
            "Cannot read repository changes on {}: {}",
            host.name,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_porcelain(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_spaces_renames_and_untracked_files() -> Result<()> {
        let changes =
            parse_porcelain(b" M src/file name.ts\0R  src/new.ts\0src/old.ts\0?? a.txt\0")?;
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].path, "src/file name.ts");
        assert_eq!(changes[1].status, "R ");
        assert_eq!(changes[1].path, "src/new.ts");
        assert_eq!(changes[2].path, "a.txt");
        assert!(parse_porcelain(b"R  src/new.ts\0").is_err());
        Ok(())
    }

    #[test]
    fn reads_local_git_without_shell_interpolation() -> Result<()> {
        let directory = tempfile::Builder::new()
            .prefix("repo ' with spaces")
            .tempdir()?;
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(directory.path())
            .status()?;
        assert!(initialized.success());
        std::fs::write(directory.path().join("new file.ts"), "console.log('ok');\n")?;
        let host = Host {
            id: "local".into(),
            name: "Local".into(),
            ssh_host: None,
            created_at: String::new(),
        };
        let path = directory.path().to_str().ok_or_else(|| {
            AppError::InvalidInput("Temporary repository path is not UTF-8".into())
        })?;
        let changes = list(&host, path)?;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "new file.ts");
        assert_eq!(changes[0].status, "??");
        Ok(())
    }

    #[test]
    fn non_git_directory_reports_an_error_instead_of_a_clean_tree() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let host = Host {
            id: "local".into(),
            name: "Local".into(),
            ssh_host: None,
            created_at: String::new(),
        };
        let path = directory.path().to_str().ok_or_else(|| {
            AppError::InvalidInput("Temporary directory path is not UTF-8".into())
        })?;
        let error = list(&host, path).err().ok_or_else(|| {
            AppError::Operation(anyhow::anyhow!("Expected Git to reject a non-repository"))
        })?;
        assert!(error.to_string().contains("Cannot read repository changes"));
        Ok(())
    }
}
