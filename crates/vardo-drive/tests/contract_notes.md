# Contract tests against live Drive

Everything in `backend.rs` runs against a scripted transport, so it proves the
backend behaves as the **documentation** says Drive does. It cannot prove the
documentation is accurate.

These are the assumptions load-bearing enough that being wrong about them
breaks the product. Each needs confirming once against a real account, then
re-confirming if Google changes anything. Until then they are stated
assumptions, not verified facts, and this file exists so that distinction stays
visible.

Use a **throwaway Google account**, never a personal one. The whole point of
the fake is that this suite stays tiny.

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
