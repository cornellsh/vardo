<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/hero-dark.png">
  <img src="docs/images/hero-light.png" alt="Vardo. Your passwords on every device, always in sync. The app shows a list of logins, each with its site's icon.">
</picture>

<p align="center">
  <a href="#download"><img height="28" alt="Windows: Vardo app" src="https://img.shields.io/badge/Windows-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgZmlsbD0id2hpdGUiPjxwYXRoIGQ9Ik0wIDBoMTEuNHYxMS40SDB6TTEyLjYgMEgyNHYxMS40SDEyLjZ6TTAgMTIuNmgxMS40VjI0SDB6TTEyLjYgMTIuNkgyNFYyNEgxMi42eiIvPjwvc3ZnPg=="></a>
  <a href="#download"><img height="28" alt="macOS: Vardo app" src="https://img.shields.io/badge/macOS-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=apple"></a>
  <a href="#download"><img height="28" alt="Linux: Vardo app" src="https://img.shields.io/badge/Linux-Vardo_app-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=linux"></a>
  <br>
  <a href="#download"><img height="28" alt="Chrome, Edge, Brave: Vardo extension" src="https://img.shields.io/badge/Chrome%2C_Edge%2C_Brave-Vardo_extension-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=googlechrome"></a>
  <a href="https://apps.apple.com/app/keepassium-password-manager/id1435127111"><img height="28" alt="iPhone and iPad: KeePassium" src="https://img.shields.io/badge/iPhone_%26_iPad-KeePassium-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=apple"></a>
  <a href="https://play.google.com/store/apps/details?id=com.kunzisoft.keepass.free"><img height="28" alt="Android: KeePassDX" src="https://img.shields.io/badge/Android-KeePassDX-1b7db7?style=flat-square&labelColor=24292f&logoColor=white&logo=android"></a>
</p>

<p align="center">
  <a href="https://github.com/cornellsh/vardo/actions/workflows/ci.yml"><img alt="Tests" src="https://github.com/cornellsh/vardo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="#license"><img alt="License: MIT or Apache 2.0" src="https://img.shields.io/badge/license-MIT_or_Apache_2.0-24292f"></a>
</p>

Vardo keeps your passwords in one encrypted file, in your Google Drive or on your own server. The app, the browser extension and the KeePass app on your phone all open that file, so every device sees the same list.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/tile-quick-dark.png">
    <img width="32%" src="docs/images/tile-quick-light.png" alt="Any password, from anywhere. Press Ctrl+Shift+Space in any app to search your logins.">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/tile-health-dark.png">
    <img width="32%" src="docs/images/tile-health-light.png" alt="Fix weak and reused passwords. Vardo lists the passwords used more than once and the ones that are easy to guess.">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/tile-sync-dark.png">
    <img width="32%" src="docs/images/tile-sync-light.png" alt="You decide when devices disagree. Vardo shows what changed on each device and asks which version to keep.">
  </picture>
</p>

## What it does

- Fills your logins in the browser.
- Makes strong passwords and flags weak or reused ones.
- Stores two-factor codes, notes and files.
- Finds any password from anywhere with Ctrl+Shift+Space.
- Locks itself and clears copied passwords.

## Private

No account. No subscription. No Vardo server. Your file is encrypted before it leaves your device, and only your master password opens it. If you forget that password, nobody can recover it.

## Already use KeePass?

Vardo opens your existing file as it is. Nothing to import.

## Download

Coming soon. Until then, you can build it yourself.

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
