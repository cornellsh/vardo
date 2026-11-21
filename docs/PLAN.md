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

### plan:drive-contract-rest

- Verify `fact:drive-file-scope-no-verification` live. Needs the project owner to publish the OAuth client in the Google Cloud console and record whether Google asks for verification.

### plan:polish

- HIBP lookups by k-anonymity. Waits on a decision: it sends a hash prefix of each password to a third party, which `dec:no-server-no-telemetry` rules out as written.

### plan:distribution

- Unsigned builds ship on every `v*` tag through `.github/workflows/release.yml` (`dec:unsigned-releases`). Left: Ed25519 updater signing, macOS notarisation, Windows Trusted Signing, Flatpak, AUR and the Chrome Web Store.

### plan:firefox-signing

- **Do:** Have Mozilla sign the Firefox build, listed on addons.mozilla.org or unlisted for download (`web-ext sign`). Release Firefox refuses an unsigned add-on except as a temporary one, which a restart removes. Needs the project owner's addons.mozilla.org API key.
- **Done when:** A signed `.xpi` installs in release Firefox and survives a restart.
