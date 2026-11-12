# Red team of RESEARCH.md

> **Historical document,** written before any code existed. Parts of it were
> later overturned in turn: §3.1 by [`ADR-001`](ADR-001-sync.md), and §3.5,
> which ruled out ever shipping a browser extension, by
> [`ADR-006`](ADR-006-browser-extension.md). Details of the author's personal
> setup were removed before publication.

I wrote `RESEARCH.md` from search snippets. Then I verified the load-bearing
claims by reading source, hitting APIs, and measuring. Three were wrong, one was
wrong in a way that would have cost weeks, and I found one thing that changes
the risk profile of the whole project.

This document supersedes `RESEARCH.md` where they conflict.

---

## Part 1 — Corrections

### 1.1 I conflated two different libraries (my error)

There are two crates with confusingly similar names:

| | `keepass` | `keepass-rs` |
|---|---|---|
| Repo | `sseemayer/keepass-rs` | `meission/keepass-rs` |
| Version | 0.13.22 (2026-08-21) | 0.2.0 (2026-06-20) |
| Downloads | **364,575** | **57** |
| License | MIT | MIT OR MulanPSL-2.0 |
| Stars | 170 | — |

The feature table I quoted in `RESEARCH.md` §5 — three-way merge, digital
signatures, OTP, Android JNI, HarmonyOS NAPI, Flutter FRB — is from the
**57-download** one, not the real one. Its impressive-looking capability matrix
was doing unearned work in my analysis.

**The crate you actually want is named `keepass`.** Note that for the Cargo.toml.

### 1.2 `Marmaladesoul/keepass-core` is AI-generated with zero users

I presented it as a credible option. Verified:

- **0 stars**, created 2026-04-21, last push 2026-07-27
- Apache-2.0, not published to crates.io
- **Ships an `AGENTS.md`** — it's agent-authored

That explains the otherwise implausible surface area: ~45 test files, a whole
`keepass-merge` crate with `or_set.rs` (OR-Set CRDT), `chaos_runner.rs`,
`tombstone.rs`, proptest regressions, `secret_hygiene.rs`, `tamper_detection.rs`,
`save_determinism.rs`, and `xml/{decoder,encoder}.rs` split.

**Do not depend on it.** An unaudited, unused, agent-written crypto library is
the worst possible foundation for a password manager.

**But do read it.** It is the best available existence proof that the
DOM-preserving design in §2.2 below is achievable, and it independently arrived
at OR-Set/tombstone semantics for merge — which is a better model than
KeePassXC's. Apache-2.0 means we can learn from it freely.

### 1.3 The Google Picker plan would not have worked

This is the expensive one. I wrote "use `drive.file` + Google Picker, user picks
the file once." Right conclusion, wrong mechanism, and the wrong mechanism fails
hard.

Google's Picker has **two distinct flows**, and their own comparison table says:

| | Web apps | Desktop & mobile apps |
|---|---|---|
| Technology | client-side JS library | OAuth 2.0 URL params + HTTP redirects |
| Rendering | in your UI | system browser tab — **"can no longer be displayed within an embedded webview"** |
| Auth | `setOAuthToken` | add `trigger_onepick=true` to the OAuth request |
| Response | JS callback | redirect URI / custom URL scheme |
| Scopes | flexible | **only `drive.file`, and it cannot be combined with other scopes** |

Embedding the JS Picker in a Tauri/Electron webview fails with `origin=file://`
or `origin=http://localhost` → `postMessage` origin mismatch, 400s, and 403s
that only appear *after packaging* (it works in dev on `localhost:3000` and
breaks in the shipped app — the worst failure mode there is). This is a
well-documented Electron graveyard.

**Corrected plan:** desktop Picker flow. Append `trigger_onepick=true` to the
authorization URL, open it in the **system browser**, user picks the file there,
selection returns via the loopback redirect alongside the auth code.

Silver lining: this flow *forces* `drive.file` exclusively, which is exactly the
non-sensitive scope we wanted. The safe path is the only path.

Also confirmed: **loopback redirect is still supported for the `Desktop app`
client type** (Google deprecated it only for iOS/Android/Chrome client types).
And Google auto-issues a client_secret for Desktop clients that the token
endpoint *requires* even with PKCE — so we embed it and, per Google's own docs,
"the client secret is obviously not treated as a secret."

### 1.4 Argon2 timing — measured, not guessed

A real-world setting (`Argon2d, m=64 MiB, t=14, p=2`) on an i7-12700H: **0.871 s**.

On a 2017-era ultrabook, budget 2.5–3 s. Async unlock with progress is confirmed
mandatory, not a nicety.

---

## Part 2 — New findings that change decisions

### 2.1 Google is putting the Drive API behind a paywall — and rclone's default setup will break

This is the biggest thing I missed.

- **May 1, 2026:** Drive API quotas moved from "requests" to **quota units**,
  with a new tiering model.
- **"Later in 2026, following 90 days of notice":** quota increases require
  billing enabled, and *"API usage over standard daily thresholds will generate
  charges on your Google Cloud bill."*
- **rclone's shared client_id is being retired because of this.** Their own
  notice: Google will start charging on it, and rclone is "many hundreds of
  times over the free quota" — they estimate **millions of dollars per year**.

**→ Worth knowing for anyone who mounts Drive with rclone:** a remote configured
without its own `client_id`/`client_secret` is on the shared credential, and
will stop working when it is retired. The fix is to create your own OAuth client
and run `rclone config reconnect <remote>:`. This is independent of Vardo.

**Does it kill native Drive sync for us?** No. I did the math:

Free daily threshold: **400,000,000 quota units per project per day** (and this
threshold cannot be raised, but usage under it is never billed).

Per-method cost: `files.get` = 5, `files.list` = 100, `files.download` = 200,
`files.update` = 50.

Realistic password-manager profile, per user per day:

| Activity | Units |
|---|---|
| Metadata poll every 5 min (288 × 5) | 1,440 |
| 3 downloads | 600 |
| 5 uploads | 250 |
| **Total** | **≈ 2,300** |

400M ÷ 2,300 ≈ **~174,000 daily active users** on the free tier. Even at
aggressive 60-second polling it's ~50,000 DAU. Egress cap (1 TB/day) works out
to ~1.6M downloads/day of a 646 KB vault — not the binding constraint.

rclone's problem is that it's a **bulk file mover** — 200 units per file
download × millions of files. We sync one small file occasionally. Completely
different profile.

**Verdict: manageable, but it is now a real operational liability** that must be
monitored, and it argues for the sequencing in §3.1. Mitigations: cache
aggressively, use `changes.watch` push instead of polling, ship a
bring-your-own-client_id escape hatch, and set a Cloud budget alert on day one.

### 2.2 `keepass` cannot be made lossless without rewriting it

I read the source. `src/db/types/entry.rs`:

```rust
pub struct Entry {
    pub(crate) id: EntryId,
    pub(crate) parent: GroupId,
    pub fields: HashMap<String, Value<String>>,
    pub autotype: Option<AutoType>,
    pub tags: Vec<String>,
    pub times: Times,
    pub custom_data: HashMap<String, CustomDataItem>,
    pub(crate) icon: Option<Icon>,
    // ... foreground_color, background_color, override_url,
    //     quality_check, attachments, previous_parent_group, history
}
```

A **closed typed struct**. No catch-all. No unknown-children bag. Anything in the
XML it doesn't model is parsed into nothing and never written back. Its parsers
end in `_ => (None, None)` and `_ => Self::None`.

Now look at the maintainer's last six commits — every one is a fidelity patch:

```
2026-08-21  fix(DeletionTime): unify timestamp serialization mode
2026-08-15  fix(xml): serialize DeletedObject DeletionTime tag
2026-07-30  feat: preserve empty fields as is in original database
2026-07-19  feat: preserve group and entry insertion order in group
2026-07-09  fix: correct several cross-library compatibility issues
```

That is not a library converging on correctness. That is **whack-a-mole**, and
it's structural: with a closed model, every unmodelled element is a future bug
report, discovered only when someone's data goes missing.

This is the difference between *"we haven't found a fidelity bug yet"* and
*"fidelity is a property of the design."* For a password manager, only the
second is acceptable.

**This kills my "fork it and add DOM preservation" plan.** Threading an
`unknown: Vec<XmlNode>` through every type, every parser, and every serializer,
in the correct document order, *is* rewriting the XML layer. You'd touch every
file and inherit the model anyway.

### 2.3 KeePassXC is GPL-2.0-or-later — porting `Merger.cpp` sets our license

Verified from the file header: *"either version 2 or (at your option) version 3."*

If we port `Merger.cpp`, **the whole project must be GPL-2+**. That's a real
constraint, not a footnote. See §3.3.

### 2.4 The premise is stronger than I claimed

I said KeePassXC is ugly. I should have checked whether they're fixing it:

- The **"UI Redesign" EPIC (#1705) was closed in November 2019.** No successor.
- The **Qt6 transition has been WIP since January 2025** and is still open;
  maintainer response to "can this ship in 2.7.10?" was *"No. This needs more
  work."* They are stuck on a toolkit migration, not designing.
- **2.8.0's release notes are literally `* Placeholder for future release notes`.**
- Recent UI work is `macOS: Add Liquid Glass` — platform veneer, not redesign.

Meanwhile they're still shipping real features (passkeys, TOTP autotype,
Bitwarden import). So: **KeePassXC is healthy engineering with a frozen 2019
interface and no plan to change that.** The gap is real and will stay open.

### 2.5 Phase 0 needs no access to a real vault (good), but the corpus is harder than I thought

I verified `keepassxc-cli` 2.7.12 is installed and can create databases and add
entries non-interactively. So the conformance corpus is fully buildable **without
anyone's real master password ever entering this project.**

Caveat found by testing: `keepassxc-cli db-create` writes **KDBX 3.1** and
exposes no format/KDF flag (only `--decryption-time`). A 4.x corpus needs the
KeePassXC GUI, KeePass 2.x under wine, or hand-built fixtures. Budget for that.

Free win: `keepass`'s repo ships **30 MIT-licensed test fixtures** (KDBX 3.1,
4.0, 4.1, keyfiles, Twofish, ChaCha20, Argon2d/2id, TOTP, deleted entries,
challenge-response, deliberately-corrupted files). Take them.

---

## Part 3 — Arguments, where I take a position

### 3.1 Native Google Drive integration is a trap for v1. Build "local file, perfectly" first

**The case against native Drive in v1:**

1. It's the exact category of work that killed Buttercup. The maintainer's
   post-mortem names it: scope that *"comes in contact with operating systems,
   mobile devices and 3rd party services… each of those have requirements for
   software, especially security software, and that requires near constant
   attention."*
2. Google is actively re-pricing the API (§2.1). Terms will change again.
3. OAuth consent screens, app verification, and branding review are a permanent
   treadmill with a vendor who does not care about you.
4. An extracted client_id is a quota-abuse vector against *your* Cloud bill.
5. **The alternative is nearly free.** Anyone can mount Drive with `rclone`
   today. A local-file backend that is genuinely excellent gets you
   Drive, Dropbox, OneDrive, iCloud Drive, Nextcloud, Syncthing, SMB, and a USB
   stick — **for zero API keys, zero OAuth, zero verification, zero cost, zero
   vendor risk.**

**The honest steelman against my own position:** local-file sync over a FUSE
cloud mount is subtly awful. `mtime` lies or has 1-second granularity. Writes
aren't atomic. `inotify` doesn't fire reliably on FUSE. You can read a
half-uploaded file. You get no etag, so you cannot do optimistic concurrency —
you're reduced to content hashing and hoping. Native Drive gives you real etags,
real revisions, and `changes.watch`. These are not small advantages, and a
password manager that corrupts a vault is dead on arrival.

**Resolution — sequence, don't choose:**

The merge engine is the hard part and it is *identical* in both worlds. Build it
against the local-file backend, where every failure is debuggable and your
existing rclone mount is a working transport on day one. Add native Drive as a
second `StorageBackend` implementation once merge is proven and fuzz-tested.
`StorageBackend` must be a trait from commit one, with the FUSE hazards above as
explicit test cases (fake a lying mtime, fake a torn read).

You get Drive sync in v1 — via rclone, which you already trust and run. You get
*native* Drive in v2, with the risky part already de-risked.

### 3.2 Write the KDBX layer ourselves, DOM-first. This is the one place NIH is correct

Given §2.2, the options collapse to: accept silent data loss, rewrite `keepass`'s
XML layer (≈ writing it), or write it.

**Design — the whole argument in one idea:**

> The parsed XML tree is the source of truth. The typed model is a *view* over
> it. Edits are surgical mutations to the tree.

Then losslessness isn't a feature you maintain — it's a thing you never broke.
An element you don't understand is an element you never touched. KeeWeb's
`kdbxweb` works essentially this way, and in a decade it never ate anyone's
database. That is the track record to copy.

**Cost:** ~4 weeks. **Scope:** the container (TLV headers, VariantDictionary,
HMAC block stream, inner header) plus the XML mapping. Roughly 2,500 lines,
**fully specified in public documents** — keepass.info's format pages, Palant's
write-up, and `shadone/KDBXKit`'s spec, which even publishes Argon2id test
vectors with a known password.

**We are not writing crypto.** AES/ChaCha20/Argon2/HMAC/SHA all come from
RustCrypto, same as everyone. We're writing a file format parser.

**Risk control:** `keepass` becomes the **differential-testing oracle**, not the
dependency. Every fixture goes through both; disagreements are bugs in one of us
and worth investigating either way. Plus `cargo-fuzz` on the parser from week one.

**When I'd be wrong:** if Phase 0 shows `keepass` round-trips the entire corpus
byte-clean, use it and save a month. I doubt it — §2.2 says it structurally
can't for *unmodelled* elements — but the corpus decides, not me.

### 3.3 License: clean-room the merge semantics, ship Apache-2.0 + MIT

Three options:

| | Pros | Cons |
|---|---|---|
| Port `Merger.cpp`, go GPL-2+ | fastest, proven semantics | forces GPL on everything; the AI-authored `keepass-core` (Apache-2.0) becomes unusable as reference; kills any future permissive reuse |
| Clean-room, Apache-2.0 + MIT | matches Rust ecosystem norm; `keepass` (MIT) and `keepass-core` (Apache-2.0) both usable | must derive semantics from docs, not code |
| Clean-room, GPL-3 anyway | copyleft ethos, Buttercup/KeeWeb precedent | no technical benefit once clean-room |

**Recommendation: clean-room, dual Apache-2.0/MIT.**

Clean-room is very achievable here because the semantics are *published as prose*,
not just as code. KeePass's own sync documentation describes the algorithm, and
KeePassXC's commit introducing it enumerates the strategies explicitly:

> Default, Duplicate, KeepLocal, KeepRemote, KeepNewer, **Synchronize** (merge
> history, newest value stays on top, apply all deletions)

That's a specification. Implement from it. Read KeePassXC's *tests and issues*
for edge cases — notably #3340, spurious conflicts on identical databases from
timestamp-precision mismatches — without copying implementation.

Dual Apache/MIT is the Rust default, keeps `keepass` (MIT) available as an
oracle, and keeps `keepass-core` (Apache-2.0) available as a reference.

### 3.4 Defending Tauri against the strongest objection

**The objection, stated properly:** it is not about CVE counts. It's that you
would render your password list inside a full browser engine, in-process, and on
Linux that engine is WebKitGTK — a large, fast-moving attack surface with a JIT.
A pure-Rust GUI (`egui`, `iced`, or something like `sindon`, which is explicitly
built zeroize-first with no WebView) removes that entire class of risk. For a
password manager, "no embedded browser" is a genuinely serious argument.

**Why I still say Tauri:**

1. **The security delta is mostly recoverable by architecture.** If the WebView
   never holds secrets in bulk — list view gets titles/usernames/URLs only,
   secrets are fetched one field at a time on explicit user action, clipboard is
   written from Rust — then a WebView compromise leaks *entry titles*, not the
   vault. That converts a catastrophic risk into a bad-but-survivable one. This
   is a hard architectural rule, not a guideline.
2. **The alternative fails at the stated goal.** You asked for Signal-Desktop
   beautiful. Getting Signal-grade polish out of immediate-mode Rust GUI is
   5–10× the design effort, and the projects that tried to "do it properly
   natively" are the ones that don't exist.
3. **Tauri is a security *gain* over the obvious alternative.** The realistic
   counterfactual isn't a Rust GUI, it's Electron — where the Node side has full
   system access by default. Note the irony: Signal Desktop itself is Electron.
4. **The escape hatch is real.** Rules 1's architecture means `ui/` is a thin
   presentation layer over typed commands. If WebKitGTK becomes intolerable, a
   native frontend swaps in against the same `vault-core`. Keeping that seam
   clean is worth doing for its own sake.

**Position: Tauri, with the no-bulk-secrets rule written into the architecture
docs and enforced in review.** Revisit only if Linux WebView pain becomes the
dominant cost.

### 3.5 The feature that will kill this project is the browser extension

Buttercup died of scope. The single largest scope trap available to you is
autofill, because it's a browser extension × 4 browsers × Manifest V3 churn ×
two extension stores' review processes × a native messaging host on three OSes.

**Do not build one.** Instead: **implement the `keepassxc-browser` native
messaging protocol** so the existing, mature, already-installed KeePassXC
extension talks to your app unmodified.

The protocol is fully documented (`keepassxc-protocol.md` + the Protocol V2 draft
wiki): X25519 key exchange via TweetNaCl `crypto_box`, per-session keypairs, a
persistent identification keypair from `associate`, and a stdio proxy bridging to
a Unix socket / named pipe. There's even a Rust proxy implementation to crib from
(`keepassxc-proxy-rust`). You implement a socket server and ~15 message handlers.
Weeks, not quarters — and zero store review, zero MV3 churn, zero extension
maintenance, forever.

Same logic already applies to mobile: **KeePassium is your iOS app.** Never write
one.

### 3.6 The uncomfortable question: should this exist?

Steelman for not building it: this is 4–6 months to reach *parity-minus* with a
free tool you already have installed. KeePassXC is correct, audited, actively
maintained, and its ugliness is an aesthetic complaint about software that
guards your entire digital life. The graveyard in `RESEARCH.md` §2 is full of
people who had this exact idea in the last 18 months.

**Why I think it's still worth doing — but not for the reason you gave:**

"KeePassXC is ugly" is a real motivation but a weak *moat*. Anyone can reskin;
a dozen people are trying right now and going nowhere.

The defensible core is **§3.1's merge engine.** Nobody has done multi-device
KDBX sync well. KeePassXC's own merger produces spurious conflicts on *identical
databases*. Strongbox's documented fix for cloud sync is "turn off safe saving."
Every KeePass client punts conflict resolution to the user or to
last-write-wins. A KDBX client where **multi-device editing simply never loses
data, and shows you exactly what a merge will do before it does it** is a thing
that does not exist, is genuinely hard, and is worth building.

The pretty UI is how you get people to try it. The merge engine is why they stay.

**So: build it, but understand which part is the product.** If the merge engine
is right and the UI is only decent, you have something. If the UI is gorgeous and
merge is naive, you're project number thirteen in the graveyard.

---

## Part 4 — Decisions I'm making

Answering my own open questions. Push back on any of these.

| # | Question | Decision | Rationale |
|---|---|---|---|
| 2 | License | **Apache-2.0 OR MIT**, clean-room merge | §3.3 |
| 3 | KDBX 4.0 vs 4.1 | **Preserve input version by default.** Read 3.1/4.0/4.1, write back what we read. 4.1 upgrade = explicit, consented, one-way, with backup | Never silently change your file's format. Kills a whole class of interop surprise |
| 6 | Personal tool or product | **Build to product standards, ship when personally satisfied.** No roadmap promises, no issue-tracker obligations until v1 | Buttercup's failure mode was obligation, not code |
| 7 | Design | **Signal's restraint, not Signal's layout** | A vault's information architecture is a tree + list + detail, not a conversation list. Copying the layout literally would be cargo-culting |
| — | Frontend | **Svelte 5** | Smaller runtime = less to audit; no virtual DOM retaining stale copies of decrypted strings |
| — | KDBX library | **Write it, DOM-first. `keepass` as differential oracle** — pending Phase 0 falsification | §3.2 |
| — | Sync v1 | **Local file (works with an rclone mount today).** Native Drive is v2 | §3.1 |
| — | Autofill | **Speak `keepassxc-browser`'s protocol.** Never ship an extension | §3.5 |

### Left open at the time

1. **Name.** Blocked the OAuth client, bundle identifier and crate names.
   Anything but "Pass*" or "*Vault", which is a swamp. Settled on Vardo.
2. **Key files and hardware keys.** Changes the `CompositeKey` work and the
   unlock UI. Settled on password only for now.
3. **Whether §3.1 was right.** Local-file-first was the strongest
   recommendation here, and it was wrong: [`ADR-001`](ADR-001-sync.md)
   explains the evidence that reversed it.

---

## Part 5 — Revised roadmap

**Phase 0 — Conformance harness + library decision (1 week)**
Build corpus with `keepassxc-cli` + KeePassXC GUI + KeePass 2.x/wine + the 30
MIT fixtures from `keepass`. Differential `read→write→read` against `keepass`.
Falsify §3.2. *No access to a real vault at any point.*

**Phase 1 — `vault-kdbx`, DOM-first (4 weeks)**
Container + XML layer. `cargo-fuzz` from day one. Differential tests in CI.
Ship `vault-cli` alongside — it's the test driver and the field-debugging tool.

**Phase 2 — Read-only viewer (3 weeks)**
Tauri shell, async unlock with progress, tree/list/detail, search,
clipboard-with-auto-clear, auto-lock. Zero write paths — a real vault is at
zero risk while the UI is built.

**Phase 3 — Writes (3 weeks)**
Edit/create/delete, attachments, history, generator. Atomic local write, backup
rotation. Every write verified against KeePassXC *and* KeePassium before merge.

**Phase 4 — The actual product: merge + sync (5 weeks)**
`StorageBackend` trait. Local-file backend with FUSE hazards as test cases.
Clean-room 3-way merge with `Synchronize` semantics. Dry-run preview UI. Chaos
testing: concurrent edits, torn reads, lying mtimes, clock skew.

**Phase 5 — Polish (4 weeks)** — design system, keyboard-first, command palette,
TOTP, password health.

**Phase 6 — Distribution** — Tauri updater (Ed25519, mandatory), Apple Developer
$99/yr + notarization, Azure Trusted Signing ~$10/mo for Windows, AppImage +
Flatpak + AUR.

**v2, only if v1 is genuinely good:** native Google Drive backend,
`keepassxc-browser` protocol server.

**Never:** mobile app, own browser extension, server, teams/sharing.

---

## Appendix — corrections summary

| `RESEARCH.md` said | Reality |
|---|---|
| `keepass-rs` has merge, signatures, OTP, Flutter bindings | That's a different, 57-download crate. The real one is `keepass`. |
| `keepass-core` is a credible dependency candidate | 0 stars, 3 months old, AI-authored. Reference only. |
| Use `drive.file` + Google Picker (implying embedded JS Picker) | Embedded Picker is **impossible** in a webview. Use the desktop `trigger_onepick=true` + system-browser flow. |
| Fork `keepass`, add DOM preservation | Its closed typed model makes that a rewrite. Write it instead. |
| Port KeePassXC's `Merger` | It's GPL-2+; that sets our license. Clean-room from published prose instead. |
| Argon2 "~1–2 s" | Measured 0.871 s here; ~3 s on older hardware. |
| Google Drive API is free | Being re-priced in 2026. ~174k DAU headroom for us — but **rclone's shared credential breaks this year.** |
| KeePassXC is ugly | Confirmed, and structurally so: redesign EPIC closed 2019, Qt6 port stalled 18 months, 2.8.0 notes are a placeholder. |
