<p align="center">
  <img src="app/src-tauri/icons/icon.svg" width="112" alt="The Vardo logo, a small wagon seen from behind">
</p>

<h1 align="center">Vardo</h1>

<p align="center">
  A free password manager for Windows, macOS and Linux.<br>
  Your passwords, on all your computers, in a file that belongs to you.
</p>

---

Vardo remembers your passwords so you don't have to. You pick one master password, and Vardo keeps everything else locked behind it.

All your passwords live in a single encrypted file. Keep it in your Google Drive or on your own server, and every computer you use sees the same list. Change a password on your laptop, and it's waiting on your desktop.

Vardo is free and open source. You don't sign up for anything, there's no subscription, and nothing about you gets sent anywhere. There isn't even a Vardo server: your file goes straight to the storage you chose, and only you can open it.

## What it does

- Keeps your passwords, notes, two-factor codes and attached files behind one master password.
- Syncs between your computers through Google Drive or your own WebDAV server.
- Fills passwords in Chrome, Edge, Brave and other Chromium browsers, when you ask it to.
- Makes strong new passwords for you, and points out the ones you've reused or that are too weak.
- Finds any password from anywhere. Press Ctrl+Shift+Space (Cmd+Shift+Space on a Mac), type a few letters and press Enter.
- Locks itself when you walk away, and clears a copied password from the clipboard after 15 seconds.

If you change the same login on two computers before they sync, Vardo doesn't quietly throw one away. It shows you both versions and lets you choose.

## On your phone

Vardo is made for computers. On your phone, use [KeePassium](https://keepassium.com) on iPhone and iPad, or [KeePassDX](https://www.keepassdx.com) on Android, and open the same file from your Google Drive.

That works because Vardo saves your passwords in the KeePass format, which is used by many apps. If you already use KeePass or KeePassXC, Vardo opens your existing file as it is. You can use both side by side, and you can stop using Vardo whenever you like without exporting anything.

## Is it safe?

Your file is encrypted on your computer before it goes anywhere. Google, or whoever stores it, only ever holds a scrambled file that is useless without your master password.

The browser extension can't look through your passwords. It gets one login only when you choose to fill it, and it never fills anything by itself. A page can't trick it into typing your password into a hidden box.

Two honest limits. No password manager can protect you from a virus that is already running on your computer, and Vardo doesn't pretend to. And if you forget your master password, nobody can get your passwords back, including us. Write it down and keep it somewhere safe.

## Get Vardo

Vardo is young, and there are no downloads yet. If you're comfortable with a terminal, you can build it yourself today.

<details>
<summary>Build Vardo from source</summary>

You need Rust 1.89 or newer, Node 22, and your system's build tools:

| System | Command |
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

The app is `target/release/vardo-app`. To look around without touching your real passwords, open the demo file. Its password is `demopass`.

```sh
cp ../fixtures/test_db_kdbx4_with_password_argon2id.kdbx /tmp/demo.kdbx
../target/release/vardo-app /tmp/demo.kdbx
```

Google Drive sync needs your own free Google sign-in key. Set `VARDO_CLIENT_ID` and `VARDO_CLIENT_SECRET` when you build. [The runbook](docs/RUNBOOKS.md#create-a-google-oauth-client) walks you through getting them in about ten minutes. Without them, everything else still works.

To add the browser extension:

1. Run `cargo build --release -p vardo-browser`, and put `vardo-browser-host` next to `vardo-app`.
2. In Vardo, open Settings, then Browser extension, and press Connect.
3. Open `chrome://extensions`, turn on Developer mode, press Load unpacked, and pick the `extension/` folder.

</details>

## Help out

Found a bug or have an idea? Open an issue. Plain descriptions are welcome, no technical detail needed.

If you'd like to work on the code, start with [AGENTS.md](AGENTS.md). It explains how Vardo fits together, and `cargo xtask check` runs every test. Please never test with a real password file. Use the ones in `fixtures/` with the password `demopass`.

## Found a security problem?

Please report it privately through GitHub's vulnerability reporting on this repository, or email kornelshakh@proton.me with "Vardo security" in the subject. Never send a real password file or a real master password.

## License

Free to use, change and share, under your choice of the [Apache License 2.0](LICENSE-APACHE) or the [MIT License](LICENSE-MIT).
