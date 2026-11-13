# Vardo

Vardo is a desktop password manager for KeePass files.
It syncs one vault across your computers without losing an edit.

Your passwords stay in an ordinary `.kdbx` file. KeePassXC and KeePassium open it unchanged.
Vardo adds sync through Google Drive or WebDAV, a merge you can review, and browser filling that waits until you ask.

> **Early software.** Version 0.0.1 has no released builds, so you build it from source.
> Linux is the daily-use platform. macOS builds and runs. On Windows only the core tests run, and the app itself is untested.

## Why Vardo

- **Your file, your provider.** Vardo has no account, no server and no telemetry. If Vardo disappeared tomorrow, your file would work exactly as before.
- **Edits on two computers both survive.** Each computer edits its own copy. Vardo merges them against the last state both agreed on, shows you the result, and asks you when it cannot tell which change wins.
- **The browser sees only what it fills.** The extension reads nothing until you click it or press its shortcut. It never submits a form for you.

## Features

### The vault

- Opens KDBX 3.1, 4.0 and 4.1, and saves 4.x in the version it read.
- Keeps every part of the file it does not understand, byte for byte.
- Saves through a temporary file, checks it, then replaces the original and keeps five backups.
- Refuses to save over a file that another program changed since Vardo opened it.
- Keeps a history of every edit, and restores an old version in one click. A restore is itself undoable.
- Moves groups by drag and drop, or from the keyboard with "Move to…".
- Shows TOTP codes, generates passwords, reports reused, weak and expired passwords, and stores attachments up to 8 MB.
- Fetches a site's icon only when you ask, from the site itself, never from a third-party icon service.

### Sync

- **Google Drive.** Vardo asks only for access to the one file you pick, not your whole Drive.
- **WebDAV.** Vardo writes only if the file is unchanged on the server, and refuses servers that cannot promise that. It is tested against Apache's `mod_dav`.
- Before anything is written, a review screen lists every change. Conflicts wait for your decision.
- Server passwords and Drive sign-ins live in your system keychain when one is available.

### In the browser

- The extension works in Chrome and other Chromium browsers. A Firefox build exists but is experimental.
- Press Alt+Shift+L on a sign-in page, or click the toolbar button, to see matching logins and fill one.
- It fills frames from the page's own site and skips hidden ones, so a page cannot trick it into typing into an invisible box.
- It saves a login you typed on a page, and asks before it replaces an existing password.
- It makes a new password for a sign-up form, and saves that password before it types it.

### Every day

- Press Cmd+Shift+Space on macOS, or Ctrl+Shift+Space elsewhere, from any app. Search, then press Enter to copy the password or Shift+Enter to copy the username.
- Press Cmd+K or Ctrl+K for a command palette inside the app.
- Copied passwords clear from the clipboard after 15 seconds.
- The vault locks after five idle minutes, on Ctrl+L, or from the tray. It saves your changes first.
- Vardo warns you when the same vault is already open in another window.
- The interface follows Telegram Desktop's design, and every action works from the keyboard.

## Security model

Vardo protects you against a lost laptop, a compromised cloud account and a hostile network.

It does not claim to protect you from malware running as your own user. Such malware can read memory and log keystrokes, so no password manager can stop it. For the same reason, any program running as you can talk to Vardo's browser connection.

Inside the app, only the Rust core ever holds a password. The window and the extension receive titles and usernames. They receive a secret only for the moment you copy, reveal or fill it.

## What Vardo does not do

- **Phones and tablets.** Use [KeePassium](https://keepassium.com), which opens the same file from Google Drive.
- **Key files.** A vault needs a master password, and only a master password.
- **Automatic filling.** Nothing fills when a page loads, and no icons appear inside form fields.
- **Breach checks.** Vardo does not send anything about your passwords to an outside service.

## Build from source

You need Rust 1.89 or newer, Node 22, and your platform's build tools:

| Platform | Command |
| --- | --- |
| Arch | `sudo pacman -S --needed webkit2gtk-4.1 base-devel librsvg libayatana-appindicator` |
| Debian, Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev build-essential librsvg2-dev libayatana-appindicator3-dev` |
| Fedora | `sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel` |
| macOS | `xcode-select --install` |
| Windows | Visual Studio Build Tools with "Desktop development with C++" |

Then build the app:

```sh
git clone https://github.com/cornellsh/vardo
cd vardo/app
npm ci
npm run app:release
```

The app is `target/release/vardo-app`. To try it safely, open the demo vault with the password `demopass`:

```sh
cp ../fixtures/test_db_kdbx4_with_password_argon2id.kdbx /tmp/demo.kdbx
../target/release/vardo-app /tmp/demo.kdbx
```

**Google Drive** needs your own free Google OAuth client. Set `VARDO_CLIENT_ID` and `VARDO_CLIENT_SECRET` when you build. Without them, Drive shows as unavailable, and local files and WebDAV still work. Creating a client takes about ten minutes, and [the runbook](docs/RUNBOOKS.md#create-a-google-oauth-client) lists the steps.

**The browser extension** needs its helper program next to the app:

1. Run `cargo build --release -p vardo-browser`, and put `vardo-browser-host` beside `vardo-app`.
2. In Vardo, open Settings, then Browser extension, and press Connect.
3. In `chrome://extensions`, turn on Developer mode, press Load unpacked, and pick the `extension/` folder.

On GNOME, the tray icon appears only with the AppIndicator extension.

## Contributing

`cargo xtask check` runs every check that CI runs. It prints one line per check and keeps the full output in `target/check/`.

The docs are written for coding agents first, and they read fine for people too. Start at [AGENTS.md](AGENTS.md):

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) shows how the parts fit together.
- [docs/INVARIANTS.md](docs/INVARIANTS.md) lists the rules that must never break, each with the test that proves it.
- [docs/DECISIONS.md](docs/DECISIONS.md) records each choice, what lost, and when to reopen it.
- [docs/PLAN.md](docs/PLAN.md) lists what comes next.

Never test with a real vault. Use the files in `fixtures/` and the password `demopass`.

## Reporting a security problem

Use GitHub's private vulnerability reporting on this repository, or email kornelshakh@proton.me with "Vardo security" in the subject. Never attach a real vault or a real master password.

## License

Vardo is available under either the [Apache License 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT), at your option.
