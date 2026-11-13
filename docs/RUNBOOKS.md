# Runbooks

Procedures a test cannot do alone: they need a person, a live account, a real screen, or a fresh machine.
Each runbook says when to use it. Keep the steps exact and short.

## Set up a machine

Use on a new machine, before `cargo xtask check`.

1. Rust through rustup: `rustup toolchain install stable 1.89 --profile minimal -c clippy,rustfmt`.
2. Node 22. CI uses 22. Run `npm ci` in `app/` and in `extension/`.
3. System libraries:
   - Arch: `sudo pacman -S --needed webkit2gtk-4.1 base-devel librsvg libayatana-appindicator`
   - Debian and Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev build-essential librsvg2-dev libayatana-appindicator3-dev`
   - Fedora: `sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel`
   - macOS: `xcode-select --install`
   - Windows: Visual Studio Build Tools with "Desktop development with C++".
4. `keepassxc-cli` for the interop tests. On macOS, `brew install --cask keepassxc` links it.
5. A browser for the e2e: `npx @puppeteer/browsers install chrome@stable --path ~/.cache/chrome-for-testing`. The check finds it there. Elsewhere, point `CHROMIUM` at the binary.
6. Secrets, never in the repo: put `oauth-client.env` (exports `VARDO_CLIENT_ID` and `VARDO_CLIENT_SECRET`) and `extension-key.pem` in `~/.config/vardo/`, dir mode 700, files mode 600.

## Create a Google OAuth client

Use when no `oauth-client.env` exists. About ten minutes, free.

1. In the Google Cloud console, create a project and select it.
2. Enable the Google Drive API and the Google Picker API.
3. In Google Auth Platform, press Get started. Give an app name and support email, choose audience External, add contact email, create.
4. Under Audience, press Publish app. Testing mode expires sign-ins after 7 days (`fact:drive-file-scope-no-verification`).
5. Under Clients, create a client of type Desktop app. No redirect URI is needed, since desktop apps use a loopback address.
6. Write the ID and secret into `~/.config/vardo/oauth-client.env` as two `export` lines, mode 600.

The desktop client secret is not secret, per Google, since anything built into a program can be read out. Still keep it out of git, because it spends the project's quota.

## Build and try the app safely

1. `set -a; . ~/.config/vardo/oauth-client.env; set +a` so the build embeds Drive support.
2. `cd app && npm run app:release`. The binary is `target/release/vardo-app`.
3. `cp fixtures/test_db_kdbx4_with_password_argon2id.kdbx /tmp/demo.kdbx`, then `target/release/vardo-app /tmp/demo.kdbx`. The password is `demopass`.
4. If Drive shows "Unavailable: this build has no Google client id", step 1 did not reach the build.

## Install the browser host and extension

1. `cargo build --release -p vardo-browser`.
2. Put `vardo-browser-host` next to `vardo-app`. The host starts the app from its own directory when the user asks.
3. In Vardo's settings, Browser extension, press Connect. It registers the host beside the app with every Chromium-family browser whose config dir exists. Disconnect removes it. `vardo-browser-host --install` and `--uninstall` do the same from a terminal, through the same code (`crates/vardo-browser/src/registration.rs`).
4. In `chrome://extensions`, enable Developer mode, choose Load unpacked, and pick `extension/`. The id is always `fkemoafedkbgljikdhjbglhlhfplamll`.
5. On a sign-in page, press Alt+Shift+L.
6. Firefox: `cd extension && npm run build:firefox`, then in `about:debugging`, This Firefox, Load Temporary Add-on, pick `extension/dist/firefox/manifest.json`. Connect registers Firefox too. Its popup link for setting a shortcut opens a Chrome page, so set the key in `about:addons` instead.

## Add Vardo to the Linux application menu

```sh
install -Dm755 target/release/vardo-app ~/.local/bin/vardo
install -Dm644 app/src-tauri/icons/icon.svg ~/.local/share/icons/hicolor/scalable/apps/vardo.svg
```

Then write `~/.local/share/applications/vardo.desktop` with `Exec=$HOME/.local/bin/vardo %f`, `Icon=vardo`, `Categories=Utility;Security;`, `MimeType=application/x-keepass2;` and `StartupWMClass=app.vardo.desktop`.

## Check the desktop shell by hand

Use after changing `app/src-tauri`, and on each platform before a release. No automated test reaches the tray or the window.

1. Launch the demo vault as above.
2. The tray icon is visible in light and dark menu bars. It is outlined when locked and filled when unlocked.
3. The tray menu offers Show, Lock now and Quit, and each works.
4. Closing the window keeps Vardo running. Clicking the tray icon, or the Dock icon on macOS, brings it back.
5. Unlock with `demopass`, copy a password, and confirm the clipboard clears after 15 seconds.
6. From another app, press Cmd+Shift+Space (Ctrl+Shift+Space elsewhere). The quick window opens over it. Type a title, press Enter, and confirm the password is on the clipboard and the window is gone. Esc and clicking elsewhere close it. While locked, Enter brings up the main window.
7. Record the result, dated, in the platform's entry under "Platforms" in `docs/FACTS.md`.

An agent can do this only with Screen Recording and Accessibility permission for its terminal (`fact:platform-macos`). Otherwise ask the user.

## Run the live Drive contract

Use when Google changes Drive, or to verify an entry under "Google Drive" in `docs/FACTS.md` marked **Unverified**. Needs a throwaway Google account, never a personal one.

```sh
cargo run -p vardo-drive --features network --example contract -- \
    --client-id <id> --client-secret <secret>
```

- The harness creates its own file and never opens the picker, so `drive.file` reaches nothing else in the account.
- Every destructive call re-reads the file name and aborts unless it starts with `vardo-contract-test-`. It deletes the file when done, also after a failure.
- `crates/vardo-core/examples/live_drive.rs` and `crates/vardo-core/examples/live_unhappy.rs` drive the full session against live Drive. They read `VARDO_CLIENT_ID` and `VARDO_CLIENT_SECRET` from the environment.

## Fuzz the parser

Use after any change to `crates/vardo-kdbx/src/xml/` or the header code.

```sh
cargo +nightly fuzz run xml_roundtrip -- -max_total_time=300
```

| Target | Asserts |
| --- | --- |
| `xml_parse` | The parser never panics, hangs or overflows the stack |
| `xml_roundtrip` | Any input the parser accepts re-serialises byte for byte, and reparsing our output is idempotent |
| `header` | The outer header and VariantDictionary survive arbitrary bytes |
| `container` | Whole-file parsing, with KDF limits tightened |

Every crash becomes a regression test in `crates/vardo-kdbx/tests/` before the fix lands.

## Cross-check against the keepass crate

`cargo run -p vardo-conformance -- roundtrip <file.kdbx> [password]` passes a file through the `keepass` crate and diffs the XML against Vardo's own reading (`dec:own-kdbx-layer`). `read` decrypts to raw XML and reports its shape. `dump` and `secrets` also exist. The password defaults to `demopass`. Use fixtures only, since `secrets` prints decrypted values.

## Report a security problem

Use GitHub's private vulnerability reporting on `cornellsh/vardo`, or email kornelshakh@proton.me with "Vardo security" in the subject. Never attach a real vault or a real master password.
