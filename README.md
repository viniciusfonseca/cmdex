# Cmdex

Cmdex is a terminal UI for managing Codex agents.

It is built with Rust and Ratatui, reads agents from `~/.cmdex.yml`, and talks to Codex through `codex app-server --stdio`.

## Features

- Chat tab with streaming responses
- Workspace tab with a file tree on the left and file preview on the right
- Git Diff tab with modified files on the left and diff preview on the right
- Agent creation form inside the UI
- Keyboard and mouse support for navigation

## Requirements

- Rust toolchain with `cargo`
- `codex` available in your `PATH`
- A working Codex login/session on your machine

## Configuration

Cmdex reads agents from `~/.cmdex.yml`.

Example:

```yml
agents:
  - name: agent-0
    workspace: ~/projects/project-foo
  - name: agent-1
    workspace: ~/projects/project-bar
```

You can also add agents from inside the UI. Cmdex will save them back to `~/.cmdex.yml`.

## Running

```bash
cargo run
```

For a quick build check:

```bash
cargo check
```

Run tests with:

```bash
cargo test
```

## Controls

- `Ctrl+Q`: quit
- `Ctrl+C`: quit
- `←` / `→`: switch tabs
- `↑` / `↓`: move selection in the sidebar
- `PageUp` / `PageDown`: scroll content preview
- `F5`: refresh `Workspace` or `Git Diff`
- `Enter`: send chat message or save a new agent
- `Tab`: switch between form fields in `Add agent`
- `Esc`: leave the add-agent form

Mouse support:

- Click tabs to switch views
- Click sidebar items to select agents, files, and diffs
- Click form fields to focus them
- Scroll with the mouse wheel in the sidebar or content area

## Notes

- Cmdex restores the latest Codex session for each configured workspace, including chat messages and summarized workspace events.
- Workspace previews are text-oriented. Binary files are not rendered.
- Git Diff uses the current repository state from the selected workspace.

## Project Layout

- [src/main.rs](src/main.rs): terminal bootstrap
- [src/app.rs](src/app.rs): UI, events, layout, and interaction flow
- [src/codex.rs](src/codex.rs): Codex app-server client
- [src/config.rs](src/config.rs): `~/.cmdex.yml` loading and saving
- [src/workspace.rs](src/workspace.rs): workspace and git diff browsing
