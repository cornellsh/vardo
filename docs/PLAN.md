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

### plan:attachment-size

- Measure saves and syncs with large attachments before recommending attachments (`fact:kdbx-no-delta`).

### plan:drive-contract-rest

- Verify `fact:drive-quota-403` and `fact:drive-file-scope-no-verification` live.

### plan:polish

- HIBP lookups by k-anonymity, restoring a whole old entry version, dragging groups to move them.

### plan:distribution

- Ed25519 updater signing, macOS notarisation, Windows Trusted Signing, AppImage, Flatpak, AUR and the Chrome Web Store.
