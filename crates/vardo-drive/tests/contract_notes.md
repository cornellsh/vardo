# Contract tests against live Drive

Everything in `backend.rs` runs against a scripted transport, so it proves the
backend behaves as the **documentation** says Drive does. It cannot prove the
documentation is accurate.

These are the assumptions load-bearing enough that being wrong about them
breaks the product. Each needs confirming once against a real account, then
re-confirming if Google changes anything. Until then they are stated
assumptions, not verified facts, and this file exists so that distinction stays
visible.

## Status: six of eight verified, 2026-08-27

Run with:

```sh
cargo run -p vardo-drive --features network --example contract -- \
    --client-id <id> --client-secret <secret>
```

The harness **creates its own file** and never opens the picker, so with
`drive.file` no other file in the account is reachable. Every destructive call
re-reads the name from Drive and aborts unless it starts with
`vardo-contract-test-`. It deletes the file when it finishes, including after a
failure.

| # | Result |
|---|---|
| 1 | **Confirmed.** `PATCH` with a deliberately wrong `If-Match` returned **200**. The header is ignored, so the race-detection protocol is necessary rather than redundant. |
| 2 | **Confirmed.** Two `uploadType=media` writes, file id unchanged. |
| 3 | **Confirmed.** Writing byte-identical content still produced a new `headRevisionId`. |
| 4 | **Confirmed, with a caveat below.** |
| 5 | **Confirmed.** An overwritten revision was still fetchable and returned its original 16 bytes. |
| 6 | **Confirmed.** A stored refresh token was reused on a later run with no consent prompt. |
| 7 | Not tested — needs a real quota breach. |
| 8 | Not tested — needs publishing the consent screen. |

### The caveat on assumption 4, which nearly became a false bug report

The first version of the harness asserted that `DriveBackend::revisions()`
returns **oldest first**, and it failed: the observed timestamps were strictly
descending.

That was the *test* being wrong. Assumption 4 is about Drive's **raw** API
response; `DriveBackend::revisions` deliberately calls `.reverse()` because the
protocol reads `revisions[1]` as "the revision directly beneath ours" and
therefore needs **newest first**. Asserting the raw-API property against the
wrapper's output conflates two layers, and acting on it would have meant
"fixing" a correct `.reverse()` into an actual bug.

The harness now asserts what the protocol actually consumes, and says so.

### One real defect the live run found

The harness originally requested `trigger_onepick=true` while its own safety
notes claimed it "never opens the picker". Running it opened a Drive file
picker over a real account — one misclick from handing the app the user's
actual password database. The picker is the only mechanism that can grant
access to a file we did not create, so it is now never requested;
`files.create` alone is enough, because a file we create is one `drive.file`
lets us read, write and delete.

## The assumptions

| # | Assumption | Why it matters if wrong |
|---|---|---|
| 1 | `If-Match` on `PATCH /upload/drive/v3/files/{id}` is **ignored**, and no conditional write exists | If a conditional write *does* work, the whole race-detection protocol is unnecessary complexity and should be replaced by a compare-and-swap |
| 2 | `files.update` with `uploadType=media` **preserves the file id** and creates a new revision | If it mints a new id, every `drive.file` grant dies on first save and KeePassium loses the vault |
| 3 | `headRevisionId` changes on **every** content write, including a write of identical bytes | The change signal is how we decide whether to merge; a stale one skips the merge and clobbers |
| 4 | The revision list is ordered **oldest first** | `revisions[1]` is how a lost race is detected; wrong order silently disables it |
| 5 | Overwritten revisions remain **fetchable** for long enough to recover | This is the entire basis for "a lost race is recoverable" |
| 6 | A `drive.file` grant obtained via `trigger_onepick=true` **persists** across sessions | If it is one-shot, the user re-picks the file on every launch |
| 7 | A 403 for quota carries a message containing "rate limit" or "quota" | We distinguish retryable quota errors from permanent permission errors by message text, which is fragile by nature |
| 8 | `drive.file` alone requires **no OAuth verification** to publish | If verification is required after all, the release plan changes completely |

## Suggested procedure

1. Create a throwaway account and a Cloud project. OAuth client type
   **Desktop app**, scope `drive.file` only.
2. Run the consent flow with `trigger_onepick=true`, pick a small `.kdbx`, and
   record the returned file id.
3. **(1)** `PATCH` with a deliberately wrong `If-Match`. Expect success, which
   confirms the header is ignored. A 412 would be excellent news and should
   trigger a redesign.
4. **(2)** Write twice, comparing `id` before and after. It must not change.
5. **(3)** Write byte-identical content twice and compare `headRevisionId`.
6. **(4)** List revisions and check `modifiedTime` is ascending.
7. **(5)** Write A, write B, then fetch A's revision and compare bytes.
8. **(6)** Discard the access token, restart, refresh, and read the file again.
9. **(7)** Trigger a quota error and record the exact message text.
10. **(8)** Publish the consent screen and observe whether verification is
    demanded.

## Not covered here

Anything about **quota cost**. Google moved Drive to a quota-unit model in May
2026 and has said billing follows "later in 2026, following 90 days of notice".
The arithmetic in `CRITIQUE.md` §2.1 says the free daily threshold leaves room
for roughly 174k daily active users at a five-minute poll, but the pricing is
not published yet. Recheck before any public release.
