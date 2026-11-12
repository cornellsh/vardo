# Plan

Work in order. Each item says why it matters and the check that proves it done.
Finished items are deleted, not ticked. Git history keeps them.
An item that changes a rule also updates `docs/INVARIANTS.md`. One that makes a choice adds to `docs/DECISIONS.md`.

## Phase 1: make the system tell the truth cheaply

These come first because every later change gets cheaper and safer once they land.

### plan:generated-ipc

- **Do:** Generate the TypeScript types and command bindings from Rust, with `tauri-specta` or `ts-rs`. Delete the hand-written mirror in `app/src/ipc/types.ts` and the string command names in `app/src/ipc/tauri.ts`. Make `app/src/ipc/mock.ts` implement the generated interface. Add a gate that fails when generated files are stale.
- **Why:** Two hand-kept copies of every view model drift without warning. Generated ones cannot.
- **Done when:** Renaming a field in `crates/vardo-core/src/model.rs` fails the interface typecheck.

### plan:core-decides-shells-act

- **Do:** Move the clipboard's clear-only-if-unchanged decision from `app/src-tauri/src/clipboard.rs` into `vardo-core` behind a small clipboard trait, with a test. Audit `app/src-tauri/src/` for any other branch that decides something.
- **Why:** Closes the gap in `inv:clipboard-written-in-rust`.
- **Done when:** The shell has no conditional logic besides error mapping, and the clipboard rule has a test.

### plan:invariant-gaps

- **Do:** Close every **Gap** in `docs/INVARIANTS.md`. An extension test asserts the exact manifest permissions. An eslint config enables `jsx-a11y` at error level, and a gate runs it. A test pins the CSP. Tests cover idle lock and save-before-lock.
- **Why:** A rule without a test is a hope.
- **Done when:** `docs/INVARIANTS.md` contains no **Gap** line.

### plan:core-modules

- **Do:** Group the flat modules of `crates/vardo-core/src/` by capability: session, sync, drive, browser, and small services such as TOTP, health, favicon and the generator. Give each module a `//!` header stating its contract and the `inv:` tags it keeps. Split `crates/vardo-core/src/session.rs`.
- **Why:** An agent should find the contract of a module from its first lines, without reading the rest.
- **Done when:** Every core module opens with a contract header, and no core source file passes 500 lines.

### plan:macos-shell

- **Do:** Commit the tray template icon in `app/src-tauri/src/tray.rs` and the Dock reopen handler in `app/src-tauri/src/lib.rs`, both uncommitted in the working tree. First a person checks the tray icon, its menu, and close-then-Dock-click (`docs/RUNBOOKS.md`).
- **Done when:** Both are committed, and `fact:platform-macos` records what the person saw.

### plan:large-ui-files

- **Do:** Split `app/src/components/EntryDetail.tsx` and `app/src/ipc/mock.ts` when next touched.
- **Why:** Large files cost an agent a full read for a small edit.

## Phase 2: product

In rough priority order. Each needs its own design pass before code.

### plan:save-login

- Offer to save a login after sign-up or a password change.
- Open question: the extension has no page access until the user acts (`inv:extension-sees-nothing-until-asked`). The design must keep that.

### plan:os-keyring

- Store the Drive refresh token in the macOS Keychain, Secret Service or DPAPI instead of a mode 600 file.

### plan:instance-lock

- A lock file so a second Vardo window on the same vault learns at once. Today the second window's save refuses later (`inv:atomic-verified-save`).

### plan:merge-properties

- Property tests that merging twice changes nothing the second time, and that merging A into B and B into A converge.

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
