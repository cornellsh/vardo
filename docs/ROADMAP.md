# What to do next, and why

> **Progress.** Sections 1, 2 and 3 are done, as are 4.1, 4.2 and 4.4, and
> most of 5. The extraction in 2 turned out to fix 1.2 as a side effect: once
> saving moved behind a plain `Session::save`, the command became a
> `spawn_blocking` wrapper and stopped freezing the window.
>
> **Still open:** property-based merge testing (4.3), the OS keyring for the
> Drive token, a lock against two instances editing one local file, the quick-
> access hotkey and command palette, signed release builds, and the browser
> extension ([`ADR-006`](ADR-006-browser-extension.md)). WebDAV waits until
> someone needs it.

Written after Phase D, when the app can read, edit, save and sync real vaults.
Ordered by what gets **more expensive the longer it waits**, not by what is
most visible.

The honest summary: the crates below the app are in good shape. The app itself
has accumulated real logic in a place where it cannot be tested, and three
specific defects that a demo will never surface.

---

## 1. Three things that are wrong now

These are not polish. Each is either a data-loss risk or a decision that gets
harder to reverse with every commit.

### 1.1 Nothing stops merging two unrelated vaults

`vardo_merge::plan(base, local, remote)` takes two vaults and merges them. It
never asks whether they are the *same* vault.

Point sync at the wrong `.kdbx` — a stale copy, a different database, a file
picked by mistake — and every entry from one is added to the other. The result
is a single fused vault with two histories, and no automatic way back. It is
recoverable only from the backup files, and only if someone notices.

**The fix is cheap.** KDBX gives each root group a UUID, and it is stable
across re-encryption:

```
test_db_kdbx4_with_password_argon2id.kdbx   root=qvGgDSuYT3iX3own5Fbogg==
test_db_kdbx4_with_password_argon2.kdbx     root=qvGgDSuYT3iX3own5Fbogg==   same vault, different KDF
test_db_kdbx41_with_password_aes.kdbx       root=rE7/paNnwki2gTNZAHhaOA==   different vault
```

So: refuse to merge when root UUIDs differ, and make the caller pass an
explicit override for the one legitimate case (deliberately merging a vault
someone sent you). This belongs in `plan()` as a `MergePlan::Refused` variant
rather than in the UI, because the UI is not the only caller.

### 1.2 Saving freezes the window

`save_vault` is a synchronous Tauri command that takes the state lock and then
does **two full KDF passes**: one to encrypt, one to re-open and verify. On the
reference vault that is 0.871 s each — call it **1.7 s of frozen UI on every
save**, with the mutex held so nothing else can even read.

This is precisely the behaviour the unlock screen was designed to avoid, and it
was reintroduced at the other end. `open_vault` already does the right thing
with `spawn_blocking`; save must too.

Doing it properly means the encode must not hold the lock for its whole
duration — snapshot what is needed, release, compute, then re-acquire to swap
in the result and handle the case where the vault changed underneath.

The verification pass is **not** the thing to cut. It is what stops a bad save
overwriting the only copy.

### 1.3 `StorageBackend` is shaped around Drive's weakness

Decided in `ADR-005` and still not done. The trait has no conditional write, so
every backend inherits Drive's inability to do one — and the live contract run
has now *confirmed* `If-Match` is ignored, so the emulation is genuinely
required for Drive and genuinely unnecessary for everyone else.

Fix the trait **before** a second backend exists, or WebDAV and S3 will be
written against the wrong shape and the migration cost doubles.

---

## 2. The structural problem underneath — **done**

**Real logic lived inside the Tauri crate, where it could not be tested.**

```
commands.rs  797 lines
review.rs    238
sync.rs      226
save.rs      169
generate.rs  157
```

`generate.rs` and `review.rs` have tests because they are pure. `save.rs` and
`sync.rs` do not, because reaching them means constructing a `tauri::State`.
That is why the command layer has almost no coverage, and why every check of
the write and sync paths so far has been *me driving a GUI with `xdotool` and
reading screenshots*.

That is not a sustainable way to verify a password manager.

**The fix**: move orchestration out of `src-tauri` into a plain crate —
`vardo-core` — that knows nothing about Tauri. Sync preview/apply, the save
path, entry listing and the view models all move. `src-tauri` keeps only
`#[tauri::command]` wrappers that deserialise, call core, and serialise back.

Then the interesting paths get ordinary Rust tests: save-verify-rejects-bad-
output, sync-preview-against-a-changed-remote, merge-refused-on-identity-
mismatch. None of those need a window.

Do this **before** adding features, because every new command written in the
old shape is another thing to move.

**Outcome.** `crates/vardo-core` now holds `Session`, entry listing, the save
path, sync orchestration, the review view models and the generator.
`app/src-tauri` is down to a window, thin command wrappers and the clipboard.
18 new tests cover paths that previously had none — including one that
serialises an `EntryDetail` and greps the JSON for a known password, which
turns the no-secrets rule from a convention about type shape into something
that fails loudly.

---

## 3. Non-happy paths that are currently unhandled

Roughly in order of likelihood.

| Case | What happens today | What should happen |
|---|---|---|
| Remote is a different vault | **Done** — `Refusal::DifferentVaults` | |
| Password changed on the other device | **Done** — `RemotePassword`, which does not blame the password that is working | |
| Remote deleted or moved | **Done** — `RemoteMissing`, distinct from the vault being missing | |
| Disk full mid-save | **Done** — `OutOfSpace`; every IO failure now states the vault is unchanged, which it is | |
| Remote is KDBX 3.1 | **Done** — `remote-read-only` warning, and the identity guard fires first | |
| Clock skew between devices | **Done** — `clock-skew` warning above an hour | |
| Sync point unreadable | **Done** — `no-base` warning; the two-way fallback is no longer silent | |
| OAuth token revoked | **Done** — reported as needing to sign in again, not as a sync failure | |
| Two Vardo instances, same vault | **Partly done** — a save refuses when the file changed since it was opened, so nothing is overwritten; there is no lock file yet, so the second window learns late | A lock file, so the second window says so on open |
| Vault with large attachments | Whole file re-uploaded per save | Measure it; if it is bad, say so before shipping attachments |

The pattern in nearly all of these: the failure is handled safely at the data
layer and then reported as a raw string. **The safety is real; the explanation
is missing.** That is a smaller job than it looks, and `errorText` plus the
`UiError` variants are already the right shape for it.

---

## 4. The testing gap

Ranked by risk.

1. ~~**No automated end-to-end test.**~~ **Done** —
   `crates/vardo-core/tests/end_to_end.rs`, 9 tests in 1.8s, replacing minutes
   of driving a window per run. Writing it surfaced three things worth knowing:

   - **Merging is entry-level, not field-level.** The newer version of an entry
     wins wholesale, including fields the editor never touched — so editing a
     stale entry reverts the other device's change to a *different* field of
     it. That is the right trade (field-level merging would assemble a
     credential that never existed anywhere), and it is survivable only because
     the loser is archived. Now pinned by a test.
   - **The sync point is per-device state.** Two sessions sharing a config
     directory share one sync point and overwrite each other's, which makes
     them one device with two windows. A convergence test written that way
     passes or fails for the wrong reasons.
   - **Same-second edits are conflicts**, so any test whose meaning depends on
     "A then B" has to space them apart explicitly rather than rely on
     wall-clock luck. That is the format's limit, not flakiness.
2. ~~**Zero frontend tests.**~~ **Done** — Vitest, with pure-logic tests and
   rendering tests in jsdom, run by `check.sh`. The first run caught an avatar
   bug that had shipped, and writing the README caught a label that said
   "UserName"; both are now pinned.
3. **No property-based merge testing beyond the chaos suite.** The chaos suite
   is good and randomised, but it only runs two-device interleavings. Worth
   adding: merge is idempotent (merging twice changes nothing the second time),
   and merge is commutative in outcome (A into B and B into A converge).
4. ~~**No test that the app cannot leak a secret.**~~ **Done**, twice: a Rust
   test greps the serialised `EntryDetail` for a known password, and a
   rendering test checks the password is absent from the page.

---

## 5. Then polish

Only after the above, because none of it is load-bearing.

- ~~Attachments: extract, add, remove.~~ Done.
- ~~TOTP countdown UI.~~ Done.
- Password health: reused, weak, expired and old are done. HIBP k-anonymity
  lookups are not.
- Entry history: browsing and copying old passwords are done. Restoring a
  whole old version is not.
- ~~Group create, rename, delete.~~ Done. Dragging groups to move them is not.
- ~~System tray.~~ Done.
- Quick-access global hotkey overlay.
- Command palette.
- The Drive refresh token in the Secret Service, Keychain and DPAPI rather
  than a `0600` file.
- Distribution: Ed25519 updater signing, notarisation, Trusted Signing.

---

## 6. What is genuinely finished

Worth stating, so effort does not get spent re-litigating it:

- **The container.** KDBX 3.1/4.x read, 4.x write, byte-stable round-trip,
  ~14.9M fuzz executions, cross-checked against `keepassxc-cli`.
- **The merge semantics.** Same-second edits are conflicts rather than coin
  flips; a disappearance without a tombstone keeps the entry; nothing is
  deleted without a base.
- **The Drive contract.** Six of eight assumptions now measured against a live
  account rather than assumed.
- **The visual language.** Ported from Telegram Desktop with contrast enforced
  in CI.

---

## Suggested order

1. Merge identity guard (1.1) — small, prevents the worst outcome.
2. Extract `vardo-core` (2) — unblocks everything else being testable.
3. Async save (1.2) — needs 2 to be done cleanly.
4. End-to-end test (4.1) — now possible.
5. `StorageBackend` conditional write (1.3) — before WebDAV.
6. Non-happy-path messages (3) — mostly mechanical once 2 is done.
7. WebDAV backend.
8. Polish (5).
