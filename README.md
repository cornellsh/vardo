<p align="center">
  <img src="app/src-tauri/icons/icon.svg" width="112" alt="The Vardo logo">
</p>

<h1 align="center">Vardo</h1>

<p align="center">Your passwords on every device, always in sync.</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/vardo-dark.png">
  <img src="docs/images/vardo-light.png" alt="Vardo showing a list of logins, each with its site's icon, and the details of one login">
</picture>

---

## Works everywhere

| Device | Use |
| --- | --- |
| Windows, macOS, Linux | The Vardo app |
| Chrome, Edge, Brave | The Vardo browser extension |
| iPhone, iPad | [KeePassium](https://keepassium.com) |
| Android | [KeePassDX](https://www.keepassdx.com) |

All of them open the same password file, so every device sees the same list.

## Sync

Keep your passwords in your Google Drive or on your own server. Change a password on one device and the others get it. If two devices change the same login, Vardo asks which one to keep.

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
