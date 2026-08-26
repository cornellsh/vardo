# Status

Container, typed view, and merge planning are done. Sync and UI are not
started.

Run `./check.sh` — fmt, clippy `-D warnings`, and the full suite. It fails
loudly rather than printing a total that hides failures, which an earlier
version of that command did.

| Crate | State |
|---|---|
| `vardo-kdbx` | complete, fuzzed |
| `vardo-vault` | typed view + safe editing |
| `vardo-merge` | planning and application, chaos-tested |
| `vardo-sync` | backends, fake Drive, race protocol |
| `vardo-drive` | Drive backend + OAuth, tested offline |
| UI | not started |

## What works

**Read** — KDBX 3.1, 4.0, 4.1. AES-256-CBC, ChaCha20, Twofish. Argon2d,
Argon2id, AES-KDF. GZip. Full authentication chain: header SHA-256, header
HMAC, per-block HMAC (4.x) or hashed block stream (3.1). Inner header with
unknown-field retention.

**Write** — KDBX 4.x, preserving the input's version, cipher, compression and
KDF. Fresh master seed, IV, KDF salt and inner stream key on every save. Files
we write are opened by `keepassxc-cli` — asserted in the test suite, not
assumed.

**Protected values** — ChaCha20 and Salsa20 inner streams, decrypted in
document order. Cross-validated against `keepassxc-cli show -a Password`,
including ordering across nested entry history.

**Byte-stable XML** — parse → serialise reproduces the input exactly, across
the whole fixture corpus and under 6.5M fuzz executions.

Deliberately not implemented: key files (password-only for now), KDBX 3.1
*writing* (the format has no authenticated integrity, and every client that
reads 3.1 also reads 4.x), ARC4 inner streams (broken since 2014).

## Numbers

| | |
|---|---|
| Tests | 188, all passing (~50 s) |
| Clippy | clean at `-D warnings`, `--all-targets` |
| Fuzz executions | ~14.9M across four targets, zero failures |
| `unsafe` | none — `#![forbid(unsafe_code)]` |

## The bug that shaped the vault layer

A merge test failed with three *untouched* entries reporting conflicting
password changes. The cause was not in merge.

`write::save` decrypted protected values from the DOM **at save time**.
Protected values share one keystream and are decrypted **by position**, so any
edit that added, removed or reordered a protected element left the ciphertext
keyed to the old positions while the ordinals came from the new ones.
**Deleting one entry corrupted the password of every entry after it** — silently,
with no error, in a file that still opened.

The fix is structural rather than careful: an unlocked [`Vault`] holds
*plaintext* in its DOM and encrypts only at save. Inside that window there is
no positional coupling at all, so arbitrary mutation is safe.

The cost is plaintext passwords in an in-memory XML tree rather than in
individually zeroized buffers, bounded by `Vault::lock()`. That is the same
exposure every KeePass client has while unlocked, and it buys the elimination
of an entire silent-corruption class. Both the hazard and the fix have
regression tests, so nobody can "simplify" it back.

Worth noting: this was found by a test asserting a *product* property, not by
review, and not by fuzzing. The container layer was already fuzzed to 14.9M
executions and could not have caught it — nothing was malformed.

## The three decisions that shaped it

**1. The DOM is the source of truth; the typed model is a view.**

Phase 0 measured the alternative. The `keepass` crate cannot write KDBX 4.0 at
all, and its 4.1 round-trip rewrites 22% of an untouched document, loses two
`IconID`s and a `CustomIconUUID`, and fabricates seven `QualityCheck` elements.
Not from bugs — from a closed typed struct that cannot represent what it does
not name.

For a password manager that is bad. For one whose value is a **merge engine**
it is fatal: a save that changes bytes nobody asked to change manufactures
phantom conflicts on every sync. That is KeePassXC issue #3340, which their
maintainers have never fully fixed.

**2. Raw payloads are not comparable; canonical payloads are.**

Every save re-encrypts protected values under a fresh inner key, because
reusing one would reuse keystream at the same position and leak the XOR of the
old and new secret. So two files holding identical data differ in every
password field, by design.

`canonical_payload()` — the XML with protected values as plaintext — is the
comparable form, and the only thing merge may diff. This is written down in
`protected.rs` because getting it wrong is subtle and catastrophic.

**3. KDF parameters are trusted before they can be authenticated.**

The header HMAC's key comes from the KDF, whose parameters live in the header.
The ordering is forced by the format:

```text
read KDF params (attacker-controlled) → run KDF (expensive) → verify HMAC
```

And the header SHA-256 is *unkeyed*, so it stops accidental corruption and
nothing else. A file claiming `Argon2 M=2GiB, I=4294967295` will allocate two
gigabytes and grind for hours before anyone discovers it was never a vault.
`Limits` is the only available defence; `tests/adversarial.rs` builds real
bombs, repairs the digest exactly as an attacker would, and times the refusal.

## What fuzzing was worth

Four bugs, three of them silent data corruption, none caught by review. The
worst: `<a/></x><b/>` parsed as `<a/>` — a stray end tag ended the epilogue
loop and the rest of the file vanished with no error. Entries missing on open,
then destroyed permanently by the next save.

Details in `fuzz/README.md`.

## Merge semantics

`plan()` never mutates. It returns what a merge *would* do, so the user sees it
first. KeePassXC added a preview dialog in PR #10173; here it is the only way in.

- Newer edit wins; the loser is preserved in history rather than dropped.
- **Same-second divergent edits are a conflict, never a coin flip.** KDBX stores
  whole seconds, so this happens for real, and guessing is how vaults get
  quietly corrupted. This is KeePassXC #3340.
- Delete-versus-edit resolves by time; the deletion wins only if strictly newer.
- A disappearance with no tombstone **keeps** the entry — an unexplained absence
  is not a recorded deletion, and keeping is recoverable while dropping is not.
- **Without a base, nothing is ever deleted.** A first sync cannot tell "they
  added it" from "we deleted it".
- Field order is a writer's choice, not user data, so reordering is not a change.

The load-bearing test is `identical_vaults_produce_no_changes`: two saves of the
same data share no ciphertext at all, and merge must still see nothing.

## Sync

Drive has no compare-and-swap, so two devices *will* overwrite each other and
no care in the transport layer prevents it. What makes that survivable is that
every write leaves a revision, so nothing is ever actually destroyed.

The dangerous direction is not obvious, and I got it wrong first: it is **not**
"did somebody overwrite us afterwards" — that costs nothing, since our write is
still a revision and they merged on top of it. It is **"did somebody write
between our read and our write, so we buried a version we never saw?"**

Detection: the revision directly beneath ours must be what was head when we
wrote. Anything else means an unseen write went under us; fold it in and retry.
This check is what every other KeePass client omits.

`FakeDrive` models Drive *including its flaws* — clobbering writes, revision
history, losable file identity, injectable faults — because a fake offering
atomic CAS would let a broken protocol pass here and lose data in production.
Its `race_before_write` lands another device's write inside ours, which is the
one thing a real API cannot be asked to do on demand.

`LocalFile` is for genuinely local disks only, and `looks_cloud_synced()` flags
the paths behind KeePassXC issues #9140, #8790, #2066, #11969 and #13050 so the
app can offer to migrate — a fix they proposed and never shipped.

## Chaos testing

`crates/vardo-sync/tests/chaos.rs` runs randomised two-device interleavings and
asserts three things every time: the devices **converge**, committed passwords
stay **reachable**, and the remote is a **readable vault at every instant**.
Seeded, so a failure reproduces exactly. 60 scenarios plus a 120-step run, in
about four seconds.

## Google Drive

`drive.file` only — a **non-sensitive** scope, so by Google's own policy an app
using only non-sensitive scopes need not complete verification. No review, no
security assessment, no user cap. Requesting the broad `drive` scope instead
would make it *restricted* and trigger a paid third-party security assessment.

The file picker runs in the **system browser** via `trigger_onepick=true` on the
authorization request. The JavaScript Picker cannot work inside a desktop
WebView: the origin is `file://` or `localhost`, `postMessage` fails the origin
check, and it 403s — frequently only once packaged, having worked in dev.

Writes are `PATCH .../files/{id}?uploadType=media`: update in place, never
create-and-delete, which would mint a new file id and kill every other client's
per-file grant. That is precisely how KeePassXC's safe save breaks mobile.

All of it is tested through an `HttpTransport` seam, offline. Code that can only
be run with a Google account is code nobody tests.

**`crates/vardo-drive/tests/contract_notes.md`** lists the eight assumptions that
still need confirming once against live Drive — chief among them that
`If-Match` really is ignored, and that `files.update` really does preserve the
file id. They are stated assumptions, not verified facts, and that file exists
so the distinction stays visible.

## Next

1. **UI** — Tauri v2 (see `ADR-002-ui-toolkit.md`). Everything below it is now
   boring, which was the point.
2. Run the contract tests once a throwaway Google account exists.
3. Distribution: Ed25519 updater signing, Apple notarisation, Azure Trusted
   Signing.

## Reading order

- `RESEARCH.md` — landscape, initial architecture
- `CRITIQUE.md` — red team of the above, with corrections
- `ADR-001-sync.md` — sync architecture (supersedes `CRITIQUE.md` §3.1)
- `PHASE0-FINDINGS.md` — why we wrote the container layer, with evidence
- `fuzz/README.md` — what fuzzing found
