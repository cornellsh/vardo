# ADR-005: Drive is a backend, not the architecture

**Status**: accepted, and it amends `ADR-001`
**Question**: is Google Drive the right long-term answer for multi-device sync?

## Short answer

Ship Drive, because that is where the vault already lives and KeePassium reads
it there. But **Drive is the weakest backend we will ever support**, and the
`StorageBackend` trait is currently shaped around its weakness. That is the
thing worth fixing now, while there is exactly one implementation.

## The finding that matters

Drive has **no compare-and-swap**. `ADR-001` established this: v3 removed
`etag`, and `If-Match` is ignored. The entire race-detection protocol in
`vardo-sync` — "is the revision directly beneath ours what was head when we
wrote?" — exists solely to emulate a conditional write that Drive will not
perform.

Both plausible alternatives *do* have conditional writes:

| Backend | Conditional write | On iOS via KeePassium |
|---|---|---|
| Google Drive | **No** | Direct connection, full support |
| WebDAV | Yes — `If-Match` with ETags | Direct connection, full support |
| S3-compatible | Yes — `If-Match` / `If-None-Match` | Not natively |

So we designed the trait against the one option that cannot do the safe thing,
and every future backend inherits that compromise. A backend able to do a real
compare-and-swap currently has no way to say so.

## The correction

The trait should express the **strongest** operation and let weak backends
emulate it, rather than standardising on the weakest:

```rust
/// Write only if the remote is still at `expected`.
///
/// Backends with real conditional writes implement this natively. Drive
/// cannot, and emulates it with the revision walk in `protocol.rs`.
fn write_if_unchanged(
    &mut self,
    bytes: &[u8],
    expected: Option<&str>,
) -> SyncResult<CasOutcome>;
```

`CasOutcome` distinguishes *written* from *precondition failed*, which is the
distinction the current protocol has to infer. On WebDAV and S3 this collapses
to one request with a header, and the race window closes to zero rather than
being detected after the fact.

This is not a rewrite. `protocol.rs` keeps the revision walk as Drive's
implementation of the same method.

## Why not just switch to WebDAV

Tempting — CAS, no OAuth, no per-provider client registration, no quota, and
self-hostable. But its ETag handling is a genuine minefield, not a theoretical
one:

- Apache's `mod_deflate`, at its default `DeflateAlterETag AddSuffix`, appends
  `-gzip` to the ETag it **serves** while evaluating `If-Match` against the
  **bare** tag. Every conditional write 412s forever, and the usual retry
  behaviour treats that as benign concurrency — so **sync goes permanently
  one-way, silently.** Nextcloud's official Docker image ships `mod_deflate`
  enabled.
- The obvious fix — deriving the bare tag and sending it — is unsafe on its
  own, because `If-Match` accepts a *list* and succeeds if **any** member
  matches, so a derived candidate can match a genuinely concurrent write and
  clobber it.

So WebDAV's CAS is real but its implementations disagree, and getting it wrong
fails in the worst possible way: quietly, in one direction, for weeks.

## Why not a sync server of our own

Still on the "never" list, and this research does not move it. It changes the
trust model from "your file, your provider" to "trust us", creates an
operational liability, and breaks the KeePassium interop that makes any of this
useful.

## Decision

1. **Drive ships first.** The vault is already there and iOS reads it there.
2. **Rework `StorageBackend` around a conditional write**, before a second
   backend exists. Drive emulates it; everyone else gets it for free and safer.
3. **WebDAV is the second backend**, and probably the better recommendation for
   anyone starting fresh. It must implement the ETag mitigations above, with the
   `mod_deflate` case as an explicit test.
4. **S3-compatible third**, for people who want R2/B2/MinIO. No iOS story, so it
   is a desktop-and-Android answer only.

## What this does not fix

**Whole-file transfer on every save.** KDBX has no delta format, so a save
uploads the entire vault. At 646 KB that is irrelevant; at 50 MB of attachments
it is a real bound, on every backend. Worth measuring before recommending
attachments as a feature.

**Drive's billing is unsettled.** The free threshold is 400,000,000 quota units
per day with a 1 TB/day egress cap, and Google says full billing details come
"later in 2026 with at least 90 days' notice", with quota *increases* requiring
billing enabled. For one person syncing one vault this is not close to
binding — but it is a reason not to make Drive the only door.

## Confidence

The CAS asymmetry is well established. The WebDAV ETag hazards come from two
independent bug reports against a shipping product, so they are real but the
list of affected servers is not exhaustive — expect more once we test widely.
The Drive contract assumptions in `crates/vardo-drive/tests/contract_notes.md`
remain unverified against a live account, and assumption 1 is exactly the one
this ADR leans on.
