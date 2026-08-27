# What to do next, and why

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

## 2. The structural problem underneath

**Real logic lives inside the Tauri crate, where it cannot be tested.**

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

---

## 3. Non-happy paths that are currently unhandled

Roughly in order of likelihood.

| Case | What happens today | What should happen |
|---|---|---|
| Remote is a different vault | Silently merges the two | Refuse; see 1.1 |
| Password changed on the other device | Sync fails to open the remote; error is a raw string | Detect and say "the other copy uses a different password" |
| Remote deleted or moved | `NotFound` surfaces as a generic IO error | Offer to re-create or re-pick |
| OAuth token revoked | Fails at the transport layer | Detect 401 and prompt to reconnect, not "sync failed" |
| Disk full mid-save | Temp write fails; original intact (good) but message is `ENOSPC` | Say the vault is unchanged, because it is |
| Two Vardo instances, same vault | Both hold the file; last save wins locally | A lock file, or at minimum detect and warn |
| Remote is KDBX 3.1, local is 4.x | Merge works, save refuses | Explain before the merge, not after |
| Clock skew between devices | Merge resolves by timestamp, so a device with a wrong clock always wins | Detect gross skew against the remote's modified time and warn |
| Vault with large attachments | Whole file re-uploaded per save | Measure it; if it is bad, say so before shipping attachments |
| Sync point stale after restore-from-backup | Base no longer matches remote; merge is two-way and deletes nothing | Already safe, but should say why |

The pattern in nearly all of these: the failure is handled safely at the data
layer and then reported as a raw string. **The safety is real; the explanation
is missing.** That is a smaller job than it looks, and `errorText` plus the
`UiError` variants are already the right shape for it.

---

## 4. The testing gap

Ranked by risk.

1. **No automated end-to-end test.** Everything about the write and sync paths
   has been verified by hand. A headless test that unlocks a fixture, edits,
   saves, mutates the file externally, syncs, and asserts the result would have
   caught the UUID-format bug and the stale-count bug without a screenshot.
2. **Zero frontend tests.** No component tests, no logic tests. The dirty/save
   state machine and the merge-review rendering are both worth testing directly.
3. **No property-based merge testing beyond the chaos suite.** The chaos suite
   is good and randomised, but it only runs two-device interleavings. Worth
   adding: merge is idempotent (merging twice changes nothing the second time),
   and merge is commutative in outcome (A into B and B into A converge).
4. **No test that the app cannot leak a secret.** The no-secrets-in-the-WebView
   rule is currently enforced by the *shape* of the types, which is good, but
   nothing fails if someone adds a field. A test that walks the serialised
   `EntryDetail` for a known password string would make it enforced.

---

## 5. Then polish

Only after the above, because none of it is load-bearing.

- Attachments: extract, add, remove.
- TOTP: the field parsing exists; the countdown UI does not.
- Password health: reused, weak, old, and HIBP k-anonymity lookups.
- Quick-access global hotkey overlay (Phase B, still unbuilt).
- Entry history browser and restore.
- Group create/rename/move.
- Command palette.
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
