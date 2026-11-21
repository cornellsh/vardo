<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/hero-dark.png">
  <img src="docs/images/hero-light.png" alt="Vardo. Your passwords on every device, always in sync. The app shows a list of logins, each with its site's icon.">
</picture>

<p align="center">
  <a href="#1-on-your-computer"><img height="28" alt="Windows: Vardo app" src="https://img.shields.io/badge/Windows-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgZmlsbD0id2hpdGUiPjxwYXRoIGQ9Ik0wIDBoMTEuNHYxMS40SDB6TTEyLjYgMEgyNHYxMS40SDEyLjZ6TTAgMTIuNmgxMS40VjI0SDB6TTEyLjYgMTIuNkgyNFYyNEgxMi42eiIvPjwvc3ZnPg=="></a>
  <a href="#1-on-your-computer"><img height="28" alt="macOS: Vardo app" src="https://img.shields.io/badge/macOS-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=apple"></a>
  <a href="#1-on-your-computer"><img height="28" alt="Linux: Vardo app" src="https://img.shields.io/badge/Linux-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=linux"></a>
  <br>
  <a href="#2-in-your-browser"><img height="28" alt="Chrome, Edge, Brave: Vardo extension" src="https://img.shields.io/badge/Chrome%2C_Edge%2C_Brave-Vardo_extension-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=googlechrome"></a>
  <a href="#2-in-your-browser"><img height="28" alt="Firefox: Vardo extension" src="https://img.shields.io/badge/Firefox-Vardo_extension-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=firefoxbrowser"></a>
  <a href="https://apps.apple.com/app/keepassium-password-manager/id1435127111"><img height="28" alt="iPhone and iPad: KeePassium" src="https://img.shields.io/badge/iPhone_%26_iPad-KeePassium-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=apple"></a>
  <a href="https://play.google.com/store/apps/details?id=com.kunzisoft.keepass.free"><img height="28" alt="Android: KeePassDX" src="https://img.shields.io/badge/Android-KeePassDX-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=android"></a>
</p>

<p align="center">
  <a href="https://github.com/cornellsh/vardo/actions/workflows/ci.yml"><img alt="Tests" src="https://github.com/cornellsh/vardo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="#license"><img alt="License: MIT or Apache 2.0" src="https://img.shields.io/badge/license-MIT_or_Apache_2.0-24292f"></a>
</p>

Vardo keeps your passwords in one encrypted file, in your Google Drive or on your own server. The app, the browser extension and the KeePass app on your phone all open that file, so every device sees the same list.

## What it does

- Fills your logins in the browser.
- Makes strong passwords and flags weak or reused ones.
- Stores two-factor codes, notes and files.
- Finds any password from anywhere with Ctrl+Shift+Space.
- Locks itself and clears copied passwords.

## Private

No account. No subscription. No Vardo server. Your file is encrypted before it leaves your device, and only your master password opens it. If you forget that password, nobody can recover it.

## Set up

All your devices open one password file. Put it in Google Drive or on your own WebDAV server, then point every device at it.

Already use KeePass? Use the file you have. Nothing to import. Don't have one yet? Create it in KeePassium or KeePassDX on your phone (step 3) and save it to Google Drive.

### 1. On your computer

Download Vardo from [Releases](https://github.com/cornellsh/vardo/releases/latest).

| Computer | File |
| --- | --- |
| Mac with Apple Silicon | `Vardo_…_aarch64.dmg` |
| Mac with Intel | `Vardo_…_x64.dmg` |
| Windows | `Vardo_…_x64-setup.exe` |
| Linux | `.deb` or `.AppImage` |

1. Install it. The builds aren't signed, so your computer asks once:
   - **Mac:** drag Vardo to Applications and open it. When macOS refuses, go to System Settings → Privacy & Security and click Open Anyway.
   - **Windows:** run the installer. When Windows warns you, click More info → Run anyway.
   - **Linux:** install the `.deb`, or make the `.AppImage` executable and run it.
2. Choose where your file lives: **Google Drive**, **WebDAV**, or **Open a file** for one on this computer.
3. Type your master password.

Do the same on every computer, and pick the same file each time.

### 2. In your browser

Download the extension for your browser from [Releases](https://github.com/cornellsh/vardo/releases/latest) and unzip it into a folder you'll keep.

| Browser | File |
| --- | --- |
| Chrome, Edge, Brave, Vivaldi | `vardo-extension-chrome-….zip` |
| Firefox | `vardo-extension-firefox-….zip` |

1. Add it to your browser:
   - **Chrome, Edge, Brave, Vivaldi:** open the extensions page, turn on Developer mode, click Load unpacked, and pick the folder.
   - **Firefox:** open `about:debugging`, click This Firefox → Load Temporary Add-on, and pick `manifest.json` in the folder. Firefox forgets it when it restarts, until Mozilla signs it.
2. In Vardo, open Settings and click **Connect** next to the browsers.
3. On a login page, click the Vardo button in the toolbar, or press Alt+Shift+L to fill.

The extension asks the Vardo app for each password, so keep the app running.

### 3. On your phone

Vardo has no phone app. Your phone opens the same file in a KeePass app.

1. Install [KeePassium](https://apps.apple.com/app/keepassium-password-manager/id1435127111) on iPhone or iPad, or [KeePassDX](https://play.google.com/store/apps/details?id=com.kunzisoft.keepass.free) on Android.
2. Open your file from Google Drive or your WebDAV server. KeePassDX opens files through Android's file picker, so install the Google Drive app first.
3. Turn on autofill:
   - **iPhone:** Settings → General → AutoFill & Passwords, then turn on KeePassium.
   - **Android:** Settings → Passwords & accounts → Autofill service, then pick KeePassDX.

Change a password anywhere and every device sees it the next time it syncs. If two devices changed the same login, Vardo shows both and asks which to keep.

<details>
<summary>Build from source</summary>

You need Rust 1.89+, Node 22, and your system's build tools (on Linux, WebKitGTK 4.1 and an AppIndicator library).

```sh
git clone https://github.com/cornellsh/vardo
cd vardo/app
npm ci
npm run app:release
```

The app lands in `target/release/vardo-app`. Try it with `fixtures/test_db_kdbx4_with_password_argon2id.kdbx`, password `demopass`.

Google Drive sync needs your own Google sign-in key ([how](docs/RUNBOOKS.md#create-a-google-oauth-client)). The browser extension is in `extension/`; connect it from Settings in the app.

</details>

## License

[Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), your choice.
