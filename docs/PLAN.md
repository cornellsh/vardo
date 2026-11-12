# Plan

Work in order. Each item says why it matters and the check that proves it done.
Finished items are deleted, not ticked. Git history keeps them.
An item that changes a rule also updates `docs/INVARIANTS.md`. One that makes a choice adds to `docs/DECISIONS.md`.

## Phase 1: make the system tell the truth cheaply

These come first because every later change gets cheaper and safer once they land.

### plan:macos-shell

- **Do:** Commit the tray template icon in `app/src-tauri/src/tray.rs` and the Dock reopen handler in `app/src-tauri/src/lib.rs`, both uncommitted in the working tree. First a person checks the tray icon, its menu, and close-then-Dock-click (`docs/RUNBOOKS.md`).
- **Done when:** Both are committed, and `fact:platform-macos` records what the person saw.

## Phase 2: product

In rough priority order. Each needs its own design pass before code.

### plan:quick-access

- A global hotkey overlay: type to search, Enter to copy.

### plan:command-palette

- A command palette over every action.

### plan:host-install-from-app

- Install and remove the browser host from the app instead of a terminal.

### plan:windows-pipe

- Windows support for the extension, with a named pipe in place of the unix socket.

### plan:firefox

- A Firefox build of the extension. Native messaging works the same with a different manifest.

### plan:iframes

- Fill inside iframes, with an origin check per frame.

### plan:generate-into-form

- Generate a password into a sign-up form.

### plan:webdav

- The WebDAV backend, with the `mod_deflate` ETag case as a test (`dec:webdav-second`).

### plan:attachment-size

- Measure saves and syncs with large attachments before recommending attachments (`fact:kdbx-no-delta`).

### plan:drive-contract-rest

- Verify `fact:drive-quota-403` and `fact:drive-file-scope-no-verification` live.

### plan:polish

- HIBP lookups by k-anonymity, restoring a whole old entry version, dragging groups to move them.

### plan:distribution

- Ed25519 updater signing, macOS notarisation, Windows Trusted Signing, AppImage, Flatpak, AUR and the Chrome Web Store.

### plan:glib-alert

- Dependabot alert 1: `glib` below 0.20 has an unsound iterator, pulled in by Tauri's GTK stack. Resolve when Tauri moves to glib 0.20, and record the decision in `deny.toml` until then.
