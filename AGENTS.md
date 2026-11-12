# Vardo

Vardo is a desktop password manager for KeePass `.kdbx` files.
It syncs one vault across devices through Google Drive without losing edits.
It fills logins in Chromium browsers through its own extension.
Rust owns every byte of the vault. The window and the extension see metadata, and a secret only at the moment of use.

This file loads into every agent session in the repo. It is the map. Everything else is one hop away.

## Where knowledge lives

| Question | Source | Tag |
| --- | --- | --- |
| What is true right now: tests, lints, CI | Run it. `cargo xtask check`, `gh run list` | none |
| How the parts form one system | `docs/ARCHITECTURE.md` | none |
| Rules that must never break, and the test that proves each | `docs/INVARIANTS.md` | `inv:` |
| Choices made, why, what lost, when to reopen | `docs/DECISIONS.md` | `dec:` |
| Facts about the outside world, and how each was verified | `docs/FACTS.md` | `fact:` |
| What to do next, in order, and when it counts as done | `docs/PLAN.md` | `plan:` |
| Steps that need a person, a live account, or a real screen | `docs/RUNBOOKS.md` | none |

Tags are the links between docs, code and tests.
`git grep -n inv:secrets-stay-in-rust` finds the rule and every comment that cites it. The rule's entry names its tests as `path::function`.
When code keeps or relies on a rule, cite its tag in a comment there.
The `doc-links` gate fails when a tag is cited but never defined, or a doc names a file or test that does not exist.

Docs never state counts, pass rates or "done". Those rot. Run the command instead.

## Commands

| Need | Command |
| --- | --- |
| Everything CI checks | `cargo xtask check`. One line per gate, full output in `target/check/<gate>.log` |
| Quick loop while editing | `cargo xtask check --fast` (doc links, fmt, clippy and tests of changed crates) |
| One gate | `cargo xtask check --only clippy`. Add `--json` for a machine-readable summary |
| One crate | `cargo test -p vardo-merge` |
| Lint as CI does | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Interface tests | `cd app && npx vitest run` |
| Interface in a browser, no Rust | `cd app && npm run build && npm run preview` |
| Extension in a real browser | `cd extension && CHROMIUM=<binary> npm run e2e` |
| Release binaries | `cd app && npm run app:release`, then `cargo build --release -p vardo-browser` |

Setup facts that cost failed runs:

- The check skips the app and extension gates unless `npm ci` ran in `app/` and `extension/`. It prints each skip and its reason.
- The e2e gate uses `$CHROMIUM`, else the newest Chrome for Testing under `~/.cache/chrome-for-testing` or `~/.cache/puppeteer`, else a Chromium on the path. Branded Chrome ignores `--load-extension`, so it is never picked.
- Compiling `vardo-app` needs the built interface in app/dist, which git ignores. The check writes a placeholder when it is missing. A bare `cargo build` does not.
- CI lints with the latest stable Rust. A new release can add a lint that fails `main` with no code change (`fact:clippy-moves`).
- The minimum Rust version is 1.88, in `Cargo.toml`. CI builds with Node 22.
- Some Rust tests call `keepassxc-cli` and skip without it. CI installs it.

## Test data

- Demo vault: `fixtures/test_db_kdbx4_with_password_argon2id.kdbx`, password `demopass`. Copy it to `/tmp` before opening it in the app.
- The interface mock in `app/src/ipc/mock.ts` serves a messy vault of about 400 entries from `app/src/ipc/mockData.ts`. Its password is also `demopass`.

## Rules

- Secrets stay in Rust (`inv:secrets-stay-in-rust`).
- Never open, read or ask for a real vault or its master password. Use `fixtures/` and `demopass`.
- Never commit secrets. The OAuth client and the extension signing key live in `~/.config/vardo/`, mode 600 (`docs/RUNBOOKS.md`).
- No Python. Scripts are Rust, Node or POSIX shell.
- No middot character anywhere: code, interface text or docs.
- A change to behaviour comes with a test that fails without it.
- A new rule gets an `inv:` entry that names its test. A reversed choice gets a new `dec:` entry, and the old one is marked superseded.
- Commit messages say why. The diff already says what.

## Where to start a change

| Change | Start in | Prove with |
| --- | --- | --- |
| File format, parsing, encryption | `crates/vardo-kdbx` | its `tests/`, and the fuzz targets in `fuzz/` |
| Entries, groups, history, attachments | `crates/vardo-vault` | `crates/vardo-vault/tests/edit.rs` |
| Merge rules | `crates/vardo-merge` | `crates/vardo-merge/tests/scenarios.rs` |
| Sync protocol, storage backends | `crates/vardo-sync` | `crates/vardo-sync/tests/races.rs`, `crates/vardo-sync/tests/chaos.rs` |
| Google Drive and sign-in | `crates/vardo-drive` | `crates/vardo-drive/tests/backend.rs` offline. Live runs are in RUNBOOKS |
| Any user-level operation | `crates/vardo-core`, starting at `Session` | `crates/vardo-core/tests/` |
| Extension messages, URL matching, the host binary | `crates/vardo-browser` | `crates/vardo-browser/src/matching.rs`, `crates/vardo-core/tests/browser.rs` |
| Window, tray, clipboard, command wrappers | `app/src-tauri` | A release build, then RUNBOOKS by hand |
| Screens and components | `app/src` | vitest and the contrast gate |
| Extension popup and filling | `extension/src` | vitest and the e2e |
