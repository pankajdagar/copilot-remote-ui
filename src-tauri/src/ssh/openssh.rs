use std::process::Command;

use crate::error::{AppError, Result};

pub fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || !alias
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        || !alias
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(AppError::InvalidInput(
            "SSH host must be a configured alias or hostname (letters, numbers, '.', '_' or '-')"
                .into(),
        ));
    }
    Ok(())
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn remote_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn command(alias: &str, remote: &[String]) -> Command {
    let mut process = Command::new("ssh");
    process
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--"])
        .arg(alias)
        .arg(remote_command(remote));
    process
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_metacharacters_as_single_arguments() {
        assert_eq!(
            quote("a 'b'; $(touch /tmp/no)"),
            "'a '\\''b'\\''; $(touch /tmp/no)'"
        );
        assert!(validate_alias("-oProxyCommand=bad").is_err());
        assert!(validate_alias("alice@host").is_err());
        assert!(validate_alias("my-host.example").is_ok());
    }
}
