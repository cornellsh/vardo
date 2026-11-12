# Decisions

Each entry is a choice that could have gone another way.
**Chose** is the decision. **Because** is the deciding reason. **Rejected** lists what lost and why. **Reopen if** names the evidence that would change it.

Do not relitigate a decision without its reopen condition. To reverse one, add a new entry and mark the old one `Superseded by dec:<new>`.
The long-form records these came from (ADR-001 to ADR-006, CRITIQUE, RESEARCH, PHASE0-FINDINGS, UI-PLAN) are in git history before the commit that added this file.

## Product

### dec:kdbx-file-is-the-product

- **Chose:** The vault is a standard KDBX file that KeePassXC and KeePassium open unchanged. No account, no Vardo server, no format of our own.
- **Because:** If Vardo disappeared, the user's file must work exactly as before.
- **Reopen if:** never. Every other decision assumes this one.

### dec:no-server-no-telemetry

- **Chose:** No server component, accounts, analytics, crash reporting that phones home, or third-party icon service.
- **Because:** Each one changes the trust model from "your file, your provider" to "trust us".
- **Reopen if:** never.

### dec:no-mobile

- **Chose:** Desktop only. KeePassium is the mobile client, so Drive interop with it matters.
- **Because:** Mobile would double the surface for a single maintainer, and KeePassium already does it well.
- **Reopen if:** KeePassium stops reading Drive-hosted vaults. That would also reopen `dec:tauri-webview-without-secrets`.

### dec:threat-model

- **Chose:** Protect against a lost laptop, a compromised cloud account and a hostile network. Do not claim protection from malware running as the user.
- **Because:** Same-user malware can read process memory and log keys. Claiming otherwise is the overclaim that discredits security tools.
- **Consequence:** The browser socket accepts any same-user process. A pairing secret on the same disk would add ceremony without protection.

### dec:license

- **Chose:** Apache-2.0 OR MIT. Merge semantics are clean-room.
- **Rejected:** Porting KeePassXC's `Merger.cpp`, which is GPL-2.0-or-later and would set the project's license.

## Format and model

### dec:own-kdbx-layer

- **Chose:** Write the KDBX layer ourselves, DOM-first. Keep the `keepass` crate as a differential oracle through `crates/vardo-conformance`.
- **Because:** The `keepass` crate cannot write KDBX 4.0 and normalises a 4.1 round trip (`fact:keepass-crate-normalises`). For a merge engine that means phantom conflicts on every save.
- **Rejected:** `keepass` (closed typed model), `keepass-core` (AI-generated, no users), `kdbxweb` (JavaScript, wrong side of the trust boundary).
- **Reopen if:** a Rust KDBX library guarantees byte-stable round trips by construction.

### dec:dom-is-truth

- **Chose:** The XML DOM is the source of truth. Typed entries and groups are views over it.
- **Because:** A closed struct cannot write back what it does not name, so unknown data from other clients would be lost.

### dec:password-only

- **Chose:** Master password only. No key files, hardware keys or challenge-response. No KDBX 3.1 writing. No ARC4 inner stream.
- **Because:** Every 3.1 reader also reads 4.x, and 3.1 lacks authenticated integrity. ARC4 has been broken since 2014. Key files were deferred, not refused.
- **Reopen if:** the user needs a key file or a hardware key.

## Sync

### dec:app-owns-network

- **Chose:** The app talks to storage over its API. The canonical local copy lives in app-private storage. Never sync through a cloud-mounted folder.
- **Because:** A mounted folder hides failed uploads, conflict forks and lock errors from the app (`fact:cloud-mounts-lose-data`).
- **Rejected:** "Local file done perfectly" over an rclone or Drive mount. It was the first recommendation and the evidence reversed it.

### dec:drive-first-backend

- **Chose:** Google Drive is the first backend, with the `drive.file` scope only, on a Published, External, unverified OAuth client.
- **Because:** The user's vault and KeePassium already live there. `drive.file` is non-sensitive, so it needs no verification and has no user cap (`fact:drive-file-scope-no-verification`).
- **Rejected:** The full `drive` scope, which is restricted and needs a paid security assessment. Push notifications via `changes.watch`, which need a public HTTPS webhook (`fact:drive-no-push`). Vardo polls.
- **Reopen if:** Drive billing makes polling costly, or the scope loses its non-sensitive status.

### dec:conditional-write-trait

- **Chose:** `StorageBackend` expresses the strongest operation, `write_if_unchanged`. Backends that can refuse a write override it. The default body is the write-then-detect emulation Drive forces.
- **Because:** Shaping the trait around Drive's weakness would make every future backend inherit it.
- **Rejected:** A trait that offered only plain writes.

### dec:webdav-second

- **Chose:** WebDAV is the next backend, then S3-compatible storage. Never a sync server of our own.
- **Because:** Both have real conditional writes. WebDAV also works with KeePassium.
- **Constraint:** WebDAV must handle the ETag hazards in `fact:webdav-etag-hazards`, with the `mod_deflate` case as an explicit test.

### dec:merge-is-the-safety-net

- **Chose:** A three-way, entry-level merge with a preview before anything changes. "Keep both" is the default conflict policy.
- **Because:** With no compare-and-swap on Drive, races are certain. Merge is the only thing between a race and data loss.
- **Rejected:** Field-level merging, which can assemble a credential that never existed (`inv:merge-whole-entries`).

## Application

### dec:tauri-webview-without-secrets

- **Chose:** Tauri 2, with the WebView as a viewer over typed commands that cannot return secrets.
- **Because:** The project is mostly Rust, and a password manager is mostly text fields, which WebViews handle well.
- **Rejected:** Electron (bundled Chromium and a Node process with full system access), Flutter (mobile is out of scope, and desktop text input and accessibility were weak), Qt (dated look, licensing friction), Slint (license fit), egui and iced (wrong for a design-led product), Compose Multiplatform (JVM, a third language).
- **Reopen if:** mobile enters scope, WebKitGTK becomes intolerable on Linux, or Flutter fixes Windows text input and desktop accessibility.

### dec:core-decides-shells-act

- **Chose:** Every decision lives in `vardo-core` with ordinary tests. Shells only marshal data and perform effects.
- **Because:** Save and sync once lived in the Tauri crate and could only be checked by driving a window and reading screenshots.

### dec:react-base-ui

- **Chose:** React 19 with the React Compiler, Base UI for every interactive primitive, Tailwind with Telegram tokens.
- **Because:** Base UI is complete, unstyled and maintained, so the look stays ours and focus management does not.
- **Rejected:** Svelte 5 (chosen first, superseded once Base UI shipped), Radix (stagnating), Bits UI (one maintainer), shadcn/ui (copy-paste components merged by hand forever).
- **Reopen if:** Base UI stops being maintained.

### dec:telegram-port

- **Chose:** The interface ports Telegram Desktop's palette, type, metrics and motion from tdesktop source, keeping Telegram's names. Where a value is missing, read the source instead of inventing one.
- **Because:** Telegram's rail, list and detail map one to one onto groups, entries and entry detail.
- **Deviations:** Monospace with slashed zero for secrets, TOTP and UUIDs. Contrast fixes recorded in `app/src/styles/tokens.css`.
- **Rules:** see "Interface system" in `docs/ARCHITECTURE.md`.

## Browser

### dec:own-extension

- **Chose:** Vardo ships its own Chromium extension built on one rule: it sees nothing until the user asks it to fill.
- **Because:** Autofill is what makes a password manager usable daily. Without it people reuse passwords.
- **Rejected:** Speaking KeePassXC-Browser's protocol. It runs in every frame of every page with broad permissions, injects clickable UI that pages can attack, registers its host under KeePassXC's name, and adds its own crypto layer (`fact:keepassxc-browser-permissions`, `fact:clickjacking-defcon33`).
- **Accepted costs:** No badge count before a click, no fill on page load, no icon inside fields, no iframe filling yet.

### dec:native-messaging-relay

- **Chose:** Extension to `vardo-browser-host` over native messaging, host to app over a user-private unix socket.
- **Because:** Chrome launches a native host only for the extension ids its manifest lists.
- **Rejected:** A localhost web server, which any website can reach.

### dec:fill-shortcut

- **Chose:** Alt+Shift+L. The popup notices a missing shortcut and links to the browser's shortcut page.
- **Because:** Chrome took Ctrl+Shift+L and refuses a suggested shortcut it owns, silently (`fact:chrome-extension-limits`).

### dec:save-on-request

- **Chose:** The popup has a "Save the login typed on this page" button. Clicking it reads the top frame once and sends the login to Vardo. Vardo adds it to the root group, titled with the host and saved under the page origin. The app writes it to storage like any other edit.
- **Because:** Offering to save after a sign-in needs a script watching every page, which `inv:extension-sees-nothing-until-asked` forbids. A button keeps the extension blind until the person acts.
- **Rejected:** Content scripts that detect form submission, which need host permissions on every site. Saving to storage from the socket thread, which would bypass the merge screen when the stored copy has moved.
- **Reopen if:** Chrome offers a submit event to extensions without page access.
- **Date:** 2026-11-13.

### dec:keyring-with-file-fallback

- **Chose:** `keyring-core` with one store per platform: the macOS Keychain, Windows Credential Manager, and the Secret Service over pure-Rust D-Bus. When no store answers, the refresh token falls back to a mode 600 file. Only the refresh token is kept, and access tokens are refreshed on use.
- **Because:** A file is readable by anything running as the user, and a keyring is not. Refusing to connect Drive on a desktop without a keyring would lose users for no gain, since the `drive.file` scope already bounds a stolen token.
- **Rejected:** The `v1` bundle of the `keyring` crate, which its own docs say applications should not link. The `dbus` crate, which needs libdbus at build time.
- **Reopen if:** A platform store loses tokens across updates, or a user needs Drive where the fallback file is unacceptable.
- **Date:** 2026-11-13.

### dec:warn-on-second-window

- **Chose:** An advisory OS file lock per vault in the app directory, held while unlocked. A second window gets a notice and stays writable. Rust 1.89 is the minimum, for `File::try_lock`.
- **Because:** The conditional save already prevents the lost update (`inv:atomic-verified-save`). The lock only moves the news to unlock time. A read-only second window would block the person in the common harmless case, a window left open and forgotten. The OS drops the lock when a process dies, so a crash leaves nothing stale.
- **Rejected:** A read-only second window. A single-instance plugin, which on Linux needs D-Bus and still says nothing about KeePassXC. A locking crate, when the standard library has one.
- **Reopen if:** People lose work to a merge they did not expect despite the notice.
- **Date:** 2026-11-13.

## Process

### dec:agent-first-docs

- **Chose:** Docs form a small tagged map for agents. `AGENTS.md` routes. Each kind of knowledge has one file: architecture, invariants, decisions, facts, plan, runbooks. Code and tests cite tags. The `doc-links` gate verifies every tag and every named file and test.
- **Because:** Prose status files went stale and contradicted the code. They claimed 14 e2e checks when there were 13, a `storage` permission the manifest lacks, and a lint gate that never ran.
- **Rejected:** Keeping the ADRs, README, STATUS and ROADMAP. They mixed decisions, history and status, so nothing in them could be checked.
- **Date:** 2026-11-12.

### dec:ts-rs-bindings

- **Chose:** `ts-rs` derives TypeScript for the view models in test builds only. A test in `crates/vardo-core/src/bindings.rs` writes one file. The `ipc-names` gate checks command names.
- **Because:** `ts-rs` 12 is a stable release, reads the serde attributes already there, and adds nothing to the shipped binary.
- **Rejected:** `tauri-specta`, which also generates the command calls but was a release candidate and changes how every command returns errors.
- **Reopen if:** `tauri-specta` ships a stable release, or argument mismatches cause a bug.
- **Date:** 2026-11-12.

### dec:xtask-check

- **Chose:** One Rust binary, `cargo xtask check` in `xtask/`, runs every gate CI runs. One summary line per gate, full output in `target/check/<gate>.log`, `--json` for tools.
- **Because:** An agent needs pass or fail and the failing log, not thousands of lines of cargo output. Rust runs on every platform the code does, and the doc-link and layer gates are easier to test in Rust than in shell.
- **Rejected:** `check.sh`, which blocked Windows and stopped at the first failure. `just` or `make`, which add a tool to install and still need shell for the logic.
- **Reopen if:** The gates need parallelism the runner cannot give simply.
- **Date:** 2026-11-12.
