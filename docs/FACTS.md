# Facts

Each entry is something true about the world outside this code, that the code relies on.
**Verified** says how and when. **Unverified** marks an assumption still waiting for evidence.
When a fact changes, update the entry and every invariant or decision that cites it.

## Google Drive

Live checks ran on 2026-08-27 with `crates/vardo-drive/examples/contract.rs` against a throwaway account (`docs/RUNBOOKS.md`).

### fact:drive-no-cas

- Drive API v3 has no conditional write. v3 removed `etag` from the File resource, sends no `ETag` header, and ignores `If-Match` on media `PATCH` and on `DELETE`. The `version` field exists but nothing enforces it.
- **Verified** live: a `PATCH` with a deliberately wrong `If-Match` returned 200.
- If this ever returns 412, replace write-then-detect with a real conditional write in `DriveBackend`.

### fact:drive-update-keeps-id

- `files.update` with `uploadType=media` keeps the file id and creates a new revision.
- **Verified** live: two writes, same id.

### fact:drive-head-revision-every-write

- `headRevisionId` changes on every content write, including a write of identical bytes.
- **Verified** live.

### fact:drive-revisions-order

- The raw revisions list comes oldest first. `DriveBackend::revisions` reverses it on purpose, because the protocol reads `revisions[1]` as the revision directly beneath ours.
- **Verified** live. An early harness asserted oldest-first on the wrapper's output and failed. The test was wrong, not the `.reverse()`.

### fact:drive-old-revisions-fetchable

- An overwritten revision stays fetchable long enough to recover it. This is the basis for "a lost race is recoverable".
- **Verified** live: an overwritten revision returned its original 16 bytes.

### fact:drive-refresh-token-persists

- A stored refresh token works on a later run with no consent prompt.
- **Verified** live.
- **Unverified:** that a grant made through the picker (`trigger_onepick=true`) persists across sessions. The harness no longer opens the picker (`fact:drive-picker-danger`).

### fact:drive-picker-danger

- The picker is the only way to grant access to a file Vardo did not create. One misclick hands over the user's real vault.
- Test harnesses must never request `trigger_onepick=true`. `files.create` alone grants full access to the created file under `drive.file`.
- **Verified** 2026-08-27: an early harness opened the picker over a real account while its own notes said it never would.

### fact:drive-picker-system-browser

- The JavaScript Picker cannot run inside a desktop WebView. Its origin is `file://` or `localhost`, `postMessage` fails the origin check, and it returns 403, often only once packaged.
- The app opens the picker in the system browser with `trigger_onepick=true` on the authorization URL.

### fact:drive-quota-403

- Rate limits, a full Drive and permission denial all answer 403. Google's error body names which in `error.errors[].reason`: `userRateLimitExceeded`, `rateLimitExceeded`, `dailyLimitExceeded`, `sharingRateLimitExceeded`, `storageQuotaExceeded`, `insufficientFilePermissions`, `appNotAuthorizedToFile` and others (developers.google.com/workspace/drive/api/guides/handle-errors, read 2026-11-13).
- The messages are capitalised prose ("User Rate Limit Exceeded"). The old lowercase text match read a real rate limit as a permission error, and a full Drive ("storage quota") as retryable. Vardo now classifies by `reason` and falls back to the message only when the body has none.
- Proof: `crates/vardo-drive/tests/backend.rs::rate_limits_and_permission_errors_share_403_and_must_be_told_apart` uses Google's documented bodies. A live quota 403 has not been provoked.

### fact:drive-file-scope-no-verification

- Google's policy says an app using only non-sensitive scopes need not complete verification. `drive.file` is non-sensitive. The full `drive` scope is restricted and needs a paid assessment.
- Apps in Testing mode get refresh tokens that expire after 7 days, so the client must be Published.
- **Unverified** live: publishing a `drive.file`-only client without being asked for verification.

### fact:drive-no-push

- `changes.watch` needs a registered domain and a public HTTPS webhook, which a desktop app lacks.
- A metadata `files.get` costs 5 quota units and an upload 50. A five-minute poll is about 2,300 units per user per day.
- The free threshold is 400,000,000 units per day with a 1 TB daily egress cap. Google said billing details follow "later in 2026 with at least 90 days' notice". Recheck before any public release.

### fact:drive-safe-save-kills-id

- A client that saves by write-temp, delete, rename (KeePassXC's safe save through a mount) gives the file a new id. The `drive.file` grant is per file, so it dies with the old id, and the app must offer to pick the file again.

## Cloud-mounted folders

### fact:cloud-mounts-lose-data

- Syncing a KDBX file through a mounted cloud folder fails silently across clients and providers. KeePassXC issues: #9140 (remote file stale for weeks while showing "syncing"), #8790 and #2066 (`(1).kdbx` conflict forks), #11969 (GVfs write fails, infinite dialog), #13050 (rclone mount blocks the atomic rename). keepass2android #2467 shows the same class on Android.
- KeePassXC's own advice is to move the database to a local folder.

## KDBX

### fact:keepass-crate-normalises

- Measured during Phase 0 on 30 fixtures from `sseemayer/keepass-rs`. The `keepass` crate cannot write KDBX 4.0 at all. Its 4.1 round trip shrank one document's XML from 20,720 to 16,218 bytes, dropped empty elements, lost 2 `IconID` and 1 `CustomIconUUID`, and injected 7 `QualityCheck` elements. No text content was lost.
- This is KeePassXC issue #3340, "conflicting changes on identical databases", in library form.

### fact:kdbx-same-second

- KDBX timestamps have whole-second resolution. Two devices editing within one second produce equal times.

### fact:kdbx-protected-positional

- Protected values share one inner keystream and decrypt in document order. Adding, removing or reordering a protected element shifts every later value.

### fact:kdbx-kdf-before-auth

- The header HMAC key derives from KDF parameters stored in the unauthenticated header, so the KDF must run before anything is authenticated. The header SHA-256 is unkeyed and stops accidents only.
- A file claiming Argon2 with 2 GiB and 4,294,967,295 iterations would allocate and grind for hours before failing. `Limits` is the only defence.

### fact:argon2-cost

- Argon2d with 64 MiB, 14 iterations and 2 lanes took 0.871 s on an i7-12700H. Those are the real vault's parameters. Unlock must run off the UI thread with progress.

### fact:kdbx-no-delta

- KDBX has no delta format, so every save uploads the whole file. At 646 KB that is irrelevant. With large attachments it bounds every backend (`fact:kdbx-attachment-cost`).

### fact:kdbx-attachment-cost

- Measured 2026-11-13 on an Apple M1 Max with `crates/vardo-core/examples/attachment_size.rs` (release build, random bytes, so nothing compresses). Cost grows linearly with attachment size: about 63 ms per MB to save, which includes the verifying re-open, and about 14 ms per MB to unlock, both on top of the KDF. At the 8 MB attachment cap that is 0.5 s per save and 0.1 s per unlock.
- On a remote vault the upload dominates: every save sends the whole file, so an 8 MB attachment costs about 6.5 s per save on a 10 Mbit/s uplink, every time anything in the vault changes.
- So attachments suit what they are for in a password manager: recovery codes, keys, a scanned document. The 8 MB cap in `crates/vardo-core/src/session/edit.rs` stays.

### fact:fuzz-findings

- Fuzzing found four real bugs that review missed: a stray end tag silently truncating the document, whitespace normalised between attributes and `trim_start` accepting non-XML whitespace, whitespace dropped inside end tags, and per-element whitespace state on the parser struct that nested elements overwrote.
- Every one has a regression test in `crates/vardo-kdbx/tests/`.
- Clean runs recorded before 2026-11-12: `xml_roundtrip` 6,489,280, `header` 5,969,932, `xml_parse` 1,730,849, `container` 676,225 executions.

## Browsers

### fact:chrome-extension-limits

- Chrome launches a native host only for extension ids listed in the host's manifest.
- A message from host to extension may not exceed 1 MB.
- Branded Google Chrome ignores `--load-extension`. Automated runs need Chromium or Chrome for Testing.
- Chrome took Ctrl+Shift+L in 2026 to collapse vertical tabs, and refuses a suggested shortcut it owns without any error.
- `chrome.commands` reports shortcuts in platform notation: `Alt+Shift+L` on Linux, `⌥⇧L` on macOS.
- On macOS, Chromium-family browsers read native host manifests from `~/Library/Application Support/<browser>/NativeMessagingHosts`, ignoring `XDG_CONFIG_HOME`.
- **Verified** by the e2e on Linux in CI and on macOS, 2026-11-12.

### fact:clickjacking-defcon33

- Marek Tóth, DEF CON 33, August 2025: autofill menus injected into pages can be made invisible and clicked through a fake cookie banner. All eleven password managers tested were vulnerable by default, including KeePassXC-Browser up to 1.9.9.2. Ten of eleven filled subdomain matches automatically. CERT/CC VU#516608.

### fact:keepassxc-browser-permissions

- KeePassXC-Browser 1.10.4.1 runs on `<all_urls>` with `all_frames: true` and asks for `tabs`, `cookies`, `webRequest` and `webNavigation`. Chrome shows that as "Read and change all your data on all websites".

## Storage protocols

### fact:webdav-etag-hazards

- Apache `mod_deflate` at its default `DeflateAlterETag AddSuffix` serves ETags with `-gzip` appended but evaluates `If-Match` against the bare tag. Every conditional write then fails with 412, and retry logic treats that as ordinary concurrency, so sync goes one-way silently. Nextcloud's official Docker image enables `mod_deflate`.
- Sending a derived bare tag is unsafe alone, because `If-Match` takes a list and succeeds if any member matches.
- Reproduced 2026-11-13 on Apache 2.4.66 with `mod_dav` and `mod_deflate` at its defaults: with gzip accepted the ETag reads `"4e20-…-gzip"` and `If-Match` with it gets 412, while with `Accept-Encoding: identity` the tag is bare and the conditional `PUT` succeeds.

### fact:apache-weak-etag-fresh-files

- Apache marks the ETag of a file modified within the last second as weak (`W/"…"`), and `If-Match` never matches a weak tag. A conditional write straight after another write fails unless the client waits about a second. The same file's tag turns strong as it ages, so comparing versions must ignore the `W/` marker. Observed 2026-11-13 on Apache 2.4.66.

## Platforms

### fact:platform-linux

- Linux is the daily-use platform. Building needs WebKitGTK 4.1, librsvg and libayatana-appindicator.
- GNOME shows tray icons only with the AppIndicator extension.

### fact:platform-macos

- Verified 2026-11-12 on macOS 15.7.7, arm64: the release build runs; the socket appears mode 600 in a mode 700 dir under `$TMPDIR/vardo-$USER`; the real host relays `status` and `logins`; the e2e passes with Chrome for Testing 157; the tray item registers.
- The uncommitted `RunEvent::Reopen` handler showed a hidden window on a reopen Apple Event.
- Not verified: the tray icon's look and menu, unlocking through the window, closing then clicking the Dock icon.
- An agent there has no Screen Recording or Accessibility permission by default, so it cannot see or click windows. `screencapture` returns only the wallpaper.

### fact:platform-windows

- CI runs the core tests on Windows, including the browser pipe (`crates/vardo-core/tests/browser_pipe.rs`). The app itself is not built or run on Windows by CI, so the window, the tray and the extension end to end are unverified there. No one uses it daily. Observed 2026-11-13.

## Tooling

### fact:firefox-build-lints-clean

- `web-ext lint` 8 reports no errors, warnings or notices for the Firefox build (`dist/firefox` inside the extension folder). The add-on has not been loaded in a real Firefox, and no e2e runs there. Observed 2026-11-13.

### fact:clippy-moves

- CI lints with the latest stable Rust. Clippy 1.99 added `useless_borrows_in_formatting`, which failed `main` on 2026-11-12 with no code change.

### fact:glib-pinned-by-tauri

- Tauri 2.11 reaches `glib` 0.18.5 through its GTK3 stack on Linux. Dependabot alert 1 (RUSTSEC-2024-0429, an unsound iterator over GLib variant strings) needs glib 0.20, which only GTK4 bindings use. Tauri 3, in alpha, is where that changes. `deny.toml` records the advisory with its reason, and Vardo never builds or iterates a GLib variant. Recheck on each Tauri upgrade, and drop the `deny.toml` entry once `cargo tree -i glib` shows 0.20. Observed 2026-11-13.

### fact:minimatch-override

- `@typescript-eslint/typescript-estree` 8.72.2 asks for `minimatch` ^10.2.2, but GHSA-g3hv-75wg-m249 is fixed only in 11.0.1, so CI's `npm audit` failed. `app/package.json` overrides it to ^11.0.1 for that package alone. The lint still passes and still catches errors. Delete the override once typescript-estree accepts minimatch 11. Observed 2026-11-12.

### fact:telegram-palette-contrast

- Telegram's palette is not APCA-verified. Day `windowSubTextFg` `#999999` on white is about 2.8:1. `dialogsBgActive` `#419fd9` under white text is about 2.9:1. The contrast gate caught nine failures that eyeballing missed, mostly hover states and the focus ring.
