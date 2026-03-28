# RustDock

Local-first desktop app for managing SSH and SFTP sessions.

## Current Scope

This first cut focuses on the application shell:

- Tauri desktop shell with a Vue 3 frontend
- xterm.js terminal viewport wired through a Tauri Channel stream
- local session persistence in SQLite
- async session runtime and a streamed terminal session abstraction
- real `russh` backend for SSH transport, PTY allocation, shell startup, input forwarding, resize, and keepalive
- persisted transfer queue with progress, cancellation, recent-event history, and auto-resume
- system keychain integration for per-session secrets
- explicit sync state on every session entry
- architecture boundaries for a future SSH engine, SFTP browser, and cloud sync service

What is not implemented yet:

- polished SFTP drag/drop and richer context menus
- cloud sync backend

The current build has been compiled successfully, and both the real SSH terminal path and the SFTP path have passed live smoke tests against a password-authenticated target host from this environment.

## Project Layout

- `frontend/`: Vue 3 + Vite + xterm.js frontend
- `src/`: shared Rust core library
- `src/domain/`: session domain types
- `src/runtime.rs`: shared Tokio runtime bootstrap
- `src/ssh/`: streamed terminal session abstractions and the real russh-backed SSH transport
- `src/sftp/`: remote directory listing plus upload/download helpers over russh-sftp
- `src/storage/`: local persistence layer
- `src/sync/`: sync summary and provider abstraction
- `src-tauri/`: Tauri desktop backend and window config

## Local Storage

The app stores data at:

- `~/.local/share/rustdock/workbench.sqlite3`

The database currently keeps session metadata only. It does not store passwords.

## Run

Install frontend dependencies:

```bash
cd rustdock
npm install
```

Run the frontend only:

```bash
cd rustdock
npm run dev
```

Build the frontend:

```bash
cd rustdock
npm run build
```

Check the desktop backend:

```bash
cd rustdock
cargo check --manifest-path src-tauri/Cargo.toml
```

Build the desktop binary:

```bash
cd rustdock
npm run tauri:build -- --debug
```

## GitHub Build

This repository is prepared for GitHub Actions manual builds.

1. Create a GitHub repository and push this directory.
2. Open `Actions`.
3. Select `build-clients`.
4. Click `Run workflow`.
5. Download the generated artifacts for Linux, Windows, or macOS.

Notes:

- The workflow builds release bundle artifacts for each platform.
- On Windows, prefer the generated installer inside the artifact bundle instead of a raw executable.
- The workflow file is [build-clients.yml](.github/workflows/build-clients.yml).
- The workflow assumes your default branch is `main` or `master`, but `workflow_dispatch` works regardless of branch naming.

## Next Steps

1. Harden the real `russh` path around runtime edge cases such as reconnects, exit-status handling, and SSH agent auth.
2. Add a richer SFTP file tree with multi-select, drag/drop, and batch operations.
3. Add task-level history filters and better failure classification/backoff controls.
4. Replace the noop sync provider with a real remote sync API.
5. Migrate from deprecated `xterm` to `@xterm/xterm`.
