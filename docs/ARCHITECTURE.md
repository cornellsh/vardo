# Architecture

Vardo is one idea carried through every layer.
The vault is an ordinary KDBX file that the user owns.
Every device edits its own copy, and a merge that never guesses reconciles them.
One process holds the plaintext, and everything outside it asks for one thing at a time.

Read this file top to bottom once. After that, use the tower to find a layer and its contract.

## The tower

Each layer depends only on layers below it. A layer never reaches up.

```text
views        app/src (React)                 extension/src (popup, fill)
                 | Tauri IPC                      | native messaging
shells       app/src-tauri                   vardo-browser-host (bin of vardo-browser)
                 |                                | unix socket
application  vardo-core <-------------------------+
                 |
storage      vardo-sync  <--  vardo-drive (one StorageBackend)
reconcile    vardo-merge
model        vardo-vault
format       vardo-kdbx

contract     vardo-browser: protocol, framing, URL matching, socket path.
             Shared by vardo-core and the host. Depends on no workspace crate.
tools        vardo-conformance, fuzz/, extension/e2e, crates/*/examples
```

The `layers` gate enforces the tower from `cargo metadata`. Its allowed edges live in `xtask/src/layers.rs`. A new crate fails until it is placed there and here.

## The two organising patterns

**Core decides, shells act** (`dec:core-decides-shells-act`).
Every decision lives in `vardo-core` as plain Rust with plain tests.
A shell only marshals data and performs effects that need a toolkit: the clipboard, the window, the tray.
`vardo_core::browser::Effect` is the model: core returns what should happen, and `app/src-tauri/src/browser.rs` does it.
Logic that sits in a shell is untested by construction, so moving it down is always progress.

**The DOM is the truth, typed views are lenses** (`dec:dom-is-truth`).
`vardo-kdbx` keeps the whole XML document and every byte of formatting.
`vardo-vault` reads and edits entries through that document rather than through a closed struct.
Anything Vardo does not understand survives a save untouched (`inv:byte-stable-roundtrip`).

## Layer contracts

Each block says what the layer owns, its entry points, what it must never do, and where its proof lives.

### Format: `vardo-kdbx`

- Owns bytes to authenticated container to XML DOM, and back. Crypto, KDF limits, the protected-value stream.
- Entry points: `open`, `open_with_limits`, `Document`, `Credentials`, `Limits`, `write`, `protected::canonical_payload`.
- Never: normalises the document, upgrades the format, runs a KDF before checking its parameters against `Limits`.
- Proof: `crates/vardo-kdbx/tests/`, `fuzz/fuzz_targets/`.
- Reads KDBX 3.1, 4.0 and 4.1. Writes 4.x in the version it read. Key files, 3.1 writing and ARC4 are deliberately absent (`dec:password-only`).

### Model: `vardo-vault`

- Owns entries, groups, history, attachments, icons, tombstones, timestamps.
- Entry points: `Vault`, `Entry`, `Group`, `EntryEdit`, `Deletion`, `Uuid`, `Timestamp`.
- Never: edits ciphertext in place. An unlocked `Vault` holds plaintext and encrypts only at save (`inv:plaintext-while-unlocked`).
- Proof: `crates/vardo-vault/tests/`.

### Reconcile: `vardo-merge`

- Owns the three-way merge of base, local and remote vaults.
- Entry points: `plan` returns a `MergePlan` of `Change` and `Conflict` items, or a `Refusal`. `apply` executes a plan under a `ConflictPolicy`.
- Never: mutates during `plan`, merges fields across versions, deletes without a base, breaks a same-second tie by guessing.
- Proof: `crates/vardo-merge/tests/`.

### Storage: `vardo-sync` and `vardo-drive`

- `vardo-sync` owns the `StorageBackend` trait, the sync protocol, `LocalFile`, `FakeDrive` and cloud-mount detection.
- Entry points: `StorageBackend::write_if_unchanged`, `sync`, `SyncPoint`, `SyncReport`, `StorageRisk`.
- `vardo-drive` owns `DriveBackend`, OAuth with PKCE over a loopback redirect, and the `HttpTransport` seam. Its `network` feature adds the real HTTP client.
- Never: trusts Drive to refuse a write (`fact:drive-no-cas`), replaces a remote file instead of updating it in place.
- Proof: `crates/vardo-sync/tests/`, `crates/vardo-drive/tests/backend.rs` offline, live contract runs by hand.

### Contract: `vardo-browser`

- Owns the wire protocol between extension and app (`crates/vardo-browser/src/protocol.rs`), native-messaging framing, URL matching, the socket path, and the `vardo-browser-host` binary.
- Messages are JSON envelopes `{v, id, type, ...}`. `v` is the protocol version and `id` is echoed so replies match requests.
- The extension's copy of the protocol is checked against golden files Rust writes (`inv:protocol-has-one-truth`). After changing a message, run `UPDATE_FIXTURES=1 cargo test -p vardo-browser --test protocol_fixtures`.
- Never: holds a vault or a key. The host is a relay (`inv:host-is-a-relay`).
- Proof: unit tests in `crates/vardo-browser/src/matching.rs`, behaviour tests in `crates/vardo-core/tests/browser.rs`.

### Application: `vardo-core`

- Owns every user-level operation: unlock, list, edit, save, sync review, Drive sign-in and picking, recents, health, TOTP, the generator, favicons, and answering the extension.
- Modules by capability: `session/` (unlock, read, edit, save), `sync/` (preview, apply, review prose), `drive/` (sign-in, listing, picker), `browser/` (extension replies, socket), and one file per small service. Each opens with a `Contract:` line naming the `inv:` tags it keeps. The `core-modules` gate fails on a missing contract or a file over 500 lines.
- Entry point: `Session`. `Session::field_value` is the only way a secret leaves, as a `Zeroizing` buffer.
- View models in `crates/vardo-core/src/model.rs` have no field that can carry a protected value.
- Never: depends on Tauri or any toolkit.
- Proof: `crates/vardo-core/tests/`.

### Shells: `app/src-tauri` and the host

- `app/src-tauri` holds `#[tauri::command]` wrappers over `Session`, the tray, the clipboard and window events.
- Commands return core view models as JSON. Their TypeScript types are generated and their names are checked (`inv:ipc-types-are-generated`).

### Views: `app/src` and `extension/src`

- `app/src/ipc/` is the only door to Rust. `app/src/ipc/index.ts` picks `app/src/ipc/tauri.ts` inside the app and `app/src/ipc/mock.ts` in a plain browser. `app/src/ipc/types.ts` re-exports the generated `app/src/ipc/bindings.ts` and declares the `Ipc` interface.
- `app/src/screens/` holds Unlock, Vault, MergeReview and Health. `app/src/components/` holds the rest.
- `extension/src/` holds the popup, the background worker, the native-messaging client and the fill script. It is plain JavaScript with no build step.

## Flows

Each flow names the layers it crosses, top to bottom.

**Unlock.** The view sends a path and password. `Session::open` reads the bytes, `vardo-kdbx` checks KDF limits, derives the key off the UI thread with progress events, authenticates and decrypts. `vardo-vault` wraps the plaintext DOM. The view receives `VaultInfo`.

**Read.** The view asks for groups, entry lists and entry details. Core returns metadata only. A protected field arrives as its name and a flag.

**Use a secret.** Copy: core hands the value and a clipboard to `crates/vardo-core/src/clipboard.rs`, which writes it and clears it after 15 seconds if it still holds that value. The shell supplies only the system clipboard. Reveal: one field, on request. Fill: see the browser flow.

**Edit and save.** The view sends a `FieldEdit`. `vardo-vault` applies it and pushes one history record. Save writes a temp file, reopens and verifies it, renames it over the original, and keeps five backups. A save refuses if the file changed on disk since it was opened.

**Sync.** Core reads the remote state, then `vardo-merge` plans against the last `SyncPoint`. The user sees the plan in MergeReview. Apply merges, and the backend writes with `write_if_unchanged`. On Drive that means write, then check that the revision beneath ours was head when we wrote. If not, fold the buried write in and retry, with a bound.

**Fill a login.** The user clicks the toolbar button or presses Alt+Shift+L. The extension gets the tab URL from Chrome and sends it over native messaging. Chrome starts `vardo-browser-host`, which relays to the app's socket. Core matches the URL against saved logins. The popup shows titles, usernames and match kind. On fill, core checks the match again and returns username, password and TOTP. The fill script types them into the top frame and never submits.

**Save a login.** The user types a login into a page, opens the popup and clicks "Save the login typed on this page". Only then does the extension read the top frame (`extension/src/capture.js`). Core saves it under the page origin, or replaces the password of the login saved for exactly that site and username once the user confirms (`inv:save-asks-before-replacing`). The shell tells the window, which refreshes and saves like any other edit.

**Lock.** The tray, Ctrl+L, or five idle minutes. Unsaved changes are saved first. If that save fails, the vault stays unlocked and says so. Locking drops the `Vault` and every derived secret.

## Trust boundaries

| Boundary | What crosses | What keeps it |
| --- | --- | --- |
| Rust to WebView | Metadata, and one secret per explicit reveal | `inv:secrets-stay-in-rust` |
| App to extension | Titles, usernames and match kind. Secrets only to fill a matching page | `inv:browser-gets-secret-only-to-fill`, `inv:fill-rechecks-match` |
| Socket | Any process running as the same user can connect | Stated, not defended (`dec:threat-model`) |
| App to Drive | One vault file per `drive.file` grant | `dec:drive-first-backend` |
| Disk | Ciphertext only, written atomically | `inv:atomic-verified-save` |

## Where state lives

| State | Location |
| --- | --- |
| Recents, Drive tokens, sync points | `app.vardo.desktop` in the OS config dir: `~/.config` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows |
| Drive refresh token | A mode 600 file in that dir, not the OS keychain yet (`plan:os-keyring`) |
| Browser socket | `$XDG_RUNTIME_DIR/vardo/browser.sock`, else `$TMPDIR/vardo-$USER/browser.sock`. Dir 700, socket 600. Unix only |
| Native host manifests | `NativeMessagingHosts/app.vardo.browser.json` in each Chromium-family browser's config dir |
| Extension id | `fkemoafedkbgljikdhjbglhlhfplamll`, fixed by the `key` in `extension/manifest.json` |
| Build-time config | `VARDO_CLIENT_ID` and `VARDO_CLIENT_SECRET`, read by `option_env!` in `DriveClient::built_in` in `crates/vardo-core/src/drive/auth.rs`. Without them, Drive reports itself unavailable |

## Vocabulary

One term per thing. Code uses the left column. Interface text may use the right.

| Term | Means | User-facing word |
| --- | --- | --- |
| vault | One KDBX file and its contents | vault |
| entry | One KeePass entry | login |
| group | A KeePass group | folder or group |
| protected field | A field stored encrypted in the inner stream | password, secret |
| session | An unlocked vault held by `vardo-core` | unlocked |
| source | Where a vault lives: a local path or a Drive file (`VaultSource`) | |
| backend | A `StorageBackend` implementation | |
| sync point | The last state both sides agreed on, the merge base. Per device | |
| plan | What a merge would do, before it does it | review |
| conflict | A difference merge refuses to decide | needs a decision |
| tombstone | A `DeletedObject` record of a deletion | |
| host | `vardo-browser-host`, the native-messaging relay | helper |
| match | Exact or Subdomain, from `vardo_browser::matching` | |
| fill | Typing a login into a page, never submitting it | fill |

## Interface system

The look is a port of Telegram Desktop, value for value (`dec:telegram-port`).
Token values live in `app/src/styles/tokens.css` under Telegram's own names, so each traces to its source.

- One accent, `windowBgActive`, for selection, badges, primary buttons and focus.
- Two weights, regular and semibold. Hierarchy comes from colour, not weight or size.
- 13px base text, 14px in dialogs, 62px list rows.
- Hairlines and flat fills, not shadows. Selected rows take a solid accent fill.
- The rail is darker than the list, which is darker than the content.
- Secrets, TOTP codes and UUIDs use a monospace with a slashed zero. TOTP uses tabular numbers.
- Motion is feedback or explanation only. Base tempo 120ms, `easeOutCirc`. The ripple grows for 650ms and fades in 200ms from where the pointer landed.
- The list column is `clamp(windowWidth * 5 / 14, min, max)`. Three columns collapse to two, then one, at Telegram's thresholds.
- Base UI supplies every interactive primitive. Do not hand-roll a combobox, dialog, menu or popover.
- Empty states say something useful and carry no illustration.
- Every text and background pair passes the contrast gate (`inv:text-contrast`).
