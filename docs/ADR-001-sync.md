# ADR-001: Sync architecture

**Status:** Accepted. Supersedes `CRITIQUE.md` §3.1, which was wrong.

**Decision:** The app owns the network path. Native Google Drive API in v1.
Never sync the vault through a cloud-mounted filesystem.

---

## I was wrong, and here's the evidence that changed my mind

In `CRITIQUE.md` §3.1 I argued for "local file done perfectly," reasoning that
your existing rclone mount already delivers Drive sync for free, so native API
integration could wait for v2.

Then I went looking for how that actually goes in practice. It goes like this —
all from KeePassXC's own issue tracker:

| Issue | What happens |
|---|---|
| **#9140** (Drive for Desktop, macOS) | User edits vault, Drive shows "syncing," **remote file's last-modified stays 3 weeks stale.** Only backup files got uploaded. Android device never sees the changes. Fix required deleting and recreating the entire Drive folder. |
| **#8790** (Drive for Desktop, shared file) | Saving creates `1Passwort (1).kdbx`. **The user's changes go into a fork, not the file.** Maintainer: "We don't add a (1) in any of our code — that must be happening on Google Drive's side." |
| **#2066** (Drive File Stream, two colleagues) | `(1).kdbx` then `(1) (1).kdbx`. Reporter: *"the metadata is updated but changes to the actual data are lost."* |
| **#11969** (GVfs, Ubuntu 24.04) | `Writing the database failed. Operation not supported.` Then an **infinite dialog loop** — the only escape is killing the process and losing the session's changes. Maintainer confirms: known issue, unfixed. |
| **#13050** (rclone mount — *your exact setup*) | `Input/output error`. The discussion spells out the mechanism: KeePassXC's atomic save is write-temp → delete-original → rename, and **step 2 fails because the cloud client holds a lock.** Its own workaround table concludes the only reliable option is *"Local-first workflow (edit local → sync after)."* |
| **keepass2android #2467** | Same class of failure on Android. |

Add the Strongbox KB article I'd already cited in `RESEARCH.md` and then failed
to apply to my own proposal: safe-save's rename **changes the cloud file's
identity**, which invalidates the mobile app's file reference.

**This is not an edge case. This is the single most common way KeePass
deployments break, across every client, every OS, and every cloud provider, for
eight years running.** KeePassXC's own advice to affected users is *"move the
database to a local folder."*

I proposed building v1 on the one code path with a documented eight-year history
of silently losing password data. Your instinct was right and my reasoning was
backwards.

### The root cause, stated once

A `.kdbx` save is a **read-modify-write of a whole file**. A sync daemon is a
**best-effort, eventually-consistent, opaque file replicator**. Compose them and
you get:

- Writes that "succeed" locally and never reach the server.
- Locks that make atomic rename impossible, so you must choose between
  "save fails" and "disable safe saves" (i.e. risk corruption on crash).
- Conflict forks (`(1).kdbx`) that the app never learns about, so your changes
  live in a file nothing will ever open again.
- No signal, ever, about whether your data is actually safe.

The application cannot fix any of this from behind a filesystem API, because the
filesystem API doesn't expose the concepts involved.

---

## But the reason to go native isn't the one I was about to concede

Before folding, I checked the technical argument *for* the Drive API — that it
gives the merge engine a compare-and-swap primitive via etags. In
`RESEARCH.md` §7.3 I'd written: *"send `If-Match` with the known etag so a
concurrent write returns 412/409 instead of clobbering."*

**That was also wrong. Drive API v3 has no compare-and-swap at all.**

Verified against a developer's actual curl tests plus the v2→v3 migration docs:

- v3 **removed `etag` from the File resource** (the migration table lists
  `Files.etag` as "n/a"). It existed in v2; it's gone.
- Drive returns **no `ETag:` response header** to feed an `If-Match` with.
- `If-Match` on `PATCH /upload/drive/v3/files/{id}` is **silently ignored** —
  no 412, the write just succeeds.
- `If-Match` on `DELETE` — same.
- There *is* a `version` field, but with no server-enforced precondition it's
  "still racy between read-version and write."

So both architectures are "read, merge, write, and hope." Native Drive is not
transactionally safer in the way I assumed.

**Here's what it actually gives you, which turns out to be enough:**

1. **A trustworthy change signal.** `files.get?fields=version,modifiedTime,md5Checksum,headRevisionId`
   costs 5 quota units and tells you the truth. A FUSE mount's `mtime` lies —
   see #9140, where the remote timestamp was three weeks stale while the daemon
   reported "syncing."
2. **Write confirmation.** `files.update` returns 200 when the bytes are
   *stored on Google's servers*. A `write()` returning on a FUSE mount means
   nothing. This is the difference between being able to tell the user "your
   changes are safe" and not.
3. **Revision history — and this is the load-bearing one.** Every content upload
   creates a new revision, and prior revisions are retained.

### The insight that makes safe sync possible without CAS

> **Drive's revision history *is* the conflict log. You cannot prevent a race,
> but no data is ever actually destroyed, so you can always detect one after the
> fact and repair it.**

That's the whole design. Losing a race is recoverable, not fatal — fetch the
revision that got clobbered, merge it in, re-upload. And it is available *only*
through the API. A FUSE mount gives you none of it: when Drive forks your file
to `(1).kdbx`, nothing tells the app, and the data is gone as far as the app is
concerned.

### Transaction protocol

```
1. GET files/{id}?fields=version,modifiedTime,md5Checksum,headRevisionId   [5u]
2. remote unchanged since last sync?
     → upload merged local                                                [50u]
   remote changed?
     → download, 3-way merge vs. last-synced base, upload
3. Immediately re-read headRevisionId.
   Does it match the revision our upload returned?
     yes → done, sync point committed
     no  → someone raced us. Their write is a revision; ours is a revision.
           Fetch both, merge, re-upload. Loop (bounded, with backoff).
4. Pin sync-point revisions with keepRevisionForever
   (cap 200, consumes storage quota — rotate).
```

Step 3 is the part every other KeePass client skips, and it's why they lose
data. The race window shrinks from "however long your sync daemon feels like" to
one HTTP round-trip, and even that window is repairable.

**The merge engine matters *more* under this design, not less.** With no CAS,
races are inevitable by construction — merge is the only thing standing between
a race and data loss. This further confirms `CRITIQUE.md` §3.6: merge is the
product.

---

## The objection I raised against native Drive was much weaker than I claimed

I listed "OAuth verification treadmill" as a reason to defer. Checked it:

> **"If your app utilizes only non-sensitive scopes, it is not mandatory for
> your app to complete the app verification process."**

`drive.file` is **non-sensitive**. The unverified-app warning screen and the
100-user cap apply to apps requesting **sensitive or restricted** scopes. We
request neither.

So: **Published / External / Unverified with `drive.file` only** — any Google
user can authorize, no scary interstitial, no user cap, no security assessment,
no annual re-verification. The only thing withheld is our **app name and logo**
on the consent screen, which needs "brand verification" — a documented
lighter-weight process, not the full review.

That was my strongest objection and it mostly evaporates. Combined with §I, the
case for native Drive in v1 is now decisive.

**Two traps found while checking:**

- **Testing mode expires refresh tokens after 7 days.** *"Authorizations by a
  test user will expire seven days from the time of consent. If your OAuth
  client requests an offline access type and receives a refresh token, that
  token will also expire."* Fine for dev; means we must move to Published
  before real use, or you'll re-authenticate weekly forever.
- **`changes.watch` push notifications are unavailable to us.** They require a
  registered domain and a publicly reachable HTTPS webhook. A desktop app has no
  such endpoint, and standing up a relay would mean running a server — which we
  are not doing. **We poll.** Quota math in `CRITIQUE.md` §2.1 already assumed
  polling: ~2,300 units/user/day against a 400M/day free threshold, so roughly
  174k daily active users of headroom. Polling is fine.

---

## Decision

### 1. The vault's canonical local copy lives in app-private storage

`~/.local/share/<app>/vaults/<id>/` — **never** in `~/GoogleDrive/`, never in
any synced folder. The app writes there atomically (temp + fsync + rename on a
real filesystem, where that actually works) and talks to Drive over HTTPS.

This single rule sidesteps every failure in the table above.

### 2. Google Drive backend, `drive.file` scope only, in v1

- **Auth:** OAuth 2.0 installed-app flow, loopback redirect (still supported for
  the `Desktop app` client type; deprecated only for iOS/Android/Chrome), PKCE.
  Google auto-issues a client_secret for Desktop clients that the token endpoint
  requires — embed it; per Google's own docs *"the client secret is obviously
  not treated as a secret."*
- **File selection:** desktop Picker flow — `trigger_onepick=true` on the
  authorization URL, system browser, result via redirect. **Not** the embedded
  JS Picker, which cannot work in a webview (`CRITIQUE.md` §1.3).
- **Writes:** `files.update` media upload on the existing `fileId`. Preserves
  file identity — which is what keeps KeePassium on your iPhone working.
- **Tokens:** OS keychain via `tauri-plugin-stronghold` or platform keyring.
  Never in the vault, never in a config file.

### 3. `StorageBackend` trait from commit one

```rust
trait StorageBackend {
    async fn stat(&self) -> Result<RemoteState>;   // version, mtime, hash, revision
    async fn read(&self) -> Result<Vec<u8>>;
    async fn write(&self, bytes: &[u8]) -> Result<RemoteState>;
    async fn revisions(&self) -> Result<Vec<Revision>>;  // conflict recovery
}
```

Implementations: `GoogleDrive` (v1), `LocalFile` (v1, trivial), `WebDAV` /
`Dropbox` / `OneDrive` (later). `LocalFile` is for genuinely local disks, USB
keys, and Syncthing — and its docs will say, in bold, do not point this at a
cloud mount.

### 4. Detect and refuse the footgun

At vault-open, check whether the path is under a known cloud mount — FUSE type
`rclone`/`gvfs`/`gdrive`, `~/Google Drive`, `~/Dropbox`, `~/OneDrive`,
`~/Library/CloudStorage`. If so, **warn prominently and offer one-click
migration** to app-private storage + native backend.

KeePassXC has an open discussion (#13050) proposing exactly this and hasn't
shipped it. We ship it in v1. It's maybe 100 lines and it prevents the most
common way this software eats people's passwords.

---

## Consequences for people who already sync through a mount

**An rclone or similar mount stops being the sync mechanism** and becomes a
plain backup location. The app takes over the Drive relationship directly.

Note the interaction: if someone keeps editing the same vault through a mount
with KeePassXC *while* the app syncs natively, KeePassXC's safe-save will
rename the file, **Drive will assign it a new `fileId`**, and our `drive.file`
grant — which is per-file — dies with the old ID. They would have to re-pick
the file. The app must detect a dead `fileId` and offer re-selection.

---

## Revised roadmap

Phases 0–3 are unchanged — conformance harness, `vault-kdbx` DOM-first,
read-only viewer, writes. Phase 4 is where this lands:

**Phase 4 — Sync + merge (6 weeks, was 5)**

1. `StorageBackend` trait + `LocalFile` (~2 days)
2. Clean-room 3-way merge, `Synchronize` semantics, dry-run preview (~2.5 weeks)
3. Google Drive backend: OAuth + Picker + `files.update` + revisions (~1.5 weeks)
4. Race protocol §3 above, including post-write revision verification and
   clobber recovery (~1 week)
5. Chaos tests: concurrent edits from two clients against a real Drive test
   account, forced races, killed processes mid-upload, clock skew, expired
   tokens, dead `fileId` (~1 week)

Step 5 is not optional. This is the part of the product that has to be *boring*.

---

## What I still hold from before

Everything else in `CRITIQUE.md` stands:

- **Write `vault-kdbx` ourselves, DOM-first** (§3.2) — reinforced, since sync
  now means round-tripping other clients' files constantly.
- **Clean-room merge, Apache-2.0 + MIT** (§3.3).
- **Tauri, with the no-bulk-secrets rule** (§3.4).
- **Never ship a browser extension — speak `keepassxc-browser`'s protocol** (§3.5).
- **Merge is the product, the UI is the invitation** (§3.6) — and ADR-001
  strengthens this: with no CAS available from Drive, correct merge is the
  *only* thing preventing data loss.

---

## Test strategy — and why this needs almost no Google account at all

I was asked to just create a throwaway Google account. I'm not going to, for two
reasons — one principled, one practical.

**Principled:** automated Google account creation violates Google's ToS, and
their signup is explicitly a real-time trust-scoring system built to detect it.
The entire "how to get around it" ecosystem is SMS-resale services (~$1–2 per
verification) and anti-detect browsers that spoof fingerprints. Not a supply
chain I'm putting anywhere near this project.

**Practical, and more important:** an auto-created account is exactly the kind
that gets flagged and suspended later. If that account owns the Cloud project
holding our OAuth client, **the app's authentication dies with it** for every
user. That's a real availability risk, not a hypothetical.

### The two things people conflate

| | What it is | Who should own it | Data access |
|---|---|---|---|
| **Cloud project + OAuth client** | a registered app identity | a stable, legitimate account | **none** — grants nothing by itself; every user still has to consent |
| **Test user account** | a Drive we write junk files into | disposable | full, to its own Drive |

These are different objects and the confusion is what made this feel blocking.
Your personal account can safely own the Cloud project — an OAuth client is a
developer credential, not a grant. It does not expose your Drive to anything.
The account we *race test clients against* is the one that should be a throwaway.

### You don't run chaos tests against a real API anyway

Even with an account in hand, driving Phase 4's chaos suite through real Drive
would be wrong: too slow for thousands of iterations, non-deterministic, flaky
in CI, and — fatally — **you cannot force the interesting races.** You can't ask
Google to deliver two writes 3 ms apart on demand.

So the test pyramid is:

**1. `FakeDrive` — in-process, deterministic, where 99% of testing happens.**
A `StorageBackend` impl that models Drive's *actual* semantics, including its
flaws: no CAS, `If-Match` silently ignored, revision history, `(1).kdbx`-style
forking, latency injection, and seeded fault injection. Thousands of seeded
property-test iterations per CI run.

Starting point: **`gfeh-drive-emulator`** — an in-process Drive v3 emulator, in
Rust, that already implements
`PATCH /upload/drive/v3/files/{id}?uploadType=media` — the exact
replace-content-preserving-id call this design depends on. Its docs independently
state our own reasoning: *"re-creating the file would mint a new id and break
every reference."* (Others exist — `pubkey/google-drive-mock`, `@emulators/google`
— but they're JS, and `google-drive-mock` self-describes as "Mostly Vibe-Coded.")

**2. Contract tests — ~20 tests, against real Drive, run manually.**
Their only job is proving `FakeDrive` doesn't lie: confirm `If-Match` really is
ignored, confirm `headRevisionId` behaves as assumed, measure the real race
window, confirm `drive.file` grant persistence and `fileId` death on rename.
This is the *only* thing needing a real account, and it's a Phase 4 concern.

**Net: Phases 0–3 — conformance harness, `vault-kdbx`, viewer, writes — need
zero Google anything.** That's ~10 weeks. The account question is not blocking
and shouldn't be treated as such.

### When we do need it (Phase 4)

Create a throwaway yourself — 5 minutes, and Google permits multiple accounts
per phone number, so your existing number works without exposing anything. Give
me only its credentials, in a vault entry or an env var, never your personal
account.

---

## Left open at the time

1. **Name.** Blocked the Cloud project and crate names. Settled on Vardo.
2. **Key file or hardware key.** Settled on password only for now.
3. **A throwaway Google account for contract tests.** Used in Phase 4; the
   live runs create and delete their own vaults and can reach nothing else.
