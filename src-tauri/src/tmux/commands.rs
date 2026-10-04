pub fn exact(name: &str) -> String {
    format!("={name}")
}

pub fn create(name: &str, path: &str, argv: &[String], cols: u16, rows: u16) -> Vec<String> {
    let mut args = vec![
        "tmux".into(),
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        name.into(),
        "-c".into(),
        path.into(),
        "-x".into(),
        cols.to_string(),
        "-y".into(),
        rows.to_string(),
        "--".into(),
    ];
    args.extend_from_slice(argv);
    args
}

pub fn list() -> Vec<String> {
    vec![
        "tmux".into(),
        "list-sessions".into(),
        "-F".into(),
        "#{session_name}".into(),
    ]
}

pub fn kill(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "kill-session".into(),
        "-t".into(),
        exact(name),
    ]
}

pub fn attach(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "attach-session".into(),
        "-t".into(),
        exact(name),
    ]
}

pub fn resize_policy(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "set-window-option".into(),
        "-t".into(),
        format!("{}:0", exact(name)),
        "window-size".into(),
        "latest".into(),
    ]
}

pub fn mouse_policy(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "set-option".into(),
        "-t".into(),
        name.into(),
        "mouse".into(),
        "on".into(),
    ]
}

pub fn interaction_policy(name: &str) -> Vec<String> {
    let mut args = resize_policy(name);
    args.push(";".into());
    args.extend(mouse_policy(name).into_iter().skip(1));
    args.extend([
        ";".into(),
        "set-option".into(),
        "-t".into(),
        name.into(),
        "status".into(),
        "off".into(),
    ]);
    args
}

pub fn scrollback(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "copy-mode".into(),
        "-u".into(),
        "-t".into(),
        format!("{}:0", exact(name)),
    ]
}

pub fn diagnostics(name: &str) -> Vec<String> {
    vec![
        "tmux".into(),
        "display-message".into(),
        "-p".into(),
        "-t".into(),
        format!("{}:0", exact(name)),
        "#{mouse_any_flag}|#{alternate_on}|#{pane_height}|#{client_height}|#{window_panes}".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_keeps_repository_and_arguments_separate() {
        let args = create(
            "crui_01JTEST",
            "/tmp/it's a repo; $(false)",
            &["copilot".into(), "--model".into(), "gpt".into()],
            120,
            40,
        );
        assert_eq!(args[6], "/tmp/it's a repo; $(false)");
        assert_eq!(&args[11..], ["--", "copilot", "--model", "gpt"]);
        assert_eq!(attach("crui_01JTEST")[3], "=crui_01JTEST");
        assert_eq!(resize_policy("crui_01JTEST")[3], "=crui_01JTEST:0");
        assert_eq!(mouse_policy("crui_01JTEST")[3], "crui_01JTEST");
        assert_eq!(&mouse_policy("crui_01JTEST")[4..], ["mouse", "on"]);
        assert_eq!(
            &interaction_policy("crui_01JTEST")[6..9],
            [";", "set-option", "-t"]
        );
        assert_eq!(
            &interaction_policy("crui_01JTEST")[12..],
            [";", "set-option", "-t", "crui_01JTEST", "status", "off"]
        );
        assert_eq!(scrollback("crui_01JTEST")[4], "=crui_01JTEST:0");
        assert_eq!(diagnostics("crui_01JTEST")[4], "=crui_01JTEST:0");
    }
}
