# Copilot Remote UI

A desktop app for Copilot chats and persistent terminals across SSH repositories.
Built with Tauri 2, React, TypeScript, xterm.js, and the Rust Copilot SDK.

The app stores session names, host and repository mappings, and Copilot session
links in SQLite. Chat history stays in the remote Copilot session store. tmux
keeps terminals and the remote headless Copilot server running after disconnect.

## Requirements

Developed and tested on macOS. Other platforms are not yet verified.

- Node.js 20.19+ and Rust 1.94+.
- [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/).
- A local Copilot CLI installation. SDK 1.0.14 checks for it even when connecting
  to a remote server; set `COPILOT_CLI_PATH` if it is outside standard locations.
- `ssh` on the desktop, and `tmux`, `git`, and Copilot CLI on each remote host.
- A remote Copilot CLI version that supports `--headless`, authenticated on that
  host and available to noninteractive SSH.

The app does not install Copilot CLI, plugins, hooks, or SSH credentials.

## Run

```sh
npm ci
npm run tauri dev
```

```sh
npm test
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

For a desktop bundle, run `npm run tauri build`. Tests requiring a live local
tmux or Copilot process are ignored by default.

## Connect a repository

Configure an SSH alias in `~/.ssh/config`, for example:

```sshconfig
Host dev-box
    HostName dev.example.com
    User developer
```

Host discovery reads literal aliases from that file and follows `Include` paths
and globs. Relative includes resolve against `~/.ssh`; absolute and `~/` paths
are supported. Wildcard and negated host patterns are not listed. Discovery
does not evaluate `Match` commands, conditions, environment variables, or
per-host Include tokens; add those aliases manually. Your system `ssh` still
handles the actual connection, including SSH configuration, the agent,
`IdentityFile`, `ProxyJump`, `ControlMaster`, and known-host checks.

Existing VS Code Remote-SSH repository folders are imported for discovered hosts.
Discovery adds records without overwriting custom names or deleting saved
sessions when a host disappears.

Choose **New Session**, select a host, and add an absolute repository path such
as `/home/developer/projects/my-app`. The terminal defaults to Bash. The command
is parsed as a program and arguments, not as a shell expression.

**Chat** currently requires an SSH host. Local repositories support the
**Terminal** view, but not SDK Chat.

## Chat and terminal

Chat renders Markdown, fenced code, tool inputs and output, approval prompts,
and task-completion summaries. Tool activity is expandable. Subagent traces
have a separate inspector alongside the main conversation. Both transcripts
are virtualized; scrolling up pauses bottom-following.

Use Cmd/Ctrl+Enter to send. Model selection is above the input; context window,
reasoning effort, and permission settings are in **Settings**.

To resume a saved CLI conversation, use **Settings → Browse recent CLI chats**.
The picker lists sessions persisted on the selected host. A conversation already
active in another CLI may be refused by that CLI. Choosing a different
repository requires confirmation. A failed attachment preserves the existing
chat link.

**Terminal** uses xterm.js and tmux, with search, copy/paste, URL detection, and
automatic resizing. **Sync terminal size** is a manual recovery control.
**Changes** reads the repository's Git status.

The app owns logical session names; renaming one does not rename its tmux
session. Disconnecting or closing the window leaves remote processes running.
**Restart terminal** replaces its tmux process. **Delete and stop tmux** stops
the terminal and removes the record; **Forget only** removes the record without
contacting the host. Neither deletes remote Copilot conversation files.

## Permissions and MCP

New chats use Autopilot when the remote CLI supports it. Autopilot continues
multi-step work and allows complete unmanaged Read requests. It is not a
blanket permission grant.

**Allow all** separately approves complete Shell, Read, Write, and URL requests
for the current session. Reviewed MCP calls require an additional confirmed
grant in Settings, with both Autopilot and Allow all active. Managed policy,
sandbox escalation, and missing or redacted arguments are not auto-approved.
Grants run on the desktop and do not persist across app exits.

**Integrations** can add a reviewed HTTPS MCP endpoint or import an existing
user-configured remote CLI server after review. Imports store a name and
configuration fingerprint, not credentials. Changed configurations require
review again. Plugin, managed, and built-in servers remain under CLI control.
Only connect servers you trust and are authorized to use.

OAuth callbacks belong to the remote CLI. A callback bound to remote localhost
cannot be completed in a desktop browser; use a browser on that host if needed.

## Connections and storage

Chat uses a headless Copilot server bound to remote `127.0.0.1` through a
dynamic local SSH tunnel. Terminal connections use a separate PTY. A working
terminal does not guarantee that the host permits Chat's TCP forwarding.
Failures show SSH diagnostics, and saved chats reconnect with bounded retries.

**Ports** shows configured SSH forwards and the app's Chat tunnel. It can release
only its own tunnel, not ports owned by VS Code or another SSH client.

On macOS, metadata is stored at:

```text
~/Library/Application Support/pankajdagar.copilot-remote-ui/sessions.sqlite3
```

The bundle identifier is `pankajdagar.copilot-remote-ui`. Builds with a previous
identifier use a different directory. Before first launch, use SQLite's backup
facility to copy the previous database into the new directory without
overwriting an existing database. Keep the original. Separate databases do not
synchronize later changes.

On macOS, active chats prevent idle sleep with `caffeinate -i`. This does not
override lid closure, manual sleep, or system policy. New permissions cannot
be handled while the desktop is asleep, closed, or disconnected.

`.gitignore` excludes local logs, SQLite databases, `.env` files, `.pem` and
`.key` files, dependencies, and build output. Do not include private configuration
or runtime data in source archives or screenshots.

## License

[MIT](LICENSE). Dependencies retain their own licenses.
