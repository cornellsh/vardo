# Research and initial architecture

> **Historical document.** This is the research written before any code
> existed, kept because the later decisions argue against it and make less
> sense without it. Several of its conclusions were overturned, first in
> [`CRITIQUE.md`](CRITIQUE.md) and then in the ADRs. For how Vardo actually
> works today, read [`STATUS.md`](STATUS.md).
>
> Details of the author's personal setup were removed before publication. The
> engineering conclusions drawn from them are unchanged.

Working name at the time: **Vault**. Goal: a KDBX-native desktop password
manager for Windows/macOS/Linux that looks and feels like Signal Desktop, syncs
via Google Drive, and stays byte-compatible with KeePassium on iOS.

---

## 1. Ground truth — the vault this has to open

Measured from a real, long-lived vault rather than assumed: KDBX 4.0, AES-256,
GZip, Argon2d with a memory-hard setting of the kind KeePassXC now recommends,
hundreds of kilobytes because of attachments, living in the root of a Google
Drive and reached through an `rclone` FUSE mount.

Two consequences that shape everything downstream:

1. **A modern Argon2 setting costs one to two seconds of CPU per unlock.**
   Unlock cannot be synchronous. It needs a background thread and a real
   progress indicator, or the app feels broken. KeePassXC gets this wrong and
   just freezes.
2. **Real vaults live wherever the user put them, not in `appDataFolder`.**
   This rules out the `drive.appdata` scope and forces a specific OAuth
   strategy (see §7).

---

## 2. Landscape — what exists and why it isn't this

### Dead or dying

| Project | Verdict |
|---|---|
| **Buttercup** | Archived Jun 2025. Maintainer burned out on scope; single dev. Used its own `.bcup` format — not KDBX — so it never had the interop you want anyway. Electron. |
| **KeeWeb** | Last real release 2021, then a v1.18.8 blip in 2026. `README` literally says "Looking for a new maintainer" (issue #2022). Grunt + Webpack + Electron v13, ~80 deps. Effectively abandoned. |
| **KeeWebX** | Fresh 2026 TS fork of KeeWeb. 19 stars, one author. **Explicitly web-only, no desktop, and says so on purpose.** Also KDBX4-only + WebDAV/IndexedDB only — no Google Drive. Not your app, but its `kdbxweb`-derived core is worth reading. |

### Alive but wrong

- **KeePassXC** — the thing you're replacing. Qt, correct, ugly, and its merge
  engine (`src/core/Merger.cpp`) is the single most valuable piece of prior art
  in this entire space. Read it, port its semantics, don't port its UI.
- **KeePassium / Strongbox** (iOS) — your mobile side. Both are polished. Both
  are Apple-only. Strongbox's support docs are a goldmine of *exactly* the
  failure modes you'll hit (see §7.3).
- **Bitwarden / 1Password** — different architecture entirely (server-mediated,
  own format). Not comparable.

### The "modern KDBX + Tauri" cohort

Searching turns up a steady stream of these: `mithril-vault`, `Vigil`,
`ashypass`, `ngxsmk-wallet`, `latch`, `Kryptix`, `PassQuantum`, `CosmoSecure`.
Pattern is consistent — 0–50 stars, one author, 3–12 months old, README promises
a roadmap that never lands. Several don't even use KDBX (they invent a format,
which is the cardinal sin here).

**Read for reference, depend on none of them.**

### The actual conclusion

There is a real, unoccupied gap: *a maintained, beautiful, cross-platform,
KDBX-native desktop client with first-class cloud sync.* Buttercup's death and
KeeWeb's stall left it empty. That's the thesis. It's sound.

But note **why** Buttercup died — the maintainer's own post-mortem says it was
scope, specifically the surface area touching OSes, mobile, and third-party
services. That is a direct warning about your plan. Mitigation is in §9: ruthless
scope discipline, desktop-only, no mobile app ever (KeePassium *is* your mobile
app), no browser extension in v1.

---

## 3. Core strategic decision

> **Do not invent a format. Invent the client.**

KDBX is your moat, not your constraint. It's what makes KeePassium-on-iOS work
for free, what makes KeePassXC a fallback if your app breaks, and what means
users can never be locked in. Every project in §2 that invented a format is
either dead or irrelevant.

Corollary: **interop correctness outranks every feature.** A gorgeous app that
silently drops a field KeePassium wrote is worse than KeePassXC. This drives the
library decision in §5 and the Phase 0 in §9.

---

## 4. Stack

### Recommendation: **Tauri v2 + Rust core + SvelteKit (or React) + TypeScript**

**Why Tauri over Electron:**

| | Tauri v2 | Electron |
|---|---|---|
| Bundle | 3–10 MB | 80–200 MB |
| Idle RAM | ~50 MB | ~120 MB+ |
| Backend | Rust | Node.js |
| Renderer | OS WebView | bundled Chromium |
| Native API access | capability-scoped, opt-in | full FS/net by default |

The security argument matters more than the size argument. In Tauri the
frontend **cannot** touch the filesystem, network, or OS keychain unless you
explicitly declare a capability, and every backend call goes through a
registered, typed `#[tauri::command]`. For a password manager that's not a nice
to have — it's the whole architecture. Electron's Node side has full system
access by default and correctly bridging it to a sandboxed renderer has been "a
class-of-bug factory for a decade."

Rust also gets you `zeroize`, `mlock`/`VirtualLock`, and the RustCrypto stack
(`argon2`, `aes`, `chacha20`, `hmac`, `sha2`) in the same language as your
crypto — no FFI seam through which plaintext leaks.

**Honest costs of Tauri, stated up front:**

- **WebView fragmentation is real.** WebKitGTK on Linux is materially behind
  WebView2/WKWebView. You will hit CSS and layout bugs Electron users never see.
  Budget for it: test on all three from week one, avoid bleeding-edge CSS.
- Smaller ecosystem for packaging/auto-update edge cases than electron-builder.
- Tauri's own IPC has had CVEs (custom-protocol origin-validation bypass in
  2.0.0–2.0.3). Pin versions, watch advisories, validate origins yourself.
- Linux Tauri deps (`webkit2gtk-4.1`) aren't installed here yet.

**Rejected alternatives:**

| Option | Why not |
|---|---|
| Electron | Signal Desktop uses it, but you'd be shipping Chromium to render a list of passwords, and you'd write your crypto in JS or bind to Rust anyway. |
| Native per-platform (SwiftUI/WinUI/GTK) | 3× the work. This is what killed every "do it properly" attempt. |
| Egui / Iced / Slint (pure Rust GUI) | No WebView attack surface, genuinely appealing for a security tool. But you asked for *Signal-Desktop-beautiful*, and getting Signal-grade polish out of immediate-mode Rust GUI is 5–10× the design effort. Revisit only if WebView pain becomes intolerable. |
| Flutter | Good-looking, but Dart crypto story is weak and you'd FFI to Rust anyway — so you get Tauri's seam without Tauri's security model. |

**Frontend framework:** Svelte 5 or React 19 both fine. Slight lean to **Svelte**
— smaller runtime, less to audit, and no reconciler holding stale copies of
decrypted strings in a virtual DOM. React is the safer hiring/ecosystem choice.
Either way: **TypeScript strict, Vite, and a hand-rolled design system** (not a
component library — Signal's look comes from restraint, and Tailwind + ~15
bespoke components will get you there).

---

## 5. The KDBX library — the riskiest decision

This is where the project succeeds or fails. Options:

### (a) `sseemayer/keepass-rs` — the incumbent

Mature-ish, widely used, KDB/KDBX3.1/4.0 read, **KDBX4.1 write (experimental)**.
Two problems:

- Writing is still labelled experimental, and a 2026 breaking change made it
  **always write 4.1**, erroring if you ask for 4.0. Your file is 4.0. KeePassium
  has supported 4.1 since v1.24 (May 2021) so an upgrade is *survivable*, but it
  must be a deliberate, user-consented migration, not a silent side effect.
- It parses XML into a typed model and re-emits. **Unknown elements are dropped.**
  That is silent data loss on round-trip with any client that writes a field it
  doesn't model.

### (b) `Marmaladesoul/keepass-core` — the one that gets it right on paper

Claims exactly the properties you need: full lossless KDBX3/4 round-trip,
**unknown XML elements preserved and re-emitted untouched**, `#![forbid(unsafe_code)]`,
`cargo-fuzz`, and an interop matrix tested against KeePass2, KeePassXC, KeeWeb,
Strongbox, KeePassium, MacPass. Problem: **pre-release, not on crates.io, single
author, unproven.** High reward, real bus-factor risk.

### (c) `kdbxweb` (JS, KeeWeb's engine)

A decade of production hardening and the most battle-tested KDBX implementation
outside the C++ ones. But it's JavaScript, which means plaintext in the WebView.
**Disqualified on architecture** — but read its source; it's the reference.

### (d) Write your own

The KDBX 4.1 spec is fully documented (keepass.info, Palant's write-up,
`shadone/KDBXKit`'s excellent spec docs). Doable — ~4–6 weeks for a correct
implementation with tests. Maximum control, maximum time cost.

### Recommendation

**Don't decide yet. Decide with data.** Phase 0 (§9) is a conformance harness:
generate a corpus of KDBX files from KeePassXC, KeePass 2.x, KeePassium, and
Strongbox exercising every feature (attachments, custom fields, history, TOTP,
custom icons, tags, previous-parent-group, KeeShare metadata), then for each
candidate library run `read → write → read` and diff the decrypted XML.

Whichever library round-trips clean, wins. Expected outcome: **fork
`keepass-rs`, add DOM-preservation**, or **adopt `keepass-core` and vendor it**.

### The architectural insight that makes losslessness cheap

Don't treat the typed model as the source of truth. **Keep the parsed XML tree
in memory as the source of truth and apply edits as surgical mutations to it.**
The typed model becomes a *view* over the DOM for the UI to bind against. Then
"preserve unknown elements" is free — you never destroyed them. This is roughly
what `kdbxweb` does and it's why KeeWeb never corrupted anyone's database.

---

## 6. Architecture

```
vault/
├─ crates/
│  ├─ vault-kdbx/      # KDBX read/write. DOM-preserving. #![forbid(unsafe_code)]
│  ├─ vault-crypto/    # Argon2/AES/ChaCha20 wrappers, SecretString, mlock+zeroize
│  ├─ vault-merge/     # KeePassXC "Synchronize" semantics, 3-way merge, dry-run
│  ├─ vault-sync/      # storage backends: local file, Google Drive, (WebDAV later)
│  ├─ vault-core/      # session state, unlock/lock, search index, autolock timer
│  └─ vault-cli/       # headless companion — also the test harness driver
├─ src-tauri/          # thin: Tauri commands, capabilities, tray, updater
└─ ui/                 # SvelteKit + TS + Tailwind
```

Rules:

- **`ui/` never sees a plaintext password unless it explicitly asks for one
  entry, one field, at one moment.** The list view gets titles, usernames, URLs,
  icons — never the secret. `reveal(entryId, field)` and `copy(entryId, field)`
  are separate commands with their own audit trail.
- **Clipboard writes happen in Rust**, with a timer that clears it. Never via
  the WebView clipboard API.
- All crypto in `vault-crypto`. Nothing else calls RustCrypto directly.
- `vault-cli` exists from day one — it's how you run the conformance suite in CI
  without a GUI, and it's the debugging tool when a user's file won't open.

---

## 7. Sync — the actual hard problem

This is where the product value is. Everyone can render a list of passwords.
Almost nobody handles sync correctly.

### 7.1 Why it's hard

A `.kdbx` is a single opaque encrypted blob. The cloud provider **cannot** merge
it — it sees binary and applies last-write-wins. Two devices editing before
either syncs = one device's changes silently vanish into version history.
Conflict resolution *must* happen client-side, after both versions are decrypted
locally. There is no way around this.

### 7.2 The design

```
remote etag/modifiedTime  ─┐
                           ├─→ changed? → download remote
local file mtime          ─┘              → decrypt both
                                          → 3-way merge vs. last-synced base
                                          → write merged locally
                                          → upload (If-Match on etag)
                                          → 409? re-fetch, re-merge, retry
```

Keep three things on disk: the **working copy**, the **last-synced base**
(for true 3-way merge, not just 2-way), and a **pre-write backup** rotation.

Merge semantics: port KeePassXC's `Merger` in **`Synchronize` mode** — merge
history, newest value on top, apply all deletions, preserve older values in
entry history rather than discarding. Also port its `--dry-run` and merge
confirmation dialog concept: **show the user what a merge is about to do before
it does it.** KeePassXC added this (PR #10173) and it's the right call.

Known trap, from KeePassXC's own issue #3340: naive comparison reports spurious
conflicts on identical databases due to timestamp precision differences. Handle
timestamp precision explicitly.

### 7.3 Google Drive specifics

**Scope choice — this determines whether you ever ship.**

- `drive` (full) → **restricted scope** → Google verification + an annual
  third-party security assessment. Costs thousands. **Never do this.**
- `drive.appdata` → non-sensitive, no review. But it's a hidden per-app folder;
  **your file is in My Drive root and KeePassium can't see appDataFolder.** Dead end.
- **`drive.file` → non-sensitive, no verification, no security assessment.**
  Grants per-file access to files the user explicitly picks via the **Google
  Picker API** or that your app created. Access persists.

**→ Use `drive.file` + Google Picker.** User picks `passwords.kdbx` once. Done.
Google's own support docs push developers to exactly this pattern.

**Writing correctly — the KeePassium-breaking bug you must not ship:**

Strongbox's support pages document this precisely: KeePassXC and KeePass do
"safe saving" — write a temp file, then rename/swap it in. On cloud drives this
**creates a new file identity**, which invalidates the iOS app's stored file
reference, and the mobile app breaks. Their workaround is telling users to
disable safe saving.

Don't inherit the bug. Use **`files.update` with media upload on the existing
`fileId`** (`PATCH /upload/drive/v3/files/{fileId}`). This preserves the file ID
and creates a new *revision* rather than a new file. Drive keeps prior blob
revisions, which is a free undo-history safety net — and you can pin critical
ones with `keepRevisionForever`.

Use resumable upload for the 646 KB+ payload, and send `If-Match` with the
known etag so a concurrent write returns 412/409 instead of clobbering.

**Migration path from rclone:** keep the `rclone` mount working as a plain
"local file" backend on day one — zero risk, zero new code. Native Drive
integration is a later phase, not a blocker.

### 7.4 Also worth supporting eventually

WebDAV (Nextcloud), Dropbox, OneDrive, plain local/network path. Build
`vault-sync` around a `StorageBackend` trait from the start so these are drop-ins.

---

## 8. Security model

Non-negotiables:

1. **Secrets never leave Rust in bulk.** Per-field, on-demand, short-lived.
2. `SecretString`/`SecretBytes` types: `zeroize` on drop, `mlock`/`VirtualLock`
   to keep them out of swap, no `Clone`, no `Display`, no `Deref<Target=str>` —
   closure-scoped `expose()` access only. (`memguard-rs` and `secrecy` are
   prior art; 1Password's Rust Foundation write-up is the canonical rationale.)
3. **Argon2 off the UI thread**, with progress. Non-negotiable at t=14/64MiB.
4. Auto-lock: idle timeout, on screen-lock, on suspend, on minimize (optional).
   Lock = drop the master key and every derived secret, zeroize, keep only the
   ciphertext in memory.
5. **Disable core dumps, deny `ptrace` attach** (Linux/macOS) at startup.
6. Strict CSP in the WebView, no remote content, no CDN, no telemetry, ever.
7. Biometric unlock (Touch ID / Windows Hello) is a **convenience layer only** —
   it wraps a key stored in the OS keychain, and it can only be enabled after a
   successful master-password unlock. Never a replacement for the master password.
8. Reproducible builds + published hashes. For a password manager this is table
   stakes for trust.

**Threat model to write down explicitly** (and put in the README): this protects
against a lost laptop, a compromised cloud account, and a malicious network. It
does **not** protect against malware already running as your user. Be honest
about it; every project that overclaims here gets torn apart.

---

## 9. Roadmap

**Phase 0 — Conformance harness (do this before anything else. ~1 week)**
Build the corpus + `read→write→read` differ described in §5. Run it against
`keepass-rs`, `keepass-core`, and `kdbxweb` (as an oracle). *Then* pick the
library. This one week de-risks the entire project.

**Phase 1 — Read-only viewer (~3 weeks)**
Open a real vault from a local path. Unlock with progress. Tree, list, detail,
search. Copy-to-clipboard with auto-clear. Auto-lock. No writes at all — zero
risk to real data while the reader is being built.

**Phase 2 — Writes (~3 weeks)**
Edit/create/delete entries and groups. Attachments. Entry history. Password
generator. Atomic local write + backup rotation. **Verify every write with
KeePassXC and KeePassium before shipping any of it.**

**Phase 3 — Sync (~4 weeks)**
`StorageBackend` trait, `rclone`-mount/local backend, then Google Drive via
`drive.file` + Picker + `files.update`. 3-way merge with dry-run preview.
Conflict UI.

**Phase 4 — Polish (~4 weeks)**
The Signal-grade pass. Design system, animation, keyboard-first navigation,
command palette, dark/light, tray, global quick-search hotkey. TOTP. Password
health (weak/reused/HIBP k-anonymity).

**Phase 5 — Distribution**
Tauri updater (Ed25519 signing, mandatory). macOS: Apple Developer $99/yr +
notarization. Windows: **Azure Trusted Signing ~$10/mo** is by far the cheapest
route past SmartScreen (individuals are eligible; OV certs are $100–400/yr, EV
$300–600/yr). Linux: AppImage + Flatpak + AUR.

**Explicitly out of scope, possibly forever:** mobile app (KeePassium already
is one), browser extension (v2 at the earliest — and if you do it, speak the
existing `keepassxc-browser` protocol so their extension works unmodified rather
than shipping your own), server component, team/sharing features.

That last paragraph is the single most important risk control in this document.
It's the exact scope creep that killed Buttercup.

---

## 10. Questions this left open, and how they were answered

1. **Name.** Vardo: a Romani living wagon. A home you take with you.
2. **License.** Apache-2.0 OR MIT, the Rust ecosystem convention. GPL-3 was
   considered and rejected because it would complicate reuse of the KDBX crate.
3. **KDBX 4.0 or 4.1.** Both are read and written; the file keeps the version
   it arrived with. Silently upgrading someone's vault is not Vardo's call.
4. **Key files and hardware keys.** Password only for now.
5. **Personal tool or real product.** A real product, which is why the threat
   model and distribution sections are not optional.
6. **Design reference.** Restraint borrowed from a messaging app, not its
   layout copied. The final reference became Telegram Desktop; see
   [`DESIGN.md`](DESIGN.md).

---

## 11. Sources

- KDBX spec: <https://keepass.info/help/kb/kdbx.html>, `kdbx_4.html`, `kdbx_4.1.html>`
- Palant, "Documenting KeePass KDBX4 file format" (2023)
- `shadone/KDBXKit` — the best modern prose spec of the container format
- `keepassxreboot/keepassxc` — `src/core/Merger.cpp`, `src/cli/Merge.cpp`
- `keepassxc-browser` protocol: `keepassxc-protocol.md` + Protocol V2 draft wiki
- Buttercup post-mortem: gist `perry-mitchell/43ebfcec4d874b77a704be1d4f2262e6`
- Google Drive scopes / appdata / Picker / restricted-scope verification docs
- Strongbox KB: "Saving With KeePass or KeePassXC Causes Database To Be Unusable"
- Tauri v2: process model, capabilities, updater, macOS/Windows signing docs
- Rust Foundation / 1Password, "Secure App Development with Rust's Memory Model"
